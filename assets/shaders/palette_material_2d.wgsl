#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct PaletteParams {
    bias: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> palette_params: PaletteParams;

@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var source_image: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(2)
var source_sampler: sampler;

@group(#{MATERIAL_BIND_GROUP}) @binding(3)
var palette_texture: texture_2d<f32>;

fn rgb_to_oklab(rgb: vec3<f32>) -> vec3<f32> {
    let l = 0.41222146 * rgb.r + 0.53633255 * rgb.g + 0.051445995 * rgb.b;
    let m = 0.2119035 * rgb.r + 0.6806995 * rgb.g + 0.10739696 * rgb.b;
    let s = 0.08830246 * rgb.r + 0.28171884 * rgb.g + 0.6299787 * rgb.b;

    let l_ = pow(max(l, 0.0), 1.0 / 3.0);
    let m_ = pow(max(m, 0.0), 1.0 / 3.0);
    let s_ = pow(max(s, 0.0), 1.0 / 3.0);

    return vec3<f32>(
        0.21045426 * l_ + 0.7936178 * m_ - 0.004072047 * s_,
        1.9779985 * l_ - 2.4285922 * m_ + 0.4505937 * s_,
        0.025904037 * l_ + 0.78277177 * m_ - 0.80867577 * s_,
    );
}

fn biased_distance_squared(color: vec3<f32>, palette_color: vec3<f32>, bias: vec3<f32>) -> f32 {
    let color_oklab = rgb_to_oklab(color);
    let palette_oklab = rgb_to_oklab(palette_color);

    let color_l = color_oklab.x;
    let color_a = color_oklab.y;
    let color_b = color_oklab.z;
    let color_c = sqrt(color_a * color_a + color_b * color_b);
    let color_h = select(0.0, atan2(color_b, color_a), color_c > 0.000001);

    let palette_l = palette_oklab.x;
    let palette_a = palette_oklab.y;
    let palette_b = palette_oklab.z;
    let palette_c = sqrt(palette_a * palette_a + palette_b * palette_b);
    let palette_h = select(0.0, atan2(palette_b, palette_a), palette_c > 0.000001);

    let dl = color_l - palette_l;
    let dc = color_c - palette_c;
    var hue_delta = abs(color_h - palette_h) % (2.0 * 3.14159265);
    if hue_delta > 3.14159265 {
        hue_delta = 2.0 * 3.14159265 - hue_delta;
    }
    let dh = sin(hue_delta * 0.5) * 2.0 * max(color_c, palette_c);

    return bias.x * dl * dl + bias.y * dc * dc + bias.z * dh * dh;
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let source = textureSample(source_image, source_sampler, mesh.uv).rgb;
    let bias = palette_params.bias.xyz;
    let palette_count = u32(max(palette_params.bias.w, 1.0));

    var best_color = textureLoad(palette_texture, vec2<i32>(0, 0), 0).rgb;
    var best_index: u32 = 0u;
    var best_distance = biased_distance_squared(source, best_color, bias);

    for (var index: u32 = 1u; index < 256u; index = index + 1u) {
        if index >= palette_count {
            break;
        }
        let candidate = textureLoad(palette_texture, vec2<i32>(i32(index), 0), 0).rgb;
        let distance = biased_distance_squared(source, candidate, bias);
        if distance < best_distance {
            best_distance = distance;
            best_color = candidate;
            best_index = index;
        }
    }

    let normalized_index = f32(best_index) / 255.0;
    return vec4<f32>(normalized_index, 0.0, 0.0, 1.0);
}

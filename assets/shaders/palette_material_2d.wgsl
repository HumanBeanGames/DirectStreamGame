#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct PaletteParams {
    bias: vec4<f32>,
};

struct LookupParams {
    flags: vec4<f32>,
};

struct InputOffsetParams {
    value_chroma: vec4<f32>,
};

struct InputHueOffsetParams {
    values: vec4<f32>,
};

struct DarkNeutralParams {
    values: vec4<f32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> palette_params: PaletteParams;

@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var source_image: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(2)
var source_sampler: sampler;

@group(#{MATERIAL_BIND_GROUP}) @binding(3)
var palette_texture: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(4)
var<uniform> lookup_params: LookupParams;

@group(#{MATERIAL_BIND_GROUP}) @binding(5)
var lookup_texture: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(6)
var<uniform> input_offset_params: InputOffsetParams;

@group(#{MATERIAL_BIND_GROUP}) @binding(7)
var<uniform> input_hue_offset_params: InputHueOffsetParams;

@group(#{MATERIAL_BIND_GROUP}) @binding(8)
var<uniform> dark_neutral_params: DarkNeutralParams;

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

fn oklab_to_oklch(oklab: vec3<f32>) -> vec3<f32> {
    let chroma = sqrt(oklab.y * oklab.y + oklab.z * oklab.z);
    let hue = select(0.0, atan2(oklab.z, oklab.y), chroma > 0.000001);
    return vec3<f32>(oklab.x, chroma, hue);
}

fn apply_input_offset(color: vec3<f32>) -> vec3<f32> {
    let value_chroma = input_offset_params.value_chroma;
    return vec3<f32>(
        clamp(color.x * (1.0 + value_chroma.x) + value_chroma.y, 0.0, 1.0),
        max(color.y * (1.0 + value_chroma.z) + value_chroma.w, 0.0),
        color.z + input_hue_offset_params.values.x * 2.0 * 3.14159265,
    );
}

fn biased_distance_squared_oklch(color: vec3<f32>, palette_color: vec3<f32>, bias: vec3<f32>) -> f32 {
    let color_l = color.x;
    let color_c = color.y;
    let color_h = color.z;
    let palette_l = palette_color.x;
    let palette_c = palette_color.y;
    let palette_h = palette_color.z;
    let dl = color_l - palette_l;
    let dc = color_c - palette_c;
    var hue_delta = abs(color_h - palette_h) % (2.0 * 3.14159265);
    if hue_delta > 3.14159265 {
        hue_delta = 2.0 * 3.14159265 - hue_delta;
    }
    let dh = sin(hue_delta * 0.5) * 2.0 * max(color_c, palette_c);
    let dark_values = dark_neutral_params.values;
    let chroma_weight = select(
        bias.y,
        bias.y * max(dark_values.w, 1.0),
        dark_values.x > 0.5 && color_l <= dark_values.y && color_c <= dark_values.z
    );

    return bias.x * dl * dl + chroma_weight * dc * dc + bias.z * dh * dh;
}

fn linear_to_srgb_channel(value: f32) -> f32 {
    let clamped = clamp(value, 0.0, 1.0);
    if clamped <= 0.0031308 {
        return clamped * 12.92;
    }
    return 1.055 * pow(clamped, 1.0 / 2.4) - 0.055;
}

fn linear_to_srgb(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        linear_to_srgb_channel(rgb.r),
        linear_to_srgb_channel(rgb.g),
        linear_to_srgb_channel(rgb.b)
    );
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let source = textureSample(source_image, source_sampler, mesh.uv).rgb;
    let source_u8 = vec3<u32>(round(linear_to_srgb(source) * 255.0));
    let lookup_index = source_u8.r * 65536u + source_u8.g * 256u + source_u8.b;
    let lookup_coord = vec2<i32>(
        i32(lookup_index % 4096u),
        i32(lookup_index / 4096u)
    );
    let palette_index = textureLoad(lookup_texture, lookup_coord, 0).r;
    return vec4<f32>(palette_index, 0.0, 0.0, 1.0);
}

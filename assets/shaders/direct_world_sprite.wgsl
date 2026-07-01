#import bevy_pbr::forward_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> tint: vec4<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var sprite_texture: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(2)
var sprite_sampler: sampler;

@group(#{MATERIAL_BIND_GROUP}) @binding(3)
var<uniform> params: vec4<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(4)
var palette_texture: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(5)
var lookup_texture: texture_2d<u32>;

fn sample_sprite_texture(uv: vec2<f32>) -> vec4<f32> {
    let dimensions = textureDimensions(sprite_texture);
    let clamped_uv = clamp(uv, vec2<f32>(0.0), vec2<f32>(0.999999));
    let texel = vec2<i32>(floor(clamped_uv * vec2<f32>(dimensions)));
    return textureLoad(sprite_texture, texel, 0);
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

fn srgb_to_linear_channel(value: f32) -> f32 {
    let clamped = clamp(value, 0.0, 1.0);
    if clamped <= 0.04045 {
        return clamped / 12.92;
    }
    return pow((clamped + 0.055) / 1.055, 2.4);
}

fn srgb_to_linear(rgb: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(
        srgb_to_linear_channel(rgb.r),
        srgb_to_linear_channel(rgb.g),
        srgb_to_linear_channel(rgb.b)
    );
}

fn lookup_direct_palette_color(source_linear: vec3<f32>) -> vec3<f32> {
    let lookup_size = textureDimensions(lookup_texture);
    if lookup_size.y < 8192u {
        return source_linear;
    }

    let source_srgb = linear_to_srgb(source_linear);
    let source_u8 = vec3<u32>(round(clamp(source_srgb, vec3<f32>(0.0), vec3<f32>(1.0)) * 255.0));
    let lookup_index = source_u8.r * 65536u + source_u8.g * 256u + source_u8.b;
    let lookup_coord = vec2<i32>(
        i32(lookup_index % 4096u),
        i32((lookup_index / 4096u) + 4096u)
    );
    let palette_width = textureDimensions(palette_texture).x;
    let palette_count = min(u32(max(params.y, 1.0)), palette_width);
    let palette_index = min(textureLoad(lookup_texture, lookup_coord, 0).r, palette_count - 1u);
    let palette_srgb = textureLoad(palette_texture, vec2<i32>(i32(palette_index), 0), 0).rgb;
    return srgb_to_linear(palette_srgb);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let direct_lookup = abs(tint.a - (254.0 / 255.0)) <= (0.5 / 255.0);
#ifdef VERTEX_UVS_A
    let color = sample_sprite_texture(in.uv) * tint;
#else
    let color = tint;
#endif

    if color.a <= 0.01 {
        discard;
    }

    let direct_opaque_pixel = direct_lookup && abs(color.a - (254.0 / 255.0)) <= (0.5 / 255.0);
    if direct_opaque_pixel && params.x > 0.5 {
        return vec4<f32>(lookup_direct_palette_color(color.rgb), 254.0 / 255.0);
    }

    return color;
}

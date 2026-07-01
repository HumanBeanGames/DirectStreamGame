#import bevy_pbr::forward_io::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var<uniform> tint: vec4<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var sprite_texture: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(2)
var sprite_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
#ifdef VERTEX_UVS_A
    let color = textureSample(sprite_texture, sprite_sampler, in.uv) * tint;
#else
    let color = tint;
#endif

    if color.a <= 0.01 {
        discard;
    }

    return color;
}

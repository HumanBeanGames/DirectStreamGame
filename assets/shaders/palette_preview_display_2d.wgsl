#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var index_image: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var index_sampler: sampler;

@group(#{MATERIAL_BIND_GROUP}) @binding(2)
var palette_texture: texture_2d<f32>;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let index_value = textureSample(index_image, index_sampler, mesh.uv).r;
    let palette_width = textureDimensions(palette_texture).x;
    let palette_index = min(u32(round(clamp(index_value, 0.0, 1.0) * 255.0)), palette_width - 1u);
    return textureLoad(palette_texture, vec2<i32>(i32(palette_index), 0), 0);
}

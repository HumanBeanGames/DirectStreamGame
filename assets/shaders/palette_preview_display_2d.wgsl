#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var indexed_image: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var indexed_sampler: sampler;

@group(#{MATERIAL_BIND_GROUP}) @binding(2)
var palette_texture: texture_2d<f32>;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let source_size = textureDimensions(indexed_image);
    let source_uv = clamp(mesh.uv, vec2<f32>(0.0), vec2<f32>(0.999999));
    let source_coord = vec2<i32>(floor(source_uv * vec2<f32>(source_size)));
    let palette_width = textureDimensions(palette_texture).x;
    let encoded = textureLoad(indexed_image, source_coord, 0).r;
    let palette_index = min(u32(floor(encoded * 255.0 + 0.5)), palette_width - 1u);
    return textureLoad(palette_texture, vec2<i32>(i32(palette_index), 0), 0);
}

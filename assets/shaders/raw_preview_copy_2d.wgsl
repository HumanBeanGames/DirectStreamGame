#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var source_image: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var source_sampler: sampler;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let source_size = textureDimensions(source_image);
    let source_uv = clamp(mesh.uv, vec2<f32>(0.0), vec2<f32>(0.999999));
    let source_coord = vec2<i32>(floor(source_uv * vec2<f32>(source_size)));
    return textureLoad(source_image, source_coord, 0);
}

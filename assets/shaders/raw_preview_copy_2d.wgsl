#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var source_image: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var source_sampler: sampler;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(source_image, source_sampler, mesh.uv);
}

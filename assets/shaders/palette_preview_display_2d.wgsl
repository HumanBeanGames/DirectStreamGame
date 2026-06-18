#import bevy_sprite::mesh2d_vertex_output::VertexOutput

@group(#{MATERIAL_BIND_GROUP}) @binding(0)
var source_image: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(1)
var source_sampler: sampler;

@group(#{MATERIAL_BIND_GROUP}) @binding(2)
var palette_texture: texture_2d<f32>;

@group(#{MATERIAL_BIND_GROUP}) @binding(3)
var lookup_texture: texture_2d<u32>;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let source_size = textureDimensions(source_image);
    let source_uv = clamp(mesh.uv, vec2<f32>(0.0), vec2<f32>(0.999999));
    let source_coord = vec2<i32>(floor(source_uv * vec2<f32>(source_size)));
    let source = clamp(textureLoad(source_image, source_coord, 0).rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    let source_u8 = vec3<u32>(round(source * 255.0));
    let lookup_index = source_u8.r * 65536u + source_u8.g * 256u + source_u8.b;
    let lookup_coord = vec2<i32>(
        i32(lookup_index % 4096u),
        i32(lookup_index / 4096u)
    );
    let palette_width = textureDimensions(palette_texture).x;
    let palette_index = min(textureLoad(lookup_texture, lookup_coord, 0).r, palette_width - 1u);
    return textureLoad(palette_texture, vec2<i32>(i32(palette_index), 0), 0);
}

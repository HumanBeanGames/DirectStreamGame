use crate::{
    palette::SharedPaletteBias,
    public_types::DirectStreamTarget,
    scene::{PendingReadback, RenderedBatchFrame, StreamReadback},
    stream_control::StreamControl,
};
use bevy::{
    asset::{RenderAssetUsages, load_internal_asset, uuid_handle},
    camera::{RenderTarget, visibility::RenderLayers},
    prelude::*,
    reflect::TypePath,
    render::gpu_readback::Readback,
    render::render_resource::{
        AsBindGroup, Extent3d, TextureDimension, TextureFormat, TextureUsages,
    },
    shader::{Shader, ShaderRef},
    sprite_render::{Material2d, Material2dPlugin},
};
use std::time::Instant;

pub(crate) const GPU_PALETTE_LAYER: usize = 1;
pub(crate) const GPU_DIRECT_TEXT_LAYER: usize = 2;
const PALETTE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("b69538c2-4fa1-4a12-89a5-32986e423f4d");

pub(crate) struct GpuPalettePlugin;

impl Plugin for GpuPalettePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            PALETTE_SHADER_HANDLE,
            "../assets/shaders/palette_material_2d.wgsl",
            Shader::from_wgsl
        );

        app.add_plugins(Material2dPlugin::<PaletteMaterial>::default())
            .add_systems(Update, sync_palette_material_bias)
            .add_systems(Update, cycle_camera_render_targets);
    }
}

#[derive(AsBindGroup, Debug, Clone, Asset, TypePath)]
pub(crate) struct PaletteMaterial {
    #[uniform(0)]
    pub(crate) params: Vec4,
    #[texture(1)]
    #[sampler(2)]
    pub(crate) source_image: Handle<Image>,
    #[texture(3)]
    pub(crate) palette_texture: Handle<Image>,
}

impl Material2d for PaletteMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(PALETTE_SHADER_HANDLE)
    }
}

#[derive(Resource, Clone)]
pub(crate) struct GpuPalettePipeline {
    pub(crate) material: Handle<PaletteMaterial>,
    pub(crate) palette_camera: Entity,
    pub(crate) overlay_camera: Entity,
    pub(crate) quad_entity: Entity,
    pub(crate) output_images: Vec<Handle<Image>>,
    pub(crate) current_output_index: usize,
    pub(crate) palette_count: usize,
    pub(crate) palette_colors: Vec<[u8; 4]>,
}

pub(crate) fn make_stream_source_image(width: u32, height: u32) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::RENDER_ATTACHMENT;
    image
}

pub(crate) fn make_stream_output_image(width: u32, height: u32) -> Image {
    let mut image = Image::new_fill(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0],
        TextureFormat::R8Unorm,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::RENDER_ATTACHMENT;
    image
}

pub(crate) fn make_palette_texture(colors: &[[u8; 4]]) -> Image {
    let width = colors.len().max(1) as u32;
    let mut data = Vec::with_capacity(width as usize * 4);
    if colors.is_empty() {
        data.extend_from_slice(&[0, 0, 0, 255]);
    } else {
        for color in colors {
            data.extend_from_slice(color);
        }
    }

    let mut image = Image::new_fill(
        Extent3d {
            width,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &data,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::default(),
    );
    image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    image
}

pub(crate) fn spawn_custom_host_pipeline(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<PaletteMaterial>,
    width: u32,
    height: u32,
    source_image: Handle<Image>,
    palette_colors: &[[u8; 4]],
    palette_bias: crate::palette::PaletteBias,
    target: &mut DirectStreamTarget,
    batch_size: usize,
) -> GpuPalettePipeline {
    let output_images: Vec<Handle<Image>> = (0..output_image_count(batch_size))
        .map(|_| images.add(make_stream_output_image(width, height)))
        .collect();
    let first_output = output_images.first().cloned().unwrap();
    let palette_texture = images.add(make_palette_texture(palette_colors));
    let material = materials.add(PaletteMaterial {
        params: palette_material_params(&palette_bias, palette_colors.len()),
        source_image,
        palette_texture,
    });

    let palette_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: 0,
                clear_color: ClearColorConfig::Custom(Color::BLACK),
                ..default()
            },
            RenderTarget::Image(first_output.clone().into()),
            RenderLayers::layer(GPU_PALETTE_LAYER),
        ))
        .id();

    let overlay_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: 1,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            RenderTarget::Image(first_output.clone().into()),
            RenderLayers::layer(GPU_DIRECT_TEXT_LAYER),
        ))
        .id();

    let quad_entity = commands
        .spawn((
            Mesh2d(meshes.add(Rectangle::default())),
            MeshMaterial2d(material.clone()),
            Transform::from_scale(Vec3::new(width as f32, height as f32, 1.0)),
            RenderLayers::layer(GPU_PALETTE_LAYER),
        ))
        .id();

    target.output_image = first_output.clone();
    target.output_is_indexed = true;
    target.overlay_camera = overlay_camera;
    target.overlay_layer = GPU_DIRECT_TEXT_LAYER;

    GpuPalettePipeline {
        material,
        palette_camera,
        overlay_camera,
        quad_entity,
        output_images,
        current_output_index: 0,
        palette_count: palette_colors.len(),
        palette_colors: palette_colors.to_vec(),
    }
}

pub(crate) fn retarget_custom_host_pipeline(
    pipeline: &mut GpuPalettePipeline,
    images: &mut Assets<Image>,
    materials: &mut Assets<PaletteMaterial>,
    camera_targets: &mut Query<&mut RenderTarget>,
    quad_transforms: &mut Query<&mut Transform>,
    width: u32,
    height: u32,
    source_image: Handle<Image>,
    target: &mut DirectStreamTarget,
    batch_size: usize,
) -> Result<(), ()> {
    let output_images: Vec<Handle<Image>> = (0..output_image_count(batch_size))
        .map(|_| images.add(make_stream_output_image(width, height)))
        .collect();
    let first_output = output_images.first().cloned().unwrap();

    if let Ok(mut camera_target) = camera_targets.get_mut(pipeline.palette_camera) {
        *camera_target = RenderTarget::Image(first_output.clone().into());
    } else {
        return Err(());
    }

    if let Ok(mut camera_target) = camera_targets.get_mut(pipeline.overlay_camera) {
        *camera_target = RenderTarget::Image(first_output.clone().into());
    } else {
        return Err(());
    }

    if let Some(material) = materials.get_mut(&pipeline.material) {
        material.source_image = source_image;
    } else {
        return Err(());
    }

    if let Ok(mut transform) = quad_transforms.get_mut(pipeline.quad_entity) {
        transform.scale = Vec3::new(width as f32, height as f32, 1.0);
    } else {
        return Err(());
    }

    pipeline.output_images = output_images;
    pipeline.current_output_index = 0;
    target.output_image = first_output;
    target.output_is_indexed = true;
    target.overlay_camera = pipeline.overlay_camera;
    target.overlay_layer = GPU_DIRECT_TEXT_LAYER;

    Ok(())
}

fn sync_palette_material_bias(
    palette_bias: Option<Res<SharedPaletteBias>>,
    pipeline: Option<Res<GpuPalettePipeline>>,
    mut materials: ResMut<Assets<PaletteMaterial>>,
) {
    let (Some(palette_bias), Some(pipeline)) = (palette_bias, pipeline) else {
        return;
    };

    if let Some(material) = materials.get_mut(&pipeline.material) {
        material.params = palette_material_params(&palette_bias.get(), pipeline.palette_count);
    }
}

pub(crate) fn cycle_camera_render_targets(
    time: Res<Time>,
    stream_control: Res<StreamControl>,
    mut pipeline: ResMut<GpuPalettePipeline>,
    mut camera_targets: Query<&mut RenderTarget>,
    mut readback: ResMut<StreamReadback>,
    stats: Res<crate::stats::SharedStats>,
    mut commands: Commands,
) {
    if pipeline.output_images.is_empty() || !stream_control.is_streaming() {
        return;
    }

    stats.with_mut(|stats| stats.record_custom_app_frame());
    readback.frame_accumulator += time.delta();
    if readback.frame_accumulator >= readback.frame_interval {
        readback.frame_due = true;
    }

    if !readback.frame_due {
        update_readback_diagnostics(&stats, &readback);
        return;
    }

    if let Some(rendered_frame) = readback.frame_waiting_for_render.take() {
        readback.textures_rendered_in_batch += 1;
        readback.rendered_batch_frames.push(rendered_frame);
    }

    if readback.textures_rendered_in_batch >= readback.batch_size {
        readback.textures_rendered_in_batch = 0;
        readback.batch_in_progress = true;
        readback.batch_started_at.get_or_insert_with(Instant::now);

        let batch_frames = readback.rendered_batch_frames.drain(..).collect::<Vec<_>>();
        for batch_frame in batch_frames {
            let current_image = readback.images[batch_frame.output_index].clone();
            let readback_entity = commands
                .spawn(Readback::texture(current_image))
                .observe(crate::capture::queue_readback_frame)
                .id();
            readback.pending_requests.insert(
                readback_entity,
                PendingReadback {
                    requested_at: Instant::now(),
                    captured_at: batch_frame.captured_at,
                    output_index: batch_frame.output_index,
                },
            );
        }
    }

    let current_output_index = pipeline.current_output_index;
    if readback
        .pending_requests
        .values()
        .any(|pending| pending.output_index == current_output_index)
    {
        update_readback_diagnostics(&stats, &readback);
        return;
    }

    let current_texture = pipeline.output_images[current_output_index].clone();

    if let Ok(mut palette_target) = camera_targets.get_mut(pipeline.palette_camera) {
        *palette_target = RenderTarget::Image(current_texture.clone().into());
    }

    if let Ok(mut overlay_target) = camera_targets.get_mut(pipeline.overlay_camera) {
        *overlay_target = RenderTarget::Image(current_texture.clone().into());
    }

    pipeline.current_output_index =
        (pipeline.current_output_index + 1) % pipeline.output_images.len();
    readback.frame_due = false;
    readback.frame_accumulator = readback
        .frame_accumulator
        .saturating_sub(readback.frame_interval);
    let captured_at = Instant::now();
    readback.frame_waiting_for_render = Some(RenderedBatchFrame {
        output_index: current_output_index,
        captured_at,
    });

    update_readback_diagnostics(&stats, &readback);
}

fn update_readback_diagnostics(stats: &crate::stats::SharedStats, readback: &StreamReadback) {
    stats.with_mut(|stats| {
        stats.custom_pending_readbacks = readback.pending_requests.len();
        stats.custom_batch_buffered_frames = readback.rendered_batch_frames.len()
            + usize::from(readback.frame_waiting_for_render.is_some());
    });
}

fn output_image_count(batch_size: usize) -> usize {
    batch_size.max(1) * 2
}

fn palette_material_params(bias: &crate::palette::PaletteBias, palette_count: usize) -> Vec4 {
    Vec4::new(
        bias.lightness,
        bias.chroma,
        bias.hue,
        palette_count.max(1) as f32,
    )
}

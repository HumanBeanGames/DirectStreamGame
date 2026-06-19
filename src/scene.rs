use crate::{
    config::{AppConfig, WindowMode, effective_custom_batch_size},
    constants::{STREAM_FPS, STREAM_HEIGHT, STREAM_WIDTH, WEB_ADDR, preview_display_scale},
    gpu_palette::{
        GPU_PREVIEW_DISPLAY_LAYER, PaletteMaterial, PalettePreviewDisplayMaterial,
        PreviewPaletteThrottle, PreviewRawDisplay, RawPreviewCopyMaterial,
        make_stream_source_image, spawn_custom_host_pipeline,
    },
    palette::load_palette_lookup_runtime,
    public_types::{DirectStreamTarget, DirectStreamWindowLayout},
    stats::{SharedStats, StatsText},
    stream_control::{
        CustomFpsInputBox, CustomFpsInputText, CustomHeightInputBox, CustomHeightInputText,
        CustomWidthInputBox, CustomWidthInputText, OpenStreamButton, PurgeChatButton,
        StartStreamButton, StopStreamButton, StreamControlStatusText,
    },
};
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    prelude::*,
    render::gpu_readback::{Readback, ReadbackComplete},
    window::PrimaryWindow,
};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

pub(crate) struct PendingReadback {
    pub(crate) captured_at: Instant,
    pub(crate) output_index: usize,
}

pub(crate) struct RenderedBatchFrame {
    pub(crate) output_index: usize,
    pub(crate) captured_at: Instant,
}

#[derive(Resource)]
pub(crate) struct StreamReadback {
    pub(crate) images: Vec<Handle<Image>>,
    pub(crate) pixel_format: ReadbackPixelFormat,
    pub(crate) readback_entities: Vec<Entity>,
    pub(crate) next_readback_entity: usize,
    pub(crate) frame_interval: Duration,
    pub(crate) frame_accumulator: Duration,
    pub(crate) frame_due: bool,
    pub(crate) pending_requests: HashMap<Entity, PendingReadback>,
    pub(crate) batch_size: usize,
    pub(crate) batch_started_at: Option<Instant>,
    pub(crate) batch_in_progress: bool,
    pub(crate) textures_rendered_in_batch: usize,
    pub(crate) frame_waiting_for_render: Option<RenderedBatchFrame>,
    pub(crate) rendered_batch_frames: Vec<RenderedBatchFrame>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadbackPixelFormat {
    Bgra,
    IndexedRgba8,
}

impl ReadbackPixelFormat {
    pub(crate) fn row_bytes(self, width: u32) -> usize {
        match self {
            Self::Bgra => width as usize * 4,
            Self::IndexedRgba8 => width as usize * 4,
        }
    }

    pub(crate) fn is_bgra(self) -> bool {
        self == Self::Bgra
    }
}

#[derive(Component)]
pub(crate) struct PreviewDisplayCamera;

#[derive(Component)]
pub(crate) struct PreviewPixelDebugText;

#[derive(Component)]
struct PreviewPixelDebugReadback {
    source: PreviewPixelSource,
    x: u32,
    y: u32,
    width: u32,
}

#[derive(Component)]
struct PreviewPaletteValidationReadback {
    source: PreviewPixelSource,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreviewPixelSource {
    Raw,
    Quantized,
}

#[derive(Resource)]
pub(crate) struct PreviewPixelDebugState {
    pub(crate) raw_image: Handle<Image>,
    pub(crate) palette_colors: Vec<[u8; 4]>,
    pub(crate) lookup_entries: Arc<[u8]>,
    raw_center: Vec2,
    quantized_center: Vec2,
    display_size: Vec2,
    output: String,
    validation_pending: bool,
    validation_warmup_updates: u32,
    validation_frames_checked: u32,
}

struct RawPixelDebug {
    x: u32,
    y: u32,
    b: u8,
    g: u8,
    r: u8,
    a: u8,
    lookup_key: usize,
    expected_index: Option<u8>,
    expected_color: [u8; 4],
}

struct QuantizedPixelDebug {
    x: u32,
    y: u32,
    palette_index: u8,
    color: [u8; 4],
}

pub(crate) fn setup_direct_stream_scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut palette_materials: ResMut<Assets<PaletteMaterial>>,
    mut preview_display_materials: ResMut<Assets<PalettePreviewDisplayMaterial>>,
    mut raw_copy_materials: ResMut<Assets<RawPreviewCopyMaterial>>,
    config: Res<AppConfig>,
    window_layout: Res<DirectStreamWindowLayout>,
) {
    let stream_image = images.add(make_stream_source_image(
        config.stream_width,
        config.stream_height,
    ));

    let stream_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.04, 0.05, 0.07)),
                ..default()
            },
            RenderTarget::Image(stream_image.clone().into()),
            RenderLayers::layer(0),
        ))
        .id();

    match config.window_mode {
        WindowMode::Preview => {
            commands.spawn((
                Camera2d,
                RenderLayers::layer(GPU_PREVIEW_DISPLAY_LAYER),
                PreviewDisplayCamera,
            ));
        }
        WindowMode::Stats => {
            commands.spawn(Camera2d);
            spawn_stats_window(&mut commands, config.custom_host, &window_layout);
        }
    }

    let mut target = DirectStreamTarget {
        camera: stream_camera,
        overlay_camera: stream_camera,
        image: stream_image.clone(),
        output_image: stream_image.clone(),
        output_is_indexed: false,
        overlay_layer: 0,
        width: config.stream_width,
        height: config.stream_height,
        fps: config.stream_fps,
    };

    if config.custom_host || config.window_mode == WindowMode::Preview {
        let palette_lookup = load_palette_lookup_runtime(&config.palette_lookup_path);
        let palette_config = palette_lookup.config();
        let palette_colors = palette_config.colors.clone();
        let palette_bias = crate::palette::PaletteBias::from(palette_config.matching);
        let batch_size = if config.custom_host {
            effective_custom_batch_size(config.custom_host_batch_size, config.stream_fps)
        } else {
            effective_custom_batch_size(config.custom_host_batch_size, config.stream_fps)
        };
        let pipeline = spawn_custom_host_pipeline(
            &mut commands,
            &mut images,
            &mut meshes,
            &mut palette_materials,
            &mut raw_copy_materials,
            config.stream_width,
            config.stream_height,
            stream_image.clone(),
            &palette_colors,
            palette_bias,
            &palette_lookup,
            &mut target,
            batch_size,
            true,
        );
        if config.window_mode == WindowMode::Preview {
            let (display_material, debug_state) = spawn_preview_comparison(
                &mut commands,
                &mut images,
                &mut meshes,
                &mut preview_display_materials,
                &mut raw_copy_materials,
                &stream_image,
                &pipeline,
                Arc::<[u8]>::from(palette_lookup.entries().to_vec()),
                batch_size,
                &window_layout,
                config.stream_width,
                config.stream_height,
            );
            commands.insert_resource(debug_state);
            commands.insert_resource(PreviewPaletteThrottle::new(
                config.stream_fps,
                batch_size,
                display_material,
            ));
        }
        let pipeline_clone = pipeline.clone();
        commands.insert_resource(pipeline);
        if config.custom_host {
            let readback_entities =
                spawn_readback_entities(&mut commands, pipeline_clone.output_images.len());
            commands.insert_resource(StreamReadback {
                images: pipeline_clone.output_images.clone(),
                pixel_format: ReadbackPixelFormat::IndexedRgba8,
                readback_entities,
                next_readback_entity: 0,
                frame_interval: Duration::from_secs_f64(1.0 / config.stream_fps as f64),
                frame_accumulator: Duration::ZERO,
                frame_due: false,
                pending_requests: HashMap::default(),
                batch_size,
                batch_started_at: None,
                batch_in_progress: false,
                textures_rendered_in_batch: 0,
                frame_waiting_for_render: None,
                rendered_batch_frames: Vec::with_capacity(batch_size),
            });
        }
    }
    commands.insert_resource(target);
}

fn spawn_preview_comparison(
    commands: &mut Commands,
    _images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    preview_display_materials: &mut Assets<PalettePreviewDisplayMaterial>,
    _raw_copy_materials: &mut Assets<RawPreviewCopyMaterial>,
    _stream_image: &Handle<Image>,
    pipeline: &crate::gpu_palette::GpuPalettePipeline,
    lookup_entries: Arc<[u8]>,
    _batch_size: usize,
    window_layout: &DirectStreamWindowLayout,
    width: u32,
    height: u32,
) -> (
    Handle<PalettePreviewDisplayMaterial>,
    PreviewPixelDebugState,
) {
    let raw_output_images = pipeline.source_images.clone();
    let first_raw_output = raw_output_images[0].clone();

    let x_offset = width as f32 * 0.5;
    let display_scale = preview_display_scale(width, height);
    let reserved_panel_offset = window_layout.right_panel_width * 0.5;
    let raw_center = Vec2::new(-x_offset * display_scale - reserved_panel_offset, 0.0);
    let quantized_center = Vec2::new(x_offset * display_scale - reserved_panel_offset, 0.0);
    let display_size = Vec2::new(width as f32 * display_scale, height as f32 * display_scale);
    commands.spawn((
        Sprite {
            image: first_raw_output,
            custom_size: Some(Vec2::new(width as f32, height as f32)),
            ..default()
        },
        Transform::from_xyz(raw_center.x, raw_center.y, 0.0).with_scale(Vec3::splat(display_scale)),
        RenderLayers::layer(GPU_PREVIEW_DISPLAY_LAYER),
        PreviewRawDisplay,
    ));

    let display_material = preview_display_materials.add(PalettePreviewDisplayMaterial {
        source_image: pipeline.source_images[0].clone(),
        palette_texture: pipeline.palette_texture.clone(),
        lookup_texture: pipeline.lookup_texture.clone(),
        dither_a: Vec4::ZERO,
        dither_b: Vec4::ZERO,
    });
    commands.spawn((
        Mesh2d(meshes.add(Rectangle::default())),
        MeshMaterial2d(display_material.clone()),
        Transform::from_xyz(quantized_center.x, quantized_center.y, 0.0).with_scale(Vec3::new(
            display_size.x,
            display_size.y,
            1.0,
        )),
        RenderLayers::layer(GPU_PREVIEW_DISPLAY_LAYER),
    ));
    spawn_preview_pixel_debug_ui(commands);
    (
        display_material,
        PreviewPixelDebugState {
            raw_image: raw_output_images[0].clone(),
            palette_colors: pipeline.palette_colors.clone(),
            lookup_entries,
            raw_center,
            quantized_center,
            display_size,
            output:
                "Pixel debug: click either preview to compare the paired raw and quantized pixels"
                    .to_owned(),
            validation_pending: false,
            validation_warmup_updates: 0,
            validation_frames_checked: 0,
        },
    )
}

fn spawn_preview_pixel_debug_ui(commands: &mut Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(10),
            bottom: px(10),
            max_width: px(640),
            padding: UiRect::all(px(8)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.02, 0.025, 0.035, 0.86)),
        BorderColor::all(Color::srgb(0.22, 0.30, 0.42)),
        children![(
            Text::new(
                "Pixel debug: click either preview to compare the paired raw and quantized pixels"
            ),
            TextFont {
                font_size: 11.0,
                ..default()
            },
            TextColor(Color::srgb(0.86, 0.92, 0.98)),
            PreviewPixelDebugText,
        )],
    ));
}

fn spawn_readback_entities(commands: &mut Commands, count: usize) -> Vec<Entity> {
    (0..count)
        .map(|_| {
            commands
                .spawn_empty()
                .observe(crate::capture::queue_readback_frame)
                .id()
        })
        .collect()
}

pub(crate) fn handle_preview_pixel_debug_clicks(
    config: Res<AppConfig>,
    buttons: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<PreviewDisplayCamera>>,
    debug_state: Option<ResMut<PreviewPixelDebugState>>,
    mut commands: Commands,
) {
    if config.window_mode != WindowMode::Preview || !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(debug_state) = debug_state else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(cursor_position) = window.cursor_position() else {
        return;
    };
    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };
    let Ok(world_position) = camera.viewport_to_world_2d(camera_transform, cursor_position) else {
        return;
    };

    let Some((_source, x, y)) = preview_sample_from_world(
        world_position,
        &debug_state,
        config.stream_width,
        config.stream_height,
    ) else {
        return;
    };
    let raw_image = debug_state.raw_image.clone();

    commands
        .spawn(PreviewPixelDebugReadback {
            source: PreviewPixelSource::Raw,
            x,
            y,
            width: config.stream_width,
        })
        .observe(handle_preview_pixel_debug_readback)
        .insert(Readback::texture(raw_image));
}

fn preview_sample_from_world(
    world_position: Vec2,
    debug_state: &PreviewPixelDebugState,
    width: u32,
    height: u32,
) -> Option<(PreviewPixelSource, u32, u32)> {
    preview_sample_from_rect(
        world_position,
        debug_state.raw_center,
        debug_state.display_size,
        PreviewPixelSource::Raw,
        width,
        height,
    )
    .or_else(|| {
        preview_sample_from_rect(
            world_position,
            debug_state.quantized_center,
            debug_state.display_size,
            PreviewPixelSource::Quantized,
            width,
            height,
        )
    })
}

fn preview_sample_from_rect(
    world_position: Vec2,
    center: Vec2,
    size: Vec2,
    source: PreviewPixelSource,
    width: u32,
    height: u32,
) -> Option<(PreviewPixelSource, u32, u32)> {
    let left = center.x - size.x * 0.5;
    let right = center.x + size.x * 0.5;
    let bottom = center.y - size.y * 0.5;
    let top = center.y + size.y * 0.5;
    if world_position.x < left
        || world_position.x >= right
        || world_position.y < bottom
        || world_position.y >= top
    {
        return None;
    }

    let u = ((world_position.x - left) / size.x).clamp(0.0, 0.999_999);
    let v = ((top - world_position.y) / size.y).clamp(0.0, 0.999_999);
    let x = (u * width as f32).floor() as u32;
    let y = (v * height as f32).floor() as u32;
    Some((source, x, y))
}

fn handle_preview_pixel_debug_readback(
    event: On<ReadbackComplete>,
    mut commands: Commands,
    readbacks: Query<&PreviewPixelDebugReadback>,
    mut debug_state: Option<ResMut<PreviewPixelDebugState>>,
) {
    let Ok(readback) = readbacks.get(event.entity) else {
        commands.entity(event.entity).despawn();
        return;
    };
    let Some(debug_state) = debug_state.as_deref_mut() else {
        commands.entity(event.entity).despawn();
        return;
    };

    if readback.source == PreviewPixelSource::Raw {
        let raw = preview_raw_pixel_debug(
            readback,
            &event.data,
            &debug_state.lookup_entries,
            &debug_state.palette_colors,
        );
        let quantized = preview_quantized_pixel_debug_from_raw(&raw);
        debug_state.output = preview_pixel_pair_text(&raw, &quantized);
    }
    commands.entity(event.entity).despawn();
}

pub(crate) fn request_preview_palette_validation(
    config: Res<AppConfig>,
    debug_state: Option<ResMut<PreviewPixelDebugState>>,
    mut commands: Commands,
) {
    if config.window_mode != WindowMode::Preview {
        return;
    }
    let Some(mut debug_state) = debug_state else {
        return;
    };
    if debug_state.validation_pending || debug_state.validation_frames_checked >= 120 {
        return;
    }
    if debug_state.validation_warmup_updates < 60 {
        debug_state.validation_warmup_updates += 1;
        return;
    }

    debug_state.validation_pending = true;
    let raw_image = debug_state.raw_image.clone();
    commands
        .spawn(PreviewPaletteValidationReadback {
            source: PreviewPixelSource::Raw,
            width: config.stream_width,
            height: config.stream_height,
        })
        .observe(handle_preview_palette_validation_readback)
        .insert(Readback::texture(raw_image));
}

fn handle_preview_palette_validation_readback(
    event: On<ReadbackComplete>,
    mut commands: Commands,
    readbacks: Query<&PreviewPaletteValidationReadback>,
    mut debug_state: Option<ResMut<PreviewPixelDebugState>>,
) {
    let Ok(readback) = readbacks.get(event.entity) else {
        commands.entity(event.entity).despawn();
        return;
    };
    let Some(debug_state) = debug_state.as_deref_mut() else {
        commands.entity(event.entity).despawn();
        return;
    };

    if readback.source == PreviewPixelSource::Raw {
        debug_state.validation_pending = false;
        debug_state.validation_frames_checked =
            debug_state.validation_frames_checked.saturating_add(1);
        let report = validate_preview_palette_frame(
            &event.data,
            readback.width,
            readback.height,
            &debug_state.lookup_entries,
            &debug_state.palette_colors,
            debug_state.validation_frames_checked,
        );
        if let Some(report) = report {
            eprintln!("{report}");
            debug_state.output = report;
            debug_state.validation_frames_checked = 120;
        } else if debug_state.validation_frames_checked == 120 {
            let report = "automatic preview palette validation: 120 full frames matched".to_owned();
            eprintln!("{report}");
            debug_state.output = report;
        }
    }
    commands.entity(event.entity).despawn();
}

fn validate_preview_palette_frame(
    raw: &[u8],
    width: u32,
    height: u32,
    lookup_entries: &[u8],
    palette_colors: &[[u8; 4]],
    frame_number: u32,
) -> Option<String> {
    let raw_row_bytes = width as usize * 4;
    let raw_aligned_row_bytes =
        bevy::render::renderer::RenderDevice::align_copy_bytes_per_row(raw_row_bytes);

    for y in 0..height as usize {
        for x in 0..width as usize {
            let raw_offset = y * raw_aligned_row_bytes + x * 4;
            if raw_offset + 3 >= raw.len() {
                return Some(format!(
                    "automatic preview palette validation frame {frame_number}\nreadback out of range at ({x}, {y})"
                ));
            }

            let b = raw[raw_offset];
            let g = raw[raw_offset + 1];
            let r = raw[raw_offset + 2];
            let a = raw[raw_offset + 3];
            let lookup_key = (usize::from(r) << 16) | (usize::from(g) << 8) | usize::from(b);
            let expected_index = lookup_entries.get(lookup_key).copied().unwrap_or(0);
            if palette_colors.get(expected_index as usize).is_none() {
                return Some(format!(
                    "automatic preview palette validation frame {frame_number} [MISMATCH]\nraw preview ({x}, {y})\nraw BGRA: {b}, {g}, {r}, {a}\nraw RGB: #{r:02X}{g:02X}{b:02X}\nlookup RGB key: {lookup_key}\nlookup index {expected_index} is outside the loaded palette"
                ));
            }
        }
    }

    None
}

fn preview_raw_pixel_debug(
    readback: &PreviewPixelDebugReadback,
    data: &[u8],
    lookup_entries: &[u8],
    palette_colors: &[[u8; 4]],
) -> RawPixelDebug {
    let row_bytes = readback.width as usize * 4;
    let aligned_row_bytes =
        bevy::render::renderer::RenderDevice::align_copy_bytes_per_row(row_bytes);
    let offset = readback.y as usize * aligned_row_bytes + readback.x as usize * 4;
    if offset + 3 >= data.len() {
        return RawPixelDebug::out_of_range(readback.x, readback.y);
    }
    let b = data[offset];
    let g = data[offset + 1];
    let r = data[offset + 2];
    let a = data[offset + 3];
    let lookup_key = (usize::from(r) << 16) | (usize::from(g) << 8) | usize::from(b);
    let expected_index = lookup_entries.get(lookup_key).copied();
    let expected_color = expected_index
        .and_then(|index| palette_colors.get(index as usize).copied())
        .unwrap_or([0, 0, 0, 255]);
    RawPixelDebug {
        x: readback.x,
        y: readback.y,
        b,
        g,
        r,
        a,
        lookup_key,
        expected_index,
        expected_color,
    }
}

impl RawPixelDebug {
    fn out_of_range(x: u32, y: u32) -> Self {
        Self {
            x,
            y,
            b: 0,
            g: 0,
            r: 0,
            a: 0,
            lookup_key: 0,
            expected_index: None,
            expected_color: [0, 0, 0, 255],
        }
    }
}

fn preview_quantized_pixel_debug_from_raw(raw: &RawPixelDebug) -> QuantizedPixelDebug {
    let palette_index = raw.expected_index.unwrap_or(0);
    QuantizedPixelDebug {
        x: raw.x,
        y: raw.y,
        palette_index,
        color: raw.expected_color,
    }
}

fn preview_pixel_pair_text(raw: &RawPixelDebug, quantized: &QuantizedPixelDebug) -> String {
    let expected_index = raw
        .expected_index
        .map(|index| index.to_string())
        .unwrap_or_else(|| "out of range".to_owned());
    let match_text = if raw.expected_index == Some(quantized.palette_index) {
        "MATCH"
    } else {
        "MISMATCH"
    };
    format!(
        "paired preview ({}, {}) -> ({}, {}) [{match_text}]\nraw BGRA: {}, {}, {}, {}\nraw RGB: #{:02X}{:02X}{:02X}\nlookup RGB key: {}\nexpected index: {}\nexpected RGBA: #{:02X}{:02X}{:02X}{:02X}\nactual index: {}\nactual RGBA: #{:02X}{:02X}{:02X}{:02X}",
        raw.x,
        raw.y,
        quantized.x,
        quantized.y,
        raw.b,
        raw.g,
        raw.r,
        raw.a,
        raw.r,
        raw.g,
        raw.b,
        raw.lookup_key,
        expected_index,
        raw.expected_color[0],
        raw.expected_color[1],
        raw.expected_color[2],
        raw.expected_color[3],
        quantized.palette_index,
        quantized.color[0],
        quantized.color[1],
        quantized.color[2],
        quantized.color[3],
    )
}

pub(crate) fn update_preview_pixel_debug_text(
    debug_state: Option<Res<PreviewPixelDebugState>>,
    mut text_query: Query<&mut Text, With<PreviewPixelDebugText>>,
) {
    let Some(debug_state) = debug_state else {
        return;
    };
    let Ok(mut text) = text_query.single_mut() else {
        return;
    };
    if debug_state.is_changed() {
        text.0.clone_from(&debug_state.output);
    }
}

fn spawn_stats_window(
    commands: &mut Commands,
    custom_host: bool,
    window_layout: &DirectStreamWindowLayout,
) {
    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                padding: UiRect {
                    left: px(10),
                    right: px(10.0 + window_layout.right_panel_width),
                    top: px(10),
                    bottom: px(10),
                },
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                justify_content: JustifyContent::FlexStart,
                align_items: AlignItems::FlexStart,
                ..default()
            },
            BackgroundColor(Color::srgb(0.02, 0.025, 0.035)),
        ))
        .with_child((
            Text::new(initial_stats_text(custom_host)),
            TextFont {
                font_size: 10.0,
                ..default()
            },
            TextColor(Color::srgb(0.86, 0.92, 0.98)),
            StatsText,
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("custom host"),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(Color::srgb(0.64, 0.72, 0.80)),
            ));
            parent
                .spawn((Node {
                    width: percent(100),
                    height: px(20),
                    column_gap: px(6),
                    ..default()
                },))
                .with_children(|row| {
                    row.spawn(compact_input_box(
                        "width",
                        CustomWidthInputBox,
                        CustomWidthInputText,
                    ));
                    row.spawn(compact_input_box(
                        "height",
                        CustomHeightInputBox,
                        CustomHeightInputText,
                    ));
                    row.spawn(compact_input_box(
                        "fps",
                        CustomFpsInputBox,
                        CustomFpsInputText,
                    ));
                });
        })
        .with_child((
            Node {
                width: percent(100),
                height: px(24),
                column_gap: px(6),
                ..default()
            },
            children![
                stream_button("Start", StartStreamButton, Color::srgb(0.05, 0.20, 0.13)),
                stream_button("End", StopStreamButton, Color::srgb(0.21, 0.06, 0.07)),
                stream_button("Open", OpenStreamButton, Color::srgb(0.07, 0.10, 0.19)),
                stream_button("Purge Chat", PurgeChatButton, Color::srgb(0.17, 0.10, 0.04)),
            ],
        ))
        .with_child((
            Text::new("stream control: idle - Ready"),
            TextFont {
                font_size: 10.0,
                ..default()
            },
            TextColor(Color::srgb(0.70, 0.78, 0.86)),
            StreamControlStatusText,
        ));
}

fn initial_stats_text(custom_host: bool) -> String {
    let mode = if custom_host {
        "custom host stats"
    } else {
        "stats"
    };
    let endpoint = if custom_host {
        "http://127.0.0.1:8080".to_owned()
    } else {
        format!("http://{WEB_ADDR}")
    };
    format!(
        "Direct Stream Game\n{}\n{}\n{}",
        stat_line("mode", mode),
        stat_line(
            "stream",
            &format!("{STREAM_WIDTH}x{STREAM_HEIGHT} @ {STREAM_FPS} fps")
        ),
        stat_line("browser", &endpoint),
    )
}

fn compact_input_box<T: Component, U: Component>(
    placeholder: &'static str,
    box_marker: T,
    text_marker: U,
) -> impl Bundle {
    (
        Button,
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            height: px(20),
            padding: UiRect::horizontal(px(6)),
            align_items: AlignItems::Center,
            ..default()
        },
        BackgroundColor(Color::srgb(0.045, 0.055, 0.07)),
        BorderColor::all(Color::srgb(0.16, 0.22, 0.30)),
        box_marker,
        children![(
            Text::new(placeholder),
            TextFont {
                font_size: 10.0,
                ..default()
            },
            TextColor(Color::srgb(0.86, 0.92, 0.98)),
            text_marker,
        )],
    )
}

fn stream_button<T: Component>(label: &'static str, marker: T, color: Color) -> impl Bundle {
    (
        Button,
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            height: px(24),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(color),
        marker,
        children![(
            Text::new(label),
            TextFont {
                font_size: 11.0,
                ..default()
            },
            TextColor(Color::srgb(0.92, 0.96, 1.0)),
        )],
    )
}

pub(crate) fn update_stats_window(
    config: Res<AppConfig>,
    target: Res<DirectStreamTarget>,
    stats: Res<SharedStats>,
    mut query: Query<&mut Text, With<StatsText>>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };

    if let Ok(stats) = stats.0.lock() {
        text.0 = if config.custom_host {
            custom_host_stats_text(&stats, &target)
        } else {
            preview_stats_text(&stats, &target)
        };
    }
}

fn custom_host_stats_text(
    stats: &crate::stats::StreamStats,
    target: &DirectStreamTarget,
) -> String {
    [
        "Direct Stream Game".to_owned(),
        stat_line("mode", "custom host stats"),
        stat_line(
            "stream",
            &format!("{}x{} @ {} fps", target.width, target.height, target.fps),
        ),
        stat_line("browser", &format!("http://{WEB_ADDR}")),
        String::new(),
        "capture".to_owned(),
        stat_line("frames captured", &stats.frames_captured.to_string()),
        stat_line("frames read", &stats.frames_read.to_string()),
        stat_line("frames encoded", &stats.frames_encoded.to_string()),
        stat_line("frames dropped", &stats.frames_dropped.to_string()),
        stat_line("sent fps", &format!("{:.2} fps", stats.custom_actual_fps)),
        stat_line("app fps", &format!("{:.2} fps", stats.custom_app_fps)),
        stat_line("batch size", &stats.custom_batch_size.to_string()),
        stat_line(
            "batch buffered",
            &stats.custom_batch_buffered_frames.to_string(),
        ),
        stat_line(
            "pending readbacks",
            &stats.custom_pending_readbacks.to_string(),
        ),
        stat_line(
            "batch latency",
            &format!("{:.2} ms", stats.custom_batch_latency_ms),
        ),
        stat_line(
            "http batch",
            &format!(
                "{} last / {:.1} avg",
                stats.custom_http_batch_last_frames, stats.custom_http_batch_avg_frames
            ),
        ),
        stat_line(
            "readback wait",
            &format!(
                "{:.2} ms last / {:.2} ms avg",
                stats.custom_readback_wait_last_ms, stats.custom_readback_wait_avg_ms
            ),
        ),
        stat_line(
            "readback cpu",
            &format!(
                "{:.2} ms last / {:.2} ms avg",
                stats.custom_readback_cpu_last_ms, stats.custom_readback_cpu_avg_ms
            ),
        ),
        stat_line(
            "encode",
            &format!(
                "{:.2} ms last / {:.2} ms avg",
                stats.custom_encode_last_ms, stats.custom_encode_avg_ms
            ),
        ),
        stat_line(
            "record write",
            &format!(
                "{:.2} ms last / {:.2} ms avg",
                stats.custom_record_last_ms, stats.custom_record_avg_ms
            ),
        ),
        stat_line(
            "publish",
            &format!(
                "{:.2} ms last / {:.2} ms avg",
                stats.custom_publish_last_ms, stats.custom_publish_avg_ms
            ),
        ),
        stat_line(
            "pipeline total",
            &format!(
                "{:.2} ms last / {:.2} ms avg",
                stats.custom_pipeline_last_ms, stats.custom_pipeline_avg_ms
            ),
        ),
        String::new(),
        "custom host".to_owned(),
        stat_line("palette mode", "ipsmap LUT"),
        stat_line("stage", stats.custom_stage),
        stat_line("error", &stats.custom_last_error),
        stat_line("packets sent", &stats.custom_frames_sent.to_string()),
        stat_line(
            "packet types",
            &format!(
                "key {} / delta {}",
                stats.custom_keyframes_sent, stats.custom_delta_frames_sent
            ),
        ),
        stat_line(
            "tile modes",
            &format!(
                "raw {} solid {} rle {} span {} xor {} cached {} skipped {}",
                stats.custom_raw_tiles_sent,
                stats.custom_solid_tiles_sent,
                stats.custom_rle_tiles_sent,
                stats.custom_span_tiles_sent,
                stats.custom_xor_tiles_sent,
                stats.custom_cached_tiles_sent,
                stats.custom_skipped_tiles
            ),
        ),
        stat_line("packet drops", &stats.custom_frames_dropped.to_string()),
        stat_line(
            "queue full drops",
            &stats.custom_queue_full_drops.to_string(),
        ),
        stat_line(
            "sender waits",
            &format!(
                "{} timeouts / {} wakeups",
                stats.custom_sender_wait_timeouts, stats.custom_sender_wait_wakeups
            ),
        ),
        stat_line("bytes sent", &stats.custom_bytes_sent.to_string()),
        stat_line(
            "audio packets",
            &stats.custom_audio_packets_sent.to_string(),
        ),
        stat_line("audio bytes", &stats.custom_audio_bytes_sent.to_string()),
        stat_line(
            "audio delay",
            &format!("{} ms", stats.custom_audio_delay_ms),
        ),
        stat_line(
            "latest packet",
            &format!("{} bytes", stats.latest_frame_bytes),
        ),
        stat_line("recording", &stats.custom_recording_path),
        stat_line("clients", &stats.stream_clients.to_string()),
        stat_line("page requests", &stats.preview_requests.to_string()),
    ]
    .join("\n")
}

fn preview_stats_text(stats: &crate::stats::StreamStats, target: &DirectStreamTarget) -> String {
    [
        "Direct Stream Game".to_owned(),
        stat_line("mode", "preview stats"),
        stat_line(
            "stream",
            &format!("{}x{} @ {} fps", target.width, target.height, target.fps),
        ),
        stat_line("local preview", &format!("http://{WEB_ADDR}")),
        String::new(),
        "capture".to_owned(),
        stat_line("frames captured", &stats.frames_captured.to_string()),
        stat_line("frames read", &stats.frames_read.to_string()),
        stat_line("frames encoded", &stats.frames_encoded.to_string()),
        stat_line("frames dropped", &stats.frames_dropped.to_string()),
        String::new(),
        "preview".to_owned(),
        stat_line("preview drops", &stats.preview_frames_dropped.to_string()),
        stat_line("clients", &stats.stream_clients.to_string()),
        stat_line("requests", &stats.preview_requests.to_string()),
        stat_line(
            "latest frame",
            &format!("{} bytes", stats.latest_frame_bytes),
        ),
    ]
    .join("\n")
}

fn stat_line(label: &str, value: &str) -> String {
    format!("{label:>16}: {value}")
}

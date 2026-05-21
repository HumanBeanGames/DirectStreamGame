use crate::{
    config::{AppConfig, WindowMode, effective_custom_batch_size},
    constants::{STREAM_FPS, STREAM_HEIGHT, STREAM_WIDTH, WEB_ADDR},
    gpu_palette::{PaletteMaterial, make_stream_source_image, spawn_custom_host_pipeline},
    palette::load_palette_runtime,
    public_types::DirectStreamTarget,
    stats::{SharedStats, StatsText},
    stream_control::{
        ChatBotUsernameInputBox, ChatBotUsernameInputText, ChatOauthTokenInputBox,
        ChatOauthTokenInputText, CustomFpsInputBox, CustomFpsInputText, CustomHeightInputBox,
        CustomHeightInputText, CustomWidthInputBox, CustomWidthInputText, OpenTwitchStreamButton,
        PaletteBiasSlider, PaletteBiasSliderFill, PaletteBiasSliderValueText, PurgeChatButton,
        StartStreamButton, StopStreamButton, StreamControl, StreamControlStatusText,
        StreamKeyInputBox, StreamKeyInputText,
    },
};
use bevy::{
    camera::{RenderTarget, visibility::RenderLayers},
    prelude::*,
    ui::RelativeCursorPosition,
};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const MAX_IN_FLIGHT_READBACKS: usize = 30;

pub(crate) struct PendingReadback {
    pub(crate) requested_at: Instant,
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
    pub(crate) frame_interval: Duration,
    pub(crate) frame_accumulator: Duration,
    pub(crate) frame_due: bool,
    pub(crate) max_in_flight: usize,
    pub(crate) pending_requests: HashMap<Entity, PendingReadback>,
    pub(crate) batch_size: usize,
    pub(crate) batch_started_at: Option<Instant>,
    pub(crate) batch_in_progress: bool,
    pub(crate) textures_rendered_in_batch: usize,
    pub(crate) frame_waiting_for_render: Option<RenderedBatchFrame>,
    pub(crate) rendered_batch_frames: Vec<RenderedBatchFrame>,
}

pub(crate) fn setup_direct_stream_scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut palette_materials: ResMut<Assets<PaletteMaterial>>,
    config: Res<AppConfig>,
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

    commands.spawn(Camera2d);
    match config.window_mode {
        WindowMode::Preview => {
            commands.spawn((
                Sprite::from_image(stream_image.clone()),
                Transform::from_scale(Vec3::ONE),
            ));
        }
        WindowMode::Stats => {
            spawn_stats_window(&mut commands, config.custom_host, config.prebaked_palette)
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

    if config.custom_host {
        let (palette_colors, palette_bias) = load_palette_runtime(&config.palette_config_path);
        let batch_size =
            effective_custom_batch_size(config.custom_host_batch_size, config.stream_fps);
        let pipeline = spawn_custom_host_pipeline(
            &mut commands,
            &mut images,
            &mut meshes,
            &mut palette_materials,
            config.stream_width,
            config.stream_height,
            stream_image.clone(),
            &palette_colors,
            palette_bias,
            &mut target,
            batch_size,
        );
        let pipeline_clone = pipeline.clone();
        commands.insert_resource(pipeline);
        commands.insert_resource(StreamReadback {
            images: pipeline_clone.output_images.clone(),
            frame_interval: Duration::from_secs_f64(1.0 / config.stream_fps as f64),
            frame_accumulator: Duration::ZERO,
            frame_due: false,
            max_in_flight: MAX_IN_FLIGHT_READBACKS,
            pending_requests: HashMap::default(),
            batch_size,
            batch_started_at: None,
            batch_in_progress: false,
            textures_rendered_in_batch: 0,
            frame_waiting_for_render: None,
            rendered_batch_frames: Vec::with_capacity(batch_size),
        });
    }
    commands.insert_resource(target);
}

fn spawn_stats_window(commands: &mut Commands, custom_host: bool, prebaked_palette: bool) {
    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                padding: UiRect::all(px(10)),
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
            if custom_host {
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
                parent.spawn((
                    Text::new(if prebaked_palette {
                        "palette match bias (prebaked)"
                    } else {
                        "palette match bias"
                    }),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(if prebaked_palette {
                        Color::srgb(0.38, 0.43, 0.50)
                    } else {
                        Color::srgb(0.64, 0.72, 0.80)
                    }),
                ));
                parent.spawn(bias_slider_row(
                    "value",
                    PaletteBiasSlider::Lightness,
                    33.3,
                    prebaked_palette,
                ));
                parent.spawn(bias_slider_row(
                    "chroma",
                    PaletteBiasSlider::Chroma,
                    33.3,
                    prebaked_palette,
                ));
                parent.spawn(bias_slider_row(
                    "hue",
                    PaletteBiasSlider::Hue,
                    33.4,
                    prebaked_palette,
                ));
            } else {
                parent.spawn((
                    Text::new("stream key"),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.64, 0.72, 0.80)),
                ));
                parent.spawn((
                    Button,
                    Node {
                        width: percent(100),
                        height: px(22),
                        padding: UiRect::horizontal(px(6)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.045, 0.055, 0.07)),
                    BorderColor::all(Color::srgb(0.16, 0.22, 0.30)),
                    StreamKeyInputBox,
                    children![(
                        Text::new("paste stream key"),
                        TextFont {
                            font_size: 11.0,
                            ..default()
                        },
                        TextColor(Color::srgb(0.86, 0.92, 0.98)),
                        StreamKeyInputText,
                    )],
                ));
                parent.spawn((
                    Text::new("chat bot"),
                    TextFont {
                        font_size: 10.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.64, 0.72, 0.80)),
                ));
                parent.spawn(input_box(
                    "bot username",
                    ChatBotUsernameInputBox,
                    ChatBotUsernameInputText,
                ));
                parent.spawn(input_box(
                    "chat oauth token",
                    ChatOauthTokenInputBox,
                    ChatOauthTokenInputText,
                ));
            }
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
                stream_button(
                    "Open",
                    OpenTwitchStreamButton,
                    Color::srgb(0.07, 0.10, 0.19)
                ),
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

fn input_box<T: Component, U: Component>(
    placeholder: &'static str,
    box_marker: T,
    text_marker: U,
) -> impl Bundle {
    (
        Button,
        Node {
            width: percent(100),
            height: px(18),
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

fn bias_slider_row(
    label: &'static str,
    slider: PaletteBiasSlider,
    initial_percent: f32,
    disabled: bool,
) -> impl Bundle {
    (
        Node {
            width: percent(100),
            height: px(18),
            column_gap: px(6),
            align_items: AlignItems::Center,
            ..default()
        },
        children![
            (
                Text::new(label),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(if disabled {
                    Color::srgb(0.42, 0.47, 0.55)
                } else {
                    Color::srgb(0.78, 0.85, 0.92)
                }),
                Node {
                    width: px(44),
                    ..default()
                },
            ),
            (
                Button,
                Node {
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    height: px(12),
                    position_type: PositionType::Relative,
                    ..default()
                },
                BackgroundColor(if disabled {
                    Color::srgb(0.035, 0.040, 0.050)
                } else {
                    Color::srgb(0.045, 0.055, 0.07)
                }),
                RelativeCursorPosition::default(),
                slider,
                children![(
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(0),
                        width: percent(initial_percent),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(if disabled {
                        Color::srgb(0.20, 0.25, 0.32)
                    } else {
                        Color::srgb(0.24, 0.48, 0.82)
                    }),
                    PaletteBiasSliderFill(slider),
                )],
            ),
            (
                Text::new(format!("{:.3}", initial_percent / 100.0)),
                TextFont {
                    font_size: 10.0,
                    ..default()
                },
                TextColor(if disabled {
                    Color::srgb(0.42, 0.47, 0.55)
                } else {
                    Color::srgb(0.86, 0.92, 0.98)
                }),
                Node {
                    width: px(42),
                    ..default()
                },
                PaletteBiasSliderValueText(slider),
            ),
        ],
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
    stream_control: Res<StreamControl>,
    target: Res<DirectStreamTarget>,
    stats: Res<SharedStats>,
    mut query: Query<&mut Text, With<StatsText>>,
) {
    let Ok(mut text) = query.single_mut() else {
        return;
    };

    if let Ok(mut stats) = stats.0.lock() {
        if stream_control.is_streaming() {
            stats.refresh_twitch_kbps(Instant::now());
        } else {
            stats.twitch_kbps = 0.0;
        }

        text.0 = if config.custom_host {
            custom_host_stats_text(&stats, &target, &stream_control)
        } else if config.window_mode == WindowMode::Stats {
            twitch_stats_text(&stats, &target)
        } else {
            preview_stats_text(&stats, &target)
        };
    }
}

fn custom_host_stats_text(
    stats: &crate::stats::StreamStats,
    target: &DirectStreamTarget,
    stream_control: &StreamControl,
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
        stat_line(
            "palette mode",
            if stream_control.prebaked_palette {
                "prebaked LUT"
            } else {
                "live matching"
            },
        ),
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
            "latest packet",
            &format!("{} bytes", stats.latest_frame_bytes),
        ),
        stat_line("recording", &stats.custom_recording_path),
        stat_line("clients", &stats.stream_clients.to_string()),
        stat_line("page requests", &stats.preview_requests.to_string()),
        stat_line(
            "bias L/C/H",
            &format!(
                "{:.3} / {:.3} / {:.3}",
                stream_control.palette_bias.lightness,
                stream_control.palette_bias.chroma,
                stream_control.palette_bias.hue
            ),
        ),
    ]
    .join("\n")
}

fn twitch_stats_text(stats: &crate::stats::StreamStats, target: &DirectStreamTarget) -> String {
    [
        "Direct Stream Game".to_owned(),
        stat_line("mode", "twitch stats"),
        stat_line(
            "stream",
            &format!("{}x{} @ {} fps", target.width, target.height, target.fps),
        ),
        String::new(),
        "capture".to_owned(),
        stat_line("frames captured", &stats.frames_captured.to_string()),
        stat_line("frames read", &stats.frames_read.to_string()),
        stat_line("frames dropped", &stats.frames_dropped.to_string()),
        String::new(),
        "twitch".to_owned(),
        stat_line("stage", &stats.twitch_stage),
        stat_line("frames sent", &stats.twitch_frames_sent.to_string()),
        stat_line("frame drops", &stats.twitch_frames_dropped.to_string()),
        stat_line("video packets", &stats.twitch_video_packets.to_string()),
        stat_line("audio packets", &stats.twitch_audio_packets.to_string()),
        stat_line("bytes sent", &stats.twitch_bytes_sent.to_string()),
        stat_line("bitrate", &format!("{:.1} kbps", stats.twitch_kbps)),
        stat_line("errors", &stats.twitch_errors.to_string()),
        stat_line("last error", &stats.twitch_last_error),
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

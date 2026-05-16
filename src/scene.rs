use crate::{
    config::{AppConfig, WindowMode},
    constants::{STREAM_FPS, STREAM_HEIGHT, STREAM_WIDTH, WEB_ADDR},
    public_types::DirectStreamTarget,
    stats::{SharedStats, StatsText},
    stream_control::{
        ChatBotUsernameInputBox, ChatBotUsernameInputText, ChatOauthTokenInputBox,
        ChatOauthTokenInputText, OpenTwitchStreamButton, StartStreamButton, StopStreamButton,
        StreamControl, StreamControlStatusText, StreamKeyInputBox, StreamKeyInputText,
    },
};
use bevy::{
    asset::RenderAssetUsages,
    camera::RenderTarget,
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
};
use std::time::Instant;

#[derive(Resource)]
pub(crate) struct StreamReadback {
    pub(crate) image: Handle<Image>,
    pub(crate) timer: Timer,
    pub(crate) in_flight: bool,
}

pub(crate) fn setup_direct_stream_scene(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    config: Res<AppConfig>,
) {
    let size = Extent3d {
        width: STREAM_WIDTH,
        height: STREAM_HEIGHT,
        depth_or_array_layers: 1,
    };

    let mut stream_image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::default(),
    );
    stream_image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
        | TextureUsages::COPY_DST
        | TextureUsages::COPY_SRC
        | TextureUsages::RENDER_ATTACHMENT;

    let stream_image = images.add(stream_image);

    let stream_camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.04, 0.05, 0.07)),
                ..default()
            },
            RenderTarget::Image(stream_image.clone().into()),
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
        WindowMode::Stats => spawn_stats_window(&mut commands),
    }

    commands.insert_resource(StreamReadback {
        image: stream_image.clone(),
        timer: Timer::from_seconds(1.0 / STREAM_FPS as f32, TimerMode::Repeating),
        in_flight: false,
    });
    commands.insert_resource(DirectStreamTarget {
        camera: stream_camera,
        image: stream_image,
        width: STREAM_WIDTH,
        height: STREAM_HEIGHT,
        fps: STREAM_FPS,
    });
}

fn spawn_stats_window(commands: &mut Commands) {
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
            Text::new(initial_stats_text()),
            TextFont {
                font_size: 10.0,
                ..default()
            },
            TextColor(Color::srgb(0.86, 0.92, 0.98)),
            StatsText,
        ))
        .with_child((
            Text::new("stream key"),
            TextFont {
                font_size: 10.0,
                ..default()
            },
            TextColor(Color::srgb(0.64, 0.72, 0.80)),
        ))
        .with_child((
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
        ))
        .with_child((
            Text::new("chat bot"),
            TextFont {
                font_size: 10.0,
                ..default()
            },
            TextColor(Color::srgb(0.64, 0.72, 0.80)),
        ))
        .with_child(input_box(
            "bot username",
            ChatBotUsernameInputBox,
            ChatBotUsernameInputText,
        ))
        .with_child(input_box(
            "chat oauth token",
            ChatOauthTokenInputBox,
            ChatOauthTokenInputText,
        ))
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

fn initial_stats_text() -> String {
    format!(
        "Direct Stream Game\n{}\n{}\n{}",
        stat_line("mode", "stats"),
        stat_line(
            "stream",
            &format!("{STREAM_WIDTH}x{STREAM_HEIGHT} @ {STREAM_FPS} fps")
        ),
        stat_line("local preview", &format!("http://{WEB_ADDR}")),
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

        text.0 = if config.window_mode == WindowMode::Stats {
            twitch_stats_text(&stats)
        } else {
            preview_stats_text(&stats)
        };
    }
}

fn twitch_stats_text(stats: &crate::stats::StreamStats) -> String {
    [
        "Direct Stream Game".to_owned(),
        stat_line("mode", "twitch stats"),
        stat_line(
            "stream",
            &format!("{STREAM_WIDTH}x{STREAM_HEIGHT} @ {STREAM_FPS} fps"),
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

fn preview_stats_text(stats: &crate::stats::StreamStats) -> String {
    [
        "Direct Stream Game".to_owned(),
        stat_line("mode", "preview stats"),
        stat_line(
            "stream",
            &format!("{STREAM_WIDTH}x{STREAM_HEIGHT} @ {STREAM_FPS} fps"),
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

use crate::{
    DirectStreamPlugin,
    audio::DirectStreamAudioTarget,
    config::{AppConfig, WindowMode},
    constants::{
        STATS_WINDOW_HEIGHT, STATS_WINDOW_WIDTH, STREAM_HEIGHT, STREAM_WIDTH, WINDOW_TITLE,
    },
    frames::{EncodedFrameHub, RawFrameSenders},
    preview::start_preview_encoder,
    stats::SharedStats,
    stream_control::StreamControl,
    web::start_local_web_server,
};
use bevy::{audio::AudioPlugin, prelude::*};

pub fn direct_stream_app() -> App {
    let config = AppConfig::from_args();
    let frame_hub = EncodedFrameHub::new();
    let audio_target = DirectStreamAudioTarget::new();
    let stats = SharedStats::new();
    let (preview_sender, preview_receiver) = crossbeam_channel::bounded(2);
    let preview_enabled = config.window_mode == WindowMode::Preview;
    let stream_control =
        StreamControl::new(&config, preview_enabled.then_some(preview_sender.clone()));
    let window_resolution = match config.window_mode {
        WindowMode::Preview => (STREAM_WIDTH, STREAM_HEIGHT),
        WindowMode::Stats => (STATS_WINDOW_WIDTH, STATS_WINDOW_HEIGHT),
    };

    start_local_web_server(frame_hub.clone(), stats.clone());
    if preview_enabled {
        start_preview_encoder(preview_receiver, frame_hub.clone(), stats.clone());
    }

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.04, 0.05, 0.07)))
        .insert_resource(frame_hub)
        .insert_resource(audio_target)
        .insert_resource(stats.clone())
        .insert_resource(stream_control)
        .insert_resource(config)
        .insert_resource(RawFrameSenders {
            preview: preview_enabled.then_some(preview_sender),
            twitch: None,
            stats: stats.clone(),
        })
        .add_plugins(
            DefaultPlugins
                .build()
                .disable::<AudioPlugin>()
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: WINDOW_TITLE.to_owned(),
                        resolution: window_resolution.into(),
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(DirectStreamPlugin);
    app
}

pub fn run_with_game(configure_game: impl FnOnce(&mut App)) {
    let mut app = direct_stream_app();
    configure_game(&mut app);
    app.run();
}

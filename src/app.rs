use crate::{
    DirectStreamPlugin,
    audio::{CustomAudioPacketHub, DirectStreamAudioTarget, start_custom_audio_packet_pump},
    chat::LocalChatHub,
    config::{AppConfig, WindowMode},
    constants::{
        STATS_WINDOW_HEIGHT, STATS_WINDOW_WIDTH, STREAM_HEIGHT, STREAM_WIDTH, WINDOW_TITLE,
    },
    frames::{EncodedFrameHub, RawFrameSenders},
    palette::{PaletteBias, PaletteFrameHub, SharedPaletteBias, start_palette_preview_encoder},
    palette_lut::load_palette_config,
    preview::start_preview_encoder,
    stats::SharedStats,
    stream_control::{CustomStreamState, StreamControl},
    web::{LocalStreamSource, start_local_web_server},
};
use bevy::{audio::AudioPlugin, prelude::*};

pub fn direct_stream_app() -> App {
    let config = AppConfig::from_args();
    let frame_hub = EncodedFrameHub::new();
    let palette_frame_hub = PaletteFrameHub::new();
    let audio_target = DirectStreamAudioTarget::new();
    let custom_audio_hub = CustomAudioPacketHub::new();
    let local_chat = LocalChatHub::default();
    let custom_stream_state = CustomStreamState::new();
    let stats = SharedStats::new();
    let palette_bias = SharedPaletteBias::new();
    if config.custom_host
        && let Ok(palette_config) = load_palette_config(&config.palette_config_path)
    {
        palette_bias.set(PaletteBias::from(palette_config.matching));
    }
    let (preview_sender, preview_receiver) = crossbeam_channel::bounded(2);
    let (custom_sender, custom_receiver) = crossbeam_channel::bounded(2);
    let preview_enabled = config.window_mode == WindowMode::Preview;
    let custom_host = config.custom_host;
    let stream_control = StreamControl::new(
        &config,
        preview_enabled.then_some(preview_sender.clone()),
        custom_host.then_some(custom_sender.clone()),
        custom_stream_state.clone(),
        palette_bias.clone(),
    );
    let window_resolution = match config.window_mode {
        WindowMode::Preview => (STREAM_WIDTH, STREAM_HEIGHT),
        WindowMode::Stats => (STATS_WINDOW_WIDTH, STATS_WINDOW_HEIGHT),
    };

    let web_source = if custom_host {
        LocalStreamSource::Palette {
            frames: palette_frame_hub.clone(),
            audio: custom_audio_hub.clone(),
            chat: local_chat.clone(),
            active: custom_stream_state.clone(),
        }
    } else {
        LocalStreamSource::Mjpeg(frame_hub.clone())
    };
    start_local_web_server(web_source, stats.clone());
    if custom_host {
        start_custom_audio_packet_pump(
            audio_target.clone(),
            custom_audio_hub,
            stats.clone(),
            custom_stream_state.clone(),
        );
        start_palette_preview_encoder(
            custom_receiver,
            palette_frame_hub.clone(),
            stats.clone(),
            palette_bias.clone(),
            custom_stream_state.clone(),
            config.palette_config_path.clone(),
            config.prebaked_palette,
        );
    } else if preview_enabled {
        start_preview_encoder(preview_receiver, frame_hub.clone(), stats.clone());
    }

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.04, 0.05, 0.07)))
        .insert_resource(frame_hub)
        .insert_resource(audio_target)
        .insert_resource(local_chat)
        .insert_resource(custom_stream_state)
        .insert_resource(stats.clone())
        .insert_resource(stream_control)
        .insert_resource(config)
        .insert_resource(RawFrameSenders {
            preview: preview_enabled.then_some(preview_sender),
            custom: None,
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

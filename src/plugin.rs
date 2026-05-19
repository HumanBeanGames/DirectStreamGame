use crate::{
    DirectStreamSet, PlayStreamSound,
    audio::{StreamAudioClip, StreamAudioMixer, collect_stream_audio_events, mix_stream_audio},
    capture::request_stream_readback,
    chat::{
        TwitchChatCommand, TwitchChatMessage, dispatch_twitch_chat_commands, poll_local_chat,
        poll_twitch_chat, start_twitch_chat_listener,
    },
    direct_text::DirectTextPlugin,
    scene::{setup_direct_stream_scene, update_stats_window},
    stream_control::{
        handle_palette_bias_sliders, handle_stream_control_interactions, handle_stream_key_typing,
        update_stream_control_ui,
    },
};
use bevy::prelude::*;

pub struct DirectStreamPlugin;

impl Plugin for DirectStreamPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<StreamAudioClip>()
            .init_resource::<StreamAudioMixer>()
            .add_plugins(DirectTextPlugin)
            .add_message::<PlayStreamSound>()
            .add_message::<TwitchChatMessage>()
            .add_message::<TwitchChatCommand>()
            .add_systems(
                Startup,
                (
                    setup_direct_stream_scene.in_set(DirectStreamSet::Setup),
                    start_twitch_chat_listener,
                ),
            )
            .add_systems(
                Update,
                (
                    collect_stream_audio_events,
                    (
                        poll_twitch_chat,
                        poll_local_chat,
                        dispatch_twitch_chat_commands,
                    )
                        .chain(),
                    mix_stream_audio,
                    request_stream_readback,
                    handle_stream_key_typing,
                    handle_stream_control_interactions,
                    handle_palette_bias_sliders,
                    update_stream_control_ui,
                    update_stats_window,
                ),
            );
    }
}

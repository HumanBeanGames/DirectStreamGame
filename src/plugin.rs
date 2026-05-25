use crate::{
    DirectStreamSet, PlayStreamSound,
    audio::{StreamAudioClip, StreamAudioMixer, collect_stream_audio_events, mix_stream_audio},
    capture::request_stream_readback,
    chat::{
        StreamChatCommand, StreamChatMessage, dispatch_stream_chat_commands,
        init_stream_chat_sender, poll_local_chat,
    },
    custom_host::{StreamPointerClick, poll_stream_pointer_clicks},
    direct_text::DirectTextPlugin,
    direct_world_sprite::DirectWorldSpritePlugin,
    gpu_palette::GpuPalettePlugin,
    scene::{setup_direct_stream_scene, update_stats_window},
    stream_control::{
        handle_palette_bias_sliders, handle_stream_input_box_interactions,
        handle_stream_key_typing, handle_stream_misc_button_interactions,
        handle_stream_start_interactions, handle_stream_stop_interactions,
        update_stream_control_ui,
    },
};
use bevy::prelude::*;

pub struct DirectStreamPlugin;

impl Plugin for DirectStreamPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<StreamAudioClip>()
            .init_resource::<StreamAudioMixer>()
            .add_plugins((GpuPalettePlugin, DirectWorldSpritePlugin, DirectTextPlugin))
            .add_message::<PlayStreamSound>()
            .add_message::<StreamChatMessage>()
            .add_message::<StreamChatCommand>()
            .add_message::<StreamPointerClick>()
            .add_systems(
                Startup,
                (
                    setup_direct_stream_scene.in_set(DirectStreamSet::Setup),
                    init_stream_chat_sender,
                ),
            )
            .add_systems(
                Update,
                (
                    collect_stream_audio_events,
                    (poll_local_chat, dispatch_stream_chat_commands).chain(),
                    mix_stream_audio,
                    poll_stream_pointer_clicks,
                    request_stream_readback,
                ),
            )
            .add_systems(
                Update,
                (
                    handle_stream_key_typing,
                    handle_stream_input_box_interactions,
                    handle_stream_start_interactions,
                    handle_stream_stop_interactions,
                    handle_stream_misc_button_interactions,
                    handle_palette_bias_sliders,
                ),
            )
            .add_systems(Update, (update_stream_control_ui, update_stats_window));
    }
}

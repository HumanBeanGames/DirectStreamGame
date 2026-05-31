use crate::{
    DirectStreamSet, PlayStreamSound,
    audio::{StreamAudioClip, StreamAudioMixer, collect_stream_audio_events, mix_stream_audio},
    capture::request_stream_readback,
    chat::{
        StreamChatCommand, StreamChatMessage, dispatch_stream_chat_commands,
        init_stream_chat_sender, poll_local_chat, sync_custom_host_viewer_name_resolver,
    },
    custom_host::{
        CustomHostPanelAction, StreamPointerClick, poll_custom_host_panel_actions,
        poll_stream_pointer_clicks,
    },
    direct_text::DirectTextPlugin,
    direct_world_sprite::DirectWorldSpritePlugin,
    gpu_palette::GpuPalettePlugin,
    scene::{setup_direct_stream_scene, update_stats_window},
    stream_control::{
        handle_direct_stream_start_requests, handle_direct_stream_stop_requests,
        handle_palette_bias_sliders, handle_stream_input_box_interactions,
        handle_stream_key_typing, handle_stream_misc_button_interactions,
        handle_stream_start_interactions, handle_stream_stop_interactions,
        update_stream_control_ui,
    },
    web::start_local_web_server_from_resources,
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
            .add_message::<CustomHostPanelAction>()
            .add_message::<crate::DirectStreamStartRequest>()
            .add_message::<crate::DirectStreamStopRequest>()
            .add_message::<crate::DirectStreamControlResult>()
            .add_systems(
                Startup,
                (
                    setup_direct_stream_scene.in_set(DirectStreamSet::Setup),
                    sync_custom_host_viewer_name_resolver,
                    start_local_web_server_from_resources,
                    init_stream_chat_sender,
                )
                    .chain(),
            )
            .add_systems(
                Update,
                (
                    collect_stream_audio_events,
                    sync_custom_host_viewer_name_resolver,
                    (poll_local_chat, dispatch_stream_chat_commands).chain(),
                    mix_stream_audio,
                    poll_stream_pointer_clicks,
                    poll_custom_host_panel_actions,
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
                    handle_direct_stream_start_requests,
                    handle_direct_stream_stop_requests,
                    handle_stream_misc_button_interactions,
                    handle_palette_bias_sliders,
                ),
            )
            .add_systems(Update, (update_stream_control_ui, update_stats_window));
    }
}

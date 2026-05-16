use bevy::prelude::*;
use direct_stream_game::TwitchCommandAppExt;

fn main() {
    direct_stream_game::direct_stream_app()
        .add_twitch_command("boing", direct_stream_game::handle_demo_boing_command)
        .add_systems(
            Startup,
            direct_stream_game::setup_demo_scene.after(direct_stream_game::DirectStreamSet::Setup),
        )
        .add_systems(
            Update,
            (
                direct_stream_game::pulse_hello_world_text,
                direct_stream_game::start_demo_music,
            ),
        )
        .run();
}

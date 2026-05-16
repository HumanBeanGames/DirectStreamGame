use crate::{
    DirectStreamSet, PlayStreamSound, StreamAudioClip, TwitchChatCommand, TwitchChatSender,
    TwitchCommandAppExt, app::direct_stream_app, public_types::DirectStreamTarget,
};
use bevy::{ecs::system::In, prelude::*};

const DEMO_MUSIC_PATH: &str = "music/Elijah_K - Iron.wav";
const DEMO_SFX_PATH: &str = "sfx/boing_x.wav";

#[derive(Component)]
pub struct HelloWorldText;

#[derive(Resource)]
pub struct DemoMusicClip(Handle<StreamAudioClip>);

#[derive(Resource)]
pub struct DemoSfxClip(Handle<StreamAudioClip>);

#[derive(Resource, Default)]
pub struct DemoMusicStarted(bool);

pub fn run_demo() {
    direct_stream_app()
        .add_twitch_command("boing", handle_demo_boing_command)
        .add_systems(Startup, setup_demo_scene.after(DirectStreamSet::Setup))
        .add_systems(Update, (pulse_hello_world_text, start_demo_music))
        .run();
}

pub fn setup_demo_scene(
    mut commands: Commands,
    target: Res<DirectStreamTarget>,
    mut clips: ResMut<Assets<StreamAudioClip>>,
) {
    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.04, 0.05, 0.07)),
            UiTargetCamera(target.camera),
        ))
        .with_child((
            Text::new("HelloWorld"),
            TextFont {
                font_size: 40.0,
                ..default()
            },
            TextColor(Color::srgb(0.92, 0.96, 1.0)),
            TextShadow {
                offset: Vec2::new(2.0, 2.0),
                color: Color::srgba(0.0, 0.0, 0.0, 0.7),
            },
            HelloWorldText,
        ));

    match StreamAudioClip::from_wav_file(DEMO_MUSIC_PATH) {
        Ok(clip) => {
            commands.insert_resource(DemoMusicClip(clips.add(clip)));
            commands.insert_resource(DemoMusicStarted::default());
        }
        Err(err) => eprintln!("Could not load demo music {DEMO_MUSIC_PATH}: {err}"),
    }

    match StreamAudioClip::from_wav_file(DEMO_SFX_PATH) {
        Ok(clip) => {
            commands.insert_resource(DemoSfxClip(clips.add(clip)));
        }
        Err(err) => eprintln!("Could not load demo SFX {DEMO_SFX_PATH}: {err}"),
    }
}

pub fn pulse_hello_world_text(
    time: Res<Time>,
    mut text_query: Query<&mut TextColor, With<HelloWorldText>>,
) {
    let pulse = ((time.elapsed_secs() * 4.0).sin() + 1.0) * 0.5;
    let color = Color::srgb(0.55 + pulse * 0.4, 0.85, 1.0);

    for mut text_color in &mut text_query {
        text_color.0 = color;
    }
}

pub fn start_demo_music(
    mut started: Option<ResMut<DemoMusicStarted>>,
    clip: Option<Res<DemoMusicClip>>,
    mut events: MessageWriter<PlayStreamSound>,
) {
    let (Some(started), Some(clip)) = (started.as_mut(), clip) else {
        return;
    };

    if !started.0 {
        events.write(PlayStreamSound::looping(clip.0.clone()).with_volume(0.20));
        started.0 = true;
    }
}

pub fn handle_demo_boing_command(
    In(command): In<TwitchChatCommand>,
    clip: Option<Res<DemoSfxClip>>,
    chat: Option<Res<TwitchChatSender>>,
    mut events: MessageWriter<PlayStreamSound>,
) {
    let Some(clip) = clip else {
        return;
    };

    events.write(PlayStreamSound::once(clip.0.clone()).with_volume(0.80));
    if let Some(chat) = chat {
        let role = if command.roles.broadcaster {
            "broadcaster"
        } else if command.roles.moderator {
            "moderator"
        } else if command.roles.vip {
            "vip"
        } else if command.roles.subscriber {
            "subscriber"
        } else {
            "viewer"
        };
        chat.send(format!("Boing, {} ({role})!", command.display_name));
    }
}

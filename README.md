# Direct Stream Game

Direct Stream Game is a reusable Bevy streaming foundation. It creates a
low-resolution offscreen render target, reads it back from the GPU, and routes
the frames to either a local browser preview or a Twitch-compatible RTMP stream
through integrated FFmpeg dynamic libraries.

The binary in this repository is a small `HelloWorld` demo. The useful piece is
the library API in `src/lib.rs`, which future games can build on without having
to manually wire the capture, encoding, preview, Twitch config, chat commands,
or stream pacing systems.

The current Twitch profile is `320x240 @ 15fps` with a `350 kbps` H.264 target
bitrate and AAC audio, matching Twitch's 240p quality tier closely enough for
early stream tests.

## Requirements

- Rust stable, currently verified with `rustc 1.95.0`.
- Bevy `0.18.1`.
- An LGPL-compatible dynamic FFmpeg SDK with headers, import libraries, and
  runtime DLLs.

The app does not launch `ffmpeg.exe`. It links to FFmpeg libraries through
`ffmpeg-next`/`ffmpeg-sys-next`.

## Windows FFmpeg SDK Setup

`ffmpeg-sys-next` needs to find FFmpeg at build time. On Windows MSVC, use one
of these dynamic-library paths. The recommended route for this repo is vcpkg
with the included `vcpkg.json` manifest.

### Recommended vcpkg path

Install vcpkg and LLVM/clang, then install FFmpeg into vcpkg's classic
installed tree. This is the layout that `ffmpeg-sys-next` discovers on
Windows/MSVC.

```powershell
git clone https://github.com/microsoft/vcpkg C:\vcpkg
C:\vcpkg\bootstrap-vcpkg.bat -disableMetrics
winget install LLVM.LLVM

$env:VCPKG_ROOT = "C:\vcpkg"
$env:VCPKG_DEFAULT_TRIPLET = "x64-windows"
C:\vcpkg\vcpkg.exe install "ffmpeg[avcodec,avformat,openh264,swresample,swscale]:x64-windows" --classic
```

Keep `VCPKG_ROOT` set when running Cargo:

```powershell
$env:VCPKG_ROOT = "C:\vcpkg"
$env:PATH = "C:\vcpkg\installed\x64-windows\bin;$env:PATH"
cargo run
```

`ffmpeg-sys-next` sets `VCPKGRS_DYNAMIC=1` for dynamic linking when the Rust
crate's `static` feature is not enabled.

The included `vcpkg.json` documents the native dependency set, but Cargo's
current FFmpeg binding discovery expects the classic vcpkg install location.

### Alternative paths

1. Install a compatible FFmpeg development package with `pkg-config`
   metadata, then put `pkg-config` on `PATH`.
2. Provide the FFmpeg include/lib/bin layout expected by `ffmpeg-sys-next` and
   set the relevant environment variables for that package.

Whichever route you choose, ensure the FFmpeg build is configured without
`--enable-gpl` or `--enable-nonfree`, and ensure the FFmpeg DLLs are available
at runtime.

## Run

```powershell
cargo run
```

Useful options:

```powershell
cargo run -- --stats-window
cargo run -- --headless-window
cargo run -- --twitch-url="rtmp://live.twitch.tv/app/live_..."
cargo run -- --ffmpeg-warnings
```

`--stats-window` keeps the Bevy window alive but displays server/stream stats
instead of the rendered output. The offscreen scene still renders and streams.
The stats window also has stream-key, chat bot username, and chat OAuth token
inputs plus Start, End, and Open buttons. Click an input, paste with `Ctrl+V`,
then use Start to begin RTMP output, End to stop it, or Open to view the Twitch
channel in your browser. This control window is not part of the streamed render
target.

Games send stream audio through the direct-stream audio mixer, which writes
interleaved stereo `f32` PCM into `DirectStreamAudioTarget`. Bevy's normal
speaker audio plugin is disabled by the app shell, so audio is intended to go
to the stream rather than the local audio device. Twitch consumes that target,
encodes AAC, and muxes it into the RTMP stream. If the game has not supplied
enough samples for a frame, the stream fills the gap with silence so video
pacing remains stable.

`--twitch-url` overrides the RTMP destination used when the stats-window Start
button is pressed. The sink prefers FFmpeg's `libopenh264` encoder for CPU
frames, then falls back to Media Foundation's `h264_mf`, and muxes AAC audio so
the stream shape matches Twitch's H.264/AAC/RTMP ingest model.

FFmpeg native logging defaults to errors-only to keep harmless decoder warnings
out of the terminal. Use `--ffmpeg-warnings` or `--ffmpeg-verbose` when you need
more detail while debugging.

## Twitch Setup

The quickest local control path is the stats window:

```powershell
cargo run -- --stats-window
```

The field is pre-filled from `twitch.toml` when that file has a real stream
key. Click the stream-key field, paste or edit your Twitch stream key with
`Ctrl+V`, and press Start. Press End to stop the RTMP sink, or Open to view the
configured Twitch channel page. Start writes the current key back to
`twitch.toml`, and the stream key is masked in the window. The stats/control
window is never sent to Twitch.

For chat commands, set `channel` in `twitch.toml` to your Twitch channel name.
The app connects to Twitch IRC and emits typed Bevy messages for chat messages
and `!command` messages. If `chat_bot_username` and `chat_oauth_token` are set,
it can also send replies back to chat. In the demo, any viewer can type:

```text
!boing
```

and the stream audio mixer will play `sfx/boing_x.wav` over the looping backing
music, then the bot replies in chat.

For bot write access, use a dedicated Twitch account if you want replies to come
from a bot name instead of your streaming account. Generate a chat OAuth token
for that account with IRC chat write permission, then set:

```toml
channel = "your_channel_name"
chat_bot_username = "your_bot_login"
chat_oauth_token = "oauth:your_chat_oauth_token_here"
```

Twitch IRC authentication uses `PASS oauth:...` plus `NICK bot_login`, and chat
messages are sent as `PRIVMSG #channel :message`. Twitch currently describes
EventSub/API chat as the preferred long-term chat path, but IRC is still a
supported practical route for this prototype.

You can also use a config file. Copy the example config and add your private
stream key:

```powershell
Copy-Item twitch.example.toml twitch.toml
notepad twitch.toml
```

Fill in `stream_key` from Twitch Creator Dashboard -> Settings -> Stream ->
Primary Stream key, and set `channel` to your Twitch channel name for chat
commands. Fill in the bot username/token if you want the app to write chat
replies. You do not need your Twitch username for RTMP ingest; the stream key is
the credential. `twitch.toml` is ignored by git.

Start with:

```toml
enabled = false
bandwidth_test = true
```

Then run:

```powershell
cargo run -- --stats-window
```

With `bandwidth_test = true`, the app appends `?bandwidthtest=true` when you
press Start, which Twitch documents as the mode for
testing bandwidth health in Twitch Inspector without making the stream viewable.
Open https://inspector.twitch.tv/ while the app is running and confirm Twitch is
receiving video/audio without sustained bitrate drops.

When you actually want to go live, change:

```toml
bandwidth_test = false
```

Then run `cargo run -- --stats-window`, confirm the key in the stats window,
confirm the chat bot fields if you are using replies, and press Start. Start
writes the current stream key and chat bot fields back to `twitch.toml`, then
connects/reconnects the chat bot with those values. The old `enabled` setting is
tolerated in `twitch.toml`, but streaming no longer auto-starts from config or
command-line flags.

Twitch's documented RTMP URL format is:

```text
rtmp://<ingest-server>/app/<stream-key>[?bandwidthtest=true]
```

The default config uses `rtmp://live.twitch.tv/app`. You can replace
`ingest_server` with a specific endpoint from Twitch's ingest list if you want
to pin a region.

For normal preview mode, run `cargo run` and open:

```text
http://127.0.0.1:8080
```

The Rust app serves both the preview page and the multipart JPEG stream on port
`8080` in normal preview mode.

Local preview and Twitch output are separate modes. `cargo run` shows the
rendered preview window and starts the local MJPEG preview encoder.
`cargo run -- --stats-window` opens the larger stats/control window instead;
when Start is pressed there, frames are routed to the RTMP sink. The stats
window follows the active mode: preview stats hide Twitch-only counters, and
Twitch stats hide local-preview counters.

The Twitch sink is clocked independently at the stream FPS. GPU readback updates
the latest available frame, and the RTMP thread repeats that frame if needed so
packet cadence stays steady for Twitch ingest.

## Game API

The streaming stack is exposed as a library. A game can start from the same
Bevy app shell and then add normal Bevy plugins/systems:

```toml
[dependencies]
bevy = "0.18.1"
direct_stream_game = { path = "../DirectStreamGame" }
```

```rust
use bevy::prelude::*;
use direct_stream_game::{direct_stream_app, DirectStreamSet, DirectStreamTarget};

fn main() {
    direct_stream_app()
        .add_systems(Startup, setup_game.after(DirectStreamSet::Setup))
        .add_systems(Update, update_game)
        .run();
}

fn setup_game(mut commands: Commands, target: Res<DirectStreamTarget>) {
    commands
        .spawn((
            Node {
                width: percent(100),
                height: percent(100),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            UiTargetCamera(target.camera),
        ))
        .with_child(Text::new("My Game"));
}

fn update_game() {}
```

If a startup system does not need `DirectStreamTarget`, it can be added normally.
Systems that need the stream camera should run after `DirectStreamSet::Setup`.
The binary in this repo is now only a demo built with the same chained app style.

Audio is stream-only. The direct-stream app disables Bevy's built-in speaker
audio plugin and installs the combined `DirectStreamPlugin`, which wires both
video capture and stream audio. It provides `StreamAudioClip` assets plus
`PlayStreamSound` messages for simple one-shot or looping stream sounds:

```rust
use bevy::prelude::*;
use direct_stream_game::{PlayStreamSound, StreamAudioClip};

#[derive(Resource)]
struct HitSound(Handle<StreamAudioClip>);

fn setup_audio(mut commands: Commands, mut clips: ResMut<Assets<StreamAudioClip>>) {
    let samples = vec![0.0; 48_000 / 10];
    commands.insert_resource(HitSound(clips.add(StreamAudioClip::from_mono_f32(samples, 48_000))));
}

fn play_hit_sound(sound: Res<HitSound>, mut sounds: MessageWriter<PlayStreamSound>) {
    sounds.write(PlayStreamSound::once(sound.0.clone()).with_volume(0.5));
}
```

For lower-level engines or generated audio, systems can also write directly to
`DirectStreamAudioTarget` with `push_stereo_f32` or `push_mono_f32`. The stream
target expects `48_000 Hz`, stereo, `f32` samples in `[-1.0, 1.0]`; clips with
other sample rates are resampled by the lightweight stream mixer.

Chat command routing lives in the stream plugin. Register a command once on the
app, then write the handler as a normal Bevy system that receives
`In<TwitchChatCommand>`. The command includes parsed arguments, display name,
login name, roles, and optional message ID.

```rust
use bevy::prelude::*;
use bevy::ecs::system::In;
use direct_stream_game::{direct_stream_app, TwitchChatCommand, TwitchChatSender, TwitchCommandAppExt};

fn main() {
    direct_stream_app()
        .add_twitch_command("boing", handle_boing)
        .add_systems(Startup, setup)
        .run();
}

fn setup() {}

fn handle_boing(In(command): In<TwitchChatCommand>, chat: Option<Res<TwitchChatSender>>) {
    if command.roles.broadcaster || command.roles.moderator {
        if let Some(chat) = chat {
            chat.send(format!("Boing accepted from {}!", command.display_name));
        }
    }
}
```

## Migrating A Bevy Game

For an existing Bevy game, replace the app bootstrap with the direct-stream app
shell, then add your normal game systems and plugins back onto it.

Before:

```rust
use bevy::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, update)
        .run();
}
```

After:

```rust
use bevy::prelude::*;
use direct_stream_game::{direct_stream_app, DirectStreamSet};

fn main() {
    direct_stream_app()
        .add_systems(Startup, setup.after(DirectStreamSet::Setup))
        .add_systems(Update, update)
        .run();
}
```

If the game renders UI, attach UI roots to the stream camera:

```rust
use bevy::prelude::*;
use direct_stream_game::DirectStreamTarget;

fn setup(mut commands: Commands, target: Res<DirectStreamTarget>) {
    commands.spawn((
        Node {
            width: percent(100),
            height: percent(100),
            ..default()
        },
        UiTargetCamera(target.camera),
    ));
}
```

Run modes are unchanged:

```powershell
cargo run -- --stats-window
```

In stats mode, streaming starts only when the Start button is pressed. The old
`--twitch` flag is accepted for compatibility but intentionally does not
auto-start the stream.

The current API is easiest for UI-first or explicitly stream-camera-driven games.
Camera-heavy 2D/3D games can still migrate, but they may need one more adapter
step so their main camera renders into `DirectStreamTarget.image` or is replaced
with the provided stream camera. That helper layer is the next natural API
improvement.

## Current Stream Shape

```text
Bevy game -> 320x240 offscreen GPU texture -> throttled GPU readback -> latest-frame hub

Local preview:
latest-frame hub -> FFmpeg MJPEG encoder -> multipart HTTP stream -> Chrome

Twitch:
latest-frame hub -> clocked 15fps RTMP thread -> H.264 video + AAC audio -> FLV/RTMP
```

The capture side allows only one outstanding GPU readback at a time. In Twitch
mode, readback updates the latest available frame and the RTMP thread sends on a
stable stream clock, repeating the newest frame when necessary. This keeps memory
bounded while giving Twitch a steady packet cadence.

For LGPL-only FFmpeg builds, be careful not to depend on GPL encoders such as
`libx264`. The recommended vcpkg setup enables `openh264`, which is not GPL and
works with CPU frames. The D3D12VA H.264 encoder is intentionally not used here
because it accepts D3D12 surfaces rather than CPU pixel buffers.

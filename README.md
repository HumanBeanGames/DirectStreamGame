# Direct Stream Game

Direct Stream Game is a Bevy streaming foundation for making games that can be
played through a browser stream. It renders the game to a low-resolution
offscreen target, reads frames back from the GPU, and sends them to either:

- Twitch, through integrated dynamic FFmpeg libraries as H.264/AAC over RTMP.
- A custom browser player, through the Indexed Pixel Stream Codec (`IPSC`) plus
  a small local chat/control layer.

The repository binary is a demo. The reusable API is the library exposed from
`src/lib.rs`.

## Current Capabilities

- Bevy `0.18.1` app shell with an offscreen stream render target.
- GPU readback with bounded in-flight capture.
- Stats/control window with Start, End, Open, Purge Chat, stream dimensions,
  palette-bias controls, and Twitch credentials.
- Stream-only audio mixer. Bevy speaker output is disabled by default.
- Twitch RTMP output using H.264 video and AAC audio.
- Twitch IRC command input and optional bot replies.
- Custom browser host using IPSC indexed-palette video, 8 kHz mono μ-law audio,
  local chat, and a static player that can be hosted on Cloudflare Pages.
- Local chat names generated from hashed viewer identity.
- In-memory local chat feed with purge/reset support.
- Palette Lab and PNG Converter Lab for creating palettes, LUTs, and IPSI still
  images.
- Demo scene with looping music, `!boing` sound effect, and drag-and-drop video
  background playback.

## Requirements

- Rust stable, currently verified with `rustc 1.95.0`.
- Bevy `0.18.1`.
- Windows/MSVC currently receives the most testing.
- Dynamic FFmpeg libraries with headers/import libs available at build time and
  DLLs available at runtime.

The app does not launch `ffmpeg.exe`. It links to FFmpeg through
`ffmpeg-next`/`ffmpeg-sys-next`.

## FFmpeg Setup On Windows

The recommended route is vcpkg with the included `vcpkg.json` manifest.

```powershell
git clone https://github.com/microsoft/vcpkg C:\vcpkg
C:\vcpkg\bootstrap-vcpkg.bat -disableMetrics
winget install LLVM.LLVM

$env:VCPKG_ROOT = "C:\vcpkg"
$env:VCPKG_DEFAULT_TRIPLET = "x64-windows"
C:\vcpkg\vcpkg.exe install "ffmpeg[avcodec,avformat,openh264,swresample,swscale]:x64-windows" --classic
```

Then keep vcpkg on the environment when building/running:

```powershell
$env:VCPKG_ROOT = "C:\vcpkg"
$env:PATH = "C:\vcpkg\installed\x64-windows\bin;$env:PATH"
cargo run --bin DirectStreamGame
```

For closed-source distribution, keep FFmpeg dynamically linked and use an
LGPL-compatible FFmpeg build. Do not enable `ffmpeg-next` static, GPL, or
nonfree build features. See `FFMPEG-LGPL-COMPLIANCE.md`.

## Running The Demo

Normal local preview:

```powershell
cargo run --bin DirectStreamGame
```

Custom browser host:

```powershell
cargo run --bin DirectStreamGame -- --stats-window --custom-host --prebaked
```

Then press **Start** in the stats window and open:

```text
http://127.0.0.1:8080
```

Twitch mode:

```powershell
cargo run --bin DirectStreamGame -- --stats-window
```

Paste or confirm the Twitch stream key in the stats window, then press Start.
The old `--twitch` flag is accepted for compatibility but no longer auto-starts
streaming.

Useful flags:

```powershell
--stats-window
--headless-window
--custom-host
--prebaked
--palette-config=palette.toml
--stream-width=128
--stream-height=128
--stream-fps=5
--twitch-config=twitch.toml
--twitch-url="rtmp://live.twitch.tv/app/live_..."
--ffmpeg-warnings
--ffmpeg-verbose
```

In custom-host mode, width and height must be equal, 8-aligned, and between
`64` and `256`. The default is `128x128 @ 5fps`.

## Demo Controls

The demo starts with a hue-gradient background and `HelloWorld` text. It also:

- Loops `music/Elijah_K - Iron.wav` as backing music.
- Plays `sfx/boing_x.wav` when chat sends `!boing`.
- Accepts video files dragged onto the Bevy window.

Supported demo video extensions:

```text
.mp4 .mov .m4v .webm .mkv .avi
```

Best first test file:

```text
MP4 container, H.264 video, yuv420p, small resolution, 24/25/30 fps
```

Video audio is ignored. The video loops and is scaled into the stream render
target. This is demo-only code, not part of the streaming library API.

## Using The Library In A Game

Add the library to your game:

```toml
[dependencies]
bevy = "0.18.1"
direct_stream_game = { path = "../DirectStreamGame" }
```

Use the direct-stream app shell instead of `App::new().add_plugins(DefaultPlugins)`:

```rust
use bevy::prelude::*;
use direct_stream_game::{direct_stream_app, DirectStreamSet, DirectStreamTarget};

fn main() {
    direct_stream_app()
        .add_systems(Startup, setup.after(DirectStreamSet::Setup))
        .add_systems(Update, update)
        .run();
}

fn setup(mut commands: Commands, target: Res<DirectStreamTarget>) {
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

fn update() {}
```

Run startup systems after `DirectStreamSet::Setup` when they need
`DirectStreamTarget` or the stream camera. Systems that do not depend on the
stream target can be scheduled normally.

### Raw Frame Processing And DirectText

For exact stream-pixel overlays, register a raw frame processor. Processors run
after GPU readback has produced CPU BGRA bytes and before the frame is sent to
preview, Twitch, or the custom host.

```rust
use direct_stream_game::{
    direct_stream_app, DirectStreamFrame, DirectStreamFrameAppExt,
};

fn main() {
    direct_stream_app()
        .add_direct_stream_frame_processor(draw_overlay)
        .run();
}

fn draw_overlay(mut frame: DirectStreamFrame) {
    let width = frame.width();
    let row_bytes = frame.row_bytes();
    let pixels = frame.bgra_mut();

    if width > 0 && pixels.len() >= row_bytes {
        pixels[0..4].copy_from_slice(&[255, 255, 255, 255]);
    }
}
```

`DirectStreamFrame` exposes final outgoing BGRA pixels with `bgra()`,
`bgra_mut()`, `width()`, `height()`, and `row_bytes()`.

The included `DirectTextPlugin` uses this hook to draw a small built-in bitmap
font directly over the outgoing stream:

```rust
use bevy::prelude::*;
use direct_stream_game::{direct_stream_app, DirectText, DirectTextPlugin};

fn main() {
    direct_stream_app()
        .add_plugins(DirectTextPlugin)
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(DirectText::new("SCORE: 10", 4, 4));
        })
        .run();
}
```

### Migrating An Existing Bevy Game

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

UI should be attached to the stream camera with `UiTargetCamera(target.camera)`.
Camera-heavy 2D/3D games may need an adapter so their main camera renders to
`DirectStreamTarget.image` or is replaced by the provided stream camera.

## Stream Audio

The app disables Bevy's normal speaker audio plugin. Audio is sent to the stream
through `DirectStreamAudioTarget`.

Simple clip playback:

```rust
use bevy::prelude::*;
use direct_stream_game::{PlayStreamSound, StreamAudioClip};

#[derive(Resource)]
struct HitSound(Handle<StreamAudioClip>);

fn setup_audio(mut commands: Commands, mut clips: ResMut<Assets<StreamAudioClip>>) {
    let samples = vec![0.0; 48_000 / 10];
    let clip = StreamAudioClip::from_mono_f32(samples, 48_000);
    commands.insert_resource(HitSound(clips.add(clip)));
}

fn play_hit(sound: Res<HitSound>, mut sounds: MessageWriter<PlayStreamSound>) {
    sounds.write(PlayStreamSound::once(sound.0.clone()).with_volume(0.5));
}
```

You can also load WAV files with `StreamAudioClip::from_wav_file`. The mixer
handles common WAV formats and caches the decode path per file. Lower-level
audio engines can push samples directly into `DirectStreamAudioTarget` with
`push_stereo_f32` or `push_mono_f32`.

The stream target expects `48_000 Hz`, stereo, `f32` samples in `[-1.0, 1.0]`.
Custom-host output currently sends browser audio as 8 kHz mono μ-law to keep
bandwidth low.

## Chat Commands

Register commands with `TwitchCommandAppExt`. The same command handler is used
for Twitch IRC commands and local custom-host chat commands.

```rust
use bevy::ecs::system::In;
use bevy::prelude::*;
use direct_stream_game::{
    direct_stream_app, TwitchChatCommand, TwitchChatSender, TwitchCommandAppExt,
};

fn main() {
    direct_stream_app()
        .add_twitch_command("boing", handle_boing)
        .run();
}

fn handle_boing(In(command): In<TwitchChatCommand>, chat: Option<Res<TwitchChatSender>>) {
    if let Some(chat) = chat {
        chat.send(format!("Boing, {}!", command.display_name));
    }
}
```

`TwitchChatCommand` includes:

- `user`
- `display_name`
- `command`
- `args`
- `roles`
- `message_id`

Local custom-host users receive generated names such as `BrightDragon-A1` based
on a hash of the viewer identity. The active app session keeps a recent chat
history and a generated-name cache. The stats window **Purge Chat** button
clears the current local chat feed.

## Twitch Setup

Copy the example config:

```powershell
Copy-Item twitch.example.toml twitch.toml
notepad twitch.toml
```

Important fields:

```toml
channel = "your_channel_name"
chat_bot_username = "your_bot_login"
chat_oauth_token = "oauth:your_chat_oauth_token_here"
ingest_server = "rtmp://live.twitch.tv/app"
stream_key = "live_your_stream_key_here"
bandwidth_test = true
```

Notes:

- `stream_key` is the RTMP credential.
- `channel` is used for Twitch chat commands.
- `chat_bot_username` and `chat_oauth_token` are only needed if the app should
  reply in Twitch chat.
- With `bandwidth_test = true`, the app appends Twitch's `bandwidthtest=true`
  query so you can test in Twitch Inspector without going live.
- Streaming starts only from the stats-window Start button.

Twitch output currently targets `320x240 @ 15fps` with a `350 kbps` H.264 video
target and AAC audio. The sink prefers `libopenh264`, then falls back to
Media Foundation `h264_mf`.

## Custom Browser Hosting

Local custom host:

```powershell
cargo run --bin DirectStreamGame -- --stats-window --custom-host --prebaked
```

Public hosting layout used by this project:

```text
humanbeangames.com
  Cloudflare Pages landing page

stream.humanbeangames.com
  Cloudflare Pages static player

game.humanbeangames.com
  Cloudflare Tunnel to http://localhost:8080 on the machine running the game
```

Export the static stream player:

```powershell
cargo run --bin ipsc_export_static_stream
```

Upload the contents of:

```text
dist/humanbeangames_stream
```

to the `stream.humanbeangames.com` Pages/Worker project.

Export the dummy landing page from:

```text
dist/humanbeangames
```

The landing page embeds `https://stream.humanbeangames.com`. The static stream
page talks to `https://game.humanbeangames.com` for:

```text
/status.json
/palette.bin
/audio.pcm
/local-chat
/local-chat-feed
```

Because the player is static, `stream.humanbeangames.com` can show **Not Online**
even when the Rust game app is closed. The raw backend hostname may show a
Cloudflare tunnel error when the app is down; that is expected.

## IPSC Video Format

IPSC is an indexed-pixel live stream format for tiny browser-playable games. It
is closer to a live state-sync stream than a GIF.

Stream header:

```text
magic:       [u8; 4] = b"IPSC"
version:     u8
width:       u16
height:      u16
tile_size:   u8 = 8
palette_len: u16
palette:     [rgba; palette_len]
```

Each frame is length-prefixed by a little-endian `u32`. Frame body:

```text
frame_type:  u8   // 0 = keyframe, 1 = delta
frame_index: u32
payload_len: u32
payload:     [u8; payload_len]
```

Keyframes are raw indexed pixels: `width * height` bytes.

Delta frames contain an 8x8 tile-change bitmask followed by tile payloads for
changed tiles only. The encoder chooses the smallest tile representation:

```text
Skipped   unchanged tile, no payload
Raw       64 palette indices
Solid     one palette index
RLE       row-major color/length runs
Span      changed spans inside the old tile
XorRLE    row-major XOR/length runs against the old tile
```

Custom-host recordings are written to:

```text
recordings/custom-*.ipsc
```

Replay a recording:

```powershell
cargo run --bin ipsc_player -- recordings\custom-1234567890.ipsc
```

The player serves `http://127.0.0.1:8090`.

## Palette And Image Tools

The default palette is:

```text
src/default_pallette/default_pallette.toml
```

Custom-host mode uses live OKLab/OKLCH palette matching by default. Pass
`--prebaked` to use a sibling `.ipsmap` direct lookup table:

```powershell
cargo run --release --bin DirectStreamGame -- --custom-host --prebaked --palette-config=palette.toml
```

The `.ipsmap` file is a 16 MB direct sRGB-to-palette lookup table. It is only
accepted when its hash matches the palette colours and matching weights.

Combined browser lab:

```powershell
cargo run --bin ipsc_lab
```

Open:

```text
http://127.0.0.1:8092
```

The lab has Palette and Converter tabs. Palette generation can export:

```text
palette.toml
palette.ipsi
palette.ipsmap
```

The converter tab uses the current Palette Lab palette automatically.

Export the static lab:

```powershell
cargo run --bin ipsc_export_static_lab
```

Upload the contents of:

```text
dist/ipsc_lab
```

to a static host such as Cloudflare Pages.

CLI PNG to IPSI conversion:

```powershell
cargo run --bin ipsc_png_to_ipsi -- input.png output.ipsi palette.toml
cargo run --bin ipsc_png_to_ipsi -- input.png output.ipsi --size 128x128
cargo run --bin ipsc_png_to_ipsi -- input.png output.ipsi --no-dither
```

View IPSI still images:

```powershell
cargo run --bin ipsc_image_viewer -- assets\palette.ipsi
```

## Project Structure

Key library modules:

```text
src/app.rs             app shell and plugin setup
src/plugin.rs          DirectStreamPlugin
src/capture.rs         GPU readback
src/frames.rs          raw frame hubs
src/palette.rs         IPSC encoder
src/audio.rs           stream audio mixer
src/chat.rs            Twitch/local chat and command routing
src/web.rs             local HTTP server and browser player HTML
src/stream_control.rs  stats-window controls
src/scene.rs           stream target and stats UI
src/twitch.rs          RTMP/H.264/AAC sink
src/demo.rs            demo-only game scene/audio/video
```

Tools:

```text
src/bin/ipsc_lab.rs
src/bin/ipsc_palette_lab.rs
src/bin/ipsc_png_converter_lab.rs
src/bin/ipsc_export_static_lab.rs
src/bin/ipsc_export_static_stream.rs
src/bin/ipsc_player.rs
src/bin/ipsc_image_viewer.rs
src/bin/ipsc_png_to_ipsi.rs
```

## Current Caveats

- The custom host is a prototype server using a small hand-written HTTP layer.
- Local chat moderation is in-memory and session-scoped.
- Static player deployment currently assumes `game.humanbeangames.com` as the
  backend origin unless you pass another origin to `ipsc_export_static_stream`.
- The demo video player is intentionally simple and demo-only. It decodes on the
  main thread and is best tested with small H.264 MP4 files.
- Twitch scaling/filtering is controlled by Twitch/browser playback, so pixel
  art may be blurred there. The custom browser player controls canvas filtering.

## Checks

Useful local checks:

```powershell
cargo check --bin DirectStreamGame
cargo check --bin ipsc_lab --bin ipsc_export_static_stream
cargo test --lib
```

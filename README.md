# Direct Stream Game

Direct Stream Game is a Bevy streaming library for games that are played through
a custom browser host. It renders your game to an offscreen stream target, reads
the final frame back from the GPU, palette-encodes it, and serves it through a
small local web server with audio, chat, panels, and click input.

The repository binary is a demo. The reusable library is exposed from
`src/lib.rs`.

## What It Provides

- Bevy `0.18.1` app shell with a dedicated stream render target.
- GPU readback with bounded in-flight capture and fixed-size frame batching.
- GPU palette indexing with optional prebaked `.ipsmap` lookup textures.
- Indexed Pixel Stream Codec (`IPSC`) custom-host video.
- 8 kHz mono mu-law browser audio for low-bandwidth custom streams.
- Stream-only audio mixer. Bevy speaker output is disabled by default.
- Local browser chat with generated viewer names, command routing, bot replies,
  temporary messages, and purge support.
- Side-panel publishing for custom app UI outside the stream canvas.
- Stream canvas click events forwarded back into Bevy.
- Stats/control window with Start, End, Open, Purge Chat, resolution, FPS, and
  palette matching controls.
- Palette Lab and PNG Converter Lab for creating palettes, LUTs, and IPSI still
  images.
- Demo scene with looping music, `!boing` sound effect, and drag-and-drop video
  background playback.

There is no Twitch/RTMP path. The library now specializes in the custom browser
host.

## Requirements

- Rust stable, currently verified with `rustc 1.95.0`.
- Bevy `0.18.1`.
- Windows/MSVC receives the most testing.
- Dynamic FFmpeg libraries with headers/import libs available at build time and
  DLLs available at runtime.

The app does not launch `ffmpeg.exe`. It links to FFmpeg through
`ffmpeg-next`/`ffmpeg-sys-next` for local preview/media tooling. For
closed-source distribution, keep FFmpeg dynamically linked and use an
LGPL-compatible FFmpeg build. Do not enable `ffmpeg-next` static, GPL, or
nonfree build features. See `FFMPEG-LGPL-COMPLIANCE.md`.

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
cargo run --bin DirectStreamGame -- --stats-window --custom-host --prebaked
```

## Running The Demo

```powershell
cargo run --bin DirectStreamGame -- --stats-window --custom-host --prebaked
```

Then press **Start** in the stats window and open:

```text
http://127.0.0.1:8080
```

Useful flags:

```text
--stats-window
--headless-window
--custom-host
--prebaked
--use_prebaked_lookup
--palette-config=palette.toml
--stream-width=128
--stream-height=128
--stream-fps=5
--batch-size=30
```

In custom-host mode, width and height must be equal, 8-aligned, and between
`64` and `256`. The default stream rate is `5fps`.

## Demo Controls

The demo starts with a hue-gradient background and `HelloWorld` text. It also:

- Loops `music/Elijah_K - Iron.wav` as backing music when present.
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

## Using The Library

Add the library to your game:

```toml
[dependencies]
bevy = "0.18.1"
direct_stream_game = { package = "DirectStreamGame", git = "https://github.com/HumanBeanGames/DirectStreamGame" }
```

For local development, a path dependency also works:

```toml
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
`DirectStreamTarget.image` or is replaced by the provided stream camera. The
library should remain usable by 3D projects; the custom stream path consumes the
final render target, not a specific 2D scene model.

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
Custom-host output currently sends browser audio as 8 kHz mono mu-law to keep
bandwidth low.

## Chat Commands

Register commands with `StreamCommandAppExt`.

```rust
use bevy::ecs::system::In;
use bevy::prelude::*;
use direct_stream_game::{
    direct_stream_app, StreamChatCommand, StreamChatSender, StreamCommandAppExt,
};

fn main() {
    direct_stream_app()
        .add_stream_command("boing", handle_boing)
        .run();
}

fn handle_boing(In(command): In<StreamChatCommand>, chat: Option<Res<StreamChatSender>>) {
    if let Some(chat) = chat {
        chat.send(format!("Boing, {}!", command.display_name));
    }
}
```

`StreamChatCommand` includes:

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

Bot/system replies can be sent through `StreamChatSender::send`. Custom local
entries can be created with `StreamChatSender::send_local` and
`LocalChatEntryOptions`, including optional TTLs, mention metadata, and safe
per-message styling. Viewer-authored custom-host messages automatically get a
stable display-name color derived from their identity hash.

```rust
use std::time::Duration;
use bevy::prelude::*;
use direct_stream_game::{LocalChatEntryOptions, StreamChatSender};

fn reply(chat: Res<StreamChatSender>) {
    chat.send_local(
        LocalChatEntryOptions::named("Market", "Salt is cheap today.")
            .with_display_name_color("#f7c548")
            .with_message_color("white")
            .with_css_class("market-reply")
            .with_ttl(Duration::from_secs(10)),
    );
}
```

Chat colors accept safe `#RGB`, `#RRGGBB`, `rgb(r,g,b)`, `hsl(h s% l%)`, or a
small named-color set. CSS classes are sanitized to short alphanumeric,
underscore, or hyphen tokens before they reach the browser.

## Panels And Clicks

Downstream games can publish arbitrary side-panel text:

```rust
use bevy::prelude::*;
use direct_stream_game::{CustomHostPanelAnchor, CustomHostPanelHub};

fn update_panel(panels: Res<CustomHostPanelHub>) {
    panels.publish_text_at(
        "town-prices",
        "Northpass Prices",
        "wool 4g\nsalt 5g",
        CustomHostPanelAnchor::LeftOfStream,
        0,
    );
}
```

Panel anchors are `LeftOfStream`, `RightOfStream`, `AboveStream`,
`BelowStream`, `OverlayTopLeft`, `OverlayTopRight`, `OverlayBottomLeft`,
`OverlayBottomRight`, and `NamedRegion(String)`. Panels in each anchor are
ordered by `order`, then `id`. `publish_text` still uses the right-side default
stack below chat, and the older `CustomHostPanelRegion` helpers remain
available for compatibility. For full control, publish a `CustomHostPanel`
with `anchor`, `order`, optional `size_hint`, and optional `style_hint`.

Browser clicks on the stream canvas are emitted as `StreamPointerClick` messages
with viewer identity, display name, pixel coordinates, and normalized
coordinates. Your game owns hit-testing and game-specific behavior.

## Direct Frame Processing

For exact pixel overlays, register a raw BGRA frame processor. Processors run
after GPU readback has produced CPU-writeable bytes and before the frame is sent
to preview/custom-host encoders.

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
    let _ = (width, row_bytes, pixels);
}
```

This is the right hook for integer-coordinate overlays such as DirectText,
because it avoids Bevy text, texture sampling, and GPU scaling artifacts.

## Direct World Sprites

`DirectWorldSprite` adds readable low-resolution sprites to ordinary world
entities without replacing their real 3D meshes, colliders, or shadows. Attach it
to an entity that already has `Transform` and `GlobalTransform`:

```rust
use bevy::prelude::*;
use direct_stream_game::{
    DirectWorldSprite, SpriteDepthMode, SpriteFacing,
};

fn spawn_caravan(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn((
        Transform::from_xyz(4.0, 0.0, -8.0),
        GlobalTransform::default(),
        DirectWorldSprite {
            image: assets.load("sprites/caravan.png"),
            atlas: None,
            atlas_index: 0,
            pixel_size: UVec2::new(8, 10),
            anchor: Vec2::new(0.5, 1.0),
            tint: Color::WHITE,
            facing: SpriteFacing::FaceStreamCamera,
            depth_mode: SpriteDepthMode::TestAndWrite,
            depth_bias: 0.0,
        },
    ));
}
```

The world anchor is projected through `DirectStreamTarget.camera`, snapped to an
integer stream pixel, and rendered at `pixel_size` in stream output pixels. The
sprite is drawn before palette conversion and before `DirectText`, so text still
lands on top.

Depth modes:

```text
TestAgainstScene       alpha-blend against scene depth without writing sprite depth
TestAndWrite           alpha-mask visible pixels and write depth, so nearer sprites occlude farther sprites
AlwaysOnTopBeforeText  draw as an overlay before DirectText
```

Texture atlases are supported from the start with `atlas` and `atlas_index`.
`DirectWorldSpriteSettings` controls whether the system is enabled and how many
sprites are synced each frame.

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
/custom-panels
/stream-click
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

Each batch contains a header plus one or more length-prefixed frame payloads.
Frame payloads may contain keyframes, deltas, and batch-local cached tile
references.

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
Cached    reference to an identical tile earlier in the same batch
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

The embedded default palette and lookup map are available to downstream apps,
so a game does not need to copy `src/default_pallette` into its own assets.

Custom-host mode uses live OKLab/OKLCH palette matching by default. Pass
`--prebaked` or `--use_prebaked_lookup` to use a sibling `.ipsmap` direct lookup
table. If a configured palette path is missing, the embedded default palette is
used. If a sibling lookup is missing, the embedded default `.ipsmap` is used
when it matches; otherwise the stream falls back to live matching.

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
src/frames.rs          frame hubs and direct frame processors
src/palette.rs         IPSC encoder
src/gpu_palette.rs     GPU palette indexing pipeline
src/audio.rs           stream audio mixer
src/chat.rs            local chat and command routing
src/custom_host.rs     custom-host packet/audio/chat/panel server state
src/web.rs             local HTTP server and browser player HTML
src/stream_control.rs  stats-window controls
src/scene.rs           stream target and stats UI
src/direct_text.rs     CPU post-readback text overlay support
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
- FFmpeg is still used for local media/preview tooling; removing that is a
  separate dependency-reduction pass.

## Checks

Useful local checks:

```powershell
cargo check --bin DirectStreamGame
cargo check --bin ipsc_lab --bin ipsc_export_static_stream
cargo test --lib
```

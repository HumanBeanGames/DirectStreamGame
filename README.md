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
- GPU palette indexing with required `.ipsmap` lookup textures in custom-host mode.
- Indexed Pixel Stream Codec (`IPSC`) custom-host video.
- 8 kHz mono mu-law browser audio for low-bandwidth custom streams.
- Stream-only audio mixer. Bevy speaker output is disabled by default.
- Local browser chat with generated viewer names, command routing, bot replies,
  temporary messages, and purge support.
- Side-panel publishing for custom app UI outside the stream canvas.
- Stream canvas click events forwarded back into Bevy.
- Stats/control window with Start, End, Open, Purge Chat, resolution, and FPS
  controls.
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
- Optional: dynamic FFmpeg libraries for MJPEG preview, demo video playback,
  and additional media decoding.

The default feature set does not link FFmpeg, so normal downstream library and
custom-host builds do not require FFmpeg DLLs. Enable the `ffmpeg-media` Cargo
feature for the local MJPEG preview, drag-and-drop demo video, or FFmpeg-backed
media decoding. The app does not launch `ffmpeg.exe`; that feature links through
`ffmpeg-next`/`ffmpeg-sys-next`.

For closed-source distribution with `ffmpeg-media`, keep FFmpeg dynamically
linked and use an LGPL-compatible FFmpeg build. Do not enable `ffmpeg-next`
static, GPL, or nonfree build features. See `FFMPEG-LGPL-COMPLIANCE.md`.

## Optional FFmpeg Setup On Windows

The recommended route for `ffmpeg-media` builds is vcpkg with the included
`vcpkg.json` manifest.

```powershell
git clone https://github.com/microsoft/vcpkg C:\vcpkg
C:\vcpkg\bootstrap-vcpkg.bat -disableMetrics
winget install LLVM.LLVM

$env:VCPKG_ROOT = "C:\vcpkg"
$env:VCPKG_DEFAULT_TRIPLET = "x64-windows"
C:\vcpkg\vcpkg.exe install "ffmpeg[avcodec,avformat,openh264,swresample,swscale]:x64-windows" --classic
```

Then keep vcpkg on the environment and enable the feature when building or
running FFmpeg-backed modes:

```powershell
$env:VCPKG_ROOT = "C:\vcpkg"
$env:PATH = "C:\vcpkg\installed\x64-windows\bin;$env:PATH"
cargo run --features ffmpeg-media --bin DirectStreamGame -- --preview
```

## Running The Demo

```powershell
cargo run --bin DirectStreamGame -- --stats-window --custom-host
```

Then press **Start** in the stats window and open:

```text
http://127.0.0.1:8080
```

Useful flags:

```text
--preview
--stats-window
--headless-window
--custom-host
--palette-lookup=palette.ipsmap
--stream-width=128
--stream-height=128
--stream-fps=5
--batch-size=30
```

`--preview` requires `--features ffmpeg-media`. The custom-host stream does not.

In custom-host mode, width and height must be equal, 8-aligned, and between
`64` and `256`. The default stream rate is `5fps`.

## Demo Controls

The demo starts with a hue-gradient background and `HelloWorld` text. It also:

- Loops `music/Elijah_K - Iron.wav` as backing music when present.
- Plays `sfx/boing_x.wav` when chat sends `!boing`.
- Accepts video files dragged onto the Bevy window when built with
  `ffmpeg-media`.

Enable demo video playback with:

```powershell
cargo run --features ffmpeg-media --bin DirectStreamGame -- --stats-window --custom-host
```

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

This default dependency does not link FFmpeg. Apps that use MJPEG preview,
demo video, or FFmpeg-backed decoding can opt in:

```toml
direct_stream_game = { package = "DirectStreamGame", git = "https://github.com/HumanBeanGames/DirectStreamGame", features = ["ffmpeg-media"] }
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

Downstream systems can read `DirectStreamState` to pause live-only work while a
custom-host stream is stopped:

```rust
use bevy::prelude::*;
use direct_stream_game::{DirectStreamMode, DirectStreamState};

fn advance_world(time: Res<Time>, stream: Res<DirectStreamState>) {
    if stream.mode == DirectStreamMode::CustomHost && !stream.active {
        return;
    }

    // Tick simulation, AI, music scheduling, etc.
}
```

`DirectStreamState` also exposes the current stream `width`, `height`, and
`fps`. In custom-host mode these update when the stats-window Start button
retargets the stream; Stop preserves the last dimensions and sets
`active = false`.

Custom-host streams can also be started or stopped from game code with messages:

```rust
use bevy::prelude::*;
use direct_stream_game::DirectStreamStartRequest;

fn auto_start(mut requests: MessageWriter<DirectStreamStartRequest>) {
    requests.write(DirectStreamStartRequest::custom_host(128, 128, 30));
}
```

Read `DirectStreamControlResult` messages if you need to react to success or
validation failures. The stats-window Start/End buttons use the same message
path.

Audio/video sync can be tuned with `DirectStreamAudioSyncConfig`:

```rust
use direct_stream_game::{AudioSyncMode, DirectStreamAudioSyncConfig};

app.insert_resource(DirectStreamAudioSyncConfig {
    mode: AudioSyncMode::MatchEstimatedVideoLatency,
    fixed_delay_ms: 1_000,
    extra_delay_ms: 0,
});
```

`Fixed` preserves the old fixed-delay behavior. `MatchEstimatedVideoLatency`
uses the configured FPS and batch size to align stream audio with the expected
video presentation delay. `MatchMeasuredVideoLatency` currently falls back to
the estimate unless measured browser telemetry is supplied in a future version.

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

You can also load WAV files with `StreamAudioClip::from_wav_file`. The built-in
decoder handles common PCM and IEEE-float WAV formats without FFmpeg. With
`ffmpeg-media`, FFmpeg decoding is attempted first. Lower-level audio engines
can push samples directly into `DirectStreamAudioTarget` with `push_stereo_f32`
or `push_mono_f32`.

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
on a hash of the viewer identity. Browser clients generate a stable
`directstream_device_id` in `localStorage` and send it with local chat, chat
feed, custom panel, and stream-click requests. That makes identity scoped to a
browser profile/device instead of collapsing everyone behind the same network
IP into one viewer. If the browser does not send a device id, the server falls
back to the old IP/proxy-header behavior.

This is a pseudonymous browser-profile id, not authentication or hardware
fingerprinting. It persists across reloads and normal browser restarts, but
changes if the viewer clears site data, uses another browser/profile, or opens
an incognito session. For dev/debug resets, run this in the browser console and
reload:

```js
localStorage.removeItem("directstream_device_id")
```

The active app session keeps a recent chat history and a generated-name cache.
The stats window **Purge Chat** button clears the current local chat feed.

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

The custom-host chat window is opt-in. Downstream apps request it with
`CustomHostChatPanelHub`; otherwise the browser page does not create a chat
panel or poll the chat feed.

```rust
use bevy::prelude::*;
use direct_stream_game::CustomHostChatPanelHub;

fn request_chat(chat_panel: Res<CustomHostChatPanelHub>) {
    chat_panel.show();
}
```

## Custom Host Page

The browser page can be branded and sized by replacing the default resources
before the app starts:

```rust
use direct_stream_game::{CustomHostBranding, CustomHostLayout, direct_stream_app};

fn main() {
    direct_stream_app()
        .insert_resource(CustomHostBranding::new("MERCANTILE", "MERCANTILE"))
        .insert_resource(
            CustomHostLayout::default()
                .prefer_larger_player()
                .with_max_player_width(1280)
                .minimizable_player(),
        )
        .run();
}
```

`CustomHostLayout` controls the maximum player width, whether the page uses the
larger default player cap, and whether the browser shows a persistent
minimize/restore stream button. The minimized state is stored in browser
`localStorage`.

Runtime and static pages both derive their visible title/header from
`CustomHostBranding`. `/status.json` also reports the active branding, layout,
package version, and latency estimates, so a static Pages export can correct
stale visible branding when it connects to a differently branded runtime host.
Static exports include version and export-time metadata in the HTML.

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
Set `CustomHostPanelStyle { hide_header: true, ..default() }` to render a panel
body without title/header chrome.

For one-line route/status panels, use the helper style:

```rust
use direct_stream_game::{CustomHostPanelStyle, PanelWhiteSpace};

let style = CustomHostPanelStyle::headerless()
    .with_body_white_space(PanelWhiteSpace::NoWrap);
```

For panels that should wrap and grow with their content without scrollbars, use
`PanelOverflowMode::WrapNoScroll` or the convenience helper:

```rust
use direct_stream_game::CustomHostPanelStyle;

let style = CustomHostPanelStyle::default()
    .wrap_no_scroll()
    .with_region_css_class("market-left-column");
```

`WrapNoScroll` preserves newlines, allows long words to wrap, sets panel content
to `min-width: 0`, and avoids the horizontal/vertical scrollbar behavior used
by the default `Auto` mode. `region_css_class` is applied to the browser layout
region that receives the panel, after browser-side class-name validation.

Panels can be shared globally or filtered to one viewer. This mirrors local chat
audiences: `All`, `ViewerIdentity(String)`, or `ViewerName(String)`. The custom
host filters `/custom-panels` per request, so every viewer can safely have a
panel with the same app-level id.

```rust
use bevy::prelude::*;
use direct_stream_game::{
    CustomHostPanel, CustomHostPanelAnchor, CustomHostPanelAudience, CustomHostPanelElement,
    CustomHostPanelElementStyle, CustomHostPanelHub, CustomHostPanelPage, PagedTextControls,
    PagedTextControlsPosition,
};

fn publish_viewer_panel(panels: Res<CustomHostPanelHub>, viewer_identity: String) {
    panels.publish(CustomHostPanel {
        id: "selected-town-prices".to_owned(),
        title: "Selected Town".to_owned(),
        body: "wool 4g\nsalt 5g".to_owned(),
        elements: vec![
            CustomHostPanelElement::Text("wool 4g\nsalt 5g\n".to_owned()),
            CustomHostPanelElement::StyledText {
                text: "14. CozyDryad-KN - 176g\n".to_owned(),
                style: CustomHostPanelElementStyle::default()
                    .with_text_color("#f7c548")
                    .with_css_class("personal-score")
                    .with_font_weight("700"),
            },
            CustomHostPanelElement::Button {
                label: "Buy Wool".to_owned(),
                action_id: "buy-wool".to_owned(),
                disabled: false,
            },
            CustomHostPanelElement::PagedText {
                id: "industries".to_owned(),
                pages: vec![
                    CustomHostPanelPage {
                        title: Some("Mill".to_owned()),
                        body: "grain > flour".to_owned(),
                    },
                    CustomHostPanelPage {
                        title: Some("Weaver".to_owned()),
                        body: "wool > cloth".to_owned(),
                    },
                ],
                initial_page: 0,
                controls: PagedTextControls {
                    position: PagedTextControlsPosition::BeforePage,
                    ..default()
                },
            },
        ],
        revision: 0,
        anchor: CustomHostPanelAnchor::LeftOfStream,
        order: 10,
        size_hint: None,
        style_hint: None,
        audience: CustomHostPanelAudience::ViewerIdentity(viewer_identity),
    });
}
```

Panel button clicks are emitted as `CustomHostPanelAction` messages. The event
includes `viewer_identity`, `viewer_name`, `panel_id`, and the stable
`action_id` string from the clicked button.
`CustomHostPanelElement::PagedText` is browser-local: previous/next controls
switch pages instantly without posting back to Bevy, and the page index is
preserved across `/custom-panels` refreshes as long as the page still exists.

```rust
use bevy::prelude::*;
use direct_stream_game::CustomHostPanelAction;

fn handle_panel_actions(mut actions: MessageReader<CustomHostPanelAction>) {
    for action in actions.read() {
        if action.panel_id == "selected-town-prices" && action.action_id == "buy-wool" {
            // Apply this only to action.viewer_identity.
        }
    }
}
```

Browser clicks on the stream canvas are emitted as `StreamPointerClick` messages
with viewer identity, display name, raw browser client coordinates, corrected
stream pixel coordinates, and normalized stream coordinates. The browser maps
clicks against the actual rendered stream image rectangle, accounting for CSS
borders, aspect-ratio containment, and letterboxing. Clicks outside the rendered
image are ignored. Your game owns hit-testing and game-specific behavior.

Viewer-scoped browser overlays can draw local-only highlights above the stream
canvas without modifying the shared stream pixels:

```rust
use bevy::prelude::*;
use direct_stream_game::{
    CustomHostOverlayElement, CustomHostOverlayHub, CustomHostPanelAudience,
    OverlayCoordinateSpace, OverlayElementKind, OverlayElementStyle,
};

fn highlight_town(overlays: Res<CustomHostOverlayHub>, viewer_identity: String) {
    overlays.publish(CustomHostOverlayElement {
        id: "selected-town".to_owned(),
        audience: CustomHostPanelAudience::ViewerIdentity(viewer_identity),
        x: 0.42,
        y: 0.61,
        coordinate_space: OverlayCoordinateSpace::NormalizedStream,
        kind: OverlayElementKind::Circle { radius: 8.0 },
        order: 0,
        style: OverlayElementStyle::default(),
        ttl_ms: None,
    });
}
```

Overlay coordinates can be stream pixels or normalized stream coordinates.
Supported overlay kinds are circles, flags, text, and simple sprite/image
references. Like panels, overlays are keyed internally by audience and id, so
two viewers can both have `selected-town` without colliding or leaking state.

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
cargo run --bin DirectStreamGame -- --stats-window --custom-host
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

The exporter accepts the same browser options for static hosting:

```powershell
cargo run --bin ipsc_export_static_stream -- https://game.humanbeangames.com --page-title MERCANTILE --header-title MERCANTILE --prefer-larger-player --max-player-width 1280 --minimizable-player
```

Downstream tools can export the same page without invoking the CLI:

```rust
use direct_stream_game::{
    CustomHostBranding, CustomHostLayout, export_static_palette_stream_page,
};

export_static_palette_stream_page(
    "dist/humanbeangames_stream",
    "https://game.humanbeangames.com",
    &CustomHostBranding::new("MERCANTILE", "MERCANTILE"),
    &CustomHostLayout::default().prefer_larger_player(),
)?;
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

Custom-host mode requires a self-contained `.ipsmap` palette lookup file. Pass
`--palette-lookup=path/to/palette.ipsmap`, or place `palette.ipsmap` in the
current working directory. Missing, invalid, or stale lookup files fail startup
immediately.

The `.ipsmap` file is a direct sRGB-to-palette lookup table plus the binary
palette colours needed to build stream headers. New maps use the `IPSMAP5`
format. IPSMAP5 stores two 16,777,216-entry lookup tables: the altered table for
normal composited scene pixels, then the direct table for explicit colours that
should bypass input offsets. The format does not store palette TOML or
matching/bias settings: those authoring controls are cooked into the lookup
entries when the map is baked. The file hash validates the embedded palette
colours and cooked entries themselves. Older self-contained IPSMAP4 files still
load, but direct-colour pixels fall back to raw nearest-palette matching until
the map is regenerated as IPSMAP5.

Palette TOML remains the editable source format used by the palette tools and
lab. Palette matching during baking has two stages:

1. Convert the input sRGB colour to OKLCH, then apply the optional input
   biases from `[matching]`.
2. Compare the adjusted input colour against the palette colours using the
   priority weights from `[matching]`.

The priority weights are:

```toml
[matching]
lightness = 0.333
chroma = 0.333
hue = 0.334
```

The optional input biases are:

```toml
lightness_multiply = 0.0
lightness_add = 0.0
chroma_multiply = 0.0
chroma_add = 0.0
hue_add = 0.0
```

`lightness_multiply` and `chroma_multiply` are applied as `1.0 + value`, so
`0.5` means “treat this channel as 1.5x higher” and `-0.5` means “treat it as
0.5x”. Additive lightness/chroma offsets are applied after multiplication.
`hue_add` is measured in turns, so `0.25` is a 90 degree hue rotation.
After these offsets are applied, the adjusted OKLCH target is clamped back into
the reachable sRGB gamut by reducing chroma at the same lightness and hue. This
keeps creative chroma boosts from asking the matcher to chase impossible dark,
high-chroma colours.

Old source TOML files without these offset fields still work in the tools; the
missing values default to zero. The runtime no longer reads those settings from
the `.ipsmap`, so always rebake after changing weights, offsets, palette
colours, or DirectStreamGame palette-matching versions.

Migration for existing apps:

1. Update the `DirectStreamGame` dependency to a version that supports cooked
   self-contained `IPSMAP5` lookup files.
2. Keep a palette TOML in your downstream app or asset pipeline if useful, but
   ship `palette.ipsmap` as the runtime artifact.
3. Regenerate `.ipsmap` with `ipsc_build_palette_lut` or the Palette Lab.
4. Launch with `--palette-lookup=palette.ipsmap`, or leave the default
   `palette.ipsmap` in the process working directory.
5. Redeploy the static lab from `dist/ipsc_lab` if viewers or tools use the
   browser Palette Lab.

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

The converter tab uses the current Palette Lab palette automatically, or a
palette TOML uploaded by the user. DirectStreamGame does not ship a built-in
palette file.

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
cargo run --bin ipsc_png_to_ipsi -- input.png output.ipsi --palette palette.toml --size 128x128
cargo run --bin ipsc_png_to_ipsi -- input.png output.ipsi --palette palette.toml --no-dither
```

View IPSI still images:

```powershell
cargo run --bin ipsc_image_viewer -- output.ipsi
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
- FFmpeg-backed preview and demo media remain optional and require compatible
  dynamic libraries at build and runtime.

## Checks

Useful local checks:

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo build --release --workspace --all-targets --all-features --locked
```

# FFmpeg LGPL Compliance Notes

This app is intended to use FFmpeg dynamically so the game can remain
closed-source while using LGPL-covered FFmpeg libraries. The app integrates
FFmpeg through native Rust bindings and expects FFmpeg to be provided as shared
libraries/DLLs at build and runtime.

## Cargo configuration

The Rust dependency is configured as:

```toml
ffmpeg-next = { version = "=8.1.0", default-features = false, features = ["codec", "format", "software-resampling", "software-scaling"] }
```

Do not enable `ffmpeg-next` features named `static`, `build`,
`build-license-gpl`, or `build-license-nonfree` for proprietary distribution.

## FFmpeg library requirements

Use FFmpeg shared libraries configured without GPL or nonfree components:

```text
./configure --disable-static --enable-shared
```

Add any required LGPL-compatible options for your target platform, but do not
add `--enable-gpl` or `--enable-nonfree`.

On Windows, distribute the FFmpeg DLLs next to the game executable or otherwise
make them discoverable through `PATH`. Do not rename the FFmpeg DLLs in an
obfuscated way.

The included `vcpkg.json` requests only the FFmpeg libraries needed by this
prototype with default features disabled. Re-check the actual generated vcpkg
build and `installed/<triplet>/share/ffmpeg/copyright` before distribution.

## Distribution checklist

Before selling or distributing the app:

1. Include an in-app notice such as:
   `This software uses libraries from the FFmpeg project under the LGPLv2.1.`
2. Include FFmpeg's license text with your third-party notices.
3. Provide the exact FFmpeg source code corresponding to the binaries shipped.
4. Include the FFmpeg configure line used for the shipped binaries.
5. Include a diff of any FFmpeg changes you made.
6. Make sure your EULA does not prohibit reverse engineering for debugging
   modifications to the LGPL-covered FFmpeg libraries.
7. Re-check every external library compiled into your FFmpeg build for license
   compatibility and patent/commercial-use considerations.

This file is a project note, not legal advice.

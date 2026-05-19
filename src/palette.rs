use crate::{
    frames::RawFrame,
    palette_lut::{
        PaletteConfig, PaletteLookup, PaletteMatching, load_lookup, load_palette_config,
        sibling_lut_path,
    },
    stats::SharedStats,
    stream_control::CustomStreamState,
};
use crossbeam_channel::Receiver;
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::Write,
    path::Path,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const CODEC_MAGIC: &[u8; 4] = b"IPSC";
const CODEC_VERSION: u8 = 1;
const TILE_SIZE: usize = 8;
const KEYFRAME_INTERVAL: u32 = 25;
const FRAME_HISTORY_SECONDS: u64 = 3;
const OKLCH_HUE_COUNT: usize = 20;
const OKLCH_HUE_OFFSET_DEGREES: f32 = 29.233885;
const OKLCH_LIGHTNESS_LEVELS: [f32; 16] = [
    0.0, 0.06666667, 0.13333334, 0.2, 0.26666668, 0.33333334, 0.4, 0.46666667, 0.53333336, 0.6,
    0.6666667, 0.73333335, 0.8, 0.8666667, 0.93333334, 1.0,
];
const OKLCH_CHROMA_LEVELS: [f32; 3] = [0.08589443, 0.17178887, 0.2576833];

#[derive(Clone, Copy)]
pub(crate) struct PaletteBias {
    pub(crate) lightness: f32,
    pub(crate) chroma: f32,
    pub(crate) hue: f32,
}

impl Default for PaletteBias {
    fn default() -> Self {
        Self {
            lightness: 0.333,
            chroma: 0.333,
            hue: 0.334,
        }
    }
}

impl From<PaletteMatching> for PaletteBias {
    fn from(matching: PaletteMatching) -> Self {
        Self {
            lightness: matching.lightness,
            chroma: matching.chroma,
            hue: matching.hue,
        }
    }
}

impl From<PaletteBias> for PaletteMatching {
    fn from(bias: PaletteBias) -> Self {
        Self {
            lightness: bias.lightness,
            chroma: bias.chroma,
            hue: bias.hue,
        }
    }
}

#[derive(Clone)]
pub(crate) struct SharedPaletteBias(Arc<Mutex<PaletteBias>>);

impl SharedPaletteBias {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Mutex::new(PaletteBias::default())))
    }

    pub(crate) fn get(&self) -> PaletteBias {
        self.0.lock().map(|bias| *bias).unwrap_or_default()
    }

    pub(crate) fn set(&self, bias: PaletteBias) {
        if let Ok(mut current) = self.0.lock() {
            *current = bias;
        }
    }
}

#[derive(Clone)]
pub(crate) struct PaletteFrameHub {
    inner: Arc<(Mutex<LatestPaletteFrame>, Condvar)>,
}

#[derive(Default)]
struct LatestPaletteFrame {
    sequence: u64,
    stream_header: Option<Arc<Vec<u8>>>,
    frame: Option<Arc<Vec<u8>>>,
    latest_keyframe: Option<(u64, Arc<Vec<u8>>)>,
    history: VecDeque<PaletteFrameEntry>,
}

#[derive(Clone)]
struct PaletteFrameEntry {
    sequence: u64,
    published_at: Instant,
    frame: Arc<Vec<u8>>,
    is_keyframe: bool,
}

impl PaletteFrameHub {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new((Mutex::new(LatestPaletteFrame::default()), Condvar::new())),
        }
    }

    fn publish(&self, stream_header: Vec<u8>, frame: Vec<u8>, is_keyframe: bool) {
        let (lock, ready) = &*self.inner;
        if let Ok(mut latest) = lock.lock() {
            latest.sequence += 1;
            let frame = Arc::new(frame);
            latest.stream_header = Some(Arc::new(stream_header));
            latest.frame = Some(frame.clone());
            if is_keyframe {
                latest.latest_keyframe = Some((latest.sequence, frame.clone()));
            }
            let sequence = latest.sequence;
            latest.history.push_back(PaletteFrameEntry {
                sequence,
                published_at: Instant::now(),
                frame,
                is_keyframe,
            });
            prune_palette_history(&mut latest.history);
            ready.notify_all();
        }
    }

    pub(crate) fn stream_header(&self) -> Option<Arc<Vec<u8>>> {
        let (lock, _) = &*self.inner;
        let latest = lock.lock().ok()?;
        latest.stream_header.clone()
    }

    pub(crate) fn wait_for_delayed_keyframe(&self, delay: Duration) -> Option<(u64, Arc<Vec<u8>>)> {
        let (lock, ready) = &*self.inner;
        let mut latest = lock.lock().ok()?;

        loop {
            let cutoff = Instant::now()
                .checked_sub(delay)
                .unwrap_or_else(Instant::now);
            if let Some(entry) = latest
                .history
                .iter()
                .rev()
                .find(|entry| entry.is_keyframe && entry.published_at <= cutoff)
            {
                return Some((entry.sequence, entry.frame.clone()));
            }

            latest = ready.wait(latest).ok()?;
        }
    }

    pub(crate) fn wait_for_delayed_frame_after(
        &self,
        last_sequence: u64,
        delay: Duration,
    ) -> Option<(u64, Arc<Vec<u8>>)> {
        let (lock, ready) = &*self.inner;
        let mut latest = lock.lock().ok()?;

        loop {
            let cutoff = Instant::now()
                .checked_sub(delay)
                .unwrap_or_else(Instant::now);
            if let Some(entry) = latest
                .history
                .iter()
                .find(|entry| entry.sequence > last_sequence && entry.published_at <= cutoff)
            {
                return Some((entry.sequence, entry.frame.clone()));
            }

            latest = ready.wait(latest).ok()?;
        }
    }
}

fn prune_palette_history(history: &mut VecDeque<PaletteFrameEntry>) {
    let cutoff = Instant::now()
        .checked_sub(Duration::from_secs(FRAME_HISTORY_SECONDS))
        .unwrap_or_else(Instant::now);
    while history
        .front()
        .is_some_and(|entry| entry.published_at < cutoff && history.len() > 1)
    {
        history.pop_front();
    }
}

pub(crate) fn start_palette_preview_encoder(
    receiver: Receiver<RawFrame>,
    frame_hub: PaletteFrameHub,
    stats: SharedStats,
    palette_bias: SharedPaletteBias,
    active: CustomStreamState,
    palette_config_path: impl AsRef<Path>,
    use_prebaked_lookup: bool,
) {
    let palette_config_path = palette_config_path.as_ref().to_owned();
    let palette_config = load_palette_config(&palette_config_path).unwrap_or_else(|err| {
        eprintln!("Could not load palette.toml, using default palette: {err}");
        PaletteConfig {
            colors: default_palette(),
            matching: PaletteMatching::default(),
        }
    });
    palette_bias.set(PaletteBias::from(palette_config.matching));

    let lookup = if use_prebaked_lookup {
        load_lookup(sibling_lut_path(&palette_config_path), &palette_config)
            .map_err(|err| {
                eprintln!("Palette LUT unavailable; using live OKLCH matching: {err}");
                err
            })
            .ok()
    } else {
        None
    };

    thread::spawn(move || {
        let mut encoder = IndexedPixelEncoder::new(
            palette_config.colors,
            PaletteBias::from(palette_config.matching),
            lookup,
        );
        let mut recording = None;
        let mut recorded_header = false;

        for raw_frame in receiver {
            if !active.is_active() {
                continue;
            }
            stats.with_mut(|stats| stats.frames_read += 1);
            match encoder.encode(&raw_frame, palette_bias.get()) {
                Ok(encoded) => {
                    if recording.is_none() {
                        match create_recording_file() {
                            Ok((file, path)) => {
                                stats.with_mut(|stats| stats.custom_recording_path = path);
                                recording = Some(file);
                            }
                            Err(err) => {
                                eprintln!("Could not create custom stream recording: {err}")
                            }
                        }
                    }

                    if let Some(recording) = recording.as_mut() {
                        if !recorded_header {
                            let _ = recording.write_all(&encoded.stream_header);
                            recorded_header = true;
                        }
                        let _ = recording.write_all(&(encoded.frame.len() as u32).to_le_bytes());
                        let _ = recording.write_all(&encoded.frame);
                    }

                    let bytes = encoded.frame.len();
                    let is_keyframe = encoded.is_keyframe;
                    let tile_counts = encoded.tile_counts;
                    if !active.is_active() {
                        continue;
                    }
                    frame_hub.publish(encoded.stream_header.clone(), encoded.frame, is_keyframe);
                    stats.with_mut(|stats| {
                        stats.frames_encoded += 1;
                        stats.custom_frames_sent += 1;
                        stats.custom_bytes_sent += bytes as u64;
                        if is_keyframe {
                            stats.custom_keyframes_sent += 1;
                        } else {
                            stats.custom_delta_frames_sent += 1;
                        }
                        stats.custom_raw_tiles_sent += tile_counts.raw;
                        stats.custom_solid_tiles_sent += tile_counts.solid;
                        stats.custom_rle_tiles_sent += tile_counts.rle;
                        stats.custom_span_tiles_sent += tile_counts.span_delta;
                        stats.custom_xor_tiles_sent += tile_counts.xor_rle;
                        stats.custom_skipped_tiles += tile_counts.skipped;
                        stats.custom_stage = "streaming";
                        stats.latest_frame_bytes = bytes;
                    });
                }
                Err(err) => {
                    eprintln!("Indexed pixel frame encode failed: {err}");
                    stats.with_mut(|stats| {
                        stats.custom_stage = "encode error";
                        stats.custom_last_error = err;
                    });
                }
            }
        }
    });
}

struct IndexedPixelEncoder {
    palette: Vec<[u8; 4]>,
    palette_oklab: Vec<Oklab>,
    lookup: Option<PaletteLookup>,
    lookup_matching: PaletteBias,
    previous: Option<Framebuffer>,
    frame_index: u32,
    header: Option<Vec<u8>>,
    header_width: u32,
    header_height: u32,
}

struct EncodedFrame {
    stream_header: Vec<u8>,
    frame: Vec<u8>,
    is_keyframe: bool,
    tile_counts: TileModeCounts,
}

#[derive(Default)]
struct TileModeCounts {
    raw: u64,
    solid: u64,
    rle: u64,
    span_delta: u64,
    xor_rle: u64,
    skipped: u64,
}

impl IndexedPixelEncoder {
    fn new(
        palette: Vec<[u8; 4]>,
        lookup_matching: PaletteBias,
        lookup: Option<PaletteLookup>,
    ) -> Self {
        let palette_oklab = palette
            .iter()
            .map(|[r, g, b, _]| rgb_to_oklab(*r, *g, *b))
            .collect();
        Self {
            palette,
            palette_oklab,
            lookup,
            lookup_matching,
            previous: None,
            frame_index: 0,
            header: None,
            header_width: 0,
            header_height: 0,
        }
    }

    fn encode(&mut self, raw: &RawFrame, bias: PaletteBias) -> Result<EncodedFrame, String> {
        if raw.width != raw.height
            || raw.width == 0
            || raw.width > u8::MAX as u32 + 1
            || raw.width as usize % TILE_SIZE != 0
            || raw.height as usize % TILE_SIZE != 0
        {
            return Err(
                "IPSC frames must be square, 8-aligned, and no larger than 256x256".to_owned(),
            );
        }

        if self.palette.is_empty() || self.palette.len() > 256 {
            return Err("IPSC palette must contain 1-256 colors".to_owned());
        }

        if self.header_width != raw.width || self.header_height != raw.height {
            self.previous = None;
            self.frame_index = 0;
            self.header = Some(stream_header(
                raw.width as u16,
                raw.height as u16,
                &self.palette,
            ));
            self.header_width = raw.width;
            self.header_height = raw.height;
        }

        let current = self.quantize(raw, bias)?;
        let header = self
            .header
            .as_ref()
            .expect("header initialized for current resolution")
            .clone();
        let is_keyframe = self.previous.is_none() || self.frame_index % KEYFRAME_INTERVAL == 0;

        let (payload, tile_counts) = if is_keyframe {
            (encode_keyframe_raw(&current), TileModeCounts::default())
        } else {
            encode_delta_frame(
                &current,
                self.previous.as_ref().expect("previous frame exists"),
            )
        };

        let mut frame = Vec::with_capacity(1 + 4 + 4 + payload.len());
        frame.push(if is_keyframe { 0 } else { 1 });
        frame.extend_from_slice(&self.frame_index.to_le_bytes());
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&payload);

        self.previous = Some(current);
        self.frame_index = self.frame_index.wrapping_add(1);

        Ok(EncodedFrame {
            stream_header: header,
            frame,
            is_keyframe,
            tile_counts,
        })
    }

    fn quantize(&self, raw: &RawFrame, bias: PaletteBias) -> Result<Framebuffer, String> {
        let pixel_count = raw.width as usize * raw.height as usize;
        if raw.bgra.len() < pixel_count * 4 {
            return Err("raw frame is shorter than expected".to_owned());
        }

        let pixels = raw
            .bgra
            .chunks_exact(4)
            .take(pixel_count)
            .map(|pixel| {
                let b = pixel[0];
                let g = pixel[1];
                let r = pixel[2];
                self.palette_index(r, g, b, bias)
            })
            .collect();

        Ok(Framebuffer {
            pixels,
            width: raw.width as usize,
            height: raw.height as usize,
        })
    }

    fn palette_index(&self, r: u8, g: u8, b: u8, bias: PaletteBias) -> u8 {
        if self.lookup_can_match(bias)
            && let Some(lookup) = &self.lookup
        {
            let index = (r as usize) << 16 | (g as usize) << 8 | b as usize;
            return lookup.entries()[index];
        }

        self.nearest_palette_index(r, g, b, bias)
    }

    fn lookup_can_match(&self, bias: PaletteBias) -> bool {
        self.lookup.is_some()
            && (bias.lightness - self.lookup_matching.lightness).abs() <= 0.000_5
            && (bias.chroma - self.lookup_matching.chroma).abs() <= 0.000_5
            && (bias.hue - self.lookup_matching.hue).abs() <= 0.000_5
    }

    fn nearest_palette_index(&self, r: u8, g: u8, b: u8, bias: PaletteBias) -> u8 {
        let color = Oklch::from(rgb_to_oklab(r, g, b));
        let mut best_index = 0;
        let mut best_distance = f32::MAX;

        for (index, palette_color) in self.palette_oklab.iter().copied().take(256).enumerate() {
            let distance = color.biased_distance_squared(Oklch::from(palette_color), bias);
            if distance < best_distance {
                best_distance = distance;
                best_index = index as u8;
            }
        }

        best_index
    }
}

fn stream_header(width: u16, height: u16, palette: &[[u8; 4]]) -> Vec<u8> {
    let mut header = Vec::with_capacity(4 + 1 + 2 + 2 + 1 + 2 + palette.len() * 4);
    header.extend_from_slice(CODEC_MAGIC);
    header.push(CODEC_VERSION);
    header.extend_from_slice(&width.to_le_bytes());
    header.extend_from_slice(&height.to_le_bytes());
    header.push(TILE_SIZE as u8);
    header.extend_from_slice(&(palette.len() as u16).to_le_bytes());
    for color in palette {
        header.extend_from_slice(color);
    }
    header
}

fn default_palette() -> Vec<[u8; 4]> {
    let mut palette = Vec::with_capacity(256);
    for lightness in OKLCH_LIGHTNESS_LEVELS {
        palette.push(greyscale_color(lightness));
    }

    for hue_index in 0..OKLCH_HUE_COUNT {
        let hue_degrees =
            OKLCH_HUE_OFFSET_DEGREES + hue_index as f32 * 360.0 / OKLCH_HUE_COUNT as f32;
        for chroma in OKLCH_CHROMA_LEVELS.iter().copied() {
            for lightness in OKLCH_LIGHTNESS_LEVELS.iter().copied() {
                if lightness > 0.0
                    && lightness < 1.0
                    && let Some(color) = checked_oklch_to_srgb(lightness, chroma, hue_degrees)
                {
                    palette.push(color);
                }
            }
        }
    }
    while palette.len() < 256 {
        palette.push([0x00, 0x00, 0x00, 0xff]);
    }
    palette
}

fn greyscale_color(lightness: f32) -> [u8; 4] {
    if lightness <= 0.0 {
        [0x00, 0x00, 0x00, 0xff]
    } else if lightness >= 1.0 {
        [0xff, 0xff, 0xff, 0xff]
    } else {
        oklch_to_srgb(lightness, 0.0, 0.0)
    }
}

#[derive(Clone, Copy)]
struct Oklab {
    l: f32,
    a: f32,
    b: f32,
}

#[derive(Clone, Copy)]
struct Oklch {
    l: f32,
    c: f32,
    h: f32,
}

impl From<Oklab> for Oklch {
    fn from(color: Oklab) -> Self {
        let c = color.a.hypot(color.b);
        let h = if c <= 0.000_001 {
            0.0
        } else {
            color.b.atan2(color.a)
        };
        Self { l: color.l, c, h }
    }
}

impl Oklch {
    fn biased_distance_squared(self, other: Self, bias: PaletteBias) -> f32 {
        let dl = self.l - other.l;
        let dc = self.c - other.c;
        let dh = (hue_delta(self.h, other.h) * 0.5).sin() * 2.0 * self.c.max(other.c);
        bias.lightness * dl * dl + bias.chroma * dc * dc + bias.hue * dh * dh
    }
}

fn hue_delta(a: f32, b: f32) -> f32 {
    let delta = (a - b).abs() % std::f32::consts::TAU;
    if delta > std::f32::consts::PI {
        std::f32::consts::TAU - delta
    } else {
        delta
    }
}

fn rgb_to_oklab(r: u8, g: u8, b: u8) -> Oklab {
    let r = srgb_to_linear(r as f32 / 255.0);
    let g = srgb_to_linear(g as f32 / 255.0);
    let b = srgb_to_linear(b as f32 / 255.0);

    let l = 0.41222146 * r + 0.53633255 * g + 0.051445995 * b;
    let m = 0.2119035 * r + 0.6806995 * g + 0.10739696 * b;
    let s = 0.08830246 * r + 0.28171884 * g + 0.6299787 * b;

    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();

    Oklab {
        l: 0.21045426 * l_ + 0.7936178 * m_ - 0.004072047 * s_,
        a: 1.9779985 * l_ - 2.4285922 * m_ + 0.4505937 * s_,
        b: 0.025904037 * l_ + 0.78277177 * m_ - 0.80867577 * s_,
    }
}

fn oklch_to_srgb(lightness: f32, chroma: f32, hue_degrees: f32) -> [u8; 4] {
    let (r, g, blue) = oklch_to_linear_srgb(lightness, chroma, hue_degrees);
    [
        linear_to_u8(r.clamp(0.0, 1.0)),
        linear_to_u8(g.clamp(0.0, 1.0)),
        linear_to_u8(blue.clamp(0.0, 1.0)),
        0xff,
    ]
}

fn checked_oklch_to_srgb(lightness: f32, chroma: f32, hue_degrees: f32) -> Option<[u8; 4]> {
    let (r, g, b) = oklch_to_linear_srgb(lightness, chroma, hue_degrees);
    in_srgb_gamut(r, g, b).then(|| oklch_to_srgb(lightness, chroma, hue_degrees))
}

fn oklch_to_linear_srgb(lightness: f32, chroma: f32, hue_degrees: f32) -> (f32, f32, f32) {
    let hue = hue_degrees.to_radians();
    let a = hue.cos() * chroma;
    let b = hue.sin() * chroma;

    let l_ = lightness + 0.39633778 * a + 0.21580376 * b;
    let m_ = lightness - 0.105561346 * a - 0.06385417 * b;
    let s_ = lightness - 0.08948418 * a - 1.2914855 * b;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    (
        4.0767417 * l - 3.3077116 * m + 0.23096994 * s,
        -1.268438 * l + 2.6097574 * m - 0.34131938 * s,
        -0.0041960863 * l - 0.7034186 * m + 1.7076147 * s,
    )
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_u8(value: f32) -> u8 {
    let srgb = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round().clamp(0.0, 255.0) as u8
}

fn in_srgb_gamut(r: f32, g: f32, b: f32) -> bool {
    r.is_finite()
        && g.is_finite()
        && b.is_finite()
        && (0.0..=1.0).contains(&r)
        && (0.0..=1.0).contains(&g)
        && (0.0..=1.0).contains(&b)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Framebuffer {
    pixels: Vec<u8>,
    width: usize,
    height: usize,
}

impl Framebuffer {
    #[cfg(test)]
    fn new(width: usize, height: usize) -> Self {
        Self {
            pixels: vec![0; width * height],
            width,
            height,
        }
    }

    #[cfg(test)]
    fn set_pixel(&mut self, x: usize, y: usize, color: u8) {
        self.pixels[y * self.width + x] = color;
    }
}

fn encode_keyframe_raw(frame: &Framebuffer) -> Vec<u8> {
    frame.pixels.clone()
}

fn encode_delta_frame(current: &Framebuffer, previous: &Framebuffer) -> (Vec<u8>, TileModeCounts) {
    let tiles_x = current.width / TILE_SIZE;
    let tiles_y = current.height / TILE_SIZE;
    let tile_count = tiles_x * tiles_y;
    let mask_len = tile_count.div_ceil(8);
    let mut mask = vec![0u8; mask_len];
    let mut payload = Vec::new();
    let mut counts = TileModeCounts::default();

    for tile_y in 0..tiles_y {
        for tile_x in 0..tiles_x {
            let tile_index = tile_y * tiles_x + tile_x;
            let current_tile = extract_tile(current, tile_x, tile_y);
            let previous_tile = extract_tile(previous, tile_x, tile_y);
            if current_tile == previous_tile {
                counts.skipped += 1;
                continue;
            }

            set_bit(&mut mask, tile_index);
            let encoded_tile = encode_best_tile(&current_tile, &previous_tile);
            counts.record(encoded_tile[0]);
            payload.extend_from_slice(&encoded_tile);
        }
    }

    mask.extend_from_slice(&payload);
    (mask, counts)
}

impl TileModeCounts {
    fn record(&mut self, mode: u8) {
        match mode {
            0 => self.raw += 1,
            1 => self.solid += 1,
            2 => self.rle += 1,
            3 => self.span_delta += 1,
            4 => self.xor_rle += 1,
            _ => {}
        }
    }
}

fn extract_tile(frame: &Framebuffer, tile_x: usize, tile_y: usize) -> [u8; 64] {
    let mut out = [0u8; 64];
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let src_x = tile_x * TILE_SIZE + x;
            let src_y = tile_y * TILE_SIZE + y;
            out[y * TILE_SIZE + x] = frame.pixels[src_y * frame.width + src_x];
        }
    }
    out
}

fn set_bit(mask: &mut [u8], index: usize) {
    mask[index / 8] |= 1 << (index % 8);
}

#[derive(Clone, Copy)]
#[repr(u8)]
enum TileMode {
    Raw = 0,
    Solid = 1,
    Rle = 2,
    SpanDelta = 3,
    XorRle = 4,
}

fn encode_best_tile(current: &[u8; 64], previous: &[u8; 64]) -> Vec<u8> {
    let mut best = encode_raw_tile(current);

    if let Some(solid) = encode_solid_tile(current)
        && solid.len() < best.len()
    {
        best = solid;
    }

    let rle = encode_rle_tile(current);
    if rle.len() < best.len() {
        best = rle;
    }

    let span_delta = encode_span_delta_tile(current, previous);
    if span_delta.len() < best.len() {
        best = span_delta;
    }

    let xor_rle = encode_xor_rle_tile(current, previous);
    if xor_rle.len() < best.len() {
        best = xor_rle;
    }

    best
}

fn encode_raw_tile(tile: &[u8; 64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(65);
    out.push(TileMode::Raw as u8);
    out.extend_from_slice(tile);
    out
}

fn encode_solid_tile(tile: &[u8; 64]) -> Option<Vec<u8>> {
    let first = tile[0];
    tile.iter()
        .all(|&pixel| pixel == first)
        .then_some(vec![TileMode::Solid as u8, first])
}

fn encode_rle_tile(tile: &[u8; 64]) -> Vec<u8> {
    let mut runs = Vec::new();
    let mut color = tile[0];
    let mut len = 1u8;

    for &pixel in &tile[1..] {
        if pixel == color && len < u8::MAX {
            len += 1;
        } else {
            runs.push((color, len));
            color = pixel;
            len = 1;
        }
    }
    runs.push((color, len));

    let mut out = Vec::with_capacity(2 + runs.len() * 2);
    out.push(TileMode::Rle as u8);
    out.push(runs.len() as u8);
    for (color, len) in runs {
        out.push(color);
        out.push(len);
    }
    out
}

fn encode_span_delta_tile(current: &[u8; 64], previous: &[u8; 64]) -> Vec<u8> {
    let mut spans: Vec<(u8, Vec<u8>)> = Vec::new();
    let mut i = 0usize;

    while i < current.len() {
        let mut skip = 0u8;
        while i < current.len() && current[i] == previous[i] {
            skip += 1;
            i += 1;
        }

        if i >= current.len() {
            break;
        }

        let mut pixels = Vec::new();
        while i < current.len() && current[i] != previous[i] {
            pixels.push(current[i]);
            i += 1;
        }
        spans.push((skip, pixels));
    }

    let mut out = Vec::new();
    out.push(TileMode::SpanDelta as u8);
    out.push(spans.len() as u8);
    for (skip, pixels) in spans {
        out.push(skip);
        out.push(pixels.len() as u8);
        out.extend_from_slice(&pixels);
    }
    out
}

fn encode_xor_rle_tile(current: &[u8; 64], previous: &[u8; 64]) -> Vec<u8> {
    let mut diff = [0u8; 64];
    for i in 0..diff.len() {
        diff[i] = current[i] ^ previous[i];
    }

    let mut runs = Vec::new();
    let mut value = diff[0];
    let mut len = 1u8;
    for &diff_byte in &diff[1..] {
        if diff_byte == value && len < u8::MAX {
            len += 1;
        } else {
            runs.push((value, len));
            value = diff_byte;
            len = 1;
        }
    }
    runs.push((value, len));

    let mut out = Vec::with_capacity(2 + runs.len() * 2);
    out.push(TileMode::XorRle as u8);
    out.push(runs.len() as u8);
    for (value, len) in runs {
        out.push(value);
        out.push(len);
    }
    out
}

fn create_recording_file() -> Result<(File, String), String> {
    fs::create_dir_all("recordings").map_err(|err| err.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let path = format!("recordings/custom-{stamp}.ipsc");
    File::create(&path)
        .map(|file| (file, path))
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyframe_roundtrip() {
        let mut frame = Framebuffer::new(128, 128);
        frame.set_pixel(12, 34, 99);
        let encoded = encode_keyframe_raw(&frame);
        assert_eq!(encoded, frame.pixels);
    }

    #[test]
    fn delta_span_roundtrip() {
        let mut previous = Framebuffer::new(128, 128);
        let mut current = previous.clone();
        current.set_pixel(10, 10, 5);
        current.set_pixel(11, 10, 5);
        current.set_pixel(12, 10, 5);

        let (encoded, counts) = encode_delta_frame(&current, &previous);
        assert_eq!(counts.span_delta, 1);
        let decoded = decode_delta_frame_for_test(&previous, &encoded);
        assert_eq!(current, decoded);

        previous = current;
        let (encoded, counts) = encode_delta_frame(&previous, &previous);
        assert_eq!(
            counts.raw + counts.solid + counts.rle + counts.span_delta + counts.xor_rle,
            0
        );
        assert_eq!(encoded.len(), 32);
    }

    #[test]
    fn best_tile_prefers_solid() {
        let previous = [0u8; 64];
        let current = [7u8; 64];
        assert_eq!(encode_best_tile(&current, &previous), vec![1, 7]);
    }

    #[test]
    fn encoder_accepts_256_square_frames() {
        let mut encoder = IndexedPixelEncoder::new(default_palette(), PaletteBias::default(), None);
        let raw = RawFrame {
            bgra: vec![0; 256 * 256 * 4],
            width: 256,
            height: 256,
        };

        let encoded = encoder.encode(&raw, PaletteBias::default()).unwrap();
        assert!(encoded.is_keyframe);
        assert_eq!(&encoded.stream_header[0..4], b"IPSC");
        assert_eq!(encoded.frame.len(), 1 + 4 + 4 + 256 * 256);
    }

    #[test]
    fn default_palette_has_256_entries() {
        assert_eq!(default_palette().len(), 256);
    }

    fn decode_delta_frame_for_test(previous: &Framebuffer, bytes: &[u8]) -> Framebuffer {
        let tiles_x = previous.width / TILE_SIZE;
        let tiles_y = previous.height / TILE_SIZE;
        let tile_count = tiles_x * tiles_y;
        let mask_len = tile_count.div_ceil(8);
        let mask = &bytes[..mask_len];
        let mut cursor = mask_len;
        let mut out = previous.clone();

        for tile_y in 0..tiles_y {
            for tile_x in 0..tiles_x {
                let tile_index = tile_y * tiles_x + tile_x;
                if mask[tile_index / 8] & (1 << (tile_index % 8)) == 0 {
                    continue;
                }

                let previous_tile = extract_tile(previous, tile_x, tile_y);
                let (tile, consumed) = decode_tile_for_test(&previous_tile, &bytes[cursor..]);
                cursor += consumed;
                write_tile_for_test(&mut out, tile_x, tile_y, &tile);
            }
        }

        out
    }

    fn decode_tile_for_test(previous: &[u8; 64], bytes: &[u8]) -> ([u8; 64], usize) {
        match bytes[0] {
            0 => {
                let mut tile = [0u8; 64];
                tile.copy_from_slice(&bytes[1..65]);
                (tile, 65)
            }
            1 => ([bytes[1]; 64], 2),
            2 => decode_rle_for_test(bytes),
            3 => decode_span_for_test(previous, bytes),
            4 => decode_xor_rle_for_test(previous, bytes),
            mode => panic!("unknown mode {mode}"),
        }
    }

    fn decode_rle_for_test(bytes: &[u8]) -> ([u8; 64], usize) {
        let run_count = bytes[1] as usize;
        let mut tile = [0u8; 64];
        let mut out_i = 0;
        let mut cursor = 2;
        for _ in 0..run_count {
            let color = bytes[cursor];
            let len = bytes[cursor + 1] as usize;
            cursor += 2;
            for _ in 0..len {
                tile[out_i] = color;
                out_i += 1;
            }
        }
        (tile, cursor)
    }

    fn decode_span_for_test(previous: &[u8; 64], bytes: &[u8]) -> ([u8; 64], usize) {
        let span_count = bytes[1] as usize;
        let mut tile = *previous;
        let mut tile_i = 0usize;
        let mut cursor = 2usize;
        for _ in 0..span_count {
            let skip = bytes[cursor] as usize;
            let len = bytes[cursor + 1] as usize;
            cursor += 2;
            tile_i += skip;
            tile[tile_i..tile_i + len].copy_from_slice(&bytes[cursor..cursor + len]);
            cursor += len;
            tile_i += len;
        }
        (tile, cursor)
    }

    fn decode_xor_rle_for_test(previous: &[u8; 64], bytes: &[u8]) -> ([u8; 64], usize) {
        let run_count = bytes[1] as usize;
        let mut tile = [0u8; 64];
        let mut out_i = 0;
        let mut cursor = 2;
        for _ in 0..run_count {
            let value = bytes[cursor];
            let len = bytes[cursor + 1] as usize;
            cursor += 2;
            for _ in 0..len {
                tile[out_i] = previous[out_i] ^ value;
                out_i += 1;
            }
        }
        (tile, cursor)
    }

    fn write_tile_for_test(frame: &mut Framebuffer, tile_x: usize, tile_y: usize, tile: &[u8; 64]) {
        for y in 0..TILE_SIZE {
            for x in 0..TILE_SIZE {
                let dst_x = tile_x * TILE_SIZE + x;
                let dst_y = tile_y * TILE_SIZE + y;
                frame.pixels[dst_y * frame.width + dst_x] = tile[y * TILE_SIZE + x];
            }
        }
    }
}

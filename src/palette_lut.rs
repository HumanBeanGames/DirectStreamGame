use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const LUT_ENTRY_COUNT: usize = 256 * 256 * 256;
const LUT_MAGIC: &[u8; 8] = b"IPSMAP1\0";

#[derive(Clone, Copy, Debug)]
pub struct PaletteMatching {
    pub lightness: f32,
    pub chroma: f32,
    pub hue: f32,
}

impl Default for PaletteMatching {
    fn default() -> Self {
        Self {
            lightness: 0.333,
            chroma: 0.333,
            hue: 0.334,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PaletteConfig {
    pub colors: Vec<[u8; 4]>,
    pub matching: PaletteMatching,
}

pub struct PaletteLookup {
    hash: u64,
    entries: Vec<u8>,
}

impl PaletteLookup {
    pub fn entries(&self) -> &[u8] {
        &self.entries
    }

    pub fn hash(&self) -> u64 {
        self.hash
    }
}

pub fn load_palette_config(path: impl AsRef<Path>) -> Result<PaletteConfig, String> {
    let contents = fs::read_to_string(path).map_err(|err| err.to_string())?;
    parse_palette_config(&contents)
}

pub fn parse_palette_config(contents: &str) -> Result<PaletteConfig, String> {
    let mut colors = Vec::new();
    let mut matching = PaletteMatching::default();
    let mut section = "";

    for raw_line in contents.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        let raw_trimmed = raw_line.trim();
        if raw_trimmed.is_empty() {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            section = line.trim_matches(['[', ']']).trim();
            continue;
        }

        if section == "matching" {
            if let Some((key, value)) = line.split_once('=') {
                match key.trim() {
                    "lightness" | "lightness_weight" | "value" | "value_weight" => {
                        let value = parse_f32_value(value)?;
                        matching.lightness = value
                    }
                    "chroma" | "chroma_weight" => matching.chroma = parse_f32_value(value)?,
                    "hue" | "hue_weight" => matching.hue = parse_f32_value(value)?,
                    _ => {}
                }
            }
        }

        for quoted in raw_line.split('"').skip(1).step_by(2) {
            if let Some(color) = parse_hex_color(quoted) {
                colors.push(color?);
            }
        }
    }

    if colors.is_empty() {
        Err("palette contains no quoted #RRGGBB colors".to_owned())
    } else if colors.len() > 256 {
        Err("palette contains more than 256 colors".to_owned())
    } else {
        Ok(PaletteConfig { colors, matching })
    }
}

fn parse_f32_value(value: &str) -> Result<f32, String> {
    value
        .trim()
        .trim_matches(',')
        .parse::<f32>()
        .map_err(|err| err.to_string())
}

pub fn sibling_lut_path(path: impl AsRef<Path>) -> PathBuf {
    path.as_ref().with_extension("ipsmap")
}

pub fn build_lookup(config: &PaletteConfig) -> Vec<u8> {
    let palette_oklch = config
        .colors
        .iter()
        .map(|[r, g, b, _]| Oklch::from(rgb_to_oklab(*r, *g, *b)))
        .collect::<Vec<_>>();
    let mut entries = Vec::with_capacity(LUT_ENTRY_COUNT);

    for r in 0..=255u8 {
        for g in 0..=255u8 {
            for b in 0..=255u8 {
                entries.push(nearest_palette_index(
                    Oklch::from(rgb_to_oklab(r, g, b)),
                    &palette_oklch,
                    config.matching,
                ));
            }
        }
    }

    entries
}

pub fn write_lookup(
    path: impl AsRef<Path>,
    config: &PaletteConfig,
    entries: &[u8],
) -> Result<(), String> {
    let bytes = encode_lookup(config, entries)?;
    fs::File::create(path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|err| err.to_string())
}

pub fn encode_lookup(config: &PaletteConfig, entries: &[u8]) -> Result<Vec<u8>, String> {
    if entries.len() != LUT_ENTRY_COUNT {
        return Err(format!(
            "LUT must contain {LUT_ENTRY_COUNT} entries, got {}",
            entries.len()
        ));
    }

    let mut bytes = Vec::with_capacity(30 + entries.len());
    bytes.extend_from_slice(LUT_MAGIC);
    bytes.extend_from_slice(&palette_hash(config).to_le_bytes());
    bytes.extend_from_slice(&(config.colors.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&config.matching.lightness.to_le_bytes());
    bytes.extend_from_slice(&config.matching.chroma.to_le_bytes());
    bytes.extend_from_slice(&config.matching.hue.to_le_bytes());
    bytes.extend_from_slice(entries);
    Ok(bytes)
}

pub fn load_lookup(
    path: impl AsRef<Path>,
    config: &PaletteConfig,
) -> Result<PaletteLookup, String> {
    let mut file = fs::File::open(path).map_err(|err| err.to_string())?;
    let mut header = [0u8; 30];
    file.read_exact(&mut header)
        .map_err(|err| err.to_string())?;

    if &header[0..8] != LUT_MAGIC {
        return Err("LUT magic/version mismatch".to_owned());
    }

    let hash = u64::from_le_bytes(header[8..16].try_into().expect("header slice length"));
    let expected_hash = palette_hash(config);
    if hash != expected_hash {
        return Err("LUT does not match palette colors and matching weights".to_owned());
    }

    let color_count = u16::from_le_bytes(header[16..18].try_into().expect("header slice length"));
    if color_count as usize != config.colors.len() {
        return Err("LUT color count does not match palette".to_owned());
    }

    let mut entries = Vec::new();
    file.read_to_end(&mut entries)
        .map_err(|err| err.to_string())?;
    if entries.len() != LUT_ENTRY_COUNT {
        return Err(format!(
            "LUT has {} entries, expected {LUT_ENTRY_COUNT}",
            entries.len()
        ));
    }

    Ok(PaletteLookup { hash, entries })
}

pub fn palette_hash(config: &PaletteConfig) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fn feed(hash: &mut u64, byte: u8) {
        *hash ^= byte as u64;
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    for color in &config.colors {
        for byte in color {
            feed(&mut hash, *byte);
        }
    }
    for value in [
        config.matching.lightness,
        config.matching.chroma,
        config.matching.hue,
    ] {
        for byte in value.to_le_bytes() {
            feed(&mut hash, byte);
        }
    }
    hash
}

fn parse_hex_color(value: &str) -> Option<Result<[u8; 4], String>> {
    let color = value.trim().trim_start_matches('#');
    if color.len() != 6 && color.len() != 8 {
        return None;
    }

    Some((|| {
        let r = u8::from_str_radix(&color[0..2], 16).map_err(|err| err.to_string())?;
        let g = u8::from_str_radix(&color[2..4], 16).map_err(|err| err.to_string())?;
        let b = u8::from_str_radix(&color[4..6], 16).map_err(|err| err.to_string())?;
        let a = if color.len() == 8 {
            u8::from_str_radix(&color[6..8], 16).map_err(|err| err.to_string())?
        } else {
            0xff
        };
        Ok([r, g, b, a])
    })())
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

fn nearest_palette_index(color: Oklch, palette: &[Oklch], matching: PaletteMatching) -> u8 {
    let mut best_index = 0;
    let mut best_distance = f32::MAX;

    for (index, palette_color) in palette.iter().copied().take(256).enumerate() {
        let distance = biased_distance_squared(color, palette_color, matching);
        if distance < best_distance {
            best_distance = distance;
            best_index = index as u8;
        }
    }

    best_index
}

fn biased_distance_squared(a: Oklch, b: Oklch, matching: PaletteMatching) -> f32 {
    let dl = a.l - b.l;
    let dc = a.c - b.c;
    let dh = (hue_delta(a.h, b.h) * 0.5).sin() * 2.0 * a.c.max(b.c);
    matching.lightness * dl * dl + matching.chroma * dc * dc + matching.hue * dh * dh
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

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

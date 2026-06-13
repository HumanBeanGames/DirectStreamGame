use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const LUT_ENTRY_COUNT: usize = 256 * 256 * 256;
pub const DEFAULT_PALETTE_TOML: &str = include_str!("default_palette/default_palette.toml");
pub const DEFAULT_PALETTE_IPSMAP: &[u8] = include_bytes!("default_palette/default_palette.ipsmap");
const LUT_MAGIC: &[u8; 8] = b"IPSMAP1\0";

#[derive(Clone, Copy, Debug)]
pub struct PaletteMatching {
    pub lightness: f32,
    pub chroma: f32,
    pub hue: f32,
    pub lightness_multiply: f32,
    pub lightness_add: f32,
    pub chroma_multiply: f32,
    pub chroma_add: f32,
    pub hue_add: f32,
    pub preserve_dark_neutrals: bool,
    pub dark_neutral_luma_threshold: f32,
    pub dark_neutral_chroma_threshold: f32,
    pub dark_neutral_chroma_weight_scale: f32,
}

impl Default for PaletteMatching {
    fn default() -> Self {
        Self {
            lightness: 0.333,
            chroma: 0.333,
            hue: 0.334,
            lightness_multiply: 0.0,
            lightness_add: 0.0,
            chroma_multiply: 0.0,
            chroma_add: 0.0,
            hue_add: 0.0,
            preserve_dark_neutrals: false,
            dark_neutral_luma_threshold: 0.18,
            dark_neutral_chroma_threshold: 0.045,
            dark_neutral_chroma_weight_scale: 8.0,
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

pub fn default_palette_config() -> Result<PaletteConfig, String> {
    parse_palette_config(DEFAULT_PALETTE_TOML)
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
                    "lightness_multiply" | "value_multiply" => {
                        matching.lightness_multiply = parse_f32_value(value)?
                    }
                    "lightness_add" | "value_add" => {
                        matching.lightness_add = parse_f32_value(value)?
                    }
                    "chroma_multiply" => matching.chroma_multiply = parse_f32_value(value)?,
                    "chroma_add" => matching.chroma_add = parse_f32_value(value)?,
                    "hue_add" => matching.hue_add = parse_f32_value(value)?,
                    "preserve_dark_neutrals" => {
                        matching.preserve_dark_neutrals = parse_bool_value(value)?
                    }
                    "dark_neutral_luma_threshold" => {
                        matching.dark_neutral_luma_threshold = parse_f32_value(value)?
                    }
                    "dark_neutral_chroma_threshold" => {
                        matching.dark_neutral_chroma_threshold = parse_f32_value(value)?
                    }
                    "dark_neutral_chroma_weight_scale" => {
                        matching.dark_neutral_chroma_weight_scale = parse_f32_value(value)?
                    }
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

fn parse_bool_value(value: &str) -> Result<bool, String> {
    match value.trim().trim_matches(',').to_ascii_lowercase().as_str() {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("invalid boolean value: {other}")),
    }
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
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    decode_lookup(&bytes, config)
}

pub fn default_palette_lookup(config: &PaletteConfig) -> Result<PaletteLookup, String> {
    decode_lookup(DEFAULT_PALETTE_IPSMAP, config)
}

pub fn decode_lookup(bytes: &[u8], config: &PaletteConfig) -> Result<PaletteLookup, String> {
    if bytes.len() < 30 {
        return Err(format!(
            "LUT has {} bytes, expected at least 30",
            bytes.len()
        ));
    }

    let header = &bytes[0..30];

    if &header[0..8] != LUT_MAGIC {
        return Err("LUT magic/version mismatch".to_owned());
    }

    let hash = u64::from_le_bytes(header[8..16].try_into().expect("header slice length"));
    let expected_hash = palette_hash(config);
    if hash != expected_hash {
        return Err("LUT does not match palette colors and matching settings".to_owned());
    }

    let color_count = u16::from_le_bytes(header[16..18].try_into().expect("header slice length"));
    if color_count as usize != config.colors.len() {
        return Err("LUT color count does not match palette".to_owned());
    }

    let entries = bytes[30..].to_vec();
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
    if config.matching.has_input_offset() {
        for value in [
            config.matching.lightness_multiply,
            config.matching.lightness_add,
            config.matching.chroma_multiply,
            config.matching.chroma_add,
            config.matching.hue_add,
        ] {
            for byte in value.to_le_bytes() {
                feed(&mut hash, byte);
            }
        }
    }
    if config.matching.has_dark_neutral_preservation() {
        feed(&mut hash, u8::from(config.matching.preserve_dark_neutrals));
        for value in [
            config.matching.dark_neutral_luma_threshold,
            config.matching.dark_neutral_chroma_threshold,
            config.matching.dark_neutral_chroma_weight_scale,
        ] {
            for byte in value.to_le_bytes() {
                feed(&mut hash, byte);
            }
        }
    }
    hash
}

impl PaletteMatching {
    pub fn has_input_offset(&self) -> bool {
        [
            self.lightness_multiply,
            self.lightness_add,
            self.chroma_multiply,
            self.chroma_add,
            self.hue_add,
        ]
        .iter()
        .any(|value| value.abs() > 0.000_001)
    }

    pub fn has_dark_neutral_preservation(&self) -> bool {
        self.preserve_dark_neutrals
    }
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
    let color = apply_input_offset(color, matching);
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

fn apply_input_offset(color: Oklch, matching: PaletteMatching) -> Oklch {
    Oklch {
        l: (color.l * (1.0 + matching.lightness_multiply) + matching.lightness_add).clamp(0.0, 1.0),
        c: (color.c * (1.0 + matching.chroma_multiply) + matching.chroma_add).max(0.0),
        h: color.h + matching.hue_add * std::f32::consts::TAU,
    }
}

fn biased_distance_squared(a: Oklch, b: Oklch, matching: PaletteMatching) -> f32 {
    let dl = a.l - b.l;
    let dc = a.c - b.c;
    let dh = (hue_delta(a.h, b.h) * 0.5).sin() * 2.0 * a.c.max(b.c);
    let chroma_weight = if matching.preserve_dark_neutrals
        && a.l <= matching.dark_neutral_luma_threshold
        && a.c <= matching.dark_neutral_chroma_threshold
    {
        matching.chroma * matching.dark_neutral_chroma_weight_scale.max(1.0)
    } else {
        matching.chroma
    };
    matching.lightness * dl * dl + chroma_weight * dc * dc + matching.hue * dh * dh
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_default_palette_and_lookup_match() {
        let config = default_palette_config().expect("embedded default palette parses");

        assert!(!config.colors.is_empty());
        assert!(config.colors.len() <= 256);

        let lookup = default_palette_lookup(&config).expect("embedded default lookup matches");

        assert_eq!(lookup.entries().len(), LUT_ENTRY_COUNT);
        assert_eq!(lookup.hash(), palette_hash(&config));
    }

    #[test]
    fn input_offsets_affect_lookup_entries_and_hash() {
        let palette = [
            Oklch {
                l: 0.0,
                c: 0.0,
                h: 0.0,
            },
            Oklch {
                l: 1.0,
                c: 0.0,
                h: 0.0,
            },
        ];
        let source = Oklch {
            l: 0.4,
            c: 0.0,
            h: 0.0,
        };
        let plain = PaletteMatching {
            lightness: 1.0,
            chroma: 0.0,
            hue: 0.0,
            ..Default::default()
        };
        let shifted = PaletteMatching {
            lightness_add: 0.4,
            ..plain
        };
        let plain_config = PaletteConfig {
            colors: vec![[0, 0, 0, 255], [255, 255, 255, 255]],
            matching: plain,
        };
        let shifted_config = PaletteConfig {
            matching: shifted,
            ..plain_config.clone()
        };

        assert_eq!(nearest_palette_index(source, &palette, plain), 0);
        assert_eq!(nearest_palette_index(source, &palette, shifted), 1);
        assert_ne!(palette_hash(&plain_config), palette_hash(&shifted_config));
    }

    #[test]
    fn dark_neutral_preservation_prefers_dark_grey_over_saturated_color() {
        let palette = [
            Oklch::from(rgb_to_oklab(8, 8, 10)),
            Oklch::from(rgb_to_oklab(64, 0, 96)),
        ];
        let source = Oklch::from(rgb_to_oklab(8, 4, 12));
        let preserve = PaletteMatching {
            lightness: 0.05,
            chroma: 0.05,
            hue: 0.90,
            preserve_dark_neutrals: true,
            dark_neutral_luma_threshold: 0.25,
            dark_neutral_chroma_threshold: 0.08,
            dark_neutral_chroma_weight_scale: 64.0,
            ..Default::default()
        };
        let config = PaletteConfig {
            colors: vec![[8, 8, 10, 255], [64, 0, 96, 255]],
            matching: preserve,
        };
        let plain_config = PaletteConfig {
            matching: PaletteMatching {
                preserve_dark_neutrals: false,
                ..preserve
            },
            ..config.clone()
        };

        assert_eq!(nearest_palette_index(source, &palette, preserve), 0);
        assert_ne!(palette_hash(&config), palette_hash(&plain_config));
    }
}

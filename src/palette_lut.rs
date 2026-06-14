use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub const LUT_ENTRY_COUNT: usize = 256 * 256 * 256;
const LUT_MAGIC_V1: &[u8; 8] = b"IPSMAP1\0";
const LUT_MAGIC_V2: &[u8; 8] = b"IPSMAP2\0";
const LUT_V1_HEADER_LEN: usize = 30;
const LUT_V2_HEADER_LEN: usize = 24;
const PALETTE_MATCHING_ALGORITHM_VERSION: &[u8] = b"oklch-gamut-clamped-v1";

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
    config: PaletteConfig,
    palette_toml: Option<String>,
}

impl PaletteLookup {
    pub fn entries(&self) -> &[u8] {
        &self.entries
    }

    pub fn hash(&self) -> u64 {
        self.hash
    }

    pub fn config(&self) -> &PaletteConfig {
        &self.config
    }

    pub fn palette_toml(&self) -> Option<&str> {
        self.palette_toml.as_deref()
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
                    "lightness_multiply" | "value_multiply" => {
                        matching.lightness_multiply = parse_f32_value(value)?
                    }
                    "lightness_add" | "value_add" => {
                        matching.lightness_add = parse_f32_value(value)?
                    }
                    "chroma_multiply" => matching.chroma_multiply = parse_f32_value(value)?,
                    "chroma_add" => matching.chroma_add = parse_f32_value(value)?,
                    "hue_add" => matching.hue_add = parse_f32_value(value)?,
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

pub fn serialize_palette_config(config: &PaletteConfig) -> String {
    let mut output = String::new();
    output.push_str("colors = [\n");
    for [r, g, b, a] in &config.colors {
        if *a == 0xff {
            output.push_str(&format!("    \"#{r:02x}{g:02x}{b:02x}\",\n"));
        } else {
            output.push_str(&format!("    \"#{r:02x}{g:02x}{b:02x}{a:02x}\",\n"));
        }
    }
    output.push_str("]\n\n[matching]\n");
    output.push_str(&format!("lightness = {}\n", config.matching.lightness));
    output.push_str(&format!("chroma = {}\n", config.matching.chroma));
    output.push_str(&format!("hue = {}\n", config.matching.hue));
    output.push_str(&format!(
        "lightness_multiply = {}\n",
        config.matching.lightness_multiply
    ));
    output.push_str(&format!(
        "lightness_add = {}\n",
        config.matching.lightness_add
    ));
    output.push_str(&format!(
        "chroma_multiply = {}\n",
        config.matching.chroma_multiply
    ));
    output.push_str(&format!("chroma_add = {}\n", config.matching.chroma_add));
    output.push_str(&format!("hue_add = {}\n", config.matching.hue_add));
    output
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
    encode_lookup_with_palette_toml(config, &serialize_palette_config(config), entries)
}

pub fn write_lookup_with_palette_toml(
    path: impl AsRef<Path>,
    config: &PaletteConfig,
    palette_toml: &str,
    entries: &[u8],
) -> Result<(), String> {
    let bytes = encode_lookup_with_palette_toml(config, palette_toml, entries)?;
    fs::File::create(path)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|err| err.to_string())
}

pub fn encode_lookup_with_palette_toml(
    config: &PaletteConfig,
    palette_toml: &str,
    entries: &[u8],
) -> Result<Vec<u8>, String> {
    if entries.len() != LUT_ENTRY_COUNT {
        return Err(format!(
            "LUT must contain {LUT_ENTRY_COUNT} entries, got {}",
            entries.len()
        ));
    }
    let embedded_config = parse_palette_config(palette_toml)?;
    let hash = palette_hash(config);
    if palette_hash(&embedded_config) != hash {
        return Err("embedded palette TOML does not match palette config".to_owned());
    }
    let toml_bytes = palette_toml.as_bytes();
    let toml_len = u32::try_from(toml_bytes.len())
        .map_err(|_| "palette TOML is too large to embed in LUT".to_owned())?;

    let mut bytes = Vec::with_capacity(LUT_V2_HEADER_LEN + toml_bytes.len() + entries.len());
    bytes.extend_from_slice(LUT_MAGIC_V2);
    bytes.extend_from_slice(&hash.to_le_bytes());
    bytes.extend_from_slice(&toml_len.to_le_bytes());
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    bytes.extend_from_slice(toml_bytes);
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

pub fn load_lookup_bundle(path: impl AsRef<Path>) -> Result<PaletteLookup, String> {
    let bytes = fs::read(path).map_err(|err| err.to_string())?;
    decode_lookup_bundle(&bytes)
}

pub fn decode_lookup(bytes: &[u8], config: &PaletteConfig) -> Result<PaletteLookup, String> {
    if bytes.len() >= LUT_MAGIC_V2.len() && &bytes[0..8] == LUT_MAGIC_V2 {
        let lookup = decode_lookup_bundle(bytes)?;
        let expected_hash = palette_hash(config);
        if lookup.hash != expected_hash {
            return Err("LUT does not match palette colors and matching settings".to_owned());
        }
        return Ok(lookup);
    }

    if bytes.len() < LUT_V1_HEADER_LEN {
        return Err(format!(
            "LUT has {} bytes, expected at least {LUT_V1_HEADER_LEN}",
            bytes.len()
        ));
    }

    let header = &bytes[0..LUT_V1_HEADER_LEN];

    if &header[0..8] != LUT_MAGIC_V1 {
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

    let entries = bytes[LUT_V1_HEADER_LEN..].to_vec();
    if entries.len() != LUT_ENTRY_COUNT {
        return Err(format!(
            "LUT has {} entries, expected {LUT_ENTRY_COUNT}",
            entries.len()
        ));
    }

    Ok(PaletteLookup {
        hash,
        entries,
        config: config.clone(),
        palette_toml: None,
    })
}

pub fn decode_lookup_bundle(bytes: &[u8]) -> Result<PaletteLookup, String> {
    if bytes.len() < LUT_V2_HEADER_LEN {
        return Err(format!(
            "LUT has {} bytes, expected at least {LUT_V2_HEADER_LEN}",
            bytes.len()
        ));
    }
    let header = &bytes[0..LUT_V2_HEADER_LEN];
    if &header[0..8] != LUT_MAGIC_V2 {
        return Err("LUT is not a self-contained IPSMAP2 lookup".to_owned());
    }
    let hash = u64::from_le_bytes(header[8..16].try_into().expect("header slice length"));
    let toml_len =
        u32::from_le_bytes(header[16..20].try_into().expect("header slice length")) as usize;
    let entry_count =
        u32::from_le_bytes(header[20..24].try_into().expect("header slice length")) as usize;
    if entry_count != LUT_ENTRY_COUNT {
        return Err(format!(
            "LUT header declares {entry_count} entries, expected {LUT_ENTRY_COUNT}"
        ));
    }
    let entries_start = LUT_V2_HEADER_LEN
        .checked_add(toml_len)
        .ok_or_else(|| "LUT embedded palette TOML length overflowed".to_owned())?;
    let expected_len = entries_start
        .checked_add(LUT_ENTRY_COUNT)
        .ok_or_else(|| "LUT entry length overflowed".to_owned())?;
    if bytes.len() != expected_len {
        return Err(format!(
            "LUT has {} bytes, expected {expected_len} for embedded palette and entries",
            bytes.len()
        ));
    }
    let palette_toml = std::str::from_utf8(&bytes[LUT_V2_HEADER_LEN..entries_start])
        .map_err(|err| format!("embedded palette TOML is not UTF-8: {err}"))?
        .to_owned();
    let config = parse_palette_config(&palette_toml)?;
    if palette_hash(&config) != hash {
        return Err("embedded palette TOML does not match LUT hash".to_owned());
    }
    let entries = bytes[entries_start..].to_vec();
    Ok(PaletteLookup {
        hash,
        entries,
        config,
        palette_toml: Some(palette_toml),
    })
}

pub fn palette_hash(config: &PaletteConfig) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    fn feed(hash: &mut u64, byte: u8) {
        *hash ^= byte as u64;
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    for byte in PALETTE_MATCHING_ALGORITHM_VERSION {
        feed(&mut hash, *byte);
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
    let mut adjusted = Oklch {
        l: (color.l * (1.0 + matching.lightness_multiply) + matching.lightness_add).clamp(0.0, 1.0),
        c: (color.c * (1.0 + matching.chroma_multiply) + matching.chroma_add).max(0.0),
        h: color.h + matching.hue_add * std::f32::consts::TAU,
    };
    if matching.has_input_offset() {
        adjusted.c = clamp_chroma_to_srgb_gamut(adjusted);
    }
    adjusted
}

fn clamp_chroma_to_srgb_gamut(color: Oklch) -> f32 {
    if color.c <= 0.0 || !color.l.is_finite() || !color.c.is_finite() || !color.h.is_finite() {
        return 0.0;
    }

    if oklch_in_srgb_gamut(color) {
        return color.c;
    }

    let mut low = 0.0;
    let mut high = color.c;
    for _ in 0..16 {
        let mid = (low + high) * 0.5;
        let candidate = Oklch { c: mid, ..color };
        if oklch_in_srgb_gamut(candidate) {
            low = mid;
        } else {
            high = mid;
        }
    }
    low
}

fn oklch_in_srgb_gamut(color: Oklch) -> bool {
    let (r, g, b) = oklch_to_linear_srgb(color);
    in_srgb_gamut(r, g, b)
}

fn oklch_to_linear_srgb(color: Oklch) -> (f32, f32, f32) {
    let a = color.h.cos() * color.c;
    let b = color.h.sin() * color.c;

    let l_ = color.l + 0.39633778 * a + 0.21580376 * b;
    let m_ = color.l - 0.105561346 * a - 0.06385417 * b;
    let s_ = color.l - 0.08948418 * a - 1.2914855 * b;

    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;

    (
        4.0767417 * l - 3.3077116 * m + 0.23096994 * s,
        -1.268438 * l + 2.6097574 * m - 0.34131938 * s,
        -0.0041960863 * l - 0.7034186 * m + 1.7076147 * s,
    )
}

fn in_srgb_gamut(r: f32, g: f32, b: f32) -> bool {
    const EPSILON: f32 = 0.000_001;
    r.is_finite()
        && g.is_finite()
        && b.is_finite()
        && r >= -EPSILON
        && r <= 1.0 + EPSILON
        && g >= -EPSILON
        && g <= 1.0 + EPSILON
        && b >= -EPSILON
        && b <= 1.0 + EPSILON
}

fn biased_distance_squared(a: Oklch, b: Oklch, matching: PaletteMatching) -> f32 {
    let dl = a.l - b.l;
    let dc = a.c - b.c;
    let dh = (hue_delta(a.h, b.h) * 0.5).sin() * 2.0 * (a.c * b.c).sqrt();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_config_parses() {
        let config = parse_palette_config(
            r##"
colors = [
    "#000000",
    "#ffffff",
]
"##,
        )
        .expect("palette parses");

        assert!(!config.colors.is_empty());
        assert!(config.colors.len() <= 256);
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
    fn self_contained_lookup_embeds_palette_config() {
        let toml = r##"
colors = [
    "#000000",
    "#ffffff",
]

[matching]
lightness = 1.0
chroma = 0.0
hue = 0.0
"##;
        let config = parse_palette_config(toml).expect("test palette parses");
        let entries = vec![0u8; LUT_ENTRY_COUNT];
        let bytes =
            encode_lookup_with_palette_toml(&config, toml, &entries).expect("lookup encodes");
        let lookup = decode_lookup_bundle(&bytes).expect("lookup decodes");

        assert_eq!(lookup.entries().len(), LUT_ENTRY_COUNT);
        assert_eq!(lookup.hash(), palette_hash(&config));
        assert_eq!(lookup.config().colors, config.colors);
        assert_eq!(lookup.palette_toml(), Some(toml));
    }

    #[test]
    fn hue_distance_does_not_penalize_neutral_palette_colors() {
        let palette = [
            Oklch::from(rgb_to_oklab(72, 72, 72)),
            Oklch::from(rgb_to_oklab(60, 66, 117)),
        ];
        let source = Oklch::from(rgb_to_oklab(62, 63, 66));
        let matching = PaletteMatching {
            lightness: 0.251,
            chroma: 0.233,
            hue: 0.516,
            chroma_add: 0.031,
            ..Default::default()
        };

        assert_eq!(nearest_palette_index(source, &palette, matching), 0);
    }

    #[test]
    fn chroma_offset_is_clamped_to_reachable_srgb_gamut() {
        let source = Oklch::from(rgb_to_oklab(0, 17, 17));
        let matching = PaletteMatching {
            chroma_add: 0.031,
            ..Default::default()
        };
        let unclamped_chroma = source.c + matching.chroma_add;
        let adjusted = apply_input_offset(source, matching);

        assert!(adjusted.c < unclamped_chroma);
        assert!(oklch_in_srgb_gamut(adjusted));
    }
}

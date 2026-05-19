use crate::{DirectStreamFrame, DirectStreamFrameAppExt};
use bevy::prelude::*;
use std::sync::{Arc, Mutex};

pub struct DirectTextPlugin;

const BITMAP_FONT_WIDTH: usize = 3;
const BITMAP_FONT_HEIGHT: usize = 5;
const BITMAP_FONT_ADVANCE: f32 = 4.0;
const BITMAP_FONT_LINE_HEIGHT: f32 = 6.0;
const DEFAULT_DIRECT_TEXT_FONT_SIZE: f32 = BITMAP_FONT_HEIGHT as f32;
const GLYPH_ON_THRESHOLD: f32 = 0.5;

impl Plugin for DirectTextPlugin {
    fn build(&self, app: &mut App) {
        let state = DirectTextState::default();
        let shared = state.entries.clone();
        app.insert_resource(state)
            .add_direct_stream_frame_processor(move |mut frame| {
                if let Ok(entries) = shared.lock() {
                    draw_direct_text_entries(&mut frame, &entries);
                }
            })
            .add_systems(Update, sync_direct_text_entries);
    }
}

#[derive(Component, Clone)]
pub struct DirectText {
    pub text: String,
    pub x: u32,
    pub y: u32,
    pub font_size: f32,
    pub threshold: Option<f32>,
    pub color: Srgba,
}

impl DirectText {
    pub fn new(text: impl Into<String>, x: u32, y: u32) -> Self {
        Self {
            text: text.into(),
            x,
            y,
            font_size: DEFAULT_DIRECT_TEXT_FONT_SIZE,
            threshold: None,
            color: Srgba::WHITE,
        }
    }

    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size.max(0.1);
        self
    }

    pub fn with_scale(mut self, scale: f32) -> Self {
        self.font_size = (DEFAULT_DIRECT_TEXT_FONT_SIZE * scale).max(0.1);
        self
    }

    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = Some(threshold.clamp(0.0, 1.0));
        self
    }

    pub fn with_color(mut self, color: Srgba) -> Self {
        self.color = color;
        self
    }
}

#[derive(Clone)]
struct DirectTextEntry {
    text: String,
    x: u32,
    y: u32,
    font_size: f32,
    threshold: Option<f32>,
    color: [u8; 4],
}

#[derive(Resource, Default)]
struct DirectTextState {
    entries: Arc<Mutex<Vec<DirectTextEntry>>>,
}

fn sync_direct_text_entries(state: Res<DirectTextState>, query: Query<&DirectText>) {
    let Ok(mut entries) = state.entries.lock() else {
        return;
    };
    entries.clear();
    entries.extend(query.iter().map(|text| {
        let color = text.color;
        DirectTextEntry {
            text: text.text.clone(),
            x: text.x,
            y: text.y,
            font_size: text.font_size.max(0.1),
            threshold: text.threshold.map(|threshold| threshold.clamp(0.0, 1.0)),
            color: [
                (color.blue * 255.0).round() as u8,
                (color.green * 255.0).round() as u8,
                (color.red * 255.0).round() as u8,
                (color.alpha * 255.0).round() as u8,
            ],
        }
    }));
}

fn draw_direct_text_entries(frame: &mut DirectStreamFrame<'_>, entries: &[DirectTextEntry]) {
    for entry in entries {
        draw_text(frame, entry);
    }
}

fn draw_text(frame: &mut DirectStreamFrame<'_>, entry: &DirectTextEntry) {
    let scale = resolve_bitmap_scale(entry.font_size);
    let threshold = entry.threshold.unwrap_or(GLYPH_ON_THRESHOLD).clamp(0.0, 1.0);
    let mut cursor_x = entry.x as f32;
    let mut cursor_y = entry.y as f32;
    let start_x = cursor_x;
    let advance = BITMAP_FONT_ADVANCE * scale;
    let line_height = BITMAP_FONT_LINE_HEIGHT * scale;

    for character in entry.text.chars() {
        match character {
            '\n' => {
                cursor_x = start_x;
                cursor_y += line_height;
            }
            '\r' => {}
            _ => {
                let glyph = glyph_columns(character);
                draw_bitmap_glyph(frame, cursor_x, cursor_y, scale, threshold, entry.color, glyph);
                cursor_x += advance;
            }
        }
    }
}

fn resolve_bitmap_scale(desired_pixel_height: f32) -> f32 {
    (desired_pixel_height.max(0.1) / BITMAP_FONT_HEIGHT as f32).max(0.02)
}

fn draw_bitmap_glyph(
    frame: &mut DirectStreamFrame<'_>,
    x: f32,
    y: f32,
    scale: f32,
    threshold: f32,
    color: [u8; 4],
    glyph_columns: [u8; BITMAP_FONT_WIDTH],
) {
    let glyph_width = BITMAP_FONT_WIDTH as f32 * scale;
    let glyph_height = BITMAP_FONT_HEIGHT as f32 * scale;
    let min_x = x.floor() as i32;
    let min_y = y.floor() as i32;
    let max_x = (x + glyph_width).ceil() as i32;
    let max_y = (y + glyph_height).ceil() as i32;

    for pixel_y in min_y..max_y {
        for pixel_x in min_x..max_x {
            let coverage = glyph_pixel_coverage(glyph_columns, x, y, scale, pixel_x, pixel_y);
            if coverage >= threshold {
                fill_rect(frame, pixel_x, pixel_y, 1, 1, color);
            }
        }
    }
}

fn glyph_pixel_coverage(
    glyph_columns: [u8; BITMAP_FONT_WIDTH],
    glyph_x: f32,
    glyph_y: f32,
    scale: f32,
    pixel_x: i32,
    pixel_y: i32,
) -> f32 {
    let pixel_min_x = pixel_x as f32;
    let pixel_max_x = pixel_min_x + 1.0;
    let pixel_min_y = pixel_y as f32;
    let pixel_max_y = pixel_min_y + 1.0;
    let mut coverage = 0.0;

    for column in 0..BITMAP_FONT_WIDTH {
        for row in 0..BITMAP_FONT_HEIGHT {
            if !glyph_bit_is_on(glyph_columns, column, row) {
                continue;
            }

            let source_min_x = glyph_x + column as f32 * scale;
            let source_max_x = source_min_x + scale;
            let source_min_y = glyph_y + row as f32 * scale;
            let source_max_y = source_min_y + scale;
            let overlap_x = (pixel_max_x.min(source_max_x) - pixel_min_x.max(source_min_x)).max(0.0);
            let overlap_y = (pixel_max_y.min(source_max_y) - pixel_min_y.max(source_min_y)).max(0.0);
            coverage += overlap_x * overlap_y;
        }
    }

    coverage.clamp(0.0, 1.0)
}

fn glyph_bit_is_on(glyph_columns: [u8; BITMAP_FONT_WIDTH], column: usize, row: usize) -> bool {
    ((glyph_columns[column] >> row) & 1) != 0
}

fn glyph_columns(character: char) -> [u8; BITMAP_FONT_WIDTH] {
    match character {
        ' ' => [0x00, 0x00, 0x00],
        '!' => [0x00, 0x17, 0x00],
        '"' => [0x03, 0x00, 0x03],
        '%' => [0x09, 0x04, 0x12],
        '\'' => [0x00, 0x03, 0x00],
        '(' => [0x0E, 0x11, 0x00],
        ')' => [0x00, 0x11, 0x0E],
        '*' => [0x05, 0x02, 0x05],
        '+' => [0x04, 0x0E, 0x04],
        ',' => [0x00, 0x10, 0x08],
        '-' => [0x04, 0x04, 0x04],
        '.' => [0x00, 0x10, 0x00],
        '/' => [0x18, 0x0E, 0x03],
        '0' => [0x1F, 0x11, 0x1F],
        '1' => [0x12, 0x1F, 0x10],
        '2' => [0x1D, 0x15, 0x17],
        '3' => [0x11, 0x15, 0x1F],
        '4' => [0x07, 0x04, 0x1F],
        '5' => [0x17, 0x15, 0x1D],
        '6' => [0x1F, 0x15, 0x1D],
        '7' => [0x01, 0x1D, 0x03],
        '8' => [0x1F, 0x15, 0x1F],
        '9' => [0x17, 0x15, 0x1F],
        ':' => [0x00, 0x0A, 0x00],
        ';' => [0x10, 0x0A, 0x00],
        '<' => [0x04, 0x0A, 0x11],
        '=' => [0x0A, 0x0A, 0x0A],
        '>' => [0x11, 0x0A, 0x04],
        '?' => [0x01, 0x15, 0x03],
        '@' => [0x0E, 0x11, 0x1D],
        'A' => [0x1E, 0x09, 0x1E],
        'B' => [0x1F, 0x15, 0x0A],
        'C' => [0x1F, 0x11, 0x11],
        'D' => [0x1F, 0x11, 0x0E],
        'E' => [0x1F, 0x15, 0x11],
        'F' => [0x1F, 0x05, 0x01],
        'G' => [0x0E, 0x11, 0x0D],
        'H' => [0x1F, 0x04, 0x1F],
        'I' => [0x11, 0x1F, 0x11],
        'J' => [0x09, 0x11, 0x0F],
        'K' => [0x1F, 0x04, 0x1B],
        'L' => [0x1F, 0x10, 0x10],
        'M' => [0x1F, 0x06, 0x1F],
        'N' => [0x1F, 0x0E, 0x1F],
        'O' => [0x0E, 0x11, 0x0E],
        'P' => [0x1F, 0x09, 0x06],
        'Q' => [0x0E, 0x11, 0x1E],
        'R' => [0x1F, 0x09, 0x16],
        'S' => [0x12, 0x15, 0x09],
        'T' => [0x01, 0x1F, 0x01],
        'U' => [0x1F, 0x10, 0x1F],
        'V' => [0x0F, 0x18, 0x0F],
        'W' => [0x1F, 0x0C, 0x1F],
        'X' => [0x1B, 0x04, 0x1B],
        'Y' => [0x03, 0x1C, 0x03],
        'Z' => [0x19, 0x15, 0x13],
        '[' => [0x00, 0x00, 0x1F],
        '\\' => [0x01, 0x02, 0x04],
        ']' => [0x00, 0x11, 0x1F],
        '^' => [0x00, 0x02, 0x01],
        '_' => [0x00, 0x10, 0x10],
        '`' => [0x00, 0x01, 0x02],
        'a' => [0x0C, 0x12, 0x1C],
        'b' => [0x1F, 0x14, 0x08],
        'c' => [0x0C, 0x12, 0x12],
        'd' => [0x08, 0x14, 0x1F],
        'e' => [0x0C, 0x12, 0x16],
        'f' => [0x1E, 0x05, 0x01],
        'g' => [0x12, 0x15, 0x0E],
        'h' => [0x1F, 0x04, 0x18],
        'i' => [0x00, 0x1D, 0x00],
        'j' => [0x08, 0x10, 0x0D],
        'k' => [0x1F, 0x08, 0x14],
        'l' => [0x0F, 0x10, 0x00],
        'm' => [0x1E, 0x04, 0x1E],
        'n' => [0x1E, 0x02, 0x1C],
        'o' => [0x0C, 0x12, 0x0C],
        'p' => [0x1E, 0x0A, 0x04],
        'q' => [0x04, 0x0A, 0x1E],
        'r' => [0x1C, 0x02, 0x02],
        's' => [0x10, 0x14, 0x0A],
        't' => [0x04, 0x0E, 0x14],
        'u' => [0x1E, 0x10, 0x1E],
        'v' => [0x0E, 0x10, 0x0E],
        'w' => [0x1E, 0x08, 0x1E],
        'x' => [0x12, 0x0C, 0x12],
        'y' => [0x12, 0x14, 0x0E],
        'z' => [0x12, 0x1A, 0x16],
        '{' => [0x04, 0x1B, 0x11],
        '|' => [0x00, 0x1F, 0x00],
        '}' => [0x11, 0x1B, 0x04],
        '~' => [0x0C, 0x04, 0x06],
        _ => [0x01, 0x15, 0x03],
    }
}

fn fill_rect(
    frame: &mut DirectStreamFrame<'_>,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    color: [u8; 4],
) {
    let frame_width = frame.width() as i32;
    let frame_height = frame.height() as i32;
    let row_bytes = frame.row_bytes();
    let bgra = frame.bgra_mut();

    for yy in y.max(0)..(y + height).min(frame_height) {
        for xx in x.max(0)..(x + width).min(frame_width) {
            let offset = yy as usize * row_bytes + xx as usize * 4;
            if offset + 3 < bgra.len() {
                bgra[offset..offset + 4].copy_from_slice(&color);
            }
        }
    }
}


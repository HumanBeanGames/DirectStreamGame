use crate::{gpu_palette::GpuPalettePipeline, public_types::DirectStreamTarget};
use bevy::{camera::visibility::RenderLayers, prelude::*};
use std::collections::HashSet;

pub struct DirectTextPlugin;

const BITMAP_FONT_WIDTH: usize = 3;
const BITMAP_FONT_HEIGHT: usize = 5;
const BITMAP_FONT_ADVANCE: f32 = 4.0;
const BITMAP_FONT_LINE_HEIGHT: f32 = 6.0;
const DEFAULT_DIRECT_TEXT_FONT_SIZE: f32 = BITMAP_FONT_HEIGHT as f32;
const GLYPH_ON_THRESHOLD: f32 = 0.5;

impl Plugin for DirectTextPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_direct_text_overlays);
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

    pub fn with_scale(mut self, scale: u32) -> Self {
        self.font_size = DEFAULT_DIRECT_TEXT_FONT_SIZE * scale.max(1) as f32;
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

#[derive(Component, Clone, Copy)]
struct DirectTextOverlayPixel {
    owner: Entity,
}

fn sync_direct_text_overlays(
    mut commands: Commands,
    target: Res<DirectStreamTarget>,
    gpu_palette: Option<Res<GpuPalettePipeline>>,
    changed_text: Query<(Entity, &DirectText), Or<(Added<DirectText>, Changed<DirectText>)>>,
    all_text: Query<(Entity, &DirectText)>,
    existing: Query<(Entity, &DirectTextOverlayPixel)>,
    mut removed_text: RemovedComponents<DirectText>,
) {
    let removed_owners: HashSet<Entity> = removed_text.read().collect();
    let target_changed = target.is_changed();
    if !target_changed && changed_text.is_empty() && removed_owners.is_empty() {
        return;
    }

    let rebuild_owners: HashSet<Entity> = if target_changed {
        all_text.iter().map(|(entity, _)| entity).collect()
    } else {
        changed_text.iter().map(|(entity, _)| entity).collect()
    };

    for (entity, overlay) in &existing {
        if removed_owners.contains(&overlay.owner) || rebuild_owners.contains(&overlay.owner) {
            commands.entity(entity).despawn();
        }
    }

    if rebuild_owners.is_empty() {
        return;
    }

    let overlay_layer = RenderLayers::layer(target.overlay_layer);
    let left = -(target.width as f32) * 0.5;
    let top = target.height as f32 * 0.5;

    for (owner, text) in &all_text {
        if !rebuild_owners.contains(&owner) {
            continue;
        }
        let scale = resolve_bitmap_scale(text.font_size);
        let threshold = text.threshold.unwrap_or(GLYPH_ON_THRESHOLD).clamp(0.0, 1.0);
        let color = overlay_color(text, &target, gpu_palette.as_deref());
        let mut cursor_x = text.x as f32;
        let mut cursor_y = text.y as f32;
        let start_x = cursor_x;
        let advance = BITMAP_FONT_ADVANCE * scale;
        let line_height = BITMAP_FONT_LINE_HEIGHT * scale;

        for character in text.text.chars() {
            match character {
                '\n' => {
                    cursor_x = start_x;
                    cursor_y += line_height;
                }
                '\r' => {}
                _ => {
                    spawn_bitmap_glyph(
                        &mut commands,
                        &overlay_layer,
                        owner,
                        color,
                        left,
                        top,
                        cursor_x,
                        cursor_y,
                        scale,
                        threshold,
                        glyph_columns(character),
                    );
                    cursor_x += advance;
                }
            }
        }
    }
}

fn resolve_bitmap_scale(desired_pixel_height: f32) -> f32 {
    quantize_bitmap_scale(desired_pixel_height / BITMAP_FONT_HEIGHT as f32)
}

fn quantize_bitmap_scale(scale: f32) -> f32 {
    scale.round().max(1.0)
}

fn spawn_bitmap_glyph(
    commands: &mut Commands,
    overlay_layer: &RenderLayers,
    owner: Entity,
    color: Color,
    left: f32,
    top: f32,
    x: f32,
    y: f32,
    scale: f32,
    threshold: f32,
    glyph_columns: [u8; BITMAP_FONT_WIDTH],
) {
    if threshold > 1.0 {
        return;
    }

    for column in 0..BITMAP_FONT_WIDTH {
        for row in 0..BITMAP_FONT_HEIGHT {
            if glyph_bit_is_on(glyph_columns, column, row) {
                let pixel_x = left + x + column as f32 * scale + scale * 0.5;
                let pixel_y = top - y - row as f32 * scale - scale * 0.5;
                commands.spawn((
                    Sprite {
                        color,
                        custom_size: Some(Vec2::splat(scale)),
                        ..default()
                    },
                    Transform::from_xyz(pixel_x, pixel_y, 0.0),
                    overlay_layer.clone(),
                    DirectTextOverlayPixel { owner },
                ));
            }
        }
    }
}

fn overlay_color(
    text: &DirectText,
    target: &DirectStreamTarget,
    gpu_palette: Option<&GpuPalettePipeline>,
) -> Color {
    if target.output_is_indexed
        && let Some(gpu_palette) = gpu_palette
    {
        let palette_index = nearest_palette_index(text.color, &gpu_palette.palette_colors);
        return Color::linear_rgba(palette_index as f32 / 255.0, 0.0, 0.0, 1.0);
    }

    Color::srgba(
        text.color.red,
        text.color.green,
        text.color.blue,
        text.color.alpha,
    )
}

fn nearest_palette_index(color: Srgba, palette: &[[u8; 4]]) -> u8 {
    let mut best_index = 0;
    let mut best_distance = f32::MAX;
    for (index, [r, g, b, _]) in palette.iter().enumerate() {
        let dr = color.red - f32::from(*r) / 255.0;
        let dg = color.green - f32::from(*g) / 255.0;
        let db = color.blue - f32::from(*b) / 255.0;
        let da = color.alpha - 1.0;
        let distance = dr * dr + dg * dg + db * db + da * da;
        if distance < best_distance {
            best_distance = distance;
            best_index = index as u8;
        }
    }
    best_index
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

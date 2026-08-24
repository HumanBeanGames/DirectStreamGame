use crate::{
    gpu_palette::{GpuPalettePipeline, INDEXED_DIRECT_OVERLAY_MARKER, indexed_unorm_byte},
    palette_lut::LUT_ENTRY_COUNT,
    public_types::{DirectColorLookup, DirectStreamTarget},
};
use bevy::{camera::visibility::RenderLayers, prelude::*, render::render_resource::TextureFormat};
use std::collections::{HashMap, HashSet};

const SPRITE_ON_ALPHA_THRESHOLD: u8 = 1;
const SPRITE_TEXT_Z_CEILING: f32 = -0.1;
const SPRITE_BASE_Z: f32 = -100.0;
const ALWAYS_ON_TOP_BASE_Z: f32 = -10.0;

pub struct DirectWorldSpritePlugin;

impl Plugin for DirectWorldSpritePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DirectWorldSpriteSettings>()
            .init_resource::<DirectWorldSpriteOverlayCache>()
            .add_systems(Update, sync_direct_world_sprites);
    }
}

#[derive(Component, Clone)]
pub struct DirectWorldSprite {
    pub image: Handle<Image>,
    pub atlas: Option<Handle<TextureAtlasLayout>>,
    pub atlas_index: usize,
    pub pixel_size: UVec2,
    pub anchor: Vec2,
    pub tint: Color,
    pub color_lookup: DirectColorLookup,
    pub facing: SpriteFacing,
    pub depth_mode: SpriteDepthMode,
    pub depth_bias: f32,
}

impl DirectWorldSprite {
    pub fn new(image: Handle<Image>, pixel_size: UVec2) -> Self {
        Self {
            image,
            atlas: None,
            atlas_index: 0,
            pixel_size,
            anchor: Vec2::splat(0.5),
            tint: Color::WHITE,
            color_lookup: DirectColorLookup::Direct,
            facing: SpriteFacing::FaceStreamCamera,
            depth_mode: SpriteDepthMode::TestAndWrite,
            depth_bias: 0.0,
        }
    }

    pub fn with_atlas(mut self, atlas: Handle<TextureAtlasLayout>, atlas_index: usize) -> Self {
        self.atlas = Some(atlas);
        self.atlas_index = atlas_index;
        self
    }

    pub fn with_anchor(mut self, anchor: Vec2) -> Self {
        self.anchor = anchor;
        self
    }

    pub fn with_tint(mut self, tint: Color) -> Self {
        self.tint = tint;
        self
    }

    pub fn with_color_lookup(mut self, color_lookup: DirectColorLookup) -> Self {
        self.color_lookup = color_lookup;
        self
    }

    pub fn with_facing(mut self, facing: SpriteFacing) -> Self {
        self.facing = facing;
        self
    }

    pub fn with_depth_mode(mut self, depth_mode: SpriteDepthMode) -> Self {
        self.depth_mode = depth_mode;
        self
    }

    pub fn with_depth_bias(mut self, depth_bias: f32) -> Self {
        self.depth_bias = depth_bias;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteFacing {
    FaceStreamCamera,
    LockY,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteDepthMode {
    TestAgainstScene,
    TestAndWrite,
    AlwaysOnTopBeforeText,
}

#[derive(Resource, Clone)]
pub struct DirectWorldSpriteSettings {
    pub enabled: bool,
    pub max_sprites: usize,
}

impl Default for DirectWorldSpriteSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            max_sprites: 1024,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct AtlasFrame {
    rect: URect,
    size: UVec2,
}

#[derive(Component, Clone, Copy)]
struct DirectWorldSpriteOverlayRoot;

#[derive(Component, Clone, Copy)]
struct DirectWorldSpriteOverlayPixel;

#[derive(Clone)]
struct DirectWorldSpriteOverlayState {
    root_entity: Entity,
    pixel_entities: Vec<Entity>,
}

#[derive(Resource, Default)]
struct DirectWorldSpriteOverlayCache {
    entries: HashMap<Entity, DirectWorldSpriteOverlayState>,
}

struct BuiltSpritePixels {
    pixels: Vec<BuiltSpritePixel>,
    source_size: UVec2,
}

struct BuiltSpritePixel {
    x: u32,
    y: u32,
    color: Color,
}

// Bevy system inputs stay explicit so the scheduler can validate their access conflicts.
#[allow(clippy::too_many_arguments)]
fn sync_direct_world_sprites(
    mut commands: Commands,
    settings: Res<DirectWorldSpriteSettings>,
    target: Res<DirectStreamTarget>,
    camera_query: Query<(&Camera, Ref<GlobalTransform>, Option<&RenderLayers>)>,
    sprites: Query<(Entity, Ref<DirectWorldSprite>, Ref<GlobalTransform>)>,
    mut removed_sprites: RemovedComponents<DirectWorldSprite>,
    mut cache: ResMut<DirectWorldSpriteOverlayCache>,
    atlases: Res<Assets<TextureAtlasLayout>>,
    images: Res<Assets<Image>>,
    gpu_palette: Option<Res<GpuPalettePipeline>>,
) {
    let Ok((camera, camera_transform, _)) = camera_query.get(target.camera) else {
        return;
    };

    let removed_owners: HashSet<Entity> = removed_sprites.read().collect();
    if !removed_owners.is_empty() {
        for owner in &removed_owners {
            despawn_overlay_state(&mut commands, cache.entries.remove(owner));
        }
    }

    if !settings.enabled {
        clear_overlay_cache(&mut commands, &mut cache);
        return;
    }

    let active_owners: HashSet<Entity> = sprites.iter().map(|(entity, _, _)| entity).collect();
    let stale_owners = cache
        .entries
        .keys()
        .filter(|owner| !active_owners.contains(owner))
        .copied()
        .collect::<Vec<_>>();
    for owner in stale_owners {
        despawn_overlay_state(&mut commands, cache.entries.remove(&owner));
    }

    let target_changed = target.is_changed();
    let palette_changed = gpu_palette
        .as_ref()
        .map(|pipeline| pipeline.is_changed())
        .unwrap_or(false);
    let overlay_layer = RenderLayers::layer(target.overlay_layer);
    let mut visible_count = 0usize;

    for (owner, sprite, owner_transform) in &sprites {
        let atlas_frame = atlas_frame(&sprite, &atlases);
        let pixel_size = integer_scaled_pixel_size(&sprite, atlas_frame, &images);
        let projected = if visible_count < settings.max_sprites {
            project_world_sprite_overlay(
                &sprite,
                pixel_size,
                &owner_transform,
                camera,
                &camera_transform,
                &target,
            )
        } else {
            None
        };

        let Some(projected) = projected else {
            hide_overlay_state(&mut commands, cache.entries.get(&owner));
            continue;
        };
        visible_count += 1;

        let needs_rebuild = target_changed
            || palette_changed
            || sprite.is_changed()
            || images.is_changed()
            || !cache.entries.contains_key(&owner);

        if needs_rebuild {
            despawn_overlay_state(&mut commands, cache.entries.remove(&owner));
            let Some(built_pixels) = build_overlay_pixels(
                &sprite,
                atlas_frame,
                &target,
                gpu_palette.as_deref(),
                &images,
            ) else {
                continue;
            };
            let render_size = integer_scaled_size_for_source(&sprite, built_pixels.source_size);
            let transform = projected.transform();
            let state = spawn_overlay_sprite(
                &mut commands,
                &built_pixels,
                &overlay_layer,
                transform,
                render_size,
            );
            cache.entries.insert(owner, state);
        } else if let Some(state) = cache.entries.get(&owner) {
            let transform = projected.transform();
            show_overlay_state(&mut commands, state, transform);
        }
    }
}

fn spawn_overlay_sprite(
    commands: &mut Commands,
    built_pixels: &BuiltSpritePixels,
    layer: &RenderLayers,
    transform: Transform,
    render_size: UVec2,
) -> DirectWorldSpriteOverlayState {
    let root_entity = commands
        .spawn((transform, Visibility::Visible, DirectWorldSpriteOverlayRoot))
        .id();
    let scale = pixel_scale(render_size, built_pixels.source_size);
    let half_size = render_size.as_vec2() * 0.5;
    let mut pixel_entities = Vec::with_capacity(built_pixels.pixels.len());

    commands.entity(root_entity).with_children(|children| {
        for pixel in &built_pixels.pixels {
            let local_x = -half_size.x + (pixel.x as f32 + 0.5) * scale.x;
            let local_y = half_size.y - (pixel.y as f32 + 0.5) * scale.y;
            let entity = children
                .spawn((
                    Sprite {
                        color: pixel.color,
                        custom_size: Some(scale),
                        ..default()
                    },
                    Transform::from_xyz(local_x, local_y, 0.0),
                    Visibility::Visible,
                    layer.clone(),
                    DirectWorldSpriteOverlayPixel,
                ))
                .id();
            pixel_entities.push(entity);
        }
    });

    DirectWorldSpriteOverlayState {
        root_entity,
        pixel_entities,
    }
}

fn clear_overlay_cache(commands: &mut Commands, cache: &mut DirectWorldSpriteOverlayCache) {
    let owners = cache.entries.keys().copied().collect::<Vec<_>>();
    for owner in owners {
        despawn_overlay_state(commands, cache.entries.remove(&owner));
    }
}

fn despawn_overlay_state(commands: &mut Commands, state: Option<DirectWorldSpriteOverlayState>) {
    let Some(state) = state else {
        return;
    };
    for pixel_entity in state.pixel_entities {
        commands.entity(pixel_entity).despawn();
    }
    commands.entity(state.root_entity).despawn();
}

fn hide_overlay_state(commands: &mut Commands, state: Option<&DirectWorldSpriteOverlayState>) {
    let Some(state) = state else {
        return;
    };
    commands
        .entity(state.root_entity)
        .insert(Visibility::Hidden);
}

fn show_overlay_state(
    commands: &mut Commands,
    state: &DirectWorldSpriteOverlayState,
    transform: Transform,
) {
    commands
        .entity(state.root_entity)
        .insert((transform, Visibility::Visible));
}

fn pixel_scale(render_size: UVec2, source_size: UVec2) -> Vec2 {
    Vec2::new(
        render_size.x.max(1) as f32 / source_size.x.max(1) as f32,
        render_size.y.max(1) as f32 / source_size.y.max(1) as f32,
    )
}

struct ProjectedOverlaySprite {
    center: Vec2,
    z: f32,
}

impl ProjectedOverlaySprite {
    fn transform(&self) -> Transform {
        Transform::from_xyz(self.center.x, self.center.y, self.z)
    }
}

fn project_world_sprite_overlay(
    sprite: &DirectWorldSprite,
    pixel_size: UVec2,
    owner_transform: &GlobalTransform,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    target: &DirectStreamTarget,
) -> Option<ProjectedOverlaySprite> {
    if pixel_size.x == 0 || pixel_size.y == 0 {
        return None;
    }

    let anchor_world = owner_transform.translation();
    let projected = camera
        .world_to_viewport(camera_transform, anchor_world)
        .ok()?;
    if projected.x < 0.0
        || projected.y < 0.0
        || projected.x >= target.width as f32
        || projected.y >= target.height as f32
    {
        return None;
    }

    let pixel_size = pixel_size.as_vec2();
    let center_viewport = sprite_center_from_projected_anchor(projected, sprite.anchor, pixel_size);
    let left = -(target.width as f32) * 0.5;
    let top = target.height as f32 * 0.5;

    Some(ProjectedOverlaySprite {
        center: Vec2::new(left + center_viewport.x, top - center_viewport.y),
        z: overlay_z(
            sprite,
            camera_transform.translation().distance(anchor_world),
        ),
    })
}

fn sprite_center_from_projected_anchor(
    projected_anchor: Vec2,
    anchor: Vec2,
    pixel_size: Vec2,
) -> Vec2 {
    let top_left = (projected_anchor - anchor * pixel_size).floor();
    top_left + pixel_size * 0.5
}

fn overlay_z(sprite: &DirectWorldSprite, projected_depth: f32) -> f32 {
    let depth_bias = sprite.depth_bias.clamp(-1000.0, 1000.0);
    let z = match sprite.depth_mode {
        SpriteDepthMode::AlwaysOnTopBeforeText => ALWAYS_ON_TOP_BASE_Z + depth_bias,
        SpriteDepthMode::TestAgainstScene | SpriteDepthMode::TestAndWrite => {
            SPRITE_BASE_Z - projected_depth + depth_bias
        }
    };
    z.min(SPRITE_TEXT_Z_CEILING)
}

fn build_overlay_pixels(
    sprite: &DirectWorldSprite,
    atlas_frame: Option<AtlasFrame>,
    target: &DirectStreamTarget,
    gpu_palette: Option<&GpuPalettePipeline>,
    images: &Assets<Image>,
) -> Option<BuiltSpritePixels> {
    let source_image = images.get(&sprite.image)?;
    let source_rect = sprite_source_rect(atlas_frame, source_image)?;
    let source_size = source_rect.size();
    if source_size.x == 0 || source_size.y == 0 {
        return None;
    }

    let mut pixels = Vec::new();
    let tint = sprite.tint.to_srgba();
    let direct_entries = if target.output_is_indexed {
        gpu_palette
            .map(|pipeline| pipeline.lookup_entries.as_ref())
            .filter(|entries| entries.len() >= LUT_ENTRY_COUNT.saturating_mul(2))
    } else {
        None
    };
    if target.output_is_indexed && direct_entries.is_none() {
        return None;
    }

    for y in 0..source_size.y {
        for x in 0..source_size.x {
            let Some(source_pixel) =
                read_image_pixel(source_image, source_rect.min.x + x, source_rect.min.y + y)
            else {
                continue;
            };
            let pixel = tint_pixel(source_pixel, tint);
            if pixel[3] < SPRITE_ON_ALPHA_THRESHOLD {
                continue;
            }
            let color = if let Some(entries) = direct_entries {
                let index =
                    sprite_palette_index_at(source_image, source_rect, x, y, tint, entries)?;
                Color::linear_rgba(
                    indexed_unorm_byte(index),
                    INDEXED_DIRECT_OVERLAY_MARKER,
                    0.0,
                    1.0,
                )
            } else {
                Color::srgba(
                    f32::from(pixel[0]) / 255.0,
                    f32::from(pixel[1]) / 255.0,
                    f32::from(pixel[2]) / 255.0,
                    1.0,
                )
            };
            pixels.push(BuiltSpritePixel { x, y, color });
        }
    }

    if pixels.is_empty() {
        return None;
    }

    Some(BuiltSpritePixels {
        pixels,
        source_size,
    })
}

fn sprite_source_rect(atlas_frame: Option<AtlasFrame>, image: &Image) -> Option<URect> {
    if let Some(frame) = atlas_frame {
        return Some(frame.rect);
    }

    let size = image.texture_descriptor.size;
    (size.width > 0 && size.height > 0).then_some(URect::from_corners(
        UVec2::ZERO,
        UVec2::new(size.width, size.height),
    ))
}

fn read_image_pixel(image: &Image, x: u32, y: u32) -> Option<[u8; 4]> {
    let data = image.data.as_ref()?;
    let width = image.texture_descriptor.size.width as usize;
    let offset = ((y as usize * width) + x as usize).checked_mul(4)?;
    let slice = data.get(offset..offset + 4)?;
    match image.texture_descriptor.format {
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb => {
            Some([slice[0], slice[1], slice[2], slice[3]])
        }
        TextureFormat::Bgra8Unorm | TextureFormat::Bgra8UnormSrgb => {
            Some([slice[2], slice[1], slice[0], slice[3]])
        }
        _ => None,
    }
}

fn tint_pixel(pixel: [u8; 4], tint: Srgba) -> [u8; 4] {
    [
        multiply_u8(pixel[0], tint.red),
        multiply_u8(pixel[1], tint.green),
        multiply_u8(pixel[2], tint.blue),
        multiply_u8(pixel[3], tint.alpha),
    ]
}

fn multiply_u8(value: u8, factor: f32) -> u8 {
    ((value as f32 * factor.clamp(0.0, 1.0)).round()).clamp(0.0, 255.0) as u8
}

fn lookup_direct_palette_index(r: u8, g: u8, b: u8, lookup_entries: &[u8]) -> Option<u8> {
    let offset = ((r as usize) << 16) | ((g as usize) << 8) | b as usize;
    lookup_entries.get(LUT_ENTRY_COUNT + offset).copied()
}

fn sprite_palette_index_at(
    image: &Image,
    source_rect: URect,
    x: u32,
    y: u32,
    tint: Srgba,
    lookup_entries: &[u8],
) -> Option<u8> {
    let source_pixel = read_image_pixel(image, source_rect.min.x + x, source_rect.min.y + y)?;
    let pixel = tint_pixel(source_pixel, tint);
    lookup_direct_palette_index(pixel[0], pixel[1], pixel[2], lookup_entries)
}

fn atlas_frame(
    sprite: &DirectWorldSprite,
    atlases: &Assets<TextureAtlasLayout>,
) -> Option<AtlasFrame> {
    let atlas = sprite.atlas.as_ref()?;
    let layout = atlases.get(atlas)?;
    let rect = layout.textures.get(sprite.atlas_index).copied()?;
    Some(AtlasFrame {
        rect,
        size: layout.size,
    })
}

fn integer_scaled_pixel_size(
    sprite: &DirectWorldSprite,
    atlas_frame: Option<AtlasFrame>,
    images: &Assets<Image>,
) -> UVec2 {
    let Some(base_size) = sprite_base_pixel_size(sprite, atlas_frame, images) else {
        return sprite.pixel_size.max(UVec2::ONE);
    };
    integer_scaled_size_for_source(sprite, base_size)
}

fn integer_scaled_size_for_source(sprite: &DirectWorldSprite, base_size: UVec2) -> UVec2 {
    if base_size.x == 0 || base_size.y == 0 {
        return sprite.pixel_size.max(UVec2::ONE);
    }

    let requested = sprite.pixel_size.max(UVec2::ONE).as_vec2();
    let base = base_size.as_vec2();
    let scale = ((requested.x / base.x + requested.y / base.y) * 0.5)
        .round()
        .max(1.0) as u32;
    base_size * scale
}

fn sprite_base_pixel_size(
    sprite: &DirectWorldSprite,
    atlas_frame: Option<AtlasFrame>,
    images: &Assets<Image>,
) -> Option<UVec2> {
    if let Some(frame) = atlas_frame {
        let size = frame.rect.size();
        if size.x > 0 && size.y > 0 {
            return Some(size);
        }
    }

    let image = images.get(&sprite.image)?;
    let size = image.texture_descriptor.size;
    (size.width > 0 && size.height > 0).then_some(UVec2::new(size.width, size.height))
}

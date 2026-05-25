use crate::public_types::DirectStreamTarget;
use bevy::{
    asset::RenderAssetUsages,
    camera::visibility::RenderLayers,
    mesh::Indices,
    prelude::*,
    render::{
        alpha::AlphaMode,
        render_resource::{Face, PrimitiveTopology},
    },
    transform::TransformSystems,
};
use std::collections::{HashMap, HashSet};

pub struct DirectWorldSpritePlugin;

impl Plugin for DirectWorldSpritePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DirectWorldSpriteSettings>()
            .add_systems(
                PostUpdate,
                sync_direct_world_sprites.after(TransformSystems::Propagate),
            );
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

#[derive(Component)]
struct DirectWorldSpriteRender {
    material: Handle<StandardMaterial>,
    mesh: Handle<Mesh>,
    image: Handle<Image>,
    atlas: Option<Handle<TextureAtlasLayout>>,
    atlas_index: usize,
    atlas_frame: Option<AtlasFrame>,
    depth_mode: SpriteDepthMode,
}

#[derive(Default)]
struct DirectWorldSpriteRenderMap(HashMap<Entity, Entity>);

fn sync_direct_world_sprites(
    mut commands: Commands,
    settings: Res<DirectWorldSpriteSettings>,
    target: Res<DirectStreamTarget>,
    camera_query: Query<(&Camera, &GlobalTransform, Option<&RenderLayers>)>,
    source_sprites: Query<
        (Entity, &DirectWorldSprite, &GlobalTransform),
        Without<DirectWorldSpriteRender>,
    >,
    mut render_sprites: Query<(
        &mut DirectWorldSpriteRender,
        &mut Mesh3d,
        &mut MeshMaterial3d<StandardMaterial>,
        &mut Transform,
        &mut Visibility,
    )>,
    mut render_map: Local<DirectWorldSpriteRenderMap>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    atlases: Res<Assets<TextureAtlasLayout>>,
) {
    let Ok((camera, camera_transform, camera_layers)) = camera_query.get(target.camera) else {
        return;
    };

    let active_owners = source_sprites
        .iter()
        .map(|(entity, _, _)| entity)
        .collect::<HashSet<_>>();

    render_map.0.retain(|owner, render_entity| {
        let keep = active_owners.contains(owner);
        if !keep {
            commands.entity(*render_entity).despawn();
        }
        keep
    });

    if !settings.enabled {
        for render_entity in render_map.0.values().copied() {
            if let Ok((_, _, _, _, mut visibility)) = render_sprites.get_mut(render_entity) {
                *visibility = Visibility::Hidden;
            }
        }
        return;
    }

    let render_layers = camera_layers
        .cloned()
        .unwrap_or_else(|| RenderLayers::layer(0));

    let mut visible_count = 0usize;
    for (owner, sprite, owner_transform) in &source_sprites {
        let render_transform = if visible_count < settings.max_sprites {
            project_world_sprite(sprite, owner_transform, camera, camera_transform, &target)
        } else {
            None
        };

        let Some(render_transform) = render_transform else {
            if let Some(render_entity) = render_map.0.get(&owner).copied()
                && let Ok((_, _, _, _, mut visibility)) = render_sprites.get_mut(render_entity)
            {
                *visibility = Visibility::Hidden;
            }
            continue;
        };

        visible_count += 1;
        let atlas_frame = atlas_frame(sprite, &atlases);
        let render_entity = if let Some(render_entity) = render_map.0.get(&owner).copied() {
            render_entity
        } else {
            let mesh = meshes.add(sprite_mesh(atlas_frame));
            let material = materials.add(sprite_material(sprite));
            let render_entity = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    render_transform,
                    Visibility::Visible,
                    render_layers.clone(),
                    DirectWorldSpriteRender {
                        material,
                        mesh,
                        image: sprite.image.clone(),
                        atlas: sprite.atlas.clone(),
                        atlas_index: sprite.atlas_index,
                        atlas_frame,
                        depth_mode: sprite.depth_mode,
                    },
                ))
                .id();
            render_map.0.insert(owner, render_entity);
            continue;
        };

        let Ok((mut render, mut mesh, mut material, mut transform, mut visibility)) =
            render_sprites.get_mut(render_entity)
        else {
            render_map.0.remove(&owner);
            continue;
        };

        if render.image != sprite.image
            || render.atlas != sprite.atlas
            || render.atlas_index != sprite.atlas_index
            || render.atlas_frame != atlas_frame
        {
            let next_mesh = meshes.add(sprite_mesh(atlas_frame));
            mesh.0 = next_mesh.clone();
            render.mesh = next_mesh;
            render.image = sprite.image.clone();
            render.atlas = sprite.atlas.clone();
            render.atlas_index = sprite.atlas_index;
            render.atlas_frame = atlas_frame;
        }

        render.depth_mode = sprite.depth_mode;
        if render.material != material.0 {
            material.0 = render.material.clone();
        }

        if let Some(existing_material) = materials.get_mut(&render.material) {
            existing_material.base_color = sprite.tint;
            existing_material.base_color_texture = Some(sprite.image.clone());
            existing_material.alpha_mode = alpha_mode(sprite.depth_mode);
            existing_material.depth_bias = depth_bias(sprite);
        }

        *transform = render_transform;
        *visibility = Visibility::Visible;
        commands.entity(render_entity).insert(render_layers.clone());
    }
}

fn project_world_sprite(
    sprite: &DirectWorldSprite,
    owner_transform: &GlobalTransform,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    target: &DirectStreamTarget,
) -> Option<Transform> {
    if sprite.pixel_size.x == 0 || sprite.pixel_size.y == 0 {
        return None;
    }

    let anchor_world = owner_transform.translation();
    let projected = camera
        .world_to_viewport_with_depth(camera_transform, anchor_world)
        .ok()?;
    if projected.z <= 0.0
        || projected.x < 0.0
        || projected.y < 0.0
        || projected.x >= target.width as f32
        || projected.y >= target.height as f32
    {
        return None;
    }

    let snapped_anchor = Vec2::new(projected.x.round(), projected.y.round());
    let pixel_size = sprite.pixel_size.as_vec2();
    let center_viewport = snapped_anchor + (Vec2::splat(0.5) - sprite.anchor) * pixel_size;
    let rotation = sprite_rotation(sprite.facing, anchor_world, camera_transform);
    let plane_normal = rotation * Vec3::Z;
    let center_world = intersect_viewport_ray_with_plane(
        camera,
        camera_transform,
        center_viewport,
        anchor_world,
        plane_normal,
    )?;
    let half_size = pixel_size * 0.5;
    let left_world = intersect_viewport_ray_with_plane(
        camera,
        camera_transform,
        center_viewport - Vec2::X * half_size.x,
        anchor_world,
        plane_normal,
    )?;
    let right_world = intersect_viewport_ray_with_plane(
        camera,
        camera_transform,
        center_viewport + Vec2::X * half_size.x,
        anchor_world,
        plane_normal,
    )?;
    let top_world = intersect_viewport_ray_with_plane(
        camera,
        camera_transform,
        center_viewport - Vec2::Y * half_size.y,
        anchor_world,
        plane_normal,
    )?;
    let bottom_world = intersect_viewport_ray_with_plane(
        camera,
        camera_transform,
        center_viewport + Vec2::Y * half_size.y,
        anchor_world,
        plane_normal,
    )?;

    Some(Transform {
        translation: center_world,
        rotation,
        scale: Vec3::new(
            left_world.distance(right_world).max(0.0001),
            top_world.distance(bottom_world).max(0.0001),
            1.0,
        ),
    })
}

fn sprite_rotation(
    facing: SpriteFacing,
    anchor_world: Vec3,
    camera_transform: &GlobalTransform,
) -> Quat {
    match facing {
        SpriteFacing::FaceStreamCamera => camera_transform.rotation(),
        SpriteFacing::LockY => {
            let camera_position = camera_transform.translation();
            let look_target = Vec3::new(camera_position.x, anchor_world.y, camera_position.z);
            if look_target.distance_squared(anchor_world) <= f32::EPSILON {
                return camera_transform.rotation();
            }
            Transform::from_translation(anchor_world)
                .looking_at(look_target, Vec3::Y)
                .rotation
        }
    }
}

fn intersect_viewport_ray_with_plane(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    viewport: Vec2,
    plane_origin: Vec3,
    plane_normal: Vec3,
) -> Option<Vec3> {
    let ray = camera.viewport_to_world(camera_transform, viewport).ok()?;
    let normal = plane_normal.normalize_or_zero();
    let denom = (*ray.direction).dot(normal);
    if denom.abs() <= f32::EPSILON {
        return None;
    }
    let distance = (plane_origin - ray.origin).dot(normal) / denom;
    if !distance.is_finite() || distance <= 0.0 {
        return None;
    }
    Some(ray.get_point(distance))
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

fn sprite_mesh(atlas_frame: Option<AtlasFrame>) -> Mesh {
    let (u_min, v_min, u_max, v_max) = if let Some(frame) = atlas_frame {
        let size = frame.size.as_vec2().max(Vec2::ONE);
        let min = frame.rect.min.as_vec2() / size;
        let max = frame.rect.max.as_vec2() / size;
        (min.x, min.y, max.x, max.y)
    } else {
        (0.0, 0.0, 1.0, 1.0)
    };

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ],
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 0.0, 1.0]; 4])
    .with_inserted_attribute(
        Mesh::ATTRIBUTE_UV_0,
        vec![
            [u_min, v_max],
            [u_max, v_max],
            [u_max, v_min],
            [u_min, v_min],
        ],
    )
    .with_inserted_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]))
}

fn sprite_material(sprite: &DirectWorldSprite) -> StandardMaterial {
    StandardMaterial {
        base_color: sprite.tint,
        base_color_texture: Some(sprite.image.clone()),
        unlit: true,
        double_sided: true,
        cull_mode: None::<Face>,
        alpha_mode: alpha_mode(sprite.depth_mode),
        depth_bias: depth_bias(sprite),
        ..default()
    }
}

fn alpha_mode(mode: SpriteDepthMode) -> AlphaMode {
    match mode {
        SpriteDepthMode::TestAgainstScene | SpriteDepthMode::AlwaysOnTopBeforeText => {
            AlphaMode::Blend
        }
        SpriteDepthMode::TestAndWrite => AlphaMode::Mask(0.01),
    }
}

fn depth_bias(sprite: &DirectWorldSprite) -> f32 {
    match sprite.depth_mode {
        SpriteDepthMode::AlwaysOnTopBeforeText => sprite.depth_bias.max(10_000.0),
        _ => sprite.depth_bias,
    }
}

use bevy::prelude::*;

#[derive(Clone, Resource)]
pub struct DirectStreamTarget {
    pub camera: Entity,
    pub overlay_camera: Entity,
    pub image: Handle<Image>,
    pub output_image: Handle<Image>,
    pub output_is_indexed: bool,
    pub overlay_layer: usize,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectStreamSet {
    Setup,
}

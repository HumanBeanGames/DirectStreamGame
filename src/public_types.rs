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

#[derive(Clone, Resource)]
pub struct DirectStreamState {
    pub mode: DirectStreamMode,
    pub active: bool,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl DirectStreamState {
    pub fn is_streaming(&self) -> bool {
        self.active
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectStreamMode {
    Preview,
    CustomHost,
}

#[derive(Clone, Copy, Debug, Message)]
pub struct DirectStreamStartRequest {
    pub mode: DirectStreamMode,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

#[derive(Clone, Copy, Debug, Message)]
pub struct DirectStreamStopRequest;

#[derive(Clone, Debug, Message)]
pub struct DirectStreamControlResult {
    pub action: DirectStreamControlAction,
    pub success: bool,
    pub status: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectStreamControlAction {
    Start,
    Stop,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectStreamSet {
    Setup,
}

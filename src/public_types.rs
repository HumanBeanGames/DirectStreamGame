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
    pub warmup_frames: u32,
    pub suppress_initial_blank_frame: bool,
}

impl DirectStreamStartRequest {
    pub fn custom_host(width: u32, height: u32, fps: u32) -> Self {
        Self {
            mode: DirectStreamMode::CustomHost,
            width,
            height,
            fps,
            warmup_frames: 8,
            suppress_initial_blank_frame: true,
        }
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioSyncMode {
    Fixed,
    MatchEstimatedVideoLatency,
    MatchMeasuredVideoLatency,
}

#[derive(Clone, Resource)]
pub struct DirectStreamAudioSyncConfig {
    pub mode: AudioSyncMode,
    pub fixed_delay_ms: u32,
    pub extra_delay_ms: i32,
}

impl Default for DirectStreamAudioSyncConfig {
    fn default() -> Self {
        Self {
            mode: AudioSyncMode::Fixed,
            fixed_delay_ms: 1_000,
            extra_delay_ms: 0,
        }
    }
}

impl DirectStreamAudioSyncConfig {
    pub fn effective_delay_ms(
        &self,
        estimated_video_latency_ms: u32,
        measured_video_latency_ms: Option<u32>,
    ) -> u32 {
        let base = match self.mode {
            AudioSyncMode::Fixed => self.fixed_delay_ms,
            AudioSyncMode::MatchEstimatedVideoLatency => estimated_video_latency_ms,
            AudioSyncMode::MatchMeasuredVideoLatency => {
                measured_video_latency_ms.unwrap_or(estimated_video_latency_ms)
            }
        };
        (base as i64 + self.extra_delay_ms as i64).max(0) as u32
    }
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirectStreamSet {
    Setup,
}

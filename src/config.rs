use crate::constants::{STREAM_FPS, STREAM_HEIGHT, STREAM_WIDTH};
use std::{env, path::PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowMode {
    Preview,
    Stats,
}

#[derive(bevy::prelude::Resource)]
pub(crate) struct AppConfig {
    pub(crate) window_mode: WindowMode,
    pub(crate) custom_host: bool,
    pub(crate) prebaked_palette: bool,
    pub(crate) stream_width: u32,
    pub(crate) stream_height: u32,
    pub(crate) stream_fps: u32,
    pub(crate) custom_host_batch_size: usize,
    pub(crate) palette_config_path: PathBuf,
}

impl AppConfig {
    pub(crate) fn from_args() -> Self {
        let mut window_mode = WindowMode::Preview;
        let mut custom_host = false;
        let mut prebaked_palette = false;
        let mut stream_width = STREAM_WIDTH;
        let mut stream_height = STREAM_HEIGHT;
        let mut stream_fps = STREAM_FPS;
        let mut custom_host_batch_size = 30;
        let mut stream_width_set = false;
        let mut stream_height_set = false;
        let mut palette_config_path = PathBuf::from("src/default_pallette/default_pallette.toml");

        for arg in env::args().skip(1) {
            if arg == "--stats-window" || arg == "--headless-window" {
                window_mode = WindowMode::Stats;
            } else if arg == "--custom-host" {
                custom_host = true;
                window_mode = WindowMode::Stats;
            } else if arg == "--prebaked" || arg == "--use_prebaked_lookup" {
                prebaked_palette = true;
            } else if let Some(width) = arg.strip_prefix("--stream-width=") {
                stream_width = width.parse().unwrap_or(stream_width);
                stream_width_set = true;
            } else if let Some(height) = arg.strip_prefix("--stream-height=") {
                stream_height = height.parse().unwrap_or(stream_height);
                stream_height_set = true;
            } else if let Some(fps) = arg.strip_prefix("--stream-fps=") {
                stream_fps = fps.parse().unwrap_or(stream_fps);
            } else if let Some(batch_size) = arg.strip_prefix("--batch-size=") {
                custom_host_batch_size = batch_size.parse().unwrap_or(custom_host_batch_size);
            } else if let Some(path) = arg.strip_prefix("--palette-config=") {
                palette_config_path = PathBuf::from(path);
            }
        }

        if custom_host {
            if !stream_width_set && !stream_height_set {
                stream_width = 128;
                stream_height = 128;
            } else if stream_width_set && !stream_height_set {
                stream_height = stream_width;
            } else if stream_height_set && !stream_width_set {
                stream_width = stream_height;
            }
        }

        Self {
            window_mode,
            custom_host,
            prebaked_palette,
            stream_width,
            stream_height,
            stream_fps,
            custom_host_batch_size,
            palette_config_path,
        }
    }
}

pub(crate) fn effective_custom_batch_size(requested_batch_size: usize, _fps: u32) -> usize {
    requested_batch_size.max(1)
}

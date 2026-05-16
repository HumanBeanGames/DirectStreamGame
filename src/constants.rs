pub(crate) const WINDOW_TITLE: &str = "Direct Stream Game";
pub(crate) const WEB_ADDR: &str = "127.0.0.1:8080";
pub(crate) const STREAM_PATH: &str = "/stream.mjpg";
pub(crate) const STREAM_WIDTH: u32 = 320;
pub(crate) const STREAM_HEIGHT: u32 = 240;
pub(crate) const STATS_WINDOW_WIDTH: u32 = 480;
pub(crate) const STATS_WINDOW_HEIGHT: u32 = 420;
pub(crate) const STREAM_FPS: u32 = 15;
pub(crate) const TWITCH_VIDEO_BITRATE: usize = 350_000;
pub(crate) const STREAM_AUDIO_SAMPLE_RATE: u32 = 48_000;
pub(crate) const STREAM_AUDIO_CHANNELS: usize = 2;
pub(crate) const STREAM_AUDIO_BUFFER_SECONDS: usize = 3;
pub(crate) const STREAM_AUDIO_MAX_MIX_FRAMES_PER_UPDATE: usize =
    STREAM_AUDIO_SAMPLE_RATE as usize / 10;

pub const DIRECT_STREAM_WIDTH: u32 = STREAM_WIDTH;
pub const DIRECT_STREAM_HEIGHT: u32 = STREAM_HEIGHT;
pub const DIRECT_STREAM_FPS: u32 = STREAM_FPS;
pub const DIRECT_STREAM_AUDIO_SAMPLE_RATE: u32 = STREAM_AUDIO_SAMPLE_RATE;
pub const DIRECT_STREAM_AUDIO_CHANNELS: usize = STREAM_AUDIO_CHANNELS;

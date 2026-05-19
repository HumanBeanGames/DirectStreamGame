pub(crate) const WINDOW_TITLE: &str = "Direct Stream Game";
pub(crate) const WEB_ADDR: &str = "127.0.0.1:8080";
pub(crate) const STREAM_PATH: &str = "/stream.mjpg";
pub(crate) const PALETTE_STREAM_PATH: &str = "/palette.bin";
pub(crate) const AUDIO_STREAM_PATH: &str = "/audio.pcm";
pub(crate) const LOCAL_CHAT_PATH: &str = "/local-chat";
pub(crate) const LOCAL_CHAT_FEED_PATH: &str = "/local-chat-feed";
pub(crate) const STREAM_STATUS_PATH: &str = "/status.json";
pub(crate) const STREAM_WIDTH: u32 = 320;
pub(crate) const STREAM_HEIGHT: u32 = 240;
pub(crate) const STATS_WINDOW_WIDTH: u32 = 560;
pub(crate) const STATS_WINDOW_HEIGHT: u32 = 680;
pub(crate) const STREAM_FPS: u32 = 5;
pub(crate) const TWITCH_VIDEO_BITRATE: usize = 350_000;
pub(crate) const STREAM_AUDIO_SAMPLE_RATE: u32 = 48_000;
pub(crate) const STREAM_AUDIO_CHANNELS: usize = 2;
pub(crate) const CUSTOM_AUDIO_SAMPLE_RATE: u32 = 8_000;
pub(crate) const CUSTOM_AUDIO_CHANNELS: usize = 1;
pub(crate) const STREAM_AUDIO_BUFFER_SECONDS: usize = 3;
pub(crate) const STREAM_AUDIO_MAX_MIX_FRAMES_PER_UPDATE: usize =
    STREAM_AUDIO_SAMPLE_RATE as usize / 10;

pub const DIRECT_STREAM_WIDTH: u32 = STREAM_WIDTH;
pub const DIRECT_STREAM_HEIGHT: u32 = STREAM_HEIGHT;
pub const DIRECT_STREAM_FPS: u32 = STREAM_FPS;
pub const DIRECT_STREAM_AUDIO_SAMPLE_RATE: u32 = STREAM_AUDIO_SAMPLE_RATE;
pub const DIRECT_STREAM_AUDIO_CHANNELS: usize = STREAM_AUDIO_CHANNELS;

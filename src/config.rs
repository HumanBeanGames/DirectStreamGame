use ffmpeg_next::util::log;
use std::{env, fs, path::PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WindowMode {
    Preview,
    Stats,
}

#[derive(bevy::prelude::Resource)]
pub(crate) struct AppConfig {
    pub(crate) window_mode: WindowMode,
    pub(crate) twitch_config_path: PathBuf,
    pub(crate) twitch_channel: String,
    pub(crate) chat_bot_username: String,
    pub(crate) chat_oauth_token: String,
    pub(crate) ingest_server: String,
    pub(crate) stream_key: String,
    pub(crate) bandwidth_test: bool,
    pub(crate) twitch_url_override: Option<String>,
}

struct TwitchConfig {
    channel: String,
    chat_bot_username: String,
    chat_oauth_token: String,
    ingest_server: String,
    stream_key: String,
    bandwidth_test: bool,
}

impl AppConfig {
    pub(crate) fn from_args() -> Self {
        let mut window_mode = WindowMode::Preview;
        let mut twitch_url_override = None;
        let mut twitch_config_path = PathBuf::from("twitch.toml");
        let mut ffmpeg_log_level = log::Level::Error;

        for arg in env::args().skip(1) {
            if arg == "--stats-window" || arg == "--headless-window" {
                window_mode = WindowMode::Stats;
            } else if arg == "--twitch" {
                // Twitch output is now controlled by the stats-window Start button.
            } else if let Some(path) = arg.strip_prefix("--twitch-config=") {
                twitch_config_path = PathBuf::from(path);
            } else if let Some(url) = arg.strip_prefix("--twitch-url=") {
                twitch_url_override = Some(url.to_owned());
            } else if arg == "--ffmpeg-warnings" {
                ffmpeg_log_level = log::Level::Warning;
            } else if arg == "--ffmpeg-verbose" {
                ffmpeg_log_level = log::Level::Info;
            }
        }

        log::set_level(ffmpeg_log_level);
        let twitch_config = TwitchConfig::from_file(&twitch_config_path).unwrap_or_default();
        let stream_key = if twitch_config.stream_key == "live_your_stream_key_here" {
            String::new()
        } else {
            twitch_config.stream_key
        };

        Self {
            window_mode,
            twitch_config_path,
            twitch_channel: twitch_config.channel,
            chat_bot_username: twitch_config.chat_bot_username,
            chat_oauth_token: twitch_config.chat_oauth_token,
            ingest_server: twitch_config.ingest_server,
            stream_key,
            bandwidth_test: twitch_config.bandwidth_test,
            twitch_url_override,
        }
    }
}

impl TwitchConfig {
    fn from_file(path: &PathBuf) -> Result<Self, String> {
        let contents = fs::read_to_string(path).map_err(|err| err.to_string())?;
        let mut ingest_server = "rtmp://live.twitch.tv/app".to_owned();
        let mut channel = String::new();
        let mut chat_bot_username = String::new();
        let mut chat_oauth_token = String::new();
        let mut stream_key = String::new();
        let mut bandwidth_test = false;

        for line in contents.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with('[') {
                continue;
            }

            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = parse_config_value(value.trim());

            match key {
                "enabled" => {}
                "channel" => channel = value,
                "chat_bot_username" => chat_bot_username = value,
                "chat_oauth_token" => chat_oauth_token = value,
                "ingest_server" => ingest_server = value,
                "stream_key" => stream_key = value,
                "bandwidth_test" => bandwidth_test = value.eq_ignore_ascii_case("true"),
                _ => {}
            }
        }

        Ok(Self {
            channel,
            chat_bot_username,
            chat_oauth_token,
            ingest_server,
            stream_key,
            bandwidth_test,
        })
    }

    fn rtmp_url(&self) -> Option<String> {
        let stream_key = self.stream_key.trim();
        if stream_key.is_empty() || stream_key == "live_your_stream_key_here" {
            return None;
        }

        let server = self.ingest_server.trim();
        let mut url = if server.contains("{stream_key}") {
            server.replace("{stream_key}", stream_key)
        } else {
            format!("{}/{}", server.trim_end_matches('/'), stream_key)
        };

        if self.bandwidth_test && !url.contains("bandwidthtest=true") {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str("bandwidthtest=true");
        }

        Some(url)
    }
}

impl Default for TwitchConfig {
    fn default() -> Self {
        Self {
            channel: String::new(),
            chat_bot_username: String::new(),
            chat_oauth_token: String::new(),
            ingest_server: "rtmp://live.twitch.tv/app".to_owned(),
            stream_key: String::new(),
            bandwidth_test: false,
        }
    }
}

pub(crate) fn twitch_rtmp_url(
    ingest_server: &str,
    stream_key: &str,
    bandwidth_test: bool,
) -> Option<String> {
    TwitchConfig {
        channel: String::new(),
        chat_bot_username: String::new(),
        chat_oauth_token: String::new(),
        ingest_server: ingest_server.to_owned(),
        stream_key: stream_key.to_owned(),
        bandwidth_test,
    }
    .rtmp_url()
}

pub(crate) fn save_twitch_stream_key(
    path: &PathBuf,
    channel: &str,
    chat_bot_username: &str,
    chat_oauth_token: &str,
    ingest_server: &str,
    stream_key: &str,
    bandwidth_test: bool,
) -> Result<(), String> {
    let escaped_key = stream_key.replace('\\', "\\\\").replace('"', "\\\"");
    let escaped_server = ingest_server.replace('\\', "\\\\").replace('"', "\\\"");
    let escaped_channel = channel.replace('\\', "\\\\").replace('"', "\\\"");
    let escaped_bot = chat_bot_username.replace('\\', "\\\\").replace('"', "\\\"");
    let escaped_oauth = chat_oauth_token.replace('\\', "\\\\").replace('"', "\\\"");
    let contents = format!(
        "enabled = false\nchannel = \"{escaped_channel}\"\nchat_bot_username = \"{escaped_bot}\"\nchat_oauth_token = \"{escaped_oauth}\"\ningest_server = \"{escaped_server}\"\nstream_key = \"{escaped_key}\"\nbandwidth_test = {}\n",
        if bandwidth_test { "true" } else { "false" }
    );
    fs::write(path, contents).map_err(|err| err.to_string())
}

fn parse_config_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .to_owned()
}

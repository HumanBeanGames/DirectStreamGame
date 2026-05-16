use crate::{
    audio::DirectStreamAudioTarget,
    chat::TwitchChatLogin,
    config::{AppConfig, save_twitch_stream_key, twitch_rtmp_url},
    frames::{RawFrameHub, RawFrameSenders},
    stats::SharedStats,
    twitch::{TwitchStreamHandle, start_twitch_sink},
};
use bevy::{input::keyboard::KeyboardInput, prelude::*};
use crossbeam_channel::Sender;
use std::process::Command;

use crate::frames::RawFrame;

#[derive(Resource)]
pub(crate) struct StreamControl {
    pub(crate) stream_key: String,
    pub(crate) chat_bot_username: String,
    pub(crate) chat_oauth_token: String,
    pub(crate) focused_input: Option<StreamControlInput>,
    pub(crate) status: String,
    twitch_config_path: std::path::PathBuf,
    twitch_channel: String,
    ingest_server: String,
    bandwidth_test: bool,
    twitch_url_override: Option<String>,
    preview_sender: Option<Sender<RawFrame>>,
    twitch_handle: Option<TwitchStreamHandle>,
}

impl StreamControl {
    pub(crate) fn new(config: &AppConfig, preview_sender: Option<Sender<RawFrame>>) -> Self {
        Self {
            stream_key: config.stream_key.clone(),
            chat_bot_username: config.chat_bot_username.clone(),
            chat_oauth_token: config.chat_oauth_token.clone(),
            focused_input: None,
            status: "Ready".to_owned(),
            twitch_config_path: config.twitch_config_path.clone(),
            twitch_channel: config.twitch_channel.clone(),
            ingest_server: config.ingest_server.clone(),
            bandwidth_test: config.bandwidth_test,
            twitch_url_override: config.twitch_url_override.clone(),
            preview_sender,
            twitch_handle: None,
        }
    }

    pub(crate) fn mark_started(&mut self, handle: TwitchStreamHandle) {
        self.twitch_handle = Some(handle);
        self.status = "Streaming".to_owned();
    }

    pub(crate) fn is_streaming(&self) -> bool {
        self.twitch_handle.is_some()
    }

    pub(crate) fn should_capture(&self) -> bool {
        self.is_streaming() || self.preview_sender.is_some()
    }

    fn start(
        &mut self,
        senders: &mut RawFrameSenders,
        stats: &SharedStats,
        audio_target: &DirectStreamAudioTarget,
        chat_login: Option<&TwitchChatLogin>,
    ) {
        if self.is_streaming() {
            self.status = "Already streaming".to_owned();
            return;
        }

        let stream_key = self.stream_key.trim();
        if stream_key.is_empty() {
            self.status = "Paste a Twitch stream key first".to_owned();
            return;
        }

        if let Err(err) = save_twitch_stream_key(
            &self.twitch_config_path,
            &self.twitch_channel,
            &self.chat_bot_username,
            &self.chat_oauth_token,
            &self.ingest_server,
            stream_key,
            self.bandwidth_test,
        ) {
            self.status = format!("Could not save twitch.toml: {err}");
            return;
        }

        if let Some(chat_login) = chat_login {
            chat_login.connect(
                &self.twitch_channel,
                &self.chat_bot_username,
                &self.chat_oauth_token,
            );
        }

        let Some(twitch_url) = self
            .twitch_url_override
            .clone()
            .or_else(|| twitch_rtmp_url(&self.ingest_server, stream_key, self.bandwidth_test))
        else {
            self.status = "Invalid Twitch stream key".to_owned();
            return;
        };

        let twitch_hub = RawFrameHub::new();
        let handle = start_twitch_sink(
            twitch_hub.clone(),
            twitch_url,
            stats.clone(),
            audio_target.clone(),
        );
        senders.preview = None;
        senders.twitch = Some(twitch_hub);
        self.mark_started(handle);
        self.status = "Streaming".to_owned();
        stats.with_mut(|stats| {
            stats.reset_twitch_session();
            stats.twitch_stage = "starting";
        });
    }

    fn stop(&mut self, senders: &mut RawFrameSenders, stats: &SharedStats) {
        let Some(handle) = self.twitch_handle.take() else {
            self.status = "Not streaming".to_owned();
            return;
        };

        handle.stop();
        senders.twitch = None;
        if self.preview_sender.is_some() {
            senders.preview = self.preview_sender.clone();
        }
        self.status = "Stopping stream".to_owned();
        stats.with_mut(|stats| {
            stats.twitch_stage = "stop requested";
            stats.twitch_kbps = 0.0;
        });
    }

    fn open_twitch_stream(&mut self) {
        let channel = self.twitch_channel.trim().trim_start_matches('#');
        if channel.is_empty() || channel == "your_channel_name" {
            self.status = "Set channel in twitch.toml first".to_owned();
            return;
        }

        let url = format!("https://www.twitch.tv/{channel}");
        match open_url(&url) {
            Ok(()) => self.status = format!("Opened twitch.tv/{channel}"),
            Err(err) => self.status = format!("Could not open Twitch URL: {err}"),
        }
    }
}

#[derive(Component)]
pub(crate) struct StreamKeyInputBox;
#[derive(Component)]
pub(crate) struct ChatBotUsernameInputBox;
#[derive(Component)]
pub(crate) struct ChatOauthTokenInputBox;

#[derive(Component)]
pub(crate) struct StreamKeyInputText;
#[derive(Component)]
pub(crate) struct ChatBotUsernameInputText;
#[derive(Component)]
pub(crate) struct ChatOauthTokenInputText;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamControlInput {
    StreamKey,
    ChatBotUsername,
    ChatOauthToken,
}

#[derive(Component)]
pub(crate) struct StreamControlStatusText;

#[derive(Component)]
pub(crate) struct StartStreamButton;

#[derive(Component)]
pub(crate) struct StopStreamButton;

#[derive(Component)]
pub(crate) struct OpenTwitchStreamButton;

pub(crate) fn handle_stream_key_typing(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut key_events: MessageReader<KeyboardInput>,
    mut control: ResMut<StreamControl>,
) {
    let Some(focused_input) = control.focused_input else {
        return;
    };

    if keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
        && keyboard.just_pressed(KeyCode::KeyV)
    {
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
            Ok(text) => {
                push_focused_text(&mut control, focused_input, text.trim());
                control.status = "Pasted".to_owned();
            }
            Err(err) => control.status = format!("Clipboard unavailable: {err}"),
        }
    }

    for event in key_events.read() {
        if !event.state.is_pressed() {
            continue;
        }

        match event.key_code {
            KeyCode::Backspace => {
                pop_focused_text(&mut control, focused_input);
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                control.focused_input = None;
            }
            _ => {
                if keyboard.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
                    continue;
                }
                if let Some(text) = &event.text
                    && text.chars().all(is_printable_char)
                {
                    push_focused_text(&mut control, focused_input, text);
                }
            }
        }
    }
}

pub(crate) fn handle_stream_control_interactions(
    mut control: ResMut<StreamControl>,
    mut senders: ResMut<RawFrameSenders>,
    stats: Res<SharedStats>,
    audio_target: Res<DirectStreamAudioTarget>,
    chat_login: Option<Res<TwitchChatLogin>>,
    mut controls: Query<
        (
            &Interaction,
            &mut BackgroundColor,
            Option<&StreamKeyInputBox>,
            Option<&ChatBotUsernameInputBox>,
            Option<&ChatOauthTokenInputBox>,
            Option<&StartStreamButton>,
            Option<&StopStreamButton>,
            Option<&OpenTwitchStreamButton>,
        ),
        Changed<Interaction>,
    >,
) {
    for (
        interaction,
        mut color,
        key_box,
        bot_box,
        token_box,
        start_button,
        stop_button,
        open_button,
    ) in &mut controls
    {
        if key_box.is_some() {
            set_input_interaction_color(
                *interaction,
                &mut color,
                &mut control,
                StreamControlInput::StreamKey,
            );
        } else if bot_box.is_some() {
            set_input_interaction_color(
                *interaction,
                &mut color,
                &mut control,
                StreamControlInput::ChatBotUsername,
            );
        } else if token_box.is_some() {
            set_input_interaction_color(
                *interaction,
                &mut color,
                &mut control,
                StreamControlInput::ChatOauthToken,
            );
        } else if start_button.is_some() {
            match *interaction {
                Interaction::Pressed => {
                    control.focused_input = None;
                    control.start(&mut senders, &stats, &audio_target, chat_login.as_deref());
                    *color = BackgroundColor(Color::srgb(0.10, 0.36, 0.22));
                }
                Interaction::Hovered => *color = BackgroundColor(Color::srgb(0.08, 0.28, 0.18)),
                Interaction::None => *color = BackgroundColor(Color::srgb(0.05, 0.20, 0.13)),
            }
        } else if stop_button.is_some() {
            match *interaction {
                Interaction::Pressed => {
                    control.focused_input = None;
                    control.stop(&mut senders, &stats);
                    *color = BackgroundColor(Color::srgb(0.38, 0.11, 0.12));
                }
                Interaction::Hovered => *color = BackgroundColor(Color::srgb(0.30, 0.09, 0.10)),
                Interaction::None => *color = BackgroundColor(Color::srgb(0.21, 0.06, 0.07)),
            }
        } else if open_button.is_some() {
            match *interaction {
                Interaction::Pressed => {
                    control.focused_input = None;
                    control.open_twitch_stream();
                    *color = BackgroundColor(Color::srgb(0.13, 0.19, 0.34));
                }
                Interaction::Hovered => *color = BackgroundColor(Color::srgb(0.10, 0.15, 0.27)),
                Interaction::None => *color = BackgroundColor(Color::srgb(0.07, 0.10, 0.19)),
            }
        } else if *interaction == Interaction::Pressed {
            control.focused_input = None;
        }
    }
}

pub(crate) fn update_stream_control_ui(
    control: Res<StreamControl>,
    mut text_query: Query<(
        &mut Text,
        Option<&StreamKeyInputText>,
        Option<&ChatBotUsernameInputText>,
        Option<&ChatOauthTokenInputText>,
        Option<&StreamControlStatusText>,
    )>,
) {
    if !control.is_changed() {
        return;
    }

    for (mut text, key_text, bot_text, token_text, status_text) in &mut text_query {
        if key_text.is_some() {
            text.0 = masked_input_text(
                &control.stream_key,
                control.focused_input == Some(StreamControlInput::StreamKey),
                "paste stream key",
            );
        } else if bot_text.is_some() {
            text.0 = plain_input_text(
                &control.chat_bot_username,
                control.focused_input == Some(StreamControlInput::ChatBotUsername),
                "bot username",
            );
        } else if token_text.is_some() {
            text.0 = masked_input_text(
                &control.chat_oauth_token,
                control.focused_input == Some(StreamControlInput::ChatOauthToken),
                "chat oauth token",
            );
        } else if status_text.is_some() {
            let mode = if control.is_streaming() {
                "live"
            } else {
                "idle"
            };
            text.0 = format!("stream control: {mode} - {}", control.status);
        }
    }
}

fn is_printable_char(chr: char) -> bool {
    let is_in_private_use_area = ('\u{e000}'..='\u{f8ff}').contains(&chr)
        || ('\u{f0000}'..='\u{ffffd}').contains(&chr)
        || ('\u{100000}'..='\u{10fffd}').contains(&chr);

    !is_in_private_use_area && !chr.is_ascii_control()
}

fn push_focused_text(control: &mut StreamControl, input: StreamControlInput, text: &str) {
    match input {
        StreamControlInput::StreamKey => control.stream_key.push_str(text),
        StreamControlInput::ChatBotUsername => control.chat_bot_username.push_str(text),
        StreamControlInput::ChatOauthToken => control.chat_oauth_token.push_str(text),
    }
}

fn pop_focused_text(control: &mut StreamControl, input: StreamControlInput) {
    match input {
        StreamControlInput::StreamKey => {
            control.stream_key.pop();
        }
        StreamControlInput::ChatBotUsername => {
            control.chat_bot_username.pop();
        }
        StreamControlInput::ChatOauthToken => {
            control.chat_oauth_token.pop();
        }
    }
}

fn set_input_interaction_color(
    interaction: Interaction,
    color: &mut BackgroundColor,
    control: &mut StreamControl,
    input: StreamControlInput,
) {
    match interaction {
        Interaction::Pressed => {
            control.focused_input = Some(input);
            *color = BackgroundColor(Color::srgb(0.10, 0.15, 0.21));
        }
        Interaction::Hovered => *color = BackgroundColor(Color::srgb(0.08, 0.12, 0.17)),
        Interaction::None => {
            *color = if control.focused_input == Some(input) {
                BackgroundColor(Color::srgb(0.10, 0.15, 0.21))
            } else {
                BackgroundColor(Color::srgb(0.045, 0.055, 0.07))
            };
        }
    }
}

fn masked_input_text(value: &str, focused: bool, placeholder: &str) -> String {
    if value.is_empty() {
        if focused {
            "|".to_owned()
        } else {
            placeholder.to_owned()
        }
    } else {
        let visible_chars = value.chars().count().min(6);
        let tail: String = value
            .chars()
            .rev()
            .take(visible_chars)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        let cursor = if focused { "|" } else { "" };
        format!("...{tail}{cursor}")
    }
}

fn plain_input_text(value: &str, focused: bool, placeholder: &str) -> String {
    if value.is_empty() {
        if focused {
            "|".to_owned()
        } else {
            placeholder.to_owned()
        }
    } else if focused {
        format!("{value}|")
    } else {
        value.to_owned()
    }
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|err| err.to_string())
    }
}

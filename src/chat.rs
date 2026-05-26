use bevy::{
    app::App,
    ecs::system::{In, IntoSystem, SystemId},
    prelude::*,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Message, Clone)]
pub struct StreamChatMessage {
    pub user: String,
    pub display_name: String,
    pub text: String,
    pub roles: StreamChatRoles,
    pub message_id: Option<String>,
}

#[derive(Message, Clone)]
pub struct StreamChatCommand {
    pub user: String,
    pub display_name: String,
    pub command: String,
    pub args: String,
    pub roles: StreamChatRoles,
    pub message_id: Option<String>,
}

#[derive(Clone, Default)]
pub struct StreamChatRoles {
    pub broadcaster: bool,
    pub moderator: bool,
    pub vip: bool,
    pub subscriber: bool,
    pub staff: bool,
}

#[derive(Clone)]
pub enum ChatAudience {
    All,
    ViewerIdentity(String),
    ViewerName(String),
}

#[derive(Clone)]
pub struct LocalChatEntryOptions {
    pub display_name: String,
    pub text: String,
    pub ttl_ms: Option<u64>,
    pub audience: ChatAudience,
    pub mentions: Vec<String>,
    pub display_name_color: Option<String>,
    pub message_color: Option<String>,
    pub css_class: Option<String>,
}

impl LocalChatEntryOptions {
    pub fn system(message: impl Into<String>) -> Self {
        let text = message.into();
        Self {
            display_name: "system".to_owned(),
            mentions: mentions_from_text(&text),
            text,
            ttl_ms: None,
            audience: ChatAudience::All,
            display_name_color: None,
            message_color: None,
            css_class: None,
        }
    }

    pub fn named(display_name: impl Into<String>, message: impl Into<String>) -> Self {
        let text = message.into();
        Self {
            display_name: display_name.into(),
            mentions: mentions_from_text(&text),
            text,
            ttl_ms: None,
            audience: ChatAudience::All,
            display_name_color: None,
            message_color: None,
            css_class: None,
        }
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl_ms = Some(ttl.as_millis().min(u128::from(u64::MAX)) as u64);
        self
    }

    pub fn for_viewer_identity(mut self, identity: impl Into<String>) -> Self {
        self.audience = ChatAudience::ViewerIdentity(identity.into());
        self
    }

    pub fn for_viewer_name(mut self, name: impl Into<String>) -> Self {
        self.audience = ChatAudience::ViewerName(name.into());
        self
    }

    pub fn with_display_name_color(mut self, color: impl Into<String>) -> Self {
        self.display_name_color = sanitize_chat_color(color.into());
        self
    }

    pub fn with_message_color(mut self, color: impl Into<String>) -> Self {
        self.message_color = sanitize_chat_color(color.into());
        self
    }

    pub fn with_css_class(mut self, class: impl Into<String>) -> Self {
        self.css_class = sanitize_chat_class(class.into());
        self
    }
}

#[derive(Clone, Resource, Default)]
pub(crate) struct LocalChatHub {
    state: Arc<Mutex<LocalChatState>>,
}

struct LocalChatState {
    messages: VecDeque<LocalChatSubmission>,
    history: VecDeque<LocalChatEntry>,
    next_id: u64,
    generation: u64,
    names_by_identity: HashMap<String, String>,
    blocked_identities: HashMap<String, String>,
}

impl Default for LocalChatState {
    fn default() -> Self {
        Self {
            messages: VecDeque::new(),
            history: VecDeque::new(),
            next_id: 1,
            generation: 0,
            names_by_identity: HashMap::new(),
            blocked_identities: HashMap::new(),
        }
    }
}

#[derive(Clone)]
struct LocalChatSubmission {
    user: String,
    display_name: String,
    text: String,
}

#[derive(Clone)]
pub(crate) struct LocalChatEntry {
    pub(crate) id: u64,
    pub(crate) user: String,
    pub(crate) display_name: String,
    pub(crate) text: String,
    pub(crate) created_at_ms: u64,
    pub(crate) ttl_ms: Option<u64>,
    pub(crate) audience: ChatAudience,
    pub(crate) mentions: Vec<String>,
    pub(crate) display_name_color: Option<String>,
    pub(crate) message_color: Option<String>,
    pub(crate) css_class: Option<String>,
}

impl LocalChatHub {
    pub(crate) fn submit(
        &self,
        identity: impl AsRef<str>,
        message: impl Into<String>,
    ) -> Option<String> {
        let identity_hash = local_chat_identity_hash(identity.as_ref());
        let mut state = self.state.lock().ok()?;
        if state.blocked_identities.contains_key(&identity_hash) {
            return None;
        }

        let display_name = display_name_for_identity_hash(&mut state, &identity_hash);
        let text = message.into();
        let entry = LocalChatEntry {
            id: state.next_id,
            user: identity_hash.clone(),
            display_name: display_name.clone(),
            mentions: mentions_from_text(&text),
            text,
            created_at_ms: current_time_millis(),
            ttl_ms: None,
            audience: ChatAudience::All,
            display_name_color: Some(display_name_color_from_hash(&identity_hash)),
            message_color: None,
            css_class: None,
        };
        state.next_id = state.next_id.wrapping_add(1);
        state.messages.push_back(LocalChatSubmission {
            user: entry.user.clone(),
            display_name: entry.display_name.clone(),
            text: entry.text.clone(),
        });
        state.history.push_back(entry);
        trim_history(&mut state);
        Some(display_name)
    }

    pub(crate) fn entries_after(
        &self,
        last_seen_id: u64,
        viewer_identity: Option<&str>,
        viewer_name: Option<&str>,
    ) -> Vec<LocalChatEntry> {
        if let Ok(mut state) = self.state.lock() {
            purge_expired_locked(&mut state, current_time_millis());
            state
                .history
                .iter()
                .filter(|entry| entry.id > last_seen_id)
                .filter(|entry| entry_matches_audience(entry, viewer_identity, viewer_name))
                .cloned()
                .collect()
        } else {
            Vec::new()
        }
    }

    pub(crate) fn latest_id(&self) -> u64 {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.history.back().map(|entry| entry.id))
            .unwrap_or(0)
    }

    pub(crate) fn viewer_for_identity(&self, identity: impl AsRef<str>) -> (String, String) {
        let identity_hash = local_chat_identity_hash(identity.as_ref());
        if let Ok(mut state) = self.state.lock() {
            let display_name = display_name_for_identity_hash(&mut state, &identity_hash);
            (identity_hash, display_name)
        } else {
            (
                identity_hash.clone(),
                local_chat_name_from_hash(&identity_hash),
            )
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.state
            .lock()
            .map(|state| state.generation)
            .unwrap_or_default()
    }

    pub(crate) fn purge(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.messages.clear();
            state.history.clear();
            state.next_id = 1;
            state.generation = state.generation.wrapping_add(1);
        }
    }

    pub(crate) fn purge_expired(&self, now_ms: u64) {
        if let Ok(mut state) = self.state.lock() {
            purge_expired_locked(&mut state, now_ms);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn block_identity(
        &self,
        identity_hash: impl Into<String>,
        reason: impl Into<String>,
    ) {
        if let Ok(mut state) = self.state.lock() {
            state
                .blocked_identities
                .insert(identity_hash.into(), reason.into());
        }
    }

    #[allow(dead_code)]
    pub(crate) fn active_names(&self) -> Vec<(String, String)> {
        if let Ok(state) = self.state.lock() {
            state
                .names_by_identity
                .iter()
                .map(|(identity, name)| (identity.clone(), name.clone()))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub(crate) fn append_local_entry(&self, options: LocalChatEntryOptions) {
        if let Ok(mut state) = self.state.lock() {
            let entry = LocalChatEntry {
                id: state.next_id,
                user: "system".to_owned(),
                display_name: options.display_name,
                text: options.text,
                created_at_ms: current_time_millis(),
                ttl_ms: options.ttl_ms,
                audience: options.audience,
                mentions: options.mentions,
                display_name_color: options.display_name_color,
                message_color: options.message_color,
                css_class: options.css_class,
            };
            state.next_id = state.next_id.wrapping_add(1);
            state.history.push_back(entry);
            trim_history(&mut state);
        }
    }

    fn drain(&self) -> Vec<LocalChatSubmission> {
        if let Ok(mut state) = self.state.lock() {
            state.messages.drain(..).collect()
        } else {
            Vec::new()
        }
    }
}

#[derive(Resource, Default)]
pub struct StreamCommandRouter {
    handlers: HashMap<String, SystemId<In<StreamChatCommand>>>,
}

#[derive(Clone, Resource)]
pub struct StreamChatSender {
    local_chat: Option<LocalChatHub>,
}

pub trait StreamCommandAppExt {
    fn add_stream_command<M>(
        &mut self,
        command: impl Into<String>,
        system: impl IntoSystem<In<StreamChatCommand>, (), M> + 'static,
    ) -> &mut Self;
}

impl StreamCommandAppExt for App {
    fn add_stream_command<M>(
        &mut self,
        command: impl Into<String>,
        system: impl IntoSystem<In<StreamChatCommand>, (), M> + 'static,
    ) -> &mut Self {
        let system_id = self.register_system(system);
        self.init_resource::<StreamCommandRouter>();
        self.world_mut()
            .resource_mut::<StreamCommandRouter>()
            .register(command, system_id);
        self
    }
}

impl StreamCommandRouter {
    pub fn register(
        &mut self,
        command: impl Into<String>,
        system_id: SystemId<In<StreamChatCommand>>,
    ) {
        let command = normalize_command_name(&command.into());
        if !command.is_empty() {
            self.handlers.insert(command, system_id);
        }
    }

    fn handler(&self, command: &str) -> Option<SystemId<In<StreamChatCommand>>> {
        self.handlers.get(command).copied()
    }
}

impl StreamChatSender {
    pub fn send(&self, message: impl Into<String>) {
        self.send_local(LocalChatEntryOptions::system(message).with_ttl(Duration::from_secs(10)));
    }

    pub fn send_local(&self, entry: LocalChatEntryOptions) {
        if let Some(local_chat) = &self.local_chat {
            local_chat.append_local_entry(entry);
        }
    }
}

pub(crate) fn init_stream_chat_sender(
    mut commands: Commands,
    local_chat: Option<Res<LocalChatHub>>,
) {
    commands.insert_resource(StreamChatSender {
        local_chat: local_chat.map(|hub| hub.clone()),
    });
}

pub(crate) fn poll_local_chat(
    hub: Option<Res<LocalChatHub>>,
    mut messages: MessageWriter<StreamChatMessage>,
    mut commands: MessageWriter<StreamChatCommand>,
) {
    let Some(hub) = hub else {
        return;
    };

    for submission in hub.drain() {
        let message = StreamChatMessage {
            user: submission.user,
            display_name: submission.display_name,
            text: submission.text,
            roles: StreamChatRoles::default(),
            message_id: None,
        };

        if let Some((command, args)) = parse_stream_command(&message.text) {
            commands.write(StreamChatCommand {
                user: message.user.clone(),
                display_name: message.display_name.clone(),
                command,
                args,
                roles: message.roles.clone(),
                message_id: None,
            });
        }
        messages.write(message);
    }
}

pub(crate) fn dispatch_stream_chat_commands(
    mut reader: MessageReader<StreamChatCommand>,
    router: Option<Res<StreamCommandRouter>>,
    mut commands: Commands,
) {
    let Some(router) = router else {
        return;
    };

    for command in reader.read() {
        if let Some(handler) = router.handler(&command.command) {
            commands.run_system_with(handler, command.clone());
        }
    }
}

fn current_time_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn display_name_for_identity_hash(state: &mut LocalChatState, identity_hash: &str) -> String {
    state
        .names_by_identity
        .entry(identity_hash.to_owned())
        .or_insert_with(|| local_chat_name_from_hash(identity_hash))
        .clone()
}

fn trim_history(state: &mut LocalChatState) {
    while state.history.len() > 200 {
        state.history.pop_front();
    }
}

fn purge_expired_locked(state: &mut LocalChatState, now_ms: u64) {
    state
        .history
        .retain(|entry| !entry_is_expired(entry, now_ms));
}

fn entry_is_expired(entry: &LocalChatEntry, now_ms: u64) -> bool {
    entry
        .ttl_ms
        .is_some_and(|ttl| now_ms >= entry.created_at_ms.saturating_add(ttl))
}

fn entry_matches_audience(
    entry: &LocalChatEntry,
    viewer_identity: Option<&str>,
    viewer_name: Option<&str>,
) -> bool {
    match &entry.audience {
        ChatAudience::All => true,
        ChatAudience::ViewerIdentity(identity) => viewer_identity == Some(identity.as_str()),
        ChatAudience::ViewerName(name) => viewer_name
            .map(|viewer_name| viewer_name.eq_ignore_ascii_case(name))
            .unwrap_or(false),
    }
}

fn sanitize_chat_color(color: String) -> Option<String> {
    let trimmed = color.trim();
    if trimmed.len() > 64 {
        return None;
    }

    if is_hex_color(trimmed)
        || is_css_rgb_function(trimmed)
        || is_css_hsl_function(trimmed)
        || is_named_chat_color(trimmed)
    {
        Some(trimmed.to_owned())
    } else {
        None
    }
}

fn is_hex_color(color: &str) -> bool {
    let Some(hex) = color.strip_prefix('#') else {
        return false;
    };
    matches!(hex.len(), 3 | 4 | 6 | 8) && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_css_rgb_function(color: &str) -> bool {
    let Some(inner) = color
        .strip_prefix("rgb(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return false;
    };
    inner
        .split(',')
        .map(str::trim)
        .all(|part| part.parse::<u8>().is_ok())
        && inner.split(',').count() == 3
}

fn is_css_hsl_function(color: &str) -> bool {
    let Some(inner) = color
        .strip_prefix("hsl(")
        .and_then(|value| value.strip_suffix(')'))
    else {
        return false;
    };
    let parts = inner.split_whitespace().collect::<Vec<_>>();
    if parts.len() != 3 {
        return false;
    }
    parts[0].parse::<u16>().is_ok_and(|hue| hue <= 360)
        && parts[1]
            .strip_suffix('%')
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|percent| percent <= 100)
        && parts[2]
            .strip_suffix('%')
            .and_then(|value| value.parse::<u8>().ok())
            .is_some_and(|percent| percent <= 100)
}

fn is_named_chat_color(color: &str) -> bool {
    color
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte == b'-')
        && matches!(
            color,
            "white"
                | "black"
                | "red"
                | "green"
                | "blue"
                | "yellow"
                | "cyan"
                | "magenta"
                | "orange"
                | "purple"
                | "pink"
                | "lime"
                | "teal"
                | "gold"
                | "silver"
                | "gray"
                | "grey"
        )
}

fn sanitize_chat_class(class: String) -> Option<String> {
    let tokens = class
        .split_whitespace()
        .filter(|token| {
            !token.is_empty()
                && token.len() <= 48
                && token
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .take(4)
        .collect::<Vec<_>>();

    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

fn mentions_from_text(text: &str) -> Vec<String> {
    text.split_whitespace()
        .filter_map(|word| {
            word.strip_prefix('@')
                .map(|mention| {
                    mention.trim_matches(|ch: char| {
                        !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_'
                    })
                })
                .filter(|mention| !mention.is_empty())
                .map(str::to_owned)
        })
        .collect()
}

fn parse_stream_command(text: &str) -> Option<(String, String)> {
    let mut parts = text.trim().splitn(2, char::is_whitespace);
    let command = parts.next()?;
    let args = parts.next().unwrap_or("").trim().to_owned();
    command
        .strip_prefix('!')
        .filter(|command| !command.is_empty())
        .map(|command| (normalize_command_name(command), args))
}

fn normalize_command_name(command: &str) -> String {
    command.trim().trim_start_matches('!').to_ascii_lowercase()
}

pub(crate) fn local_chat_identity_hash(identity: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;
    let mut hash = FNV_OFFSET;
    for byte in identity.trim().to_ascii_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

fn local_chat_name_from_hash(hash: &str) -> String {
    const ADJECTIVES: &[&str] = &[
        "Amber", "Brave", "Bright", "Clever", "Cozy", "Daring", "Gentle", "Jolly", "Lucky",
        "Merry", "Nimble", "Quiet", "Ruby", "Silver", "Sunny", "Velvet",
    ];
    const CREATURES: &[&str] = &[
        "Dragon", "Sprite", "Phoenix", "Griffin", "Wyvern", "Unicorn", "Kelpie", "Dryad", "Golem",
        "Pixie", "Wisp", "Sphinx", "Kirin", "Sylph", "Basilisk", "Chimera",
    ];
    const SYMBOLS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

    let value = u64::from_str_radix(hash, 16).unwrap_or(0);
    let adjective = ADJECTIVES[(value as usize) % ADJECTIVES.len()];
    let creature = CREATURES[((value >> 8) as usize) % CREATURES.len()];
    let suffix_a = SYMBOLS[((value >> 16) as usize) % SYMBOLS.len()] as char;
    let suffix_b = SYMBOLS[((value >> 24) as usize) % SYMBOLS.len()] as char;
    format!("{adjective}{creature}-{suffix_a}{suffix_b}")
}

fn display_name_color_from_hash(hash: &str) -> String {
    let value = u64::from_str_radix(hash, 16).unwrap_or(0);
    let hue = value % 360;
    format!("hsl({hue} 78% 68%)")
}

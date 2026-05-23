use crate::config::AppConfig;
use bevy::prelude::*;
use bevy::{
    app::App,
    ecs::system::{In, IntoSystem, SystemId},
};
use crossbeam_channel::{Receiver, Sender};
use std::collections::{HashMap, VecDeque};
use std::{
    io::{BufRead, BufReader, ErrorKind, Write},
    net::TcpStream,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Message, Clone)]
pub struct TwitchChatMessage {
    pub user: String,
    pub display_name: String,
    pub text: String,
    pub roles: TwitchChatRoles,
    pub message_id: Option<String>,
}

#[derive(Message, Clone)]
pub struct TwitchChatCommand {
    pub user: String,
    pub display_name: String,
    pub command: String,
    pub args: String,
    pub roles: TwitchChatRoles,
    pub message_id: Option<String>,
}

#[derive(Clone, Default)]
pub struct TwitchChatRoles {
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
        };
        state.next_id = state.next_id.wrapping_add(1);
        state.messages.push_back(LocalChatSubmission {
            user: entry.user.clone(),
            display_name: entry.display_name.clone(),
            text: entry.text.clone(),
        });
        state.history.push_back(entry);
        while state.history.len() > 200 {
            state.history.pop_front();
        }
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

    #[allow(dead_code)]
    pub(crate) fn append_system_message(
        &self,
        display_name: impl Into<String>,
        message: impl Into<String>,
    ) {
        self.append_local_entry(LocalChatEntryOptions::named(display_name, message));
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
            };
            state.next_id = state.next_id.wrapping_add(1);
            state.history.push_back(entry);
            while state.history.len() > 200 {
                state.history.pop_front();
            }
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
pub struct TwitchCommandRouter {
    handlers: HashMap<String, SystemId<In<TwitchChatCommand>>>,
}

#[derive(Resource)]
pub(crate) struct TwitchChatReceiver {
    receiver: Receiver<TwitchChatMessage>,
}

#[derive(Clone, Resource)]
pub struct TwitchChatSender {
    sender: Sender<String>,
    local_chat: Option<LocalChatHub>,
}

#[derive(Clone, Resource)]
pub struct TwitchChatLogin {
    sender: Sender<TwitchChatLoginRequest>,
}

#[derive(Clone)]
struct TwitchChatLoginRequest {
    channel: String,
    bot_username: String,
    oauth_token: Option<String>,
}

enum ChatConnectionOutcome {
    Disconnected,
    Reconnect(TwitchChatLoginRequest),
}

pub trait TwitchCommandAppExt {
    fn add_twitch_command<M>(
        &mut self,
        command: impl Into<String>,
        system: impl IntoSystem<In<TwitchChatCommand>, (), M> + 'static,
    ) -> &mut Self;
}

impl TwitchCommandAppExt for App {
    fn add_twitch_command<M>(
        &mut self,
        command: impl Into<String>,
        system: impl IntoSystem<In<TwitchChatCommand>, (), M> + 'static,
    ) -> &mut Self {
        let system_id = self.register_system(system);
        self.init_resource::<TwitchCommandRouter>();
        self.world_mut()
            .resource_mut::<TwitchCommandRouter>()
            .register(command, system_id);
        self
    }
}

impl TwitchCommandRouter {
    pub fn register(
        &mut self,
        command: impl Into<String>,
        system_id: SystemId<In<TwitchChatCommand>>,
    ) {
        let command = normalize_command_name(&command.into());
        if !command.is_empty() {
            self.handlers.insert(command, system_id);
        }
    }

    fn handler(&self, command: &str) -> Option<SystemId<In<TwitchChatCommand>>> {
        self.handlers.get(command).copied()
    }
}

impl TwitchChatSender {
    pub fn send(&self, message: impl Into<String>) {
        let message = message.into();
        if let Some(local_chat) = &self.local_chat {
            local_chat.append_local_entry(
                LocalChatEntryOptions::system(message.clone()).with_ttl(Duration::from_secs(10)),
            );
        }
        let _ = self.sender.send(message);
    }

    pub fn send_local(&self, entry: LocalChatEntryOptions) {
        if let Some(local_chat) = &self.local_chat {
            local_chat.append_local_entry(entry);
        }
    }
}

impl TwitchChatLogin {
    pub fn connect(&self, channel: &str, bot_username: &str, oauth_token: &str) {
        let channel = channel.trim().trim_start_matches('#').to_lowercase();
        if channel.is_empty() || channel == "your_channel_name" {
            return;
        }

        let _ = self.sender.send(TwitchChatLoginRequest {
            channel,
            bot_username: bot_username.trim().to_lowercase(),
            oauth_token: normalize_oauth_token(oauth_token),
        });
    }
}

pub(crate) fn start_twitch_chat_listener(
    mut commands: Commands,
    config: Res<AppConfig>,
    local_chat: Option<Res<LocalChatHub>>,
) {
    let channel = config
        .twitch_channel
        .trim()
        .trim_start_matches('#')
        .to_lowercase();
    let (incoming_sender, incoming_receiver) = crossbeam_channel::unbounded();
    let (outgoing_sender, outgoing_receiver) = crossbeam_channel::unbounded();
    let (login_sender, login_receiver) = crossbeam_channel::unbounded();
    commands.insert_resource(TwitchChatReceiver {
        receiver: incoming_receiver,
    });
    commands.insert_resource(TwitchChatSender {
        sender: outgoing_sender,
        local_chat: local_chat.map(|hub| hub.clone()),
    });
    commands.insert_resource(TwitchChatLogin {
        sender: login_sender.clone(),
    });

    let bot_username = config.chat_bot_username.trim().to_lowercase();
    let oauth_token = normalize_oauth_token(&config.chat_oauth_token);
    thread::spawn(move || {
        let initial_login = if channel.is_empty() || channel == "your_channel_name" {
            eprintln!("Twitch chat listener waiting: set channel in twitch.toml");
            None
        } else {
            Some(TwitchChatLoginRequest {
                channel,
                bot_username,
                oauth_token,
            })
        };

        run_twitch_chat_listener(
            incoming_sender,
            outgoing_receiver,
            login_receiver,
            initial_login,
        )
    });
}

pub(crate) fn poll_twitch_chat(
    receiver: Option<Res<TwitchChatReceiver>>,
    mut messages: MessageWriter<TwitchChatMessage>,
    mut commands: MessageWriter<TwitchChatCommand>,
) {
    let Some(receiver) = receiver else {
        return;
    };

    while let Ok(message) = receiver.receiver.try_recv() {
        if let Some((command, args)) = parse_twitch_command(&message.text) {
            commands.write(TwitchChatCommand {
                user: message.user.clone(),
                display_name: message.display_name.clone(),
                command,
                args,
                roles: message.roles.clone(),
                message_id: message.message_id.clone(),
            });
        }
        messages.write(message);
    }
}

pub(crate) fn poll_local_chat(
    hub: Option<Res<LocalChatHub>>,
    mut messages: MessageWriter<TwitchChatMessage>,
    mut commands: MessageWriter<TwitchChatCommand>,
) {
    let Some(hub) = hub else {
        return;
    };

    for submission in hub.drain() {
        let message = TwitchChatMessage {
            user: submission.user,
            display_name: submission.display_name,
            text: submission.text,
            roles: TwitchChatRoles {
                broadcaster: false,
                moderator: false,
                vip: false,
                subscriber: false,
                staff: false,
            },
            message_id: None,
        };

        if let Some((command, args)) = parse_twitch_command(&message.text) {
            commands.write(TwitchChatCommand {
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

pub(crate) fn dispatch_twitch_chat_commands(
    mut reader: MessageReader<TwitchChatCommand>,
    router: Option<Res<TwitchCommandRouter>>,
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

fn run_twitch_chat_listener(
    incoming: Sender<TwitchChatMessage>,
    outgoing: Receiver<String>,
    login_receiver: Receiver<TwitchChatLoginRequest>,
    initial_login: Option<TwitchChatLoginRequest>,
) {
    let mut login = initial_login;

    loop {
        while let Ok(new_login) = login_receiver.try_recv() {
            login = Some(new_login);
        }

        let Some(active_login) = login.clone().or_else(|| login_receiver.recv().ok()) else {
            return;
        };
        login = Some(active_login.clone());

        match connect_and_read_chat(&active_login, &incoming, &outgoing, &login_receiver) {
            Ok(ChatConnectionOutcome::Disconnected) => {
                eprintln!(
                    "Twitch chat listener disconnected from #{}",
                    active_login.channel
                );
            }
            Ok(ChatConnectionOutcome::Reconnect(new_login)) => {
                login = Some(new_login);
                continue;
            }
            Err(err) => eprintln!(
                "Twitch chat listener error for #{}: {err}",
                active_login.channel
            ),
        }
        std::thread::sleep(Duration::from_secs(5));
    }
}

fn connect_and_read_chat(
    login: &TwitchChatLoginRequest,
    incoming: &Sender<TwitchChatMessage>,
    outgoing: &Receiver<String>,
    login_receiver: &Receiver<TwitchChatLoginRequest>,
) -> Result<ChatConnectionOutcome, String> {
    let mut stream = TcpStream::connect("irc.chat.twitch.tv:6667")
        .map_err(|err| format!("connect failed: {err}"))?;
    let nick = if login.bot_username.is_empty() || login.oauth_token.is_none() {
        anonymous_nick()
    } else {
        login.bot_username.clone()
    };
    let pass = login.oauth_token.as_deref().unwrap_or("SCHMOOPIIE");
    let channel = &login.channel;
    write!(
        stream,
        "CAP REQ :twitch.tv/tags twitch.tv/commands\r\nPASS {pass}\r\nNICK {nick}\r\nJOIN #{channel}\r\n"
    )
    .map_err(|err| format!("IRC handshake failed: {err}"))?;
    stream
        .flush()
        .map_err(|err| format!("IRC flush failed: {err}"))?;

    let reader_stream = stream
        .try_clone()
        .map_err(|err| format!("IRC stream clone failed: {err}"))?;
    reader_stream
        .set_read_timeout(Some(Duration::from_millis(250)))
        .map_err(|err| format!("IRC read timeout setup failed: {err}"))?;
    let mut reader = BufReader::new(reader_stream);
    let can_write = login.oauth_token.is_some();
    let mut line = String::new();

    loop {
        while let Ok(message) = outgoing.try_recv() {
            if !can_write {
                continue;
            }

            write!(
                stream,
                "PRIVMSG #{channel} :{}\r\n",
                sanitize_outgoing_chat(&message)
            )
            .map_err(|err| format!("IRC chat write failed: {err}"))?;
            stream
                .flush()
                .map_err(|err| format!("IRC chat flush failed: {err}"))?;
        }

        if let Ok(new_login) = login_receiver.try_recv() {
            return Ok(ChatConnectionOutcome::Reconnect(new_login));
        }

        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return Ok(ChatConnectionOutcome::Disconnected),
            Ok(_) => {}
            Err(err)
                if err.kind() == ErrorKind::WouldBlock || err.kind() == ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(err) => return Err(format!("IRC read failed: {err}")),
        }

        let line = line.trim_end_matches(['\r', '\n']);
        if line.starts_with("PING ") {
            let payload = line.strip_prefix("PING ").unwrap_or(":tmi.twitch.tv");
            write!(stream, "PONG {payload}\r\n")
                .map_err(|err| format!("IRC PONG failed: {err}"))?;
            stream
                .flush()
                .map_err(|err| format!("IRC PONG flush failed: {err}"))?;
            continue;
        }

        if let Some(message) = parse_privmsg(&line) {
            let _ = incoming.send(message);
        }
    }
}

fn anonymous_nick() -> String {
    let millis = current_time_millis() % 1_000_000;
    format!("justinfan{millis}")
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

fn parse_privmsg(line: &str) -> Option<TwitchChatMessage> {
    let (tags, line) = parse_tags(line);
    let prefix = line.strip_prefix(':')?;
    let (sender, rest) = prefix.split_once(" PRIVMSG ")?;
    let user = sender.split('!').next()?.to_owned();
    let (_, text) = rest.split_once(" :")?;
    let roles = roles_from_tags(&tags);
    let display_name = tag_value(&tags, "display-name")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| user.clone());
    let message_id = tag_value(&tags, "id");
    Some(TwitchChatMessage {
        user,
        display_name,
        text: text.to_owned(),
        roles,
        message_id,
    })
}

fn parse_twitch_command(text: &str) -> Option<(String, String)> {
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

fn parse_tags(line: &str) -> (Vec<(String, String)>, &str) {
    let Some(tag_line) = line.strip_prefix('@') else {
        return (Vec::new(), line);
    };
    let Some((tags, rest)) = tag_line.split_once(' ') else {
        return (Vec::new(), line);
    };
    (
        tags.split(';')
            .filter_map(|tag| {
                let (key, value) = tag.split_once('=')?;
                Some((key.to_owned(), value.to_owned()))
            })
            .collect(),
        rest,
    )
}

fn roles_from_tags(tags: &[(String, String)]) -> TwitchChatRoles {
    let badges = tag_value(tags, "badges").unwrap_or_default();
    let has_badge = |name: &str| badges.split(',').any(|badge| badge.starts_with(name));
    TwitchChatRoles {
        broadcaster: has_badge("broadcaster/"),
        moderator: has_badge("moderator/") || tag_value(tags, "mod").as_deref() == Some("1"),
        vip: has_badge("vip/"),
        subscriber: has_badge("subscriber/")
            || tag_value(tags, "subscriber").as_deref() == Some("1"),
        staff: has_badge("staff/"),
    }
}

fn tag_value(tags: &[(String, String)], key: &str) -> Option<String> {
    tags.iter()
        .find(|(tag_key, _)| tag_key == key)
        .map(|(_, value)| value.clone())
}

fn normalize_oauth_token(token: &str) -> Option<String> {
    let token = token.trim();
    if token.is_empty() || token == "oauth:your_chat_oauth_token_here" {
        None
    } else if token.starts_with("oauth:") {
        Some(token.to_owned())
    } else {
        Some(format!("oauth:{token}"))
    }
}

fn sanitize_outgoing_chat(message: &str) -> String {
    message.replace(['\r', '\n'], " ")
}

fn local_chat_identity_hash(identity: &str) -> String {
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

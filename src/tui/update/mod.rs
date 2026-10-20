//! All state changes go through [`update`], which returns the side effects to
//! run instead of performing them, so it can be tested without Slack.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crossterm::event::KeyEvent;

use crate::config::{AvatarStyle, Choice, Layout, Settings, SidebarState, SidebarTab};
use crate::slack::media::{self, Playable};
use crate::slack::rtm::RtmEvent;
use crate::slack::{
    ChannelSection, Conversation, ConversationCount, FileKind, Message, SearchMatch, User, mrkdwn,
};

use super::compose;
use super::input::Input;
use super::state::{
    App, Channel, Connection, Editing, EmojiPicker, Focus, History, Jump, Mode, Overlay,
    ReactionTarget, SETTINGS_ROWS, Search, SearchStatus, SidebarItem, SwitcherTarget, Thread,
};

// Events are few and short-lived; boxing the large variants would only add noise.
#[allow(clippy::large_enum_variant)]
pub enum Event {
    Key(KeyEvent),
    Paste(String),
    Api(ApiEvent),
    Rtm(RtmEvent),
}

#[allow(clippy::large_enum_variant)]
pub enum ApiEvent {
    Users(Vec<User>),
    Conversations(Vec<Conversation>),
    History {
        channel: String,
        messages: Vec<Message>,
        has_more: bool,
    },
    /// A page of messages older than the ones loaded.
    Older {
        channel: String,
        messages: Vec<Message>,
        has_more: bool,
    },
    Sections(Vec<ChannelSection>),
    SearchResults {
        query: String,
        result: Result<Vec<SearchMatch>, String>,
    },
    CustomEmoji(HashMap<String, String>),
    /// A profile picture (keyed by user id) or a preview (`file:<id>`).
    Picture {
        key: String,
        image: image::DynamicImage,
    },
    /// Something to tell the user in the status bar.
    Notice(String),
    /// A video file is downloaded, at this path.
    PlayReady(String),
    /// Slack refused a reaction that was already shown: undo it.
    ReactionFailed {
        channel: String,
        ts: String,
        name: String,
        added: bool,
        reason: String,
    },
    Replies {
        channel: String,
        ts: String,
        messages: Vec<Message>,
    },
    Sent {
        channel: String,
        message: Message,
    },
    Counts(Vec<ConversationCount>),
    CountsUnavailable,
    /// Every public channel of the workspace, for the switcher.
    PublicChannels(Vec<Conversation>),
    /// The conversations the user muted.
    Muted(HashSet<String>),
    /// One of the user's messages, rewritten.
    Edited {
        channel: String,
        ts: String,
        text: String,
    },
    Deleted {
        channel: String,
        ts: String,
    },
    /// A conversation the user just started or joined, to open.
    Opened(Conversation),
    /// Slack no longer accepts the session.
    SessionExpired,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileToOpen {
    pub id: String,
    pub name: String,
    pub url: String,
    pub video: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    LoadUsers,
    LoadConversations,
    LoadCounts,
    LoadSections,
    LoadHistory(String),
    LoadOlder {
        channel: String,
        before: String,
    },
    LoadReplies {
        channel: String,
        ts: String,
    },
    Send {
        channel: String,
        text: String,
        thread_ts: Option<String>,
    },
    MarkRead {
        channel: String,
        ts: String,
    },
    SaveSettings {
        settings: Settings,
        path: PathBuf,
    },
    SaveSidebar {
        team: String,
        state: SidebarState,
    },
    /// Show the next workspace. Run by the event loop, which holds them all.
    NextWorkspace,
    /// Show this workspace, by its place in `App::workspaces`.
    SwitchWorkspace(usize),
    /// Open this workspace (team id) next time.
    RememberWorkspace(String),
    Search(String),
    LoadEmoji,
    /// Profile pictures to fetch: user id and picture URL.
    LoadAvatars(Vec<(String, String)>),
    /// Previews of files to fetch: `file:<id>` key and URL.
    LoadPreviews(Vec<(String, String)>),
    /// Download the files of a message and open them.
    OpenFiles(Vec<FileToOpen>),
    /// Open a web link in the browser.
    OpenUrl(String),
    /// Download a video file, to play it once ready.
    DownloadToPlay(FileToOpen),
    /// Play a video file or link in the terminal, which the interface hands
    /// over meanwhile. Run by the event loop itself, not the executor.
    Play(String),
    React {
        channel: String,
        ts: String,
        name: String,
        add: bool,
    },
    LoadPublicChannels,
    LoadMuted,
    /// Open the direct conversation with this user, starting it if needed.
    OpenDirect(String),
    /// Join this public channel.
    Join(String),
    Edit {
        channel: String,
        ts: String,
        text: String,
    },
    Delete {
        channel: String,
        ts: String,
    },
}

const PAGE: usize = 10;
/// Pages of older history fetched at most to reach a search result.
const MAX_JUMP_PAGES: u32 = 15;

pub fn initial_commands() -> Vec<Command> {
    vec![
        Command::LoadUsers,
        Command::LoadConversations,
        Command::LoadCounts,
        Command::LoadSections,
        Command::LoadEmoji,
        Command::LoadMuted,
    ]
}

pub fn update(app: &mut App, event: Event) -> Vec<Command> {
    let mut commands = match event {
        Event::Key(key) => keys::on_key(app, key),
        Event::Paste(text) => {
            on_paste(app, &text);
            Vec::new()
        }
        Event::Api(event) => on_api(app, event),
        Event::Rtm(event) => on_rtm(app, event),
    };
    compose::follow_conversation(app);
    compose::refresh(app);
    commands.extend(request_pictures(app));
    commands.extend(request_public_channels(app));
    commands
}

/// The switcher also offers the public channels the user has not joined:
/// they are listed the first time it opens.
fn request_public_channels(app: &mut App) -> Option<Command> {
    let switcher = matches!(app.overlay, Some(Overlay::Switcher { .. }));
    (switcher && !app.public_channels_requested).then(|| {
        app.public_channels_requested = true;
        Command::LoadPublicChannels
    })
}

/// Asks for the pictures on screen not requested yet: the authors' when
/// profile pictures are shown, and file previews when the terminal can
/// draw them.
fn request_pictures(app: &mut App) -> Vec<Command> {
    let thread = app.thread.iter().flat_map(|t| t.messages.iter());
    let shown: Vec<&Message> = app.current_messages().iter().chain(thread).collect();

    let mut avatars = Vec::new();
    if app.settings.avatars == AvatarStyle::Images {
        for user in shown.iter().filter_map(|m| m.user.as_deref()) {
            let url = app
                .users
                .get(user)
                .and_then(|u| u.profile.image_72.as_deref());
            if let Some(url) = url
                && !app.pictures_requested.contains(user)
                && !avatars.iter().any(|(id, _)| id == user)
            {
                avatars.push((user.to_string(), url.to_string()));
            }
        }
    }
    let mut previews = Vec::new();
    if app.graphics {
        for preview in shown.iter().flat_map(|m| media::previews(m)) {
            if !app.pictures_requested.contains(&preview.key)
                && !previews.iter().any(|(key, _)| *key == preview.key)
            {
                previews.push((preview.key, preview.url));
            }
        }
    }

    let requested = avatars.iter().chain(&previews).map(|(key, _)| key.clone());
    app.pictures_requested.extend(requested.collect::<Vec<_>>());
    let mut commands = Vec::new();
    if !avatars.is_empty() {
        commands.push(Command::LoadAvatars(avatars));
    }
    if !previews.is_empty() {
        commands.push(Command::LoadPreviews(previews));
    }
    commands
}

fn on_paste(app: &mut App, text: &str) {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    match &mut app.overlay {
        Some(Overlay::Switcher { query, cursor }) => {
            query.insert_str(&text.replace('\n', " "));
            *cursor = 0;
        }
        Some(Overlay::Search(search)) => search.query.insert_str(&text.replace('\n', " ")),
        Some(Overlay::Emoji(picker)) => picker.query.insert_str(text.trim()),
        Some(Overlay::Settings { .. } | Overlay::Help { .. } | Overlay::DeleteMessage(_)) => {}
        None if app.mode == Mode::Insert => app.input.insert_str(&text),
        None => {}
    }
}

// ---------------------------------------------------------------- API results

fn on_api(app: &mut App, event: ApiEvent) -> Vec<Command> {
    match event {
        ApiEvent::Users(users) => {
            app.users = users.into_iter().map(|u| (u.id.clone(), u)).collect();
            app.sort_channels();
            Vec::new()
        }
        ApiEvent::Conversations(conversations) => {
            let previous: HashMap<String, Channel> =
                app.channels.drain(..).map(|c| (c.id.clone(), c)).collect();
            app.channels = conversations
                .into_iter()
                .map(|conversation| {
                    let mut channel = Channel::from_conversation(conversation);
                    if let Some(old) = previous.get(&channel.id) {
                        channel.unread = old.unread;
                        channel.mentions = old.mentions;
                        channel.listed = old.listed;
                        channel.latest = old.latest.clone();
                    }
                    if app.pending_unread.remove(&channel.id) {
                        channel.unread = true;
                        channel.mentions += u32::from(channel.is_direct());
                    }
                    channel
                })
                .collect();
            app.channels_loaded = true;
            app.sort_channels();
            app.apply_muted();
            app.apply_counts();
            match &app.current {
                Some(_) => Vec::new(),
                None => match default_channel(app) {
                    Some(id) => open_channel(app, &id),
                    None => Vec::new(),
                },
            }
        }
        ApiEvent::History {
            channel,
            messages,
            has_more,
        } => {
            let history = app.histories.entry(channel.clone()).or_default();
            // Keep messages received in real time while the history was loading.
            let live = std::mem::take(&mut history.messages);
            history.messages = messages;
            for message in live {
                insert_message(&mut history.messages, message);
            }
            history.loaded = true;
            history.has_more = has_more;
            let mut commands = try_jump(app);
            if app.visible && app.current.as_deref() == Some(&channel) {
                commands.extend(mark_read(app, &channel));
            }
            commands
        }
        ApiEvent::Older {
            channel,
            messages,
            has_more,
        } => {
            let selected_ts = (app.current.as_deref() == Some(&channel))
                .then(|| app.selected.map(|i| app.current_messages()[i].ts.clone()))
                .flatten();
            let history = app.histories.entry(channel.clone()).or_default();
            for message in messages {
                insert_message(&mut history.messages, message);
            }
            history.has_more = has_more;
            history.loading_older = false;
            // Older messages shift the indices: keep the same message selected.
            if let Some(ts) = selected_ts {
                app.selected = history.messages.iter().position(|m| m.ts == ts);
            }
            try_jump(app)
        }
        ApiEvent::Sections(sections) => {
            app.set_sections(sections);
            Vec::new()
        }
        ApiEvent::SearchResults { query, result } => {
            if let Some(Overlay::Search(search)) = &mut app.overlay
                && search.submitted == query
            {
                search.status = match result {
                    Ok(matches) => SearchStatus::Done(matches),
                    Err(reason) => SearchStatus::Failed(reason),
                };
                search.cursor = 0;
            }
            Vec::new()
        }
        ApiEvent::Replies {
            channel,
            ts,
            messages,
        } => {
            if let Some(thread) = &mut app.thread
                && thread.channel == channel
                && thread.ts == ts
            {
                thread.messages = messages;
                thread.loaded = true;
            }
            Vec::new()
        }
        ApiEvent::Sent { channel, message } => {
            on_rtm(app, RtmEvent::Message { channel, message });
            Vec::new()
        }
        ApiEvent::Counts(counts) => {
            app.counts = Some(counts.into_iter().map(|c| (c.id.clone(), c)).collect());
            app.apply_counts();
            Vec::new()
        }
        ApiEvent::CountsUnavailable => {
            app.counts_unavailable = true;
            app.apply_counts();
            Vec::new()
        }
        ApiEvent::PublicChannels(channels) => {
            app.public_channels = channels;
            Vec::new()
        }
        ApiEvent::Muted(muted) => {
            app.muted = muted;
            app.apply_muted();
            Vec::new()
        }
        ApiEvent::Edited { channel, ts, text } => {
            let me = app.my_id.clone();
            for messages in messages_of(app, &channel) {
                if let Some(message) = messages.iter_mut().find(|m| m.ts == ts) {
                    message.text = text.clone();
                    message.edited = Some(serde_json::json!({ "user": me }));
                }
            }
            Vec::new()
        }
        ApiEvent::Deleted { channel, ts } => on_rtm(app, RtmEvent::MessageDeleted { channel, ts }),
        ApiEvent::Opened(conversation) => {
            let id = conversation.id.clone();
            if app.channel(&id).is_none() {
                app.channels.push(Channel::from_conversation(conversation));
                app.sort_channels();
            }
            app.focus = Focus::Messages;
            open_channel(app, &id)
        }
        ApiEvent::Picture { key, image } => {
            app.pictures.insert(key, image);
            Vec::new()
        }
        ApiEvent::Notice(notice) => {
            app.notice = Some(notice);
            Vec::new()
        }
        ApiEvent::PlayReady(path) => vec![Command::Play(path)],
        ApiEvent::CustomEmoji(emoji) => {
            app.custom_emoji = emoji.into_keys().collect();
            app.custom_emoji.sort();
            Vec::new()
        }
        ApiEvent::ReactionFailed {
            channel,
            ts,
            name,
            added,
            reason,
        } => {
            let me = app.my_id.clone();
            for messages in messages_of(app, &channel) {
                if let Some(message) = messages.iter_mut().find(|m| m.ts == ts) {
                    if added {
                        message.remove_reaction(&name, &me);
                    } else {
                        message.add_reaction(&name, &me);
                    }
                }
            }
            app.notice = Some(format!("could not react with :{name}: ({reason})"));
            Vec::new()
        }
        ApiEvent::SessionExpired => {
            session_expired(app);
            Vec::new()
        }
        ApiEvent::Failed(reason) => {
            app.notice = Some(reason);
            Vec::new()
        }
    }
}

/// Every request fails from now on: the status bar says how to fix it, and
/// a search waiting for results stops waiting.
fn session_expired(app: &mut App) {
    app.connection = Connection::SessionExpired;
    if let Some(Overlay::Search(search)) = &mut app.overlay
        && matches!(search.status, SearchStatus::Loading)
    {
        search.status = SearchStatus::Failed("session expired".into());
    }
}

fn default_channel(app: &App) -> Option<String> {
    app.channels
        .iter()
        .find(|c| c.name == "general" || c.name == "général")
        .or_else(|| app.channels.first())
        .map(|c| c.id.clone())
}

// ---------------------------------------------------------- real-time events

fn on_rtm(app: &mut App, event: RtmEvent) -> Vec<Command> {
    match event {
        RtmEvent::Connected => app.connection = Connection::Connected,
        RtmEvent::SessionExpired => session_expired(app),
        RtmEvent::Reconnecting { in_secs, reason } => {
            app.connection = Connection::Reconnecting { in_secs, reason }
        }
        RtmEvent::Message { channel, message } => return on_message(app, channel, message),
        RtmEvent::MessageChanged { channel, message } => {
            for messages in messages_of(app, &channel) {
                if let Some(existing) = messages.iter_mut().find(|m| m.ts == message.ts) {
                    *existing = message.clone();
                }
            }
        }
        RtmEvent::MessageDeleted { channel, ts } => {
            for messages in messages_of(app, &channel) {
                messages.retain(|m| m.ts != ts);
            }
            let len = app.current_messages().len();
            app.selected = app.selected.filter(|&i| i < len);
        }
        RtmEvent::ReactionAdded {
            channel,
            ts,
            name,
            user,
        } => {
            for messages in messages_of(app, &channel) {
                if let Some(message) = messages.iter_mut().find(|m| m.ts == ts) {
                    message.add_reaction(&name, &user);
                }
            }
        }
        RtmEvent::ReactionRemoved {
            channel,
            ts,
            name,
            user,
        } => {
            for messages in messages_of(app, &channel) {
                if let Some(message) = messages.iter_mut().find(|m| m.ts == ts) {
                    message.remove_reaction(&name, &user);
                }
            }
        }
    }
    Vec::new()
}

fn on_message(app: &mut App, channel: String, message: Message) -> Vec<Command> {
    if message.is_thread_reply() {
        if let Some(thread) = &mut app.thread
            && thread.channel == channel
            && message.thread_ts.as_deref() == Some(&thread.ts)
        {
            insert_message(&mut thread.messages, message);
        }
        return Vec::new();
    }

    let mine = message.user.as_deref() == Some(app.my_id.as_str());
    let mentions_me = !mine && mrkdwn::mentions(&message.text, &app.my_id);
    if let Some(known) = app.channel_mut(&channel) {
        known.listed = true;
        if message.ts > known.latest {
            known.latest = message.ts.clone();
        }
        if known.is_direct() {
            app.sort_channels();
        }
    }
    // Keep it even when the history is not loaded yet: it is merged on load.
    insert_message(
        &mut app.histories.entry(channel.clone()).or_default().messages,
        message,
    );

    if app.visible && app.current.as_deref() == Some(&channel) {
        return mark_read(app, &channel).into_iter().collect();
    }
    if mine {
        return Vec::new();
    }
    match app.channel_mut(&channel) {
        Some(known) => {
            let notifies = mentions_me || known.is_direct();
            if notifies {
                known.mentions += 1;
            }
            known.unread |= notifies || !known.muted;
            Vec::new()
        }
        None => {
            app.pending_unread.insert(channel);
            vec![Command::LoadConversations]
        }
    }
}

/// The message lists of a channel that are in memory: its history and the
/// open thread if it belongs to that channel.
fn messages_of<'a>(app: &'a mut App, channel: &str) -> Vec<&'a mut Vec<Message>> {
    let mut lists = Vec::new();
    if let Some(history) = app.histories.get_mut(channel) {
        lists.push(&mut history.messages);
    }
    if let Some(thread) = &mut app.thread
        && thread.channel == channel
    {
        lists.push(&mut thread.messages);
    }
    lists
}

/// Inserts in timestamp order, replacing a message already present.
fn insert_message(messages: &mut Vec<Message>, message: Message) {
    // Slack timestamps have a fixed width, so they sort as strings.
    match messages.binary_search_by(|m| m.ts.cmp(&message.ts)) {
        Ok(index) => messages[index] = message,
        Err(index) => messages.insert(index, message),
    }
}

/// The workspace comes on screen: what arrived meanwhile in the open
/// conversation is now read.
pub fn on_shown(app: &mut App) -> Vec<Command> {
    app.visible = true;
    let Some(channel) = app.current.clone() else {
        return Vec::new();
    };
    if let Some(open) = app.channel_mut(&channel) {
        open.unread = false;
        open.mentions = 0;
    }
    mark_read(app, &channel).into_iter().collect()
}

fn mark_read(app: &App, channel: &str) -> Option<Command> {
    let history = app.histories.get(channel)?;
    let last = history.messages.last()?;
    history.loaded.then(|| Command::MarkRead {
        channel: channel.to_string(),
        ts: last.ts.clone(),
    })
}

fn open_channel(app: &mut App, id: &str) -> Vec<Command> {
    stop_editing(app);
    app.current = Some(id.to_string());
    app.selected = None;
    app.thread = None;
    if app.focus == Focus::Thread {
        app.focus = Focus::Messages;
    }
    app.sidebar_cursor = Some(SidebarItem::Channel(id.to_string()));
    if let Some(index) = app.channels.iter().position(|c| c.id == id) {
        let channel = &mut app.channels[index];
        channel.unread = false;
        channel.mentions = 0;
        channel.listed = true;
    }
    match app.histories.get(id) {
        Some(History { loaded: true, .. }) => mark_read(app, id).into_iter().collect(),
        _ => vec![Command::LoadHistory(id.to_string())],
    }
}

/// Puts back what the message box held before an edit started.
fn stop_editing(app: &mut App) {
    if let Some(editing) = app.editing.take() {
        app.input = editing.draft;
    }
}

/// Fetches the page of history before the oldest loaded message, if any.
fn load_older(app: &mut App) -> Vec<Command> {
    let Some(channel) = app.current.clone() else {
        return Vec::new();
    };
    let Some(history) = app.histories.get_mut(&channel) else {
        return Vec::new();
    };
    let Some(oldest) = history.messages.first() else {
        return Vec::new();
    };
    if !history.loaded || !history.has_more || history.loading_older {
        return Vec::new();
    }
    history.loading_older = true;
    vec![Command::LoadOlder {
        before: oldest.ts.clone(),
        channel,
    }]
}

/// Moves the selection to the message a search result pointed at, loading
/// older history until it is reached.
fn try_jump(app: &mut App) -> Vec<Command> {
    let Some(jump) = app.jump.clone() else {
        return Vec::new();
    };
    if app.current.as_deref() != Some(&jump.channel) {
        app.jump = None;
        return Vec::new();
    }
    let Some(history) = app.histories.get(&jump.channel).filter(|h| h.loaded) else {
        return Vec::new();
    };
    if let Some(index) = history.messages.iter().position(|m| m.ts == jump.ts) {
        app.selected = Some(index);
        app.jump = None;
        return Vec::new();
    }
    let older = history.messages.first().is_some_and(|m| jump.ts < m.ts);
    if older && history.has_more && jump.pages < MAX_JUMP_PAGES {
        let commands = load_older(app);
        if !commands.is_empty() {
            app.jump = Some(Jump {
                pages: jump.pages + 1,
                ..jump
            });
        }
        return commands;
    }
    // Out of reach: select the closest message instead.
    let closest = history.messages.iter().position(|m| m.ts >= jump.ts);
    app.selected = closest.or(history.messages.len().checked_sub(1));
    app.jump = None;
    if older {
        app.notice = Some("message too old, showing the closest one".into());
    }
    Vec::new()
}

/// Adds the reaction, or removes it if the user already reacted with it,
/// showing the change right away.
fn toggle_reaction(app: &mut App, target: &ReactionTarget, name: &str) -> Vec<Command> {
    let me = app.my_id.clone();
    let add = !app
        .find_message(&target.channel, &target.ts)
        .and_then(|m| m.reactions.iter().find(|r| r.name == name))
        .is_some_and(|r| r.users.contains(&me));
    for messages in messages_of(app, &target.channel) {
        if let Some(message) = messages.iter_mut().find(|m| m.ts == target.ts) {
            if add {
                message.add_reaction(name, &me);
            } else {
                message.remove_reaction(name, &me);
            }
        }
    }
    vec![Command::React {
        channel: target.channel.clone(),
        ts: target.ts.clone(),
        name: name.to_string(),
        add,
    }]
}

/// `o`: the files of the message, downloaded and opened, or else its first
/// link, opened in the browser.
fn open_files(app: &mut App) -> Vec<Command> {
    let Some(message) = target_message(app) else {
        return Vec::new();
    };
    let files: Vec<FileToOpen> = message.files.iter().filter_map(file_to_open).collect();
    if !files.is_empty() {
        return vec![Command::OpenFiles(files)];
    }
    if let Some(link) = mrkdwn::links(&message.text).into_iter().next() {
        return vec![Command::OpenUrl(link)];
    }
    app.notice = Some("nothing to open in this message".into());
    Vec::new()
}

/// `p`: the message's video, played in the terminal; a file is downloaded
/// first.
fn play(app: &mut App) -> Vec<Command> {
    let Some(message) = target_message(app) else {
        return Vec::new();
    };
    match media::playable(&message) {
        Some(Playable::File(index)) => match file_to_open(&message.files[index]) {
            Some(file) => vec![Command::DownloadToPlay(file)],
            None => {
                app.notice = Some("this video can't be downloaded".into());
                Vec::new()
            }
        },
        Some(Playable::Link(url)) => vec![Command::Play(url)],
        None => {
            app.notice = Some("no video in this message".into());
            Vec::new()
        }
    }
}

/// The message `r`, `o` and `p` act on.
fn target_message(app: &App) -> Option<Message> {
    let target = app.reaction_target()?;
    app.find_message(&target.channel, &target.ts).cloned()
}

fn file_to_open(file: &crate::slack::File) -> Option<FileToOpen> {
    Some(FileToOpen {
        id: file.id.clone(),
        name: file.name.clone(),
        url: file.download_url()?.to_string(),
        video: file.kind() == FileKind::Video,
    })
}

mod keys;
mod overlays;
#[cfg(test)]
mod tests;

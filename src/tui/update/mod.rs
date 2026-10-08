//! All state changes go through [`update`], which returns the side effects to
//! run instead of performing them, so it can be tested without Slack.

use std::collections::HashMap;

use crossterm::event::KeyEvent;

use crate::config::{Choice, Layout, Settings, SidebarState, SidebarTab};
use crate::slack::rtm::RtmEvent;
use crate::slack::{
    ChannelSection, Conversation, ConversationCount, Message, SearchMatch, User, mrkdwn,
};

use super::input::Input;
use super::state::{
    App, Channel, Connection, EmojiPicker, Focus, History, Jump, Mode, Overlay, ReactionTarget,
    SETTINGS_ROWS, Search, SearchStatus, SidebarItem, Thread,
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
    Failed(String),
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
    SaveSettings(Settings),
    SaveSidebar(SidebarState),
    Search(String),
    LoadEmoji,
    React {
        channel: String,
        ts: String,
        name: String,
        add: bool,
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
    ]
}

pub fn update(app: &mut App, event: Event) -> Vec<Command> {
    match event {
        Event::Key(key) => keys::on_key(app, key),
        Event::Paste(text) => {
            on_paste(app, &text);
            Vec::new()
        }
        Event::Api(event) => on_api(app, event),
        Event::Rtm(event) => on_rtm(app, event),
    }
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
        Some(Overlay::Settings { .. }) => {}
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
            if app.current.as_deref() == Some(&channel) {
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
            app.notice = Some(format!("réaction :{name}: : {reason}"));
            Vec::new()
        }
        ApiEvent::Failed(reason) => {
            app.notice = Some(reason);
            Vec::new()
        }
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

    if app.current.as_deref() == Some(&channel) {
        return mark_read(app, &channel).into_iter().collect();
    }
    if mine {
        return Vec::new();
    }
    match app.channel_mut(&channel) {
        Some(known) => {
            known.unread = true;
            if mentions_me || known.is_direct() {
                known.mentions += 1;
            }
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

fn mark_read(app: &App, channel: &str) -> Option<Command> {
    let history = app.histories.get(channel)?;
    let last = history.messages.last()?;
    history.loaded.then(|| Command::MarkRead {
        channel: channel.to_string(),
        ts: last.ts.clone(),
    })
}

fn open_channel(app: &mut App, id: &str) -> Vec<Command> {
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
        app.notice = Some("message trop ancien, affichage du plus proche".into());
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

mod keys;
mod overlays;
#[cfg(test)]
mod tests;

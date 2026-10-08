//! All state changes go through [`update`], which returns the side effects to
//! run instead of performing them, so it can be tested without Slack.

use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{Choice, Layout, Settings};
use crate::slack::rtm::RtmEvent;
use crate::slack::{Conversation, ConversationCount, Message, User, mrkdwn};

use super::input::Input;
use super::state::{
    App, Channel, Connection, Focus, History, Mode, Overlay, SETTINGS_ROWS, Thread,
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
    LoadHistory(String),
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
}

const PAGE: usize = 10;

pub fn initial_commands() -> Vec<Command> {
    vec![
        Command::LoadUsers,
        Command::LoadConversations,
        Command::LoadCounts,
    ]
}

pub fn update(app: &mut App, event: Event) -> Vec<Command> {
    match event {
        Event::Key(key) => on_key(app, key),
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
        ApiEvent::History { channel, messages } => {
            let history = app.histories.entry(channel.clone()).or_default();
            // Keep messages received in real time while the history was loading.
            let live = std::mem::take(&mut history.messages);
            history.messages = messages;
            for message in live {
                insert_message(&mut history.messages, message);
            }
            history.loaded = true;
            if app.current.as_deref() == Some(&channel) {
                mark_read(app, &channel).into_iter().collect()
            } else {
                Vec::new()
            }
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
    if let Some(index) = app.channels.iter().position(|c| c.id == id) {
        app.sidebar_cursor = index;
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

// --------------------------------------------------------------------- keys

fn on_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return Vec::new();
    }
    app.notice = None;

    if app.overlay.is_some() {
        return on_overlay_key(app, key);
    }
    if ctrl && key.code == KeyCode::Char('k') {
        open_switcher(app);
        return Vec::new();
    }
    match app.mode {
        Mode::Insert => on_insert_key(app, key),
        Mode::Normal => on_normal_key(app, key),
    }
}

fn on_normal_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char(',') => app.overlay = Some(Overlay::Settings { row: 0 }),
        KeyCode::Tab => cycle_focus(app, 1),
        KeyCode::BackTab => cycle_focus(app, -1),
        KeyCode::Char('1') => focus_sidebar(app),
        KeyCode::Char('2') => app.focus = Focus::Messages,
        KeyCode::Char('3') if app.thread.is_some() => app.focus = Focus::Thread,
        KeyCode::Char('i') | KeyCode::Char('a') if app.current.is_some() => {
            if app.focus == Focus::Sidebar {
                app.focus = Focus::Messages;
            }
            app.mode = Mode::Insert;
        }
        KeyCode::Esc if app.thread.is_some() => close_thread(app),
        KeyCode::Esc => app.selected = None,
        _ => {
            return match app.focus {
                Focus::Sidebar => on_sidebar_key(app, key),
                Focus::Messages => on_messages_key(app, key),
                Focus::Thread => on_thread_key(app, key),
            };
        }
    }
    Vec::new()
}

fn on_sidebar_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    // The cursor only visits channels shown in the sidebar.
    let listed = app.listed_indices();
    let position = listed
        .iter()
        .position(|&i| i == app.sidebar_cursor)
        .unwrap_or(0);
    let move_to = |position: usize| listed.get(position).copied();
    let target = match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            move_to((position + 1).min(listed.len().saturating_sub(1)))
        }
        KeyCode::Char('k') | KeyCode::Up => move_to(position.saturating_sub(1)),
        KeyCode::Char('g') | KeyCode::Home => move_to(0),
        KeyCode::Char('G') | KeyCode::End => move_to(listed.len().saturating_sub(1)),
        _ => None,
    };
    if let Some(index) = target {
        app.sidebar_cursor = index;
        return Vec::new();
    }
    match key.code {
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
            if let Some(id) = app.channels.get(app.sidebar_cursor).map(|c| c.id.clone()) {
                app.focus = Focus::Messages;
                return open_channel(app, &id);
            }
        }
        _ => {}
    }
    Vec::new()
}

fn on_messages_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    let len = app.current_messages().len();
    match key.code {
        KeyCode::Char('k') | KeyCode::Up => select_older(app, 1, len),
        KeyCode::Char('j') | KeyCode::Down => select_newer(app, 1, len),
        KeyCode::PageUp => select_older(app, PAGE, len),
        KeyCode::PageDown => select_newer(app, PAGE, len),
        KeyCode::Char('g') | KeyCode::Home if len > 0 => app.selected = Some(0),
        KeyCode::Char('G') | KeyCode::End => app.selected = None,
        KeyCode::Char('t') => return open_thread(app),
        KeyCode::Enter => app.mode = Mode::Insert,
        KeyCode::Char('h') | KeyCode::Left => focus_sidebar(app),
        _ => {}
    }
    Vec::new()
}

fn on_thread_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    match key.code {
        KeyCode::Enter => app.mode = Mode::Insert,
        KeyCode::Char('h') | KeyCode::Left => app.focus = Focus::Messages,
        _ => {}
    }
    Vec::new()
}

fn select_older(app: &mut App, step: usize, len: usize) {
    if len == 0 {
        return;
    }
    app.selected = Some(match app.selected {
        None => len.saturating_sub(step),
        Some(i) => i.saturating_sub(step),
    });
}

fn select_newer(app: &mut App, step: usize, len: usize) {
    app.selected = match app.selected {
        Some(i) if i + step < len => Some(i + step),
        _ => None,
    };
}

fn open_thread(app: &mut App) -> Vec<Command> {
    let messages = app.current_messages();
    let Some(message) = app
        .selected
        .map_or(messages.last(), |i| messages.get(i))
        .cloned()
    else {
        return Vec::new();
    };
    let Some(channel) = app.current.clone() else {
        return Vec::new();
    };
    let ts = message
        .thread_ts
        .clone()
        .unwrap_or_else(|| message.ts.clone());
    let has_replies = message.has_thread();
    app.thread = Some(Thread {
        channel: channel.clone(),
        ts: ts.clone(),
        messages: vec![message],
        loaded: !has_replies,
    });
    app.focus = Focus::Thread;
    if has_replies {
        vec![Command::LoadReplies { channel, ts }]
    } else {
        Vec::new()
    }
}

fn close_thread(app: &mut App) {
    app.thread = None;
    if app.focus == Focus::Thread {
        app.focus = Focus::Messages;
    }
}

fn focus_sidebar(app: &mut App) {
    if app.settings.layout == Layout::Focus {
        open_switcher(app);
    } else {
        app.focus = Focus::Sidebar;
    }
}

fn cycle_focus(app: &mut App, step: isize) {
    let mut order = Vec::new();
    if app.settings.layout != Layout::Focus {
        order.push(Focus::Sidebar);
    }
    order.push(Focus::Messages);
    if app.thread.is_some() {
        order.push(Focus::Thread);
    }
    let index = order.iter().position(|f| *f == app.focus).unwrap_or(0) as isize;
    app.focus = order[(index + step).rem_euclid(order.len() as isize) as usize];
}

fn open_switcher(app: &mut App) {
    app.mode = Mode::Normal;
    app.overlay = Some(Overlay::Switcher {
        query: Input::default(),
        cursor: 0,
    });
}

fn on_insert_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let newline = key
        .modifiers
        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT);
    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Enter if newline => app.input.insert('\n'),
        KeyCode::Char('j') if ctrl => app.input.insert('\n'),
        KeyCode::Enter => return send(app),
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::ALT) => app.input.delete_word(),
        KeyCode::Backspace => app.input.backspace(),
        KeyCode::Delete => app.input.delete(),
        KeyCode::Left => app.input.left(),
        KeyCode::Right => app.input.right(),
        KeyCode::Home => app.input.home(),
        KeyCode::End => app.input.end(),
        KeyCode::Char('a') if ctrl => app.input.home(),
        KeyCode::Char('e') if ctrl => app.input.end(),
        KeyCode::Char('u') if ctrl => app.input.clear(),
        KeyCode::Char('w') if ctrl => app.input.delete_word(),
        KeyCode::Char(c) if !ctrl => app.input.insert(c),
        _ => {}
    }
    Vec::new()
}

fn send(app: &mut App) -> Vec<Command> {
    if app.input.text().trim().is_empty() {
        return Vec::new();
    }
    let Some((channel, thread_ts)) = app.compose_target() else {
        return Vec::new();
    };
    let text = escape(app.input.take().trim());
    vec![Command::Send {
        channel,
        text,
        thread_ts,
    }]
}

/// Slack reads `<`, `>` and `&` as markup, so typed text must escape them.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ------------------------------------------------------------------ overlays

fn on_overlay_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    match app.overlay {
        Some(Overlay::Switcher { .. }) => on_switcher_key(app, key),
        Some(Overlay::Settings { .. }) => on_settings_key(app, key),
        None => Vec::new(),
    }
}

fn on_switcher_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let Some(Overlay::Switcher { query, cursor }) = &mut app.overlay else {
        return Vec::new();
    };
    match key.code {
        KeyCode::Esc => app.overlay = None,
        KeyCode::Up => *cursor = cursor.saturating_sub(1),
        KeyCode::Char('p') if ctrl => *cursor = cursor.saturating_sub(1),
        KeyCode::Down | KeyCode::Tab => *cursor += 1,
        KeyCode::Char('n') if ctrl => *cursor += 1,
        KeyCode::Backspace => {
            query.backspace();
            *cursor = 0;
        }
        KeyCode::Char(c) if !ctrl => {
            query.insert(c);
            *cursor = 0;
        }
        KeyCode::Enter => {
            let (query, cursor) = (query.text().to_string(), *cursor);
            let results = app.switcher_results(&query);
            let chosen = results
                .get(cursor.min(results.len().saturating_sub(1)))
                .map(|r| app.channels[r.channel].id.clone());
            app.overlay = None;
            if let Some(id) = chosen {
                app.focus = Focus::Messages;
                return open_channel(app, &id);
            }
        }
        _ => {}
    }
    // Keep the cursor on an existing result.
    if let Some(Overlay::Switcher { query, cursor }) = &app.overlay {
        let count = app.switcher_results(query.text()).len();
        let clamped = (*cursor).min(count.saturating_sub(1));
        if let Some(Overlay::Switcher { cursor, .. }) = &mut app.overlay {
            *cursor = clamped;
        }
    }
    Vec::new()
}

fn on_settings_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    let Some(Overlay::Settings { row }) = &mut app.overlay else {
        return Vec::new();
    };
    let step = match key.code {
        KeyCode::Esc | KeyCode::Char(',') | KeyCode::Char('q') => {
            app.overlay = None;
            return vec![Command::SaveSettings(app.settings)];
        }
        KeyCode::Char('j') | KeyCode::Down => {
            *row = (*row + 1).min(SETTINGS_ROWS - 1);
            return Vec::new();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            *row = row.saturating_sub(1);
            return Vec::new();
        }
        KeyCode::Char('h') | KeyCode::Left => -1,
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => 1,
        _ => return Vec::new(),
    };
    let settings = &mut app.settings;
    match *row {
        0 => settings.layout = settings.layout.cycle(step),
        1 => settings.theme = settings.theme.cycle(step),
        _ => settings.accent = settings.accent.cycle(step),
    }
    if app.settings.layout == Layout::Focus && app.focus == Focus::Sidebar {
        app.focus = Focus::Messages;
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slack::test_message as message;
    use crate::tui::state::fixtures::*;

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
    }

    fn typed(app: &mut App, text: &str) {
        for c in text.chars() {
            update(app, key(KeyCode::Char(c)));
        }
    }

    /// An app showing #general with two messages, and #random unread-free.
    fn loaded_app() -> App {
        let mut app = app();
        update(
            &mut app,
            Event::Api(ApiEvent::Conversations(vec![
                conversation("C1", "general"),
                conversation("C2", "random"),
                direct("D1", "U2"),
            ])),
        );
        update(
            &mut app,
            Event::Api(ApiEvent::History {
                channel: "C1".into(),
                messages: vec![
                    message("1.0", "U2", "salut"),
                    message("2.0", "U2", "ça va ?"),
                ],
            }),
        );
        app
    }

    #[test]
    fn opens_the_general_channel_once_conversations_load() {
        let mut app = app();
        let commands = update(
            &mut app,
            Event::Api(ApiEvent::Conversations(vec![
                conversation("C2", "random"),
                conversation("C1", "general"),
            ])),
        );
        assert_eq!(app.current.as_deref(), Some("C1"));
        assert_eq!(commands, [Command::LoadHistory("C1".into())]);
    }

    #[test]
    fn marks_the_channel_read_when_its_history_arrives() {
        let mut app = app();
        update(
            &mut app,
            Event::Api(ApiEvent::Conversations(vec![conversation("C1", "general")])),
        );
        let commands = update(
            &mut app,
            Event::Api(ApiEvent::History {
                channel: "C1".into(),
                messages: vec![message("1.0", "U2", "salut")],
            }),
        );
        assert_eq!(
            commands,
            [Command::MarkRead {
                channel: "C1".into(),
                ts: "1.0".into()
            }]
        );
    }

    #[test]
    fn new_messages_elsewhere_mark_channels_unread() {
        let mut app = loaded_app();
        update(
            &mut app,
            Event::Rtm(RtmEvent::Message {
                channel: "C2".into(),
                message: message("3.0", "U2", "hey <@UME>"),
            }),
        );
        let random = app.channel("C2").unwrap();
        assert!(random.unread);
        assert_eq!(random.mentions, 1);
    }

    #[test]
    fn own_messages_do_not_mark_channels_unread() {
        let mut app = loaded_app();
        update(
            &mut app,
            Event::Rtm(RtmEvent::Message {
                channel: "C2".into(),
                message: message("3.0", "UME", "from my phone"),
            }),
        );
        assert!(!app.channel("C2").unwrap().unread);
    }

    #[test]
    fn messages_in_the_current_channel_are_appended_once() {
        let mut app = loaded_app();
        let event = || {
            Event::Rtm(RtmEvent::Message {
                channel: "C1".into(),
                message: message("3.0", "U2", "nouveau"),
            })
        };
        let commands = update(&mut app, event());
        update(&mut app, event());

        assert_eq!(app.current_messages().len(), 3);
        assert_eq!(
            commands,
            [Command::MarkRead {
                channel: "C1".into(),
                ts: "3.0".into()
            }]
        );
    }

    #[test]
    fn messages_from_unknown_channels_reload_the_list() {
        let mut app = loaded_app();
        let commands = update(
            &mut app,
            Event::Rtm(RtmEvent::Message {
                channel: "D9".into(),
                message: message("3.0", "U3", "salut"),
            }),
        );
        assert_eq!(commands, [Command::LoadConversations]);

        update(
            &mut app,
            Event::Api(ApiEvent::Conversations(vec![
                conversation("C1", "general"),
                direct("D9", "U3"),
            ])),
        );
        assert!(app.channel("D9").unwrap().unread);
    }

    #[test]
    fn edits_and_deletions_apply_to_the_history() {
        let mut app = loaded_app();
        let mut edited = message("1.0", "U2", "salut à tous");
        edited.edited = Some(serde_json::json!({}));
        update(
            &mut app,
            Event::Rtm(RtmEvent::MessageChanged {
                channel: "C1".into(),
                message: edited,
            }),
        );
        assert_eq!(app.current_messages()[0].text, "salut à tous");

        update(
            &mut app,
            Event::Rtm(RtmEvent::MessageDeleted {
                channel: "C1".into(),
                ts: "1.0".into(),
            }),
        );
        assert_eq!(app.current_messages().len(), 1);
    }

    #[test]
    fn typing_and_enter_sends_an_escaped_message() {
        let mut app = loaded_app();
        update(&mut app, key(KeyCode::Char('i')));
        typed(&mut app, "a < b & c");
        let commands = update(&mut app, key(KeyCode::Enter));

        assert_eq!(
            commands,
            [Command::Send {
                channel: "C1".into(),
                text: "a &lt; b &amp; c".into(),
                thread_ts: None
            }]
        );
        assert!(app.input.is_empty());
        assert_eq!(app.mode, Mode::Insert);
    }

    #[test]
    fn replies_go_to_the_open_thread() {
        let mut app = loaded_app();
        app.focus = Focus::Messages;
        update(&mut app, key(KeyCode::Char('t')));
        assert_eq!(app.focus, Focus::Thread);

        update(&mut app, key(KeyCode::Char('i')));
        typed(&mut app, "ok");
        let commands = update(&mut app, key(KeyCode::Enter));

        assert_eq!(
            commands,
            [Command::Send {
                channel: "C1".into(),
                text: "ok".into(),
                thread_ts: Some("2.0".into())
            }]
        );
    }

    #[test]
    fn thread_replies_stay_out_of_the_channel() {
        let mut app = loaded_app();
        let mut reply = message("3.0", "U2", "réponse");
        reply.thread_ts = Some("1.0".into());
        update(
            &mut app,
            Event::Rtm(RtmEvent::Message {
                channel: "C1".into(),
                message: reply,
            }),
        );
        assert_eq!(app.current_messages().len(), 2);
    }

    #[test]
    fn selection_moves_through_messages_and_back_to_live() {
        let mut app = loaded_app();
        app.focus = Focus::Messages;
        update(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.selected, Some(1));
        update(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.selected, Some(0));
        update(&mut app, key(KeyCode::Char('j')));
        update(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.selected, None);
    }

    #[test]
    fn switcher_opens_the_chosen_channel() {
        let mut app = loaded_app();
        update(&mut app, ctrl('k'));
        typed(&mut app, "rand");
        let commands = update(&mut app, key(KeyCode::Enter));

        assert!(app.overlay.is_none());
        assert_eq!(app.current.as_deref(), Some("C2"));
        assert_eq!(commands, [Command::LoadHistory("C2".into())]);
    }

    #[test]
    fn settings_change_live_and_save_on_close() {
        let mut app = loaded_app();
        update(&mut app, key(KeyCode::Char(',')));
        update(&mut app, key(KeyCode::Char('l')));
        assert_eq!(app.settings.layout, Layout::Stream);

        update(&mut app, key(KeyCode::Char('j')));
        update(&mut app, key(KeyCode::Char('l')));
        let commands = update(&mut app, key(KeyCode::Esc));

        assert!(app.overlay.is_none());
        assert_eq!(commands, [Command::SaveSettings(app.settings)]);
        assert_ne!(app.settings.theme, Settings::default().theme);
    }

    fn count(id: &str, unread: bool, latest: &str) -> ConversationCount {
        ConversationCount {
            id: id.into(),
            has_unreads: unread,
            mention_count: 0,
            latest: Some(latest.into()),
        }
    }

    fn group(id: &str) -> Conversation {
        serde_json::from_value(
            serde_json::json!({"id": id, "is_mpim": true, "name": "mpdm-a--b-1"}),
        )
        .unwrap()
    }

    fn conversations() -> Event {
        Event::Api(ApiEvent::Conversations(vec![
            conversation("C1", "general"),
            conversation("C2", "random"),
            direct("D1", "U2"),
            direct("D2", "U3"),
            group("G1"),
        ]))
    }

    fn counts() -> Event {
        Event::Api(ApiEvent::Counts(vec![
            count("C2", true, "4.0"),
            count("D1", false, "3.0"),
        ]))
    }

    fn listed(app: &App) -> Vec<String> {
        app.listed_indices()
            .into_iter()
            .map(|i| app.channels[i].id.clone())
            .collect()
    }

    #[test]
    fn counts_apply_whichever_arrives_first() {
        for counts_first in [true, false] {
            let mut app = app();
            if counts_first {
                update(&mut app, counts());
                update(&mut app, conversations());
            } else {
                update(&mut app, conversations());
                update(&mut app, counts());
            }
            assert!(app.channel("C2").unwrap().unread);
            assert_eq!(listed(&app), ["C1", "C2", "D1"]);
        }
    }

    #[test]
    fn without_counts_direct_messages_are_listed_but_not_groups() {
        let mut app = app();
        update(&mut app, conversations());
        update(&mut app, Event::Api(ApiEvent::CountsUnavailable));
        assert_eq!(listed(&app), ["C1", "C2", "D1", "D2"]);
    }

    #[test]
    fn a_new_direct_message_lists_the_conversation_first() {
        let mut app = app();
        update(&mut app, conversations());
        update(&mut app, counts());
        update(
            &mut app,
            Event::Rtm(RtmEvent::Message {
                channel: "D2".into(),
                message: message("9.0", "U3", "salut"),
            }),
        );
        assert_eq!(listed(&app), ["C1", "C2", "D2", "D1"]);
        assert_eq!(app.channel("D2").unwrap().mentions, 1);
    }

    #[test]
    fn the_sidebar_cursor_skips_hidden_conversations() {
        let mut app = app();
        update(&mut app, conversations());
        update(&mut app, counts());
        for _ in 0..5 {
            update(&mut app, key(KeyCode::Char('j')));
        }
        assert_eq!(app.channels[app.sidebar_cursor].id, "D1");
    }

    #[test]
    fn the_focus_layout_has_no_sidebar_to_focus() {
        let mut app = loaded_app();
        app.settings.layout = Layout::Focus;
        app.focus = Focus::Messages;
        update(&mut app, key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Messages);
        update(&mut app, key(KeyCode::Char('1')));
        assert!(matches!(app.overlay, Some(Overlay::Switcher { .. })));
    }
}

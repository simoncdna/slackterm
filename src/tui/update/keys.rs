use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::tui::keymap::Action;

pub(super) fn on_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    if ctrl && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return Vec::new();
    }
    app.notice = None;

    if app.overlay.is_some() {
        return overlays::on_overlay_key(app, key);
    }
    match app.mode {
        Mode::Insert => match app.keymap.action_while_writing(key) {
            Some(Action::GoTo) => {
                open_switcher(app);
                Vec::new()
            }
            Some(Action::Search) => {
                open_search(app);
                Vec::new()
            }
            _ => on_insert_key(app, key),
        },
        Mode::Normal => on_normal_key(app, key),
    }
}

fn on_normal_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    if key.code == KeyCode::Esc {
        if app.thread.is_some() {
            close_thread(app);
        } else {
            app.selected = None;
        }
        return Vec::new();
    }
    let Some(action) = app.keymap.action(app.focus, key) else {
        return Vec::new();
    };
    match action {
        Action::Quit => app.should_quit = true,
        Action::GoTo => open_switcher(app),
        Action::Search => open_search(app),
        Action::Settings => app.overlay = Some(Overlay::Settings { row: 0 }),
        Action::Help => app.overlay = Some(Overlay::Help { scroll: 0 }),
        Action::NextPane => cycle_focus(app, 1),
        Action::PreviousPane => cycle_focus(app, -1),
        Action::FocusSidebar => focus_sidebar(app),
        Action::FocusMessages => app.focus = Focus::Messages,
        Action::FocusThread if app.thread.is_some() => app.focus = Focus::Thread,
        Action::Write if app.current.is_some() => {
            if app.focus == Focus::Sidebar {
                app.focus = Focus::Messages;
            }
            app.mode = Mode::Insert;
        }
        action => {
            return match app.focus {
                Focus::Sidebar => on_sidebar_action(app, action),
                Focus::Messages => on_messages_action(app, action),
                Focus::Thread => on_thread_action(app, action),
            };
        }
    }
    Vec::new()
}

fn on_sidebar_action(app: &mut App, action: Action) -> Vec<Command> {
    let items = app.sidebar_items();
    let Some(last) = items.len().checked_sub(1) else {
        return Vec::new();
    };
    let position = app.sidebar_position(&items);
    let target = match action {
        Action::Down => (position + 1).min(last),
        Action::Up => position.saturating_sub(1),
        Action::Top => 0,
        Action::Bottom => last,
        Action::Left => return switch_tab(app, SidebarTab::Channels),
        Action::Right => return switch_tab(app, SidebarTab::Direct),
        Action::Open => {
            return match &items[position] {
                SidebarItem::Section(id) => toggle_section(app, id),
                SidebarItem::Channel(id) => {
                    app.focus = Focus::Messages;
                    let id = id.clone();
                    open_channel(app, &id)
                }
            };
        }
        _ => return Vec::new(),
    };
    app.sidebar_cursor = Some(items[target].clone());
    Vec::new()
}

fn toggle_section(app: &mut App, id: &str) -> Vec<Command> {
    if !app.collapsed.remove(id) {
        app.collapsed.insert(id.to_string());
    }
    app.sidebar_cursor = Some(SidebarItem::Section(id.to_string()));
    vec![Command::SaveSidebar(app.sidebar_state())]
}

fn switch_tab(app: &mut App, tab: SidebarTab) -> Vec<Command> {
    if app.sidebar_tab == tab {
        return Vec::new();
    }
    app.sidebar_tab = tab;
    // Land on the open conversation if the tab lists it, else at the top.
    app.sidebar_cursor = None;
    vec![Command::SaveSidebar(app.sidebar_state())]
}

fn on_messages_action(app: &mut App, action: Action) -> Vec<Command> {
    let len = app.current_messages().len();
    match action {
        // Going up from the oldest loaded message fetches older ones.
        Action::Up | Action::PageUp if app.selected == Some(0) => {
            return load_older(app);
        }
        Action::Up => select_older(app, 1, len),
        Action::Down => select_newer(app, 1, len),
        Action::PageUp => select_older(app, PAGE, len),
        Action::PageDown => select_newer(app, PAGE, len),
        Action::Top if len > 0 => app.selected = Some(0),
        Action::Bottom => app.selected = None,
        Action::Thread => return open_thread(app),
        Action::React => open_emoji_picker(app),
        Action::OpenFiles => return open_files(app),
        Action::Play => return play(app),
        Action::Left => focus_sidebar(app),
        _ => {}
    }
    Vec::new()
}

fn on_thread_action(app: &mut App, action: Action) -> Vec<Command> {
    let Some(thread) = &mut app.thread else {
        return Vec::new();
    };
    let len = thread.messages.len();
    match action {
        Action::Left => app.focus = Focus::Messages,
        Action::Up if len > 0 => {
            thread.selected = Some(thread.selected.unwrap_or(len).saturating_sub(1));
        }
        Action::Down => {
            thread.selected = thread.selected.map(|i| i + 1).filter(|&i| i < len);
        }
        Action::Bottom => thread.selected = None,
        Action::React => open_emoji_picker(app),
        Action::OpenFiles => return open_files(app),
        Action::Play => return play(app),
        _ => {}
    }
    Vec::new()
}

fn open_emoji_picker(app: &mut App) {
    if let Some(target) = app.reaction_target() {
        app.overlay = Some(Overlay::Emoji(EmojiPicker {
            query: Input::default(),
            cursor: 0,
            target,
        }));
    }
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
        selected: None,
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

fn open_search(app: &mut App) {
    app.mode = Mode::Normal;
    app.overlay = Some(Overlay::Search(Search {
        query: Input::default(),
        submitted: String::new(),
        status: SearchStatus::Idle,
        cursor: 0,
    }));
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
    if !newline && on_suggestions_key(app, key) {
        return Vec::new();
    }
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

/// Keys that pick a suggestion while some show. Returns whether the key
/// was theirs; the others write the message.
fn on_suggestions_key(app: &mut App, key: KeyEvent) -> bool {
    let Some(completion) = app.completion.clone() else {
        return false;
    };
    let suggestions = compose::suggestions(app, &completion);
    if suggestions.is_empty() {
        return false;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let last = suggestions.len() - 1;
    let selected = completion.selected.min(last);
    let moved = match key.code {
        KeyCode::Tab | KeyCode::Enter => {
            compose::accept(app, &completion, &suggestions[selected]);
            return true;
        }
        KeyCode::Esc => {
            app.completion = None;
            app.completion_closed = Some(completion.start);
            return true;
        }
        KeyCode::Down => (selected + 1) % (last + 1),
        KeyCode::Char('n') if ctrl => (selected + 1) % (last + 1),
        KeyCode::Up => (selected + last) % (last + 1),
        KeyCode::Char('p') if ctrl => (selected + last) % (last + 1),
        _ => return false,
    };
    if let Some(completion) = &mut app.completion {
        completion.selected = moved;
    }
    true
}

fn send(app: &mut App) -> Vec<Command> {
    if app.input.text().trim().is_empty() {
        return Vec::new();
    }
    let Some((channel, thread_ts)) = app.compose_target() else {
        return Vec::new();
    };
    let typed = app.input.take();
    let text = compose::encode(typed.trim(), app);
    vec![Command::Send {
        channel,
        text,
        thread_ts,
    }]
}

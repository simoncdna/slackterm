use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;

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
    if ctrl && key.code == KeyCode::Char('k') {
        open_switcher(app);
        return Vec::new();
    }
    if ctrl && key.code == KeyCode::Char('f') {
        open_search(app);
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
        KeyCode::Char('/') => open_search(app),
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
    let items = app.sidebar_items();
    let Some(last) = items.len().checked_sub(1) else {
        return Vec::new();
    };
    let position = app.sidebar_position(&items);
    let target = match key.code {
        KeyCode::Char('j') | KeyCode::Down => (position + 1).min(last),
        KeyCode::Char('k') | KeyCode::Up => position.saturating_sub(1),
        KeyCode::Char('g') | KeyCode::Home => 0,
        KeyCode::Char('G') | KeyCode::End => last,
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('[') => {
            return switch_tab(app, SidebarTab::Channels);
        }
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(']') => {
            return switch_tab(app, SidebarTab::Direct);
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
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

fn on_messages_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    let len = app.current_messages().len();
    match key.code {
        // Going up from the oldest loaded message fetches older ones.
        KeyCode::Char('k') | KeyCode::Up | KeyCode::PageUp if app.selected == Some(0) => {
            return load_older(app);
        }
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

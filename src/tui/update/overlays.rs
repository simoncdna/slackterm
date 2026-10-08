use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;

pub(super) fn on_overlay_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    match app.overlay {
        Some(Overlay::Switcher { .. }) => on_switcher_key(app, key),
        Some(Overlay::Settings { .. }) => on_settings_key(app, key),
        Some(Overlay::Search(_)) => on_search_key(app, key),
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

/// Enter runs the query when it changed, and otherwise opens the selected
/// result.
fn on_search_key(app: &mut App, key: KeyEvent) -> Vec<Command> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let Some(Overlay::Search(search)) = &mut app.overlay else {
        return Vec::new();
    };
    let last = match &search.status {
        SearchStatus::Done(matches) => matches.len().saturating_sub(1),
        _ => 0,
    };
    match key.code {
        KeyCode::Esc => app.overlay = None,
        KeyCode::Up => search.cursor = search.cursor.saturating_sub(1),
        KeyCode::Char('p') if ctrl => search.cursor = search.cursor.saturating_sub(1),
        KeyCode::Down | KeyCode::Tab => search.cursor = (search.cursor + 1).min(last),
        KeyCode::Char('n') if ctrl => search.cursor = (search.cursor + 1).min(last),
        KeyCode::Backspace => search.query.backspace(),
        KeyCode::Left => search.query.left(),
        KeyCode::Right => search.query.right(),
        KeyCode::Char('u') if ctrl => search.query.clear(),
        KeyCode::Char(c) if !ctrl => search.query.insert(c),
        KeyCode::Enter => {
            let query = search.query.text().trim().to_string();
            let shown = match &search.status {
                SearchStatus::Done(matches) if query == search.submitted => {
                    matches.get(search.cursor).cloned()
                }
                _ => None,
            };
            if let Some(found) = shown {
                app.overlay = None;
                return open_search_result(app, found);
            }
            if query.is_empty()
                || (query == search.submitted && matches!(search.status, SearchStatus::Loading))
            {
                return Vec::new();
            }
            search.submitted = query.clone();
            search.status = SearchStatus::Loading;
            search.cursor = 0;
            return vec![Command::Search(query)];
        }
        _ => {}
    }
    Vec::new()
}

/// Opens the conversation of a search result and selects the message, or
/// opens its thread when it is a reply.
fn open_search_result(app: &mut App, found: SearchMatch) -> Vec<Command> {
    let channel = found.channel.id.clone();
    if app.channel(&channel).is_none() {
        app.channels.push(Channel::from_search(&found.channel));
        app.sort_channels();
    }
    app.focus = Focus::Messages;
    let mut commands = open_channel(app, &channel);
    let target = match found.thread_ts() {
        Some(parent) => {
            app.thread = Some(Thread {
                channel: channel.clone(),
                ts: parent.clone(),
                messages: Vec::new(),
                loaded: false,
            });
            app.focus = Focus::Thread;
            commands.push(Command::LoadReplies {
                channel: channel.clone(),
                ts: parent.clone(),
            });
            parent
        }
        None => found.ts,
    };
    app.jump = Some(Jump {
        channel,
        ts: target,
        pages: 0,
    });
    commands.extend(try_jump(app));
    commands
}

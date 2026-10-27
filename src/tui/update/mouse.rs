//! The wheel scrolls the list under the pointer; a click opens, folds,
//! selects or starts writing, from where the last frame drew things.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use super::keys::{
    on_messages_action, on_sidebar_action, on_thread_action, switch_tab, toggle_section,
};
use super::*;
use crate::tui::keymap::Action;
use crate::tui::state::ScreenMap;

pub(super) fn on_mouse(app: &mut App, event: MouseEvent) -> Vec<Command> {
    if app.overlay.is_some() {
        return Vec::new();
    }
    let at = Position::new(event.column, event.row);
    let screen = app.screen.borrow().clone();
    let inside = |area: Option<Rect>| area.is_some_and(|area| area.contains(at));
    match event.kind {
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            let action = if event.kind == MouseEventKind::ScrollUp {
                Action::Up
            } else {
                Action::Down
            };
            if inside(screen.sidebar) {
                app.focus = Focus::Sidebar;
                on_sidebar_action(app, action)
            } else if inside(screen.thread) {
                app.focus = Focus::Thread;
                on_thread_action(app, action)
            } else if inside(screen.messages) {
                app.focus = Focus::Messages;
                on_messages_action(app, action)
            } else {
                Vec::new()
            }
        }
        MouseEventKind::Down(MouseButton::Left) => on_click(app, &screen, at),
        _ => Vec::new(),
    }
}

fn on_click(app: &mut App, screen: &ScreenMap, at: Position) -> Vec<Command> {
    let inside = |area: Option<Rect>| area.is_some_and(|area| area.contains(at));
    if let Some((_, index)) = screen.workspaces.iter().find(|(name, _)| name.contains(at)) {
        return vec![Command::SwitchWorkspace(*index)];
    }
    if let Some((_, focus)) = screen.composers.iter().find(|(area, _)| area.contains(at)) {
        if let Some(focus) = focus {
            app.focus = *focus;
        }
        if app.current.is_some() {
            app.mode = Mode::Insert;
        }
        return Vec::new();
    }
    // Anywhere else, writing pauses; the draft stays.
    app.mode = Mode::Normal;
    if let Some((tabs, dms)) = screen.tabs
        && tabs.contains(at)
    {
        app.focus = Focus::Sidebar;
        let tab = if at.x < dms {
            SidebarTab::Channels
        } else {
            SidebarTab::Direct
        };
        return switch_tab(app, tab);
    }
    if inside(screen.sidebar) {
        app.focus = Focus::Sidebar;
        let Some(item) = row_at(&screen.sidebar_rows, at) else {
            return Vec::new();
        };
        app.sidebar_cursor = Some(item.clone());
        return match item {
            SidebarItem::Section(id) => toggle_section(app, &id),
            SidebarItem::Channel(id) => {
                app.focus = Focus::Messages;
                open_channel(app, &id)
            }
        };
    }
    if inside(screen.thread) {
        app.focus = Focus::Thread;
        if let Some(index) = row_at(&screen.thread_rows, at)
            && let Some(thread) = &mut app.thread
        {
            thread.selected = Some(index);
        }
        return Vec::new();
    }
    if inside(screen.messages) {
        app.focus = Focus::Messages;
        if let Some(index) = row_at(&screen.message_rows, at) {
            app.selected = Some(index);
        }
    }
    Vec::new()
}

fn row_at<T: Clone>(rows: &[(u16, T)], at: Position) -> Option<T> {
    rows.iter()
        .find(|(row, _)| *row == at.y)
        .map(|(_, value)| value.clone())
}

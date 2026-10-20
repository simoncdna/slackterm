use std::path::Path;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::config::{AvatarStyle, Choice, Settings};
use crate::slack::{HIGHLIGHT_END, HIGHLIGHT_START, SearchMatch, mrkdwn};
use crate::tui::compose::{self, Completion};
use crate::tui::input::Input;
use crate::tui::keymap::{ACTIONS, Scope};
use crate::tui::state::{
    App, EmojiPicker, ReactionTarget, Search, SearchStatus, SwitcherTarget, WorkspaceTab,
};
use crate::tui::theme::Theme;

use super::text::{Styled, align_left, local_time, short_datetime, truncate, truncate_styled};

fn popup(frame: &mut Frame, area: Rect, theme: &Theme, title: &str, right: String) -> Rect {
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.panel).fg(theme.fg))
        .padding(Padding::horizontal(1))
        .title(Line::styled(
            format!(" {title} "),
            Style::new().fg(theme.accent).bold(),
        ))
        .title(Line::styled(format!(" {right} "), Style::new().fg(theme.muted)).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

fn hint_line(theme: &Theme, hints: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::new();
    for (key, label) in hints {
        spans.push(Span::styled(key.to_string(), Style::new().fg(theme.fg)));
        spans.push(Span::styled(
            format!(" {label}   "),
            Style::new().fg(theme.muted),
        ));
    }
    Line::from(spans)
}

pub fn switcher(frame: &mut Frame, app: &App, theme: &Theme, query: &Input, cursor: usize) -> Rect {
    let screen = frame.area();
    let width = screen.width.saturating_sub(4).min(72);
    let height = screen.height.saturating_sub(4).min(18);
    let area = Rect {
        x: screen.x + (screen.width - width) / 2,
        y: screen.y + (screen.height / 6).min(screen.height - height),
        width,
        height,
    };
    let results = app.switcher_results(query.text());
    let inner = popup(
        frame,
        area,
        theme,
        "go to",
        format!("{} found", results.len()),
    );

    let [input_area, list_area, hint_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let prompt = Line::from(vec![
        Span::styled("› ", Style::new().fg(theme.accent)),
        Span::styled(query.text().to_string(), Style::new().fg(theme.fg)),
    ]);
    frame.render_widget(Paragraph::new(prompt), input_area);
    let (_, (_, cursor_col)) = query.layout(u16::MAX);
    frame.set_cursor_position((input_area.x + 2 + cursor_col, input_area.y));

    let visible = list_area.height as usize;
    let start = (cursor + 1).saturating_sub(visible);
    let rows: Vec<Line> = results
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .map(|(index, result)| {
            let selected = index == cursor;
            let mut spans = vec![if selected {
                Span::styled("› ", Style::new().fg(theme.accent))
            } else {
                Span::raw("  ")
            }];
            for (i, c) in result.label.chars().enumerate() {
                let style = if result.matched.contains(&i) {
                    Style::new().fg(theme.accent).bold()
                } else {
                    Style::new().fg(theme.fg)
                };
                spans.push(Span::styled(c.to_string(), style));
            }
            match &result.target {
                SwitcherTarget::Channel(index) => {
                    let channel = &app.channels[*index];
                    if channel.mentions > 0 {
                        spans.push(Span::styled(
                            format!("  @{}", channel.mentions),
                            Style::new().fg(theme.accent),
                        ));
                    } else if channel.unread {
                        spans.push(Span::styled("  •", Style::new().fg(theme.unread)));
                    }
                }
                SwitcherTarget::Person(_) => spans.push(Span::styled(
                    "  start a conversation",
                    Style::new().fg(theme.muted),
                )),
                SwitcherTarget::Join(_) => {
                    spans.push(Span::styled("  join", Style::new().fg(theme.muted)))
                }
                SwitcherTarget::Workspace(index) => {
                    spans.push(Span::styled("  workspace", Style::new().fg(theme.muted)));
                    spans.extend(workspace_badge(&app.workspaces[*index], theme));
                }
            }
            let line = Line::from(spans);
            if selected {
                line.style(Style::new().bg(theme.surface))
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(rows), list_area);

    frame.render_widget(
        Paragraph::new(hint_line(
            theme,
            &[("↑↓", "select"), ("⏎", "open"), ("esc", "close")],
        )),
        hint_area,
    );
    area
}

/// ` @2` when people wait for the user there, ` •` when there is news.
pub fn workspace_badge(tab: &WorkspaceTab, theme: &Theme) -> Option<Span<'static>> {
    if tab.mentions > 0 {
        Some(Span::styled(
            format!(" @{}", tab.mentions),
            Style::new().fg(theme.accent).bold(),
        ))
    } else if tab.unread {
        Some(Span::styled(" •", Style::new().fg(theme.unread)))
    } else {
        None
    }
}

pub fn settings(
    frame: &mut Frame,
    theme: &Theme,
    settings: &Settings,
    path: Option<&Path>,
    images: &str,
    row: usize,
) -> Rect {
    let screen = frame.area();
    let width = screen.width.saturating_sub(4).min(64);
    let height = 12.min(screen.height);
    let area = Rect {
        x: screen.x + (screen.width - width) / 2,
        y: screen.y + (screen.height - height) / 3,
        width,
        height,
    };
    let inner = popup(frame, area, theme, "settings", "applied live".into());
    let [rows_area, path_area, hint_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let entries = [
        ("Layout", settings.layout.label()),
        ("Theme", settings.theme.label()),
        ("Accent", settings.accent.label()),
        ("Avatars", settings.avatars.label()),
        ("Clock", settings.clock.label()),
    ];
    // The picture protocol in use, to tell why only initials show.
    let notes = [
        None,
        None,
        None,
        (settings.avatars == AvatarStyle::Images).then_some(images),
        None,
    ];
    let mut lines = vec![Line::default()];
    for (index, ((name, value), note)) in entries.into_iter().zip(notes).enumerate() {
        let selected = index == row;
        let arrows = Style::new().fg(if selected { theme.accent } else { theme.border });
        let line = Line::from(vec![
            Span::styled(
                if selected { "› " } else { "  " },
                Style::new().fg(theme.accent),
            ),
            Span::styled(format!("{name:<12}"), Style::new().fg(theme.muted)),
            Span::styled("‹ ", arrows),
            Span::styled(value, Style::new().fg(theme.fg).bold()),
            Span::styled(" ›", arrows),
            Span::styled(
                note.map(|n| format!("  {n}")).unwrap_or_default(),
                Style::new().fg(theme.muted),
            ),
        ]);
        lines.push(if selected {
            line.style(Style::new().bg(theme.surface))
        } else {
            line
        });
    }
    frame.render_widget(Paragraph::new(lines), rows_area);

    let saved = match path {
        Some(path) => format!("saved to {}", path.display()),
        None => "settings not saved".to_string(),
    };
    let label = truncate(&saved, path_area.width as usize);
    frame.render_widget(
        Paragraph::new(Line::styled(label, Style::new().fg(theme.muted))),
        path_area,
    );
    frame.render_widget(
        Paragraph::new(hint_line(
            theme,
            &[("j/k", "select"), ("h/l", "change"), ("esc", "close")],
        )),
        hint_area,
    );
    area
}

pub fn search(frame: &mut Frame, app: &App, theme: &Theme, search: &Search) -> Rect {
    let screen = frame.area();
    let width = screen.width.saturating_sub(4).min(110);
    let height = screen.height.saturating_sub(2).min(34);
    let area = Rect {
        x: screen.x + (screen.width - width) / 2,
        y: screen.y + (screen.height - height) / 4,
        width,
        height,
    };
    let status = match &search.status {
        SearchStatus::Idle => "⏎ to search".to_string(),
        SearchStatus::Loading => "searching…".to_string(),
        SearchStatus::Done(matches) => {
            let plural = if matches.len() == 1 { "" } else { "s" };
            format!("{} result{plural}", matches.len())
        }
        SearchStatus::Failed(_) => "error".to_string(),
    };
    let inner = popup(frame, area, theme, "search messages", status);
    let [input_area, tip_area, list_area, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let prompt = Line::from(vec![
        Span::styled("/ ", Style::new().fg(theme.accent)),
        Span::styled(search.query.text().to_string(), Style::new().fg(theme.fg)),
    ]);
    frame.render_widget(Paragraph::new(prompt), input_area);
    let (_, (_, cursor_col)) = search.query.layout(u16::MAX);
    frame.set_cursor_position((input_area.x + 2 + cursor_col, input_area.y));
    let tip = "in:#channel   from:@person   during:today   \"exact phrase\"";
    frame.render_widget(
        Paragraph::new(Line::styled(tip, Style::new().fg(theme.muted))),
        tip_area,
    );

    let message = |text: String, color| Paragraph::new(Line::styled(text, Style::new().fg(color)));
    match &search.status {
        SearchStatus::Idle => frame.render_widget(
            message(
                "Type your search, then ⏎. ⏎ on a result opens it.".into(),
                theme.muted,
            ),
            list_area,
        ),
        SearchStatus::Loading => {
            frame.render_widget(message("searching…".into(), theme.muted), list_area)
        }
        SearchStatus::Failed(reason) => {
            frame.render_widget(message(reason.clone(), theme.unread), list_area)
        }
        SearchStatus::Done(matches) if matches.is_empty() => {
            frame.render_widget(message("No results.".into(), theme.muted), list_area)
        }
        SearchStatus::Done(matches) => {
            let visible = (list_area.height as usize / 3).max(1);
            let start = (search.cursor + 1).saturating_sub(visible);
            let today = chrono::Local::now().date_naive();
            let mut lines = Vec::new();
            for (index, found) in matches.iter().enumerate().skip(start).take(visible) {
                let selected = index == search.cursor;
                let (header, snippet) =
                    result_lines(app, theme, found, list_area.width as usize, selected, today);
                let style = if selected {
                    Style::new().bg(theme.surface)
                } else {
                    Style::new()
                };
                lines.push(header.style(style));
                lines.push(snippet.style(style));
                lines.push(Line::default());
            }
            frame.render_widget(Paragraph::new(lines), list_area);
        }
    }

    frame.render_widget(
        Paragraph::new(hint_line(
            theme,
            &[("⏎", "search / open"), ("↑↓", "select"), ("esc", "close")],
        )),
        hint_area,
    );
    area
}

/// Two rows per result: where and who, then the text with the matched
/// terms highlighted.
fn result_lines(
    app: &App,
    theme: &Theme,
    found: &SearchMatch,
    width: usize,
    selected: bool,
    today: chrono::NaiveDate,
) -> (Line<'static>, Line<'static>) {
    let place = match app.channel(&found.channel.id) {
        Some(channel) => app.channel_label(channel),
        None if found.channel.is_im => format!("@{}", app.user_name(&found.channel.name)),
        None => format!("#{}", found.channel.name),
    };
    let author = match &found.user {
        Some(user) => app.user_name(user),
        None => found.username.clone().unwrap_or_default(),
    };
    let key = found.user.clone().unwrap_or_else(|| author.clone());
    let mut header = vec![
        Span::styled(
            if selected { "› " } else { "  " },
            Style::new().fg(theme.accent),
        ),
        Span::styled(place, Style::new().fg(theme.link)),
        Span::styled(" · ", Style::new().fg(theme.muted)),
        Span::styled(author, Style::new().fg(theme.nick(&key)).bold()),
        Span::styled(
            format!(
                " · {}",
                short_datetime(local_time(found.epoch_seconds()), today, app.settings.clock)
            ),
            Style::new().fg(theme.muted),
        ),
    ];
    if found.thread_ts().is_some() {
        header.push(Span::styled("  ↳ thread", Style::new().fg(theme.link)));
    }

    let normal = Style::new().fg(theme.fg);
    let highlight = Style::new().fg(theme.accent).bold();
    let mut highlighted = false;
    let mut parts: Vec<Styled> = Vec::new();
    for segment in mrkdwn::parse(&found.text, &app.my_id, app) {
        let base = match segment.kind {
            mrkdwn::Kind::Text => normal,
            mrkdwn::Kind::Code => normal.bg(theme.panel),
            _ => Style::new().fg(theme.link),
        };
        let base = super::widgets::dress(base, segment.emphasis);
        let mut current = String::new();
        for c in segment.text.chars() {
            match c {
                HIGHLIGHT_START | HIGHLIGHT_END => {
                    let style = if highlighted { highlight } else { base };
                    parts.push((std::mem::take(&mut current), style));
                    highlighted = c == HIGHLIGHT_START;
                }
                '\n' => current.push(' '),
                c => current.push(c),
            }
        }
        parts.push((current, if highlighted { highlight } else { base }));
    }
    parts.retain(|(text, _)| !text.is_empty());
    let mut snippet = vec![Span::raw("    ")];
    snippet.extend(
        truncate_styled(parts, width.saturating_sub(4))
            .into_iter()
            .map(|(text, style)| Span::styled(text, style)),
    );
    (Line::from(header), Line::from(snippet))
}

pub fn emoji_picker(frame: &mut Frame, app: &App, theme: &Theme, picker: &EmojiPicker) -> Rect {
    let screen = frame.area();
    let width = screen.width.saturating_sub(4).min(64);
    let height = screen.height.saturating_sub(4).min(22);
    let area = Rect {
        x: screen.x + (screen.width - width) / 2,
        y: screen.y + (screen.height - height) / 4,
        width,
        height,
    };
    let inner = popup(
        frame,
        area,
        theme,
        "react",
        format!("message from {}", picker.target.author),
    );
    let [preview_area, input_area, list_area, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let preview: String = mrkdwn::parse(&picker.target.text, &app.my_id, app)
        .into_iter()
        .map(|segment| segment.text.replace('\n', " "))
        .collect();
    frame.render_widget(
        Paragraph::new(Line::styled(
            truncate(&format!("“{preview}”"), preview_area.width as usize),
            Style::new().fg(theme.muted),
        )),
        preview_area,
    );

    let prompt = Line::from(vec![
        Span::styled(":", Style::new().fg(theme.accent)),
        Span::styled(picker.query.text().to_string(), Style::new().fg(theme.fg)),
    ]);
    frame.render_widget(Paragraph::new(prompt), input_area);
    let (_, (_, cursor_col)) = picker.query.layout(u16::MAX);
    frame.set_cursor_position((input_area.x + 1 + cursor_col, input_area.y));

    let message = app.find_message(&picker.target.channel, &picker.target.ts);
    let mine = |name: &str| {
        message
            .and_then(|m| m.reactions.iter().find(|r| r.name == name))
            .is_some_and(|r| r.users.contains(&app.my_id))
    };
    let choices = app.emoji_choices(picker.query.text());
    if choices.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::styled(
                "No matching emoji.",
                Style::new().fg(theme.muted),
            )),
            list_area,
        );
    }
    let visible = list_area.height as usize;
    let start = (picker.cursor + 1).saturating_sub(visible);
    let rows: Vec<Line> = choices
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .map(|(index, choice)| {
            let selected = index == picker.cursor;
            let glyph = choice.glyph.clone().unwrap_or_default();
            // Emoji are two columns wide; pad so the names line up.
            let pad = " ".repeat(
                3usize.saturating_sub(unicode_width::UnicodeWidthStr::width(glyph.as_str())),
            );
            let mut spans = vec![
                Span::styled(
                    if selected { "› " } else { "  " },
                    Style::new().fg(theme.accent),
                ),
                Span::raw(format!("{glyph}{pad}")),
                Span::styled(":", Style::new().fg(theme.muted)),
            ];
            for (i, c) in choice.name.chars().enumerate() {
                let style = if choice.matched.contains(&i) {
                    Style::new().fg(theme.accent).bold()
                } else {
                    Style::new().fg(theme.fg)
                };
                spans.push(Span::styled(c.to_string(), style));
            }
            spans.push(Span::styled(":", Style::new().fg(theme.muted)));
            if choice.glyph.is_none() {
                spans.push(Span::styled("  custom", Style::new().fg(theme.muted)));
            }
            if mine(&choice.name) {
                spans.push(Span::styled(
                    "  ✓ already added",
                    Style::new().fg(theme.accent),
                ));
            }
            let line = Line::from(spans);
            if selected {
                line.style(Style::new().bg(theme.surface))
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(rows), list_area);

    frame.render_widget(
        Paragraph::new(hint_line(
            theme,
            &[("⏎", "react / remove"), ("↑↓", "select"), ("esc", "close")],
        )),
        hint_area,
    );
    area
}

/// Asks before deleting one of the user's messages, showing its start.
pub fn delete_message(
    frame: &mut Frame,
    app: &App,
    theme: &Theme,
    target: &ReactionTarget,
) -> Rect {
    let screen = frame.area();
    let width = screen.width.saturating_sub(4).min(64);
    let height = 7.min(screen.height);
    let area = Rect {
        x: screen.x + (screen.width - width) / 2,
        y: screen.y + (screen.height - height) / 3,
        width,
        height,
    };
    let inner = popup(frame, area, theme, "delete message", "for everyone".into());
    let [text_area, hint_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);
    let text: String = mrkdwn::parse(&target.text, &app.my_id, app)
        .into_iter()
        .map(|segment| segment.text.replace('\n', " "))
        .collect();
    let lines = vec![
        Line::default(),
        Line::styled(
            truncate(&format!("“{text}”"), text_area.width as usize),
            Style::new().fg(theme.fg),
        ),
    ];
    frame.render_widget(Paragraph::new(lines), text_area);
    frame.render_widget(
        Paragraph::new(hint_line(theme, &[("y", "delete"), ("n", "keep")])),
        hint_area,
    );
    area
}

/// Every key: the keymap's, then the ones that never change.
pub fn help(frame: &mut Frame, app: &App, theme: &Theme, scroll: usize) -> Rect {
    let lines = help_lines(app, theme);
    let screen = frame.area();
    let width = screen.width.saturating_sub(4).min(72);
    let height = (lines.len() as u16 + 3).min(screen.height.saturating_sub(2));
    let area = Rect {
        x: screen.x + (screen.width - width) / 2,
        y: screen.y + (screen.height - height) / 2,
        width,
        height,
    };
    let inner = popup(
        frame,
        area,
        theme,
        "keys",
        "change them in [keys] of config.toml".into(),
    );
    let [list_area, hint_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);
    let max = lines.len().saturating_sub(list_area.height as usize);
    app.help_max_scroll.set(max);
    let visible: Vec<Line> = lines.into_iter().skip(scroll.min(max)).collect();
    frame.render_widget(Paragraph::new(visible), list_area);
    frame.render_widget(
        Paragraph::new(hint_line(theme, &[("j/k", "scroll"), ("esc", "close")])),
        hint_area,
    );
    area
}

/// Keys of the message box, which the keymap leaves alone so that typing
/// stays typing.
const WRITING: &[(&str, &str)] = &[
    ("⏎", "Send"),
    ("@ #  tab", "Mention someone or a channel: type, then pick"),
    ("⇧⏎ alt+⏎ ^j", "New line"),
    ("^a ^e", "Start / end of the line"),
    ("^w alt+⌫", "Delete the previous word"),
    ("^u", "Clear the message"),
    ("esc", "Back to normal mode"),
];

fn help_lines(app: &App, theme: &Theme) -> Vec<Line<'static>> {
    const KEYS: usize = 16;
    let heading =
        |title: &str| Line::styled(title.to_string(), Style::new().fg(theme.accent).bold());
    let row = |keys: &str, label: &str| {
        Line::from(vec![
            Span::styled(
                format!("  {}", align_left(keys, KEYS)),
                Style::new().fg(theme.fg),
            ),
            Span::styled(label.to_string(), Style::new().fg(theme.muted)),
        ])
    };
    let mut lines = Vec::new();
    let scopes = [
        (Scope::Anywhere, "Anywhere"),
        (Scope::Lists, "Sidebar, messages and thread"),
        (Scope::Message, "On a message"),
    ];
    for (scope, title) in scopes {
        lines.push(heading(title));
        for definition in ACTIONS.iter().filter(|d| d.scope == scope) {
            let keys: Vec<String> = app
                .keymap
                .keys(definition.action)
                .iter()
                .map(ToString::to_string)
                .collect();
            let keys = if keys.is_empty() {
                "—".to_string()
            } else {
                keys.join(" ")
            };
            lines.push(row(&keys, definition.help));
        }
        lines.push(Line::default());
    }
    lines.push(heading("While writing"));
    for (keys, label) in WRITING {
        lines.push(row(keys, label));
    }
    lines.push(Line::default());
    lines.push(heading("Always"));
    lines.push(row("^c", "Quit"));
    lines.push(row("esc", "Close the thread or the popup, deselect"));
    lines
}

/// People or channels for the name being typed, just above the message box.
pub fn suggestions(
    frame: &mut Frame,
    app: &App,
    theme: &Theme,
    completion: &Completion,
) -> Option<Rect> {
    let suggestions = compose::suggestions(app, completion);
    let composer = app.composer_area.get()?;
    if suggestions.is_empty() {
        return None;
    }
    let screen = frame.area();
    let width = screen.width.saturating_sub(composer.x).min(64);
    let height = suggestions.len() as u16 + 2;
    let area = Rect {
        x: composer.x,
        y: composer.y.saturating_sub(height),
        width,
        height: height.min(composer.y),
    };
    let title = if completion.sigil == '#' {
        "channels"
    } else {
        "people"
    };
    let inner = popup(frame, area, theme, title, "tab to complete".into());
    let selected = completion.selected.min(suggestions.len() - 1);
    let rows: Vec<Line> = suggestions
        .iter()
        .enumerate()
        .map(|(index, suggestion)| {
            let room = (inner.width as usize).saturating_sub(suggestion.text.width() + 4);
            let line = Line::from(vec![
                Span::styled(
                    if index == selected { "› " } else { "  " },
                    Style::new().fg(theme.accent),
                ),
                Span::styled(suggestion.text.clone(), Style::new().fg(theme.fg).bold()),
                Span::styled(
                    format!("  {}", truncate(&suggestion.detail, room)),
                    Style::new().fg(theme.muted),
                ),
            ]);
            if index == selected {
                line.style(Style::new().bg(theme.surface))
            } else {
                line
            }
        })
        .collect();
    frame.render_widget(Paragraph::new(rows), inner);
    Some(area)
}

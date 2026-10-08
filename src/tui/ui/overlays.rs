use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Padding, Paragraph};

use crate::config::{Choice, Settings};
use crate::tui::input::Input;
use crate::tui::state::App;
use crate::tui::theme::Theme;

use super::text::truncate;

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

pub fn switcher(frame: &mut Frame, app: &App, theme: &Theme, query: &Input, cursor: usize) {
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
        "aller à",
        format!("{} / {}", results.len(), app.channels.len()),
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
            let channel = &app.channels[result.channel];
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
            if channel.mentions > 0 {
                spans.push(Span::styled(
                    format!("  @{}", channel.mentions),
                    Style::new().fg(theme.accent),
                ));
            } else if channel.unread {
                spans.push(Span::styled("  •", Style::new().fg(theme.unread)));
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
            &[("↑↓", "choisir"), ("⏎", "ouvrir"), ("esc", "fermer")],
        )),
        hint_area,
    );
}

pub fn settings(frame: &mut Frame, theme: &Theme, settings: &Settings, row: usize) {
    let screen = frame.area();
    let width = screen.width.saturating_sub(4).min(64);
    let height = 10.min(screen.height);
    let area = Rect {
        x: screen.x + (screen.width - width) / 2,
        y: screen.y + (screen.height - height) / 3,
        width,
        height,
    };
    let inner = popup(frame, area, theme, "réglages", "appliqués en direct".into());
    let [rows_area, path_area, hint_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let entries = [
        ("Affichage", settings.layout.label()),
        ("Thème", settings.theme.label()),
        ("Accent", settings.accent.label()),
    ];
    let mut lines = vec![Line::default()];
    for (index, (name, value)) in entries.into_iter().enumerate() {
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
        ]);
        lines.push(if selected {
            line.style(Style::new().bg(theme.surface))
        } else {
            line
        });
    }
    frame.render_widget(Paragraph::new(lines), rows_area);

    let path = Settings::path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let label = truncate(&format!("enregistré dans {path}"), path_area.width as usize);
    frame.render_widget(
        Paragraph::new(Line::styled(label, Style::new().fg(theme.muted))),
        path_area,
    );
    frame.render_widget(
        Paragraph::new(hint_line(
            theme,
            &[("j/k", "choisir"), ("h/l", "changer"), ("esc", "fermer")],
        )),
        hint_area,
    );
}

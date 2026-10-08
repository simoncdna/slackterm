//! The three screen layouts the user can pick in the settings.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::tui::state::{App, Focus};
use crate::tui::theme::Theme;

use super::text::truncate;
use super::widgets::{
    InputStyle, MessageStyle, SidebarStyle, input_box, input_height, message_lines, panel,
    render_messages, separator, sidebar, status_bar,
};

fn channel_title(app: &App) -> String {
    app.current_channel()
        .map(|c| app.channel_label(c))
        .unwrap_or_else(|| "…".to_string())
}

fn placeholder(app: &App) -> String {
    if thread_target(app) {
        "Répondre dans le fil… (i)".to_string()
    } else {
        format!("Écrire dans {}… (i)", channel_title(app))
    }
}

fn thread_target(app: &App) -> bool {
    app.thread.is_some() && app.focus == Focus::Thread
}

/// The current channel's messages, styled for `style`.
fn channel_messages(frame: &mut Frame, area: Rect, app: &App, theme: &Theme, style: MessageStyle) {
    let selected = if app.focus == Focus::Messages {
        app.selected
    } else {
        None
    };
    let rendered = message_lines(
        app,
        theme,
        app.current_messages(),
        area.width,
        selected,
        style,
        false,
    );
    let loading = app.current_history().is_none_or(|h| !h.loaded);
    render_messages(frame, area, rendered, theme, loading);
}

fn thread_messages(frame: &mut Frame, area: Rect, app: &App, theme: &Theme, style: MessageStyle) {
    let Some(thread) = &app.thread else { return };
    let rendered = message_lines(app, theme, &thread.messages, area.width, None, style, true);
    render_messages(frame, area, rendered, theme, !thread.loaded);
}

fn thread_title(app: &App) -> String {
    let Some(thread) = &app.thread else {
        return "fil".to_string();
    };
    let Some(parent) = thread.messages.first() else {
        return "fil".to_string();
    };
    let replies = (parent.reply_count as usize).max(thread.messages.len() - 1);
    let author = app.author(parent);
    format!(
        "fil · {author} · {replies} réponse{}",
        if replies > 1 { "s" } else { "" }
    )
}

// ---------------------------------------------------- A · trois panneaux

pub fn panels(frame: &mut Frame, app: &App, theme: &Theme) {
    let [main, status] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
    let sidebar_width = (main.width / 4).clamp(20, 32);
    let thread_width = if app.thread.is_some() {
        (main.width / 3).clamp(32, 60)
    } else {
        0
    };
    let [side, center, right] = Layout::horizontal([
        Constraint::Length(sidebar_width),
        Constraint::Fill(1),
        Constraint::Length(thread_width),
    ])
    .spacing(1)
    .areas(main);

    let block = panel(
        theme,
        format!("[1] {}", app.team_name),
        None,
        app.focus == Focus::Sidebar,
    );
    let inner = block.inner(side);
    frame.render_widget(block, side);
    sidebar(
        frame,
        inner,
        app,
        theme,
        app.focus == Focus::Sidebar,
        SidebarStyle::Sections,
    );

    let block = panel(
        theme,
        format!("[2] {}", channel_title(app)),
        None,
        app.focus == Focus::Messages,
    );
    let inner = block.inner(center);
    frame.render_widget(block, center);
    let composer = InputStyle::Boxed { title: "message" };
    let topic = app
        .current_channel()
        .map(|c| c.topic.clone())
        .unwrap_or_default();
    let [topic_area, messages_area, input_area] = Layout::vertical([
        Constraint::Length(if topic.is_empty() { 0 } else { 1 }),
        Constraint::Fill(1),
        Constraint::Length(input_height(app, inner.width, &composer)),
    ])
    .areas(inner);
    frame.render_widget(
        Paragraph::new(Line::styled(
            truncate(&format!(" {topic}"), topic_area.width as usize),
            Style::new().fg(theme.muted),
        )),
        topic_area,
    );
    channel_messages(frame, messages_area, app, theme, MessageStyle::Panels);
    let channel_placeholder = format!("Écrire dans {}… (i)", channel_title(app));
    input_box(
        frame,
        input_area,
        app,
        theme,
        &composer,
        !thread_target(app),
        &channel_placeholder,
    );

    if app.thread.is_some() {
        let block = panel(
            theme,
            "[3] fil".into(),
            Some("esc fermer".into()),
            app.focus == Focus::Thread,
        );
        let inner = block.inner(right);
        frame.render_widget(block, right);
        let composer = InputStyle::Boxed { title: "réponse" };
        let [messages_area, input_area] = Layout::vertical([
            Constraint::Fill(1),
            Constraint::Length(input_height(app, inner.width, &composer)),
        ])
        .areas(inner);
        thread_messages(frame, messages_area, app, theme, MessageStyle::Panels);
        input_box(
            frame,
            input_area,
            app,
            theme,
            &composer,
            thread_target(app),
            "Répondre… (3 puis i)",
        );
    }

    status_bar(frame, status, app, theme, None);
}

// ------------------------------------------------- B · flux dense (IRC)

pub fn stream(frame: &mut Frame, app: &App, theme: &Theme) {
    let target = if thread_target(app) { "fil" } else { "" };
    let prompt = if target.is_empty() {
        format!("[{}] ", app.my_name)
    } else {
        format!("[{} → fil] ", app.my_name)
    };
    let composer = InputStyle::Prompt { prompt };
    let [title, main, thread, status, input] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Percentage(if app.thread.is_some() { 35 } else { 0 }),
        Constraint::Length(1),
        Constraint::Length(input_height(
            app,
            frame.area().width.saturating_sub(2),
            &composer,
        )),
    ])
    .areas(frame.area());

    let topic = app
        .current_channel()
        .map(|c| c.topic.clone())
        .unwrap_or_default();
    let title_line = Line::from(vec![
        Span::styled(
            format!(" {}", channel_title(app)),
            Style::new().fg(theme.fg).bold(),
        ),
        Span::styled(" │ ", Style::new().fg(theme.border)),
        Span::styled(topic, Style::new().fg(theme.muted)),
    ]);
    frame.render_widget(
        Paragraph::new(title_line).style(Style::new().bg(theme.panel)),
        title,
    );

    let [buffers, messages] =
        Layout::horizontal([Constraint::Length(26), Constraint::Fill(1)]).areas(main);
    let block = Block::new()
        .borders(Borders::RIGHT)
        .border_style(Style::new().fg(if app.focus == Focus::Sidebar {
            theme.accent
        } else {
            theme.border
        }));
    let inner = block.inner(buffers);
    frame.render_widget(block, buffers);
    sidebar(
        frame,
        inner,
        app,
        theme,
        app.focus == Focus::Sidebar,
        SidebarStyle::Numbered,
    );
    let messages = messages.inner(ratatui::layout::Margin::new(1, 0));
    channel_messages(frame, messages, app, theme, MessageStyle::Stream);

    if app.thread.is_some() {
        let [rule, body] =
            Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(thread);
        let color = if app.focus == Focus::Thread {
            theme.accent
        } else {
            theme.muted
        };
        frame.render_widget(
            Paragraph::new(separator(
                &thread_title(app),
                rule.width as usize,
                color,
                theme.border,
            )),
            rule,
        );
        thread_messages(
            frame,
            body.inner(ratatui::layout::Margin::new(1, 0)),
            app,
            theme,
            MessageStyle::Stream,
        );
    }

    status_bar(frame, status, app, theme, Some(theme.panel));
    let input = input.inner(ratatui::layout::Margin::new(1, 0));
    input_box(frame, input, app, theme, &composer, true, &placeholder(app));
}

// ---------------------------------------------- C · focus + sélecteur

pub fn focus(frame: &mut Frame, app: &App, theme: &Theme) {
    let composer = InputStyle::Prompt {
        prompt: "› ".into(),
    };
    let area = frame.area();
    let width = area.width.saturating_sub(4);
    let [header, messages, thread, input, status] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Percentage(if app.thread.is_some() { 35 } else { 0 }),
        Constraint::Length(input_height(app, width, &composer) + 1),
        Constraint::Length(1),
    ])
    .areas(area);

    let block = Block::new()
        .borders(Borders::BOTTOM)
        .border_style(Style::new().fg(theme.border));
    let header_inner = block
        .inner(header)
        .inner(ratatui::layout::Margin::new(2, 0));
    frame.render_widget(block, header);
    let topic = app
        .current_channel()
        .map(|c| c.topic.clone())
        .unwrap_or_default();
    let hint = "^k aller à   , réglages";
    let room = (header_inner.width as usize).saturating_sub(hint.width() + 2);
    let title = channel_title(app);
    let left = vec![
        Span::styled(title.clone(), Style::new().fg(theme.accent).bold()),
        Span::styled(
            truncate(&format!("   {topic}"), room.saturating_sub(title.width())),
            Style::new().fg(theme.muted),
        ),
    ];
    let [left_area, hint_area] =
        Layout::horizontal([Constraint::Fill(1), Constraint::Length(hint.width() as u16)])
            .areas(header_inner);
    frame.render_widget(Paragraph::new(Line::from(left)), left_area);
    frame.render_widget(
        Paragraph::new(Line::styled(hint, Style::new().fg(theme.muted))),
        hint_area,
    );

    let messages = messages.inner(ratatui::layout::Margin::new(2, 1));
    channel_messages(frame, messages, app, theme, MessageStyle::Focus);

    if app.thread.is_some() {
        let block = Block::new()
            .borders(Borders::TOP)
            .border_style(Style::new().fg(theme.border))
            .title(Line::styled(
                format!(" {} ", thread_title(app)),
                Style::new().fg(if app.focus == Focus::Thread {
                    theme.accent
                } else {
                    theme.muted
                }),
            ));
        let inner = block
            .inner(thread)
            .inner(ratatui::layout::Margin::new(2, 0));
        frame.render_widget(block, thread);
        thread_messages(frame, inner, app, theme, MessageStyle::Focus);
    }

    let block = Block::new()
        .borders(Borders::TOP)
        .border_style(Style::new().fg(theme.border));
    let input_inner = block.inner(input).inner(ratatui::layout::Margin::new(2, 0));
    frame.render_widget(block, input);
    input_box(
        frame,
        input_inner,
        app,
        theme,
        &composer,
        true,
        &placeholder(app),
    );

    status_bar(frame, status, app, theme, Some(theme.panel));
}

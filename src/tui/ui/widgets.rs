//! Pieces shared by the three layouts: message list, sidebar, input box,
//! status bar and bordered panels.

use chrono::Local;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect, Size};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Padding, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::config::AvatarStyle;
use crate::config::SidebarTab;
use crate::slack::mrkdwn::{self, Kind};
use crate::slack::{File, FileKind, Message};
use crate::slack::{emoji, media};
use crate::tui::keymap::Action;
use crate::tui::pictures::{AVATAR_SIZE, Pictures};
use crate::tui::state::{App, Channel, ChannelKind, Connection, Focus, Mode, Section, SidebarRow};
use crate::tui::theme::Theme;

use super::text::{
    Styled, align_left, align_right, day_label, format_duration, format_size, local_time,
    recency_label, time_of_day, time_width, truncate, wrap,
};

/// Consecutive messages from the same author within this delay share a header.
const GROUPING_SECONDS: i64 = 300;
const MAX_INPUT_ROWS: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageStyle {
    /// Time column, author on its own row (layout A).
    Panels,
    /// One row per message with aligned author names (layout B).
    Stream,
    /// Author then time, airy spacing (layout C).
    Focus,
}

/// Rows left blank for a file's preview, drawn over them afterwards.
pub struct PreviewAnchor {
    pub line: usize,
    pub column: u16,
    pub size: Size,
    pub key: String,
}

pub struct Rendered {
    pub lines: Vec<Line<'static>>,
    /// Header rows that begin with a picture, and whose picture it is.
    pub avatars: Vec<(usize, String)>,
    pub previews: Vec<PreviewAnchor>,
    /// Rows of the selected message, to keep it in view.
    pub selected: Option<(usize, usize)>,
}

impl Rendered {
    /// Adds a row above the messages, moving down every row that points
    /// into them: pictures would otherwise be drawn one row too high.
    pub fn push_front(&mut self, line: Line<'static>) {
        self.lines.insert(0, line);
        for (row, _) in &mut self.avatars {
            *row += 1;
        }
        for preview in &mut self.previews {
            preview.line += 1;
        }
        self.selected = self.selected.map(|(start, end)| (start + 1, end + 1));
    }
}

pub fn message_lines(
    app: &App,
    theme: &Theme,
    messages: &[Message],
    width: u16,
    selected: Option<usize>,
    style: MessageStyle,
    in_thread: bool,
) -> Rendered {
    let width = width as usize;
    let today = Local::now().date_naive();
    let mut lines = Vec::new();
    let mut selected_rows = None;
    let mut anchors = Vec::new();
    let mut previews = Vec::new();
    let mut previous: Option<&Message> = None;
    let avatars = app.settings.avatars != AvatarStyle::Off && style != MessageStyle::Stream;

    for (index, message) in messages.iter().enumerate() {
        let day = local_time(message.epoch_seconds()).date_naive();
        if previous.is_none_or(|p| local_time(p.epoch_seconds()).date_naive() != day) {
            lines.push(separator(
                &day_label(day, today),
                width,
                theme.muted,
                theme.border,
            ));
            previous = None;
        }
        let grouped = previous.is_some_and(|p| {
            same_author(p, message)
                && message.epoch_seconds() - p.epoch_seconds() < GROUPING_SECONDS
                && !is_system(p)
                && !is_system(message)
        });
        let is_selected = selected == Some(index);
        let start = lines.len();
        let piece = MessagePiece {
            app,
            theme,
            message,
            grouped,
            is_selected,
            in_thread,
            avatars,
            width,
        };
        let header = match style {
            MessageStyle::Panels => piece.panels(&mut lines, &mut previews),
            MessageStyle::Stream => {
                piece.stream(&mut lines, &mut previews);
                None
            }
            MessageStyle::Focus => piece.focus(&mut lines, previous.is_some(), &mut previews),
        };
        if avatars && let (Some(line), Some(user)) = (header, &message.user) {
            anchors.push((line, user.clone()));
        }
        if is_selected {
            for line in &mut lines[start..] {
                line.style = line.style.bg(theme.surface);
            }
            selected_rows = Some((start, lines.len()));
        }
        previous = Some(message);
        if in_thread && index == 0 {
            let count = message.reply_count.max(messages.len() as u32 - 1);
            let label = format!("{count} repl{}", if count == 1 { "y" } else { "ies" });
            lines.push(separator(&label, width, theme.muted, theme.border));
            previous = None;
        }
    }
    Rendered {
        lines,
        avatars: anchors,
        previews,
        selected: selected_rows,
    }
}

struct MessagePiece<'a> {
    app: &'a App,
    theme: &'a Theme,
    message: &'a Message,
    grouped: bool,
    is_selected: bool,
    /// Inside the thread view, where the reply count would be redundant.
    in_thread: bool,
    /// Headers start with the author's picture, or their initials.
    avatars: bool,
    width: usize,
}

impl MessagePiece<'_> {
    fn author(&self) -> Span<'static> {
        let key = self
            .message
            .user
            .clone()
            .unwrap_or_else(|| self.app.author(self.message));
        Span::styled(
            self.app.author(self.message),
            Style::new().fg(self.theme.nick(&key)).bold(),
        )
    }

    fn time(&self) -> Span<'static> {
        let time = time_of_day(
            local_time(self.message.epoch_seconds()),
            self.app.settings.clock,
        );
        Span::styled(time, Style::new().fg(self.theme.muted))
    }

    fn marker(&self, show: bool) -> Span<'static> {
        if self.is_selected && show {
            Span::styled("▌ ", Style::new().fg(self.theme.accent))
        } else {
            Span::raw("  ")
        }
    }

    fn body(&self) -> Vec<Styled> {
        let theme = self.theme;
        let muted = Style::new().fg(theme.muted);
        let base = if is_system(self.message) {
            muted.italic()
        } else {
            Style::new().fg(theme.fg)
        };
        let names = |id: &str| self.app.users.get(id).map(|u| u.display_name().to_string());
        let mut body: Vec<Styled> = mrkdwn::parse(&self.message.text, &self.app.my_id, names)
            .into_iter()
            .map(|segment| {
                let style = match segment.kind {
                    Kind::Text => base,
                    Kind::Mention { me: true } => Style::new().fg(theme.accent).bold(),
                    Kind::Mention { me: false } => Style::new().fg(theme.link).bold(),
                    Kind::Channel => Style::new().fg(theme.link),
                    Kind::Link => Style::new().fg(theme.link).underlined(),
                    Kind::Code => Style::new().fg(theme.fg).bg(theme.panel),
                    Kind::Emoji => muted,
                };
                (segment.text, style)
            })
            .collect();
        for file in &self.message.files {
            let separator = if body.is_empty() { "" } else { "\n" };
            body.push((format!("{separator}{}", file_label(file)), muted));
        }
        for video in media::video_links(self.message) {
            let separator = if body.is_empty() { "" } else { "\n" };
            body.push((
                format!("{separator}▶ {}", video.service),
                Style::new().fg(theme.accent).bold(),
            ));
            let title = video.title.as_deref().unwrap_or(&video.url);
            let by = video.author.map(|a| format!(" — {a}")).unwrap_or_default();
            body.push((format!(" · {title}{by}"), Style::new().fg(theme.link)));
        }
        if body.is_empty() {
            body.push(("[content not shown]".into(), muted.italic()));
        }
        if self.message.is_edited() {
            body.push((" (edited)".into(), muted));
        }
        body
    }

    /// Leaves room under the body for each file's preview, drawn over these
    /// rows once downloaded.
    fn previews(
        &self,
        lines: &mut Vec<Line<'static>>,
        prefix: impl Fn() -> Vec<Span<'static>>,
        column: usize,
        anchors: &mut Vec<PreviewAnchor>,
    ) {
        if !self.app.graphics {
            return;
        }
        for preview in media::previews(self.message) {
            let size = preview_size(preview.dimensions, self.width.saturating_sub(column));
            anchors.push(PreviewAnchor {
                line: lines.len(),
                column: column as u16,
                size,
                key: preview.key,
            });
            for _ in 0..size.height {
                lines.push(Line::from(prefix()));
            }
        }
    }

    /// Reaction chips and the thread indicator, one row each.
    fn extras(&self) -> Vec<Vec<Span<'static>>> {
        let theme = self.theme;
        let mut rows = Vec::new();
        if !self.message.reactions.is_empty() {
            let mut chips = Vec::new();
            for reaction in &self.message.reactions {
                let mine = reaction.users.contains(&self.app.my_id);
                let fg = if mine { theme.accent } else { theme.muted };
                let glyph =
                    emoji::lookup(&reaction.name).unwrap_or_else(|| format!(":{}:", reaction.name));
                chips.push(Span::styled(
                    format!(" {glyph} {} ", reaction.count),
                    Style::new().fg(fg).bg(theme.panel),
                ));
                chips.push(Span::raw(" "));
            }
            rows.push(chips);
        }
        if self.message.has_thread() && !self.in_thread {
            let count = self.message.reply_count;
            let replies = if count == 1 { "reply" } else { "replies" };
            rows.push(vec![Span::styled(
                format!("↳ {count} {replies}"),
                Style::new().fg(theme.link),
            )]);
        }
        rows
    }

    /// The author's initials on their color, under where the picture goes.
    fn badge(&self) -> Span<'static> {
        let name = self.app.author(self.message);
        let mut words = name.split_whitespace();
        let initials: String = match (words.next(), words.next()) {
            (Some(first), Some(second)) => first
                .chars()
                .take(1)
                .chain(second.chars().take(1))
                .collect(),
            (Some(only), None) => only.chars().take(2).collect(),
            _ => "?".to_string(),
        };
        let key = self.message.user.clone().unwrap_or_else(|| name.clone());
        Span::styled(
            format!(" {} ", align_left(&initials.to_uppercase(), 2)),
            Style::new()
                .fg(self.theme.bg)
                .bg(self.theme.nick(&key))
                .bold(),
        )
    }

    /// Pushes the message; returns the row of its header, if it has one.
    fn panels(
        &self,
        lines: &mut Vec<Line<'static>>,
        previews: &mut Vec<PreviewAnchor>,
    ) -> Option<usize> {
        if self.avatars {
            return self.with_avatar(lines, false, previews);
        }
        const PREFIX: usize = 2 + 5 + 2;
        let pad = || Span::raw(" ".repeat(5 + 2));
        let header = (!self.grouped).then(|| {
            lines.push(Line::from(vec![
                self.marker(true),
                self.time(),
                Span::raw("  "),
                self.author(),
            ]));
            lines.len() - 1
        });
        let body = wrap(&self.body(), self.width.saturating_sub(PREFIX));
        for (i, row) in body.into_iter().enumerate() {
            let mut spans = vec![self.marker(self.grouped && i == 0), pad()];
            spans.extend(to_spans(row));
            lines.push(Line::from(spans));
        }
        self.previews(lines, || vec![self.marker(false), pad()], PREFIX, previews);
        for extra in self.extras() {
            let mut spans = vec![self.marker(false), pad()];
            spans.extend(extra);
            lines.push(Line::from(spans));
        }
        header
    }

    /// Picture (or initials) on the left of the header and first body row,
    /// author then time on the header, body below.
    fn with_avatar(
        &self,
        lines: &mut Vec<Line<'static>>,
        follows_message: bool,
        previews: &mut Vec<PreviewAnchor>,
    ) -> Option<usize> {
        const PREFIX: usize = 2 + 4 + 1;
        let pad = || Span::raw(" ".repeat(4 + 1));
        let header = (!self.grouped).then(|| {
            if follows_message {
                lines.push(Line::default());
            }
            lines.push(Line::from(vec![
                self.marker(true),
                self.badge(),
                Span::raw(" "),
                self.author(),
                Span::raw("  "),
                self.time(),
            ]));
            lines.len() - 1
        });
        let body = wrap(&self.body(), self.width.saturating_sub(PREFIX));
        for (i, row) in body.into_iter().enumerate() {
            let mut spans = vec![self.marker(self.grouped && i == 0), pad()];
            spans.extend(to_spans(row));
            lines.push(Line::from(spans));
        }
        self.previews(lines, || vec![self.marker(false), pad()], PREFIX, previews);
        for extra in self.extras() {
            let mut spans = vec![self.marker(false), pad()];
            spans.extend(extra);
            lines.push(Line::from(spans));
        }
        header
    }

    fn stream(&self, lines: &mut Vec<Line<'static>>, previews: &mut Vec<PreviewAnchor>) {
        const NICK: usize = 16;
        let time_column = time_width(self.app.settings.clock);
        let prefix = time_column + 1 + NICK + 3;
        let bar = || Span::styled(" │ ", Style::new().fg(self.theme.border));
        let blank = || Span::raw(" ".repeat(time_column + 1 + NICK));
        let author = self.author();
        let time = self.time();
        let head = vec![
            Span::styled(align_right(&time.content, time_column), time.style),
            Span::raw(" "),
            Span::styled(align_right(&author.content, NICK), author.style),
            bar(),
        ];

        let body = wrap(&self.body(), self.width.saturating_sub(prefix));
        for (i, row) in body.into_iter().enumerate() {
            let mut spans = if i == 0 {
                head.clone()
            } else {
                vec![blank(), bar()]
            };
            spans.extend(to_spans(row));
            lines.push(Line::from(spans));
        }
        self.previews(lines, || vec![blank(), bar()], prefix, previews);
        for extra in self.extras() {
            let mut spans = vec![blank(), bar()];
            spans.extend(extra);
            lines.push(Line::from(spans));
        }
    }

    fn focus(
        &self,
        lines: &mut Vec<Line<'static>>,
        follows_message: bool,
        previews: &mut Vec<PreviewAnchor>,
    ) -> Option<usize> {
        if self.avatars {
            return self.with_avatar(lines, follows_message, previews);
        }
        let header = (!self.grouped).then(|| {
            if follows_message {
                lines.push(Line::default());
            }
            lines.push(Line::from(vec![
                self.marker(true),
                self.author(),
                Span::raw("  "),
                self.time(),
            ]));
            lines.len() - 1
        });
        let body = wrap(&self.body(), self.width.saturating_sub(2));
        for (i, row) in body.into_iter().enumerate() {
            let mut spans = vec![self.marker(self.grouped && i == 0)];
            spans.extend(to_spans(row));
            lines.push(Line::from(spans));
        }
        self.previews(lines, || vec![self.marker(false)], 2, previews);
        for extra in self.extras() {
            let mut spans = vec![self.marker(false)];
            spans.extend(extra);
            lines.push(Line::from(spans));
        }
        header
    }
}

/// `▶ demo.mp4 · 0:19 · 10,7 Mo`
fn file_label(file: &File) -> String {
    let (icon, details) = match file.kind() {
        FileKind::Video => ("▶", file.duration_ms.map(format_duration)),
        FileKind::Image => (
            "▣",
            file.original_w
                .zip(file.original_h)
                .map(|(w, h)| format!("{w}×{h}")),
        ),
        FileKind::Other => ("▤", None),
    };
    let size = (file.size > 0).then(|| format_size(file.size));
    let mut label = format!("{icon} {}", file.name);
    for detail in details.into_iter().chain(size) {
        label.push_str(" · ");
        label.push_str(&detail);
    }
    label
}

/// At most 36 columns wide and 12 rows tall, keeping the picture's shape
/// (a cell is about twice as tall as it is wide).
fn preview_size(dimensions: Option<(u32, u32)>, available: usize) -> Size {
    let (w, h) = dimensions.unwrap_or((16, 9));
    let (w, h) = (w as f32, h as f32);
    let max_width = available.clamp(8, 36) as f32;
    let height = (max_width * h / w / 2.0).round().clamp(3.0, 12.0);
    let width = (height * 2.0 * w / h).round().clamp(4.0, max_width);
    Size::new(width as u16, height as u16)
}

fn to_spans(row: Vec<Styled>) -> Vec<Span<'static>> {
    row.into_iter()
        .map(|(text, style)| Span::styled(text, style))
        .collect()
}

fn same_author(a: &Message, b: &Message) -> bool {
    (&a.user, &a.username) == (&b.user, &b.username)
}

fn is_system(message: &Message) -> bool {
    matches!(
        message.subtype.as_deref(),
        Some(
            "channel_join" | "channel_leave" | "channel_topic" | "channel_purpose" | "channel_name"
        )
    )
}

/// A centered label between two rules: `──── Today ────`.
pub fn separator(label: &str, width: usize, text: Color, rule: Color) -> Line<'static> {
    let label = format!(" {label} ");
    let rest = width.saturating_sub(label.width());
    let left = rest / 2;
    Line::from(vec![
        Span::styled("─".repeat(left), Style::new().fg(rule)),
        Span::styled(label, Style::new().fg(text)),
        Span::styled("─".repeat(rest - left), Style::new().fg(rule)),
    ])
}

/// Draws the message list anchored to the bottom, scrolled so the selected
/// message stays visible.
/// The rows of a rendered message list that made it on screen.
pub struct Viewport {
    /// Index of the first row shown.
    pub first: usize,
    pub shown: usize,
    /// Screen row of the first row shown.
    pub top: u16,
}

/// Draws the message list anchored to the bottom, scrolled so the selected
/// message stays visible, then the authors' pictures over their initials.
pub fn render_messages(
    frame: &mut Frame,
    area: Rect,
    mut rendered: Rendered,
    theme: &Theme,
    loading: bool,
    app: &App,
    pictures: &mut Pictures,
) {
    let avatars = std::mem::take(&mut rendered.avatars);
    let previews = std::mem::take(&mut rendered.previews);
    let Some(viewport) = render_lines(frame, area, rendered, theme, loading) else {
        return;
    };
    // Only pictures entirely on screen are drawn.
    let spot = |line: usize, column: u16, size: Size| {
        let fits = line >= viewport.first
            && line + size.height as usize <= viewport.first + viewport.shown;
        fits.then(|| Rect {
            x: area.x + column,
            y: viewport.top + (line - viewport.first) as u16,
            width: size.width,
            height: size.height,
        })
    };
    if app.settings.avatars == AvatarStyle::Images {
        for (line, user) in avatars {
            if let Some(area) = spot(line, 2, AVATAR_SIZE) {
                pictures.draw(frame, area, &user, &app.pictures);
            }
        }
    }
    for preview in previews {
        if let Some(area) = spot(preview.line, preview.column, preview.size) {
            pictures.draw(frame, area, &preview.key, &app.pictures);
        }
    }
}

fn render_lines(
    frame: &mut Frame,
    area: Rect,
    rendered: Rendered,
    theme: &Theme,
    loading: bool,
) -> Option<Viewport> {
    if loading && rendered.lines.is_empty() {
        let text = Line::styled("loading…", Style::new().fg(theme.muted)).centered();
        frame.render_widget(
            Paragraph::new(text),
            area.centered_vertically(Constraint::Length(1)),
        );
        return None;
    }
    if rendered.lines.is_empty() {
        let text = Line::styled("No messages yet.", Style::new().fg(theme.muted)).centered();
        frame.render_widget(
            Paragraph::new(text),
            area.centered_vertically(Constraint::Length(1)),
        );
        return None;
    }

    let height = area.height as usize;
    let total = rendered.lines.len();
    let mut start = total.saturating_sub(height);
    if let Some((first, end)) = rendered.selected {
        if first < start {
            start = first;
        } else if end > start + height {
            start = end.saturating_sub(height);
        }
    }
    let visible: Vec<Line> = rendered
        .lines
        .into_iter()
        .skip(start)
        .take(height)
        .collect();
    let offset = (height - visible.len()) as u16;
    let target = Rect {
        y: area.y + offset,
        height: visible.len() as u16,
        ..area
    };
    let shown = visible.len();
    frame.render_widget(Paragraph::new(visible), target);
    Some(Viewport {
        first: start,
        shown,
        top: target.y,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarStyle {
    /// Channels and direct messages under section headers (layout A).
    Sections,
    /// A numbered buffer list (layout B).
    Numbered,
}

pub fn sidebar(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    theme: &Theme,
    focused: bool,
    style: SidebarStyle,
) {
    if !app.channels_loaded {
        let text = Line::styled("loading…", Style::new().fg(theme.muted));
        frame.render_widget(Paragraph::new(text), area);
        return;
    }
    let [tabs_area, area] =
        Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(area);
    frame.render_widget(
        Paragraph::new(tab_bar(app, theme, tabs_area.width as usize)),
        tabs_area,
    );

    let width = area.width as usize;
    let today = Local::now().date_naive();
    let sidebar_rows = app.sidebar_rows();
    let cursor_row = app.sidebar_position(&app.sidebar_items());
    let mut rows: Vec<Line> = Vec::with_capacity(sidebar_rows.len());
    let mut number = 0;

    for (row_index, row) in sidebar_rows.into_iter().enumerate() {
        let is_cursor = focused && row_index == cursor_row;
        let line = match row {
            SidebarRow::Section {
                index,
                collapsed,
                hidden,
            } => section_row(
                &app.sections[index],
                theme,
                width,
                collapsed,
                hidden,
                is_cursor,
            ),
            SidebarRow::Channel(index) => {
                number += 1;
                let channel = &app.channels[index];
                let number = (style == SidebarStyle::Numbered).then_some(number);
                // The private tab shows when each conversation was last active.
                let when = (app.sidebar_tab == SidebarTab::Direct)
                    .then(|| epoch_of(&channel.latest))
                    .flatten()
                    .map(|epoch| recency_label(local_time(epoch), today, app.settings.clock));
                channel_row(app, theme, channel, width, is_cursor, number, when)
            }
        };
        rows.push(line);
    }

    let height = area.height as usize;
    let start = cursor_row
        .saturating_sub(height.saturating_sub(2))
        .min(rows.len().saturating_sub(height));
    let visible: Vec<Line> = rows.into_iter().skip(start).take(height).collect();
    frame.render_widget(Paragraph::new(visible), area);
}

fn epoch_of(ts: &str) -> Option<i64> {
    ts.split('.').next()?.parse().ok().filter(|&secs| secs > 0)
}

/// `Channels │ DMs`, the active tab underlined, the other one badged when
/// it holds unread conversations.
fn tab_bar(app: &App, theme: &Theme, width: usize) -> Vec<Line<'static>> {
    let mut labels = Vec::new();
    let mut rule = Vec::new();
    let mut used = 0;
    for (tab, name) in [
        (SidebarTab::Channels, "Channels"),
        (SidebarTab::Direct, "DMs"),
    ] {
        if tab == SidebarTab::Direct {
            labels.push(Span::styled(" │", Style::new().fg(theme.border)));
            rule.push(Span::styled("─".repeat(2), Style::new().fg(theme.border)));
            used += 2;
        }
        let active = app.sidebar_tab == tab;
        let (unread, mentions) = app.tab_activity(tab);
        let badge = match (active, mentions, unread) {
            (true, _, _) | (false, 0, false) => String::new(),
            (false, 0, true) => " •".to_string(),
            (false, count, _) => format!(" {count}"),
        };
        let label = format!(" {name}");
        let label_width = label.width() + badge.width();
        let style = if active {
            Style::new().fg(theme.accent).bold()
        } else {
            Style::new().fg(theme.muted)
        };
        labels.push(Span::styled(label, style));
        labels.push(Span::styled(badge, Style::new().fg(theme.unread).bold()));
        let rule_color = if active { theme.accent } else { theme.border };
        rule.push(Span::styled(
            "─".repeat(label_width),
            Style::new().fg(rule_color),
        ));
        used += label_width;
    }
    rule.push(Span::styled(
        "─".repeat(width.saturating_sub(used)),
        Style::new().fg(theme.border),
    ));
    vec![Line::from(labels), Line::from(rule)]
}

fn section_row(
    section: &Section,
    theme: &Theme,
    width: usize,
    collapsed: bool,
    hidden: usize,
    is_cursor: bool,
) -> Line<'static> {
    let arrow = if collapsed { "▸" } else { "▾" };
    let marker = if is_cursor { "›" } else { " " };
    let title = match &section.emoji {
        Some(emoji) => format!("{emoji} {}", section.name),
        None => section.name.clone(),
    };
    let count = if hidden > 0 {
        format!(" {hidden}")
    } else {
        String::new()
    };
    let room = width.saturating_sub(4 + count.width());
    let line = Line::from(vec![
        Span::styled(format!("{marker} "), Style::new().fg(theme.accent)),
        Span::styled(format!("{arrow} "), Style::new().fg(theme.muted)),
        Span::styled(truncate(&title, room), Style::new().fg(theme.muted).bold()),
        Span::styled(count, Style::new().fg(theme.border)),
    ]);
    if is_cursor {
        line.style(Style::new().bg(theme.surface))
    } else {
        line
    }
}

fn channel_row(
    app: &App,
    theme: &Theme,
    channel: &Channel,
    width: usize,
    is_cursor: bool,
    number: Option<usize>,
    when: Option<String>,
) -> Line<'static> {
    let is_current = app.current.as_deref() == Some(channel.id.as_str());
    let prefix = match channel.kind {
        ChannelKind::Public => "#",
        ChannelKind::Private => "◇",
        ChannelKind::Direct => "@",
        ChannelKind::Group => "&",
    };
    let (badge, badge_style) = if channel.mentions > 0 {
        let text = if channel.is_direct() {
            channel.mentions.to_string()
        } else {
            format!("@{}", channel.mentions)
        };
        let color = if channel.is_direct() {
            theme.unread
        } else {
            theme.accent
        };
        (text, Style::new().fg(color).bold())
    } else if channel.unread {
        ("•".to_string(), Style::new().fg(theme.unread))
    } else {
        (when.unwrap_or_default(), Style::new().fg(theme.muted))
    };
    let name_style = if is_current {
        Style::new().fg(theme.accent).bold()
    } else if channel.unread {
        Style::new().fg(theme.fg).bold()
    } else {
        Style::new().fg(theme.muted)
    };

    let mut spans = vec![if is_cursor {
        Span::styled("› ", Style::new().fg(theme.accent))
    } else {
        Span::raw("  ")
    }];
    match number {
        Some(number) => spans.push(Span::styled(
            format!("{number:>2} "),
            Style::new().fg(theme.muted),
        )),
        // Indented under the section headers of the channels tab.
        None if app.sidebar_tab == SidebarTab::Channels => spans.push(Span::raw("  ")),
        None => {}
    }
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    // Keep a space between a long name and its badge.
    let spacing = usize::from(!badge.is_empty());
    let room = width.saturating_sub(used + spacing + badge.width() + 1);
    let name = truncate(&format!("{prefix} {}", app.channel_name(channel)), room);
    let gap = width.saturating_sub(used + name.width() + badge.width() + 1);
    spans.push(Span::styled(name, name_style));
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(badge, badge_style));

    let line = Line::from(spans);
    if is_cursor {
        line.style(Style::new().bg(theme.surface))
    } else {
        line
    }
}

#[derive(Debug, Clone)]
pub enum InputStyle {
    /// A rounded box with a title (layout A).
    Boxed { title: &'static str },
    /// A bare line after a prompt (layouts B and C).
    Prompt { prompt: String },
}

impl InputStyle {
    fn chrome(&self) -> (u16, u16) {
        match self {
            Self::Boxed { .. } => (4, 2),
            Self::Prompt { prompt } => (prompt.width() as u16, 0),
        }
    }
}

pub fn input_height(app: &App, width: u16, style: &InputStyle) -> u16 {
    let (chrome_w, chrome_h) = style.chrome();
    let (rows, _) = app.input.layout(width.saturating_sub(chrome_w));
    rows.len().clamp(1, MAX_INPUT_ROWS) as u16 + chrome_h
}

/// Draws the composer. Only the box that receives typed text (`is_target`)
/// shows the draft; the others show `placeholder`.
pub fn input_box(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    theme: &Theme,
    style: &InputStyle,
    is_target: bool,
    placeholder: &str,
) {
    let active = is_target && app.mode == Mode::Insert;
    let inner = match style {
        InputStyle::Boxed { title } => {
            let block = Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(if active { theme.accent } else { theme.border }))
                .title(Line::styled(
                    format!(" {title} "),
                    Style::new().fg(if active { theme.accent } else { theme.muted }),
                ))
                .padding(Padding::horizontal(1));
            let inner = block.inner(area);
            frame.render_widget(block, area);
            inner
        }
        InputStyle::Prompt { prompt } => {
            let [prompt_area, inner] = Layout::horizontal([
                Constraint::Length(prompt.width() as u16),
                Constraint::Fill(1),
            ])
            .areas(area);
            let color = if active { theme.accent } else { theme.muted };
            frame.render_widget(
                Paragraph::new(Line::styled(prompt.clone(), Style::new().fg(color))),
                prompt_area,
            );
            inner
        }
    };

    if !is_target || (app.input.is_empty() && !active) {
        let text = Line::styled(placeholder.to_string(), Style::new().fg(theme.muted));
        frame.render_widget(Paragraph::new(text), inner);
        return;
    }

    let (rows, (cursor_row, cursor_col)) = app.input.layout(inner.width);
    let visible = inner.height as usize;
    let start = (cursor_row as usize + 1).saturating_sub(visible);
    let lines: Vec<Line> = rows
        .into_iter()
        .skip(start)
        .take(visible)
        .map(|row| Line::styled(row, Style::new().fg(theme.fg)))
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
    if active && app.overlay.is_none() {
        frame.set_cursor_position((inner.x + cursor_col, inner.y + cursor_row - start as u16));
    }
}

pub fn panel(theme: &Theme, title: String, right: Option<String>, focused: bool) -> Block<'static> {
    let color = if focused { theme.accent } else { theme.border };
    let title_style = if focused {
        Style::new().fg(theme.accent).bold()
    } else {
        Style::new().fg(theme.muted)
    };
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(color))
        .title(Line::styled(format!(" {title} "), title_style));
    if let Some(right) = right {
        block = block.title(
            Line::styled(format!(" {right} "), Style::new().fg(theme.muted)).right_aligned(),
        );
    }
    block
}

pub fn status_bar(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    theme: &Theme,
    background: Option<Color>,
) {
    let muted = Style::new().fg(theme.muted);
    let (mode, mode_bg) = match app.mode {
        Mode::Normal => (" NORMAL ", theme.accent),
        Mode::Insert => (" INSERT ", theme.link),
    };
    let mut crumbs = format!("  {}", app.team_name);
    if let Some(channel) = app.current_channel() {
        crumbs.push_str(&format!(" › {}", app.channel_label(channel)));
    }
    if app.thread.is_some() {
        crumbs.push_str(" › thread");
    }
    let left = vec![
        Span::styled(mode, Style::new().fg(theme.on_accent).bg(mode_bg).bold()),
        Span::styled(crumbs, muted),
    ];
    let left_width: usize = left.iter().map(|s| s.content.width()).sum();

    let connection = match &app.connection {
        Connection::Connected => vec![
            Span::styled("●", Style::new().fg(theme.success)),
            Span::styled(" connected", muted),
        ],
        Connection::Connecting => {
            vec![Span::styled("◌ connecting…", Style::new().fg(theme.accent))]
        }
        Connection::Reconnecting { in_secs, reason } => vec![Span::styled(
            format!("◌ reconnecting in {in_secs}s ({reason})"),
            Style::new().fg(theme.accent),
        )],
    };
    let connection_width: usize = connection.iter().map(|s| s.content.width()).sum();
    let room = (area.width as usize).saturating_sub(left_width + connection_width + 3);

    let mut right: Vec<Span> = Vec::new();
    if let Some(notice) = &app.notice {
        right.push(Span::styled(
            truncate(notice, room),
            Style::new().fg(theme.unread),
        ));
        right.push(Span::raw("   "));
    } else {
        let mut used = 0;
        for (key, label) in hints(app) {
            let width = key.width() + label.width() + 4;
            if used + width > room {
                break;
            }
            used += width;
            right.push(Span::styled(key, Style::new().fg(theme.fg)));
            right.push(Span::styled(format!(" {label}   "), muted));
        }
    }
    right.extend(connection);
    let right_width: usize = right.iter().map(|s| s.content.width()).sum();

    let style = background.map_or(Style::new(), |bg| Style::new().bg(bg));
    let [left_area, right_area] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(right_width as u16 + 1),
    ])
    .areas(area);
    frame.render_widget(Paragraph::new(Line::from(left)).style(style), left_area);
    frame.render_widget(Paragraph::new(Line::from(right)).style(style), right_area);
}

/// The keys that matter where the user is, as the keymap has them.
fn hints(app: &App) -> Vec<(String, &'static str)> {
    if app.mode == Mode::Insert {
        return [("⏎", "send"), ("⇧⏎", "new line"), ("esc", "done")]
            .map(|(key, label)| (key.to_string(), label))
            .into();
    }
    let keys = &app.keymap;
    let mut hints = match app.focus {
        Focus::Sidebar => vec![
            (keys.pair(Action::Down, Action::Up), "move"),
            (keys.hint(Action::Open), "open / fold"),
            (keys.pair(Action::Left, Action::Right), "channels / DMs"),
        ],
        Focus::Messages => vec![
            (keys.pair(Action::Down, Action::Up), "select"),
            (keys.hint(Action::React), "react"),
            (keys.hint(Action::OpenFiles), "open"),
            (keys.hint(Action::Play), "play"),
            (keys.hint(Action::Thread), "thread"),
            (keys.hint(Action::Write), "write"),
        ],
        Focus::Thread => vec![
            (keys.pair(Action::Down, Action::Up), "select"),
            (keys.hint(Action::React), "react"),
            (keys.hint(Action::OpenFiles), "open"),
            (keys.hint(Action::Play), "play"),
            (keys.hint(Action::Write), "reply"),
            (Some("esc".to_string()), "close thread"),
        ],
    };
    hints.extend([
        (keys.hint(Action::GoTo), "go to"),
        (keys.hint(Action::Search), "search"),
        (keys.hint(Action::Settings), "settings"),
        (keys.hint(Action::Help), "help"),
        (keys.hint(Action::Quit), "quit"),
    ]);
    hints
        .into_iter()
        .filter_map(|(key, label)| Some((key?, label)))
        .collect()
}

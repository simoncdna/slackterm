mod layouts;
mod overlays;
mod text;
mod widgets;

use ratatui::Frame;
use ratatui::style::Style;
use ratatui::widgets::Block;

use crate::config::Layout;

use super::state::{App, Overlay};
use super::theme::Theme;

pub fn draw(frame: &mut Frame, app: &App) {
    let theme = Theme::from_settings(&app.settings);
    frame.render_widget(
        Block::new().style(Style::new().bg(theme.bg).fg(theme.fg)),
        frame.area(),
    );
    match app.settings.layout {
        Layout::Panels => layouts::panels(frame, app, &theme),
        Layout::Stream => layouts::stream(frame, app, &theme),
        Layout::Focus => layouts::focus(frame, app, &theme),
    }
    match &app.overlay {
        Some(Overlay::Switcher { query, cursor }) => {
            overlays::switcher(frame, app, &theme, query, *cursor)
        }
        Some(Overlay::Settings { row }) => overlays::settings(
            frame,
            &theme,
            &app.settings,
            app.settings_path.as_deref(),
            *row,
        ),
        Some(Overlay::Search(search)) => overlays::search(frame, app, &theme, search),
        Some(Overlay::Emoji(picker)) => overlays::emoji_picker(frame, app, &theme, picker),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::config::Choice;
    use crate::slack::test_message as message;
    use crate::tui::state::fixtures::*;
    use crate::tui::state::{Focus, Mode};
    use crate::tui::update::{ApiEvent, Event, update};

    fn screen(app: &App, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut text = String::new();
        for y in 0..buffer.area.height {
            let mut x = 0;
            while x < buffer.area.width {
                let symbol = buffer[(x, y)].symbol();
                text.push_str(symbol);
                // A wide character also covers the next cell.
                x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
            }
            text.push('\n');
        }
        text
    }

    fn sample_app() -> App {
        let mut app = app();
        app.users.insert("U2".into(), user("U2", "camille"));
        update(
            &mut app,
            Event::Api(ApiEvent::Conversations(vec![
                conversation("C1", "general"),
                conversation("C2", "deploys"),
                direct("D1", "U2"),
            ])),
        );
        let mut with_thread = message("1700000100.000000", "U2", "le déploiement est passé <@UME>");
        with_thread.reply_count = 2;
        update(
            &mut app,
            Event::Api(ApiEvent::History {
                channel: "C1".into(),
                messages: vec![message("1700000000.000000", "U2", "salut"), with_thread],
                has_more: false,
            }),
        );
        app
    }

    #[test]
    fn every_layout_shows_channels_and_messages() {
        let mut app = sample_app();
        for layout in Layout::ALL {
            app.settings.layout = *layout;
            let text = screen(&app, 120, 30);
            assert!(
                text.contains("le déploiement est passé"),
                "{layout:?}:\n{text}"
            );
            assert!(
                text.contains("@simon") || text.contains("@UME"),
                "{layout:?}:\n{text}"
            );
            assert!(text.contains("2 réponses"), "{layout:?}:\n{text}");
            assert!(text.contains("NORMAL"), "{layout:?}:\n{text}");
            if *layout != Layout::Focus {
                assert!(text.contains("deploys"), "{layout:?}:\n{text}");
            }
        }
    }

    #[test]
    fn overlays_render_on_top() {
        let mut app = sample_app();
        app.overlay = Some(Overlay::Settings { row: 1 });
        let text = screen(&app, 100, 30);
        assert!(text.contains("réglages"));
        assert!(text.contains("A · trois panneaux"));
        assert!(text.contains("Nuit"));

        app.overlay = Some(Overlay::Switcher {
            query: Default::default(),
            cursor: 0,
        });
        let text = screen(&app, 100, 30);
        assert!(text.contains("aller à"));
        assert!(text.contains("@camille"));
    }

    #[test]
    fn open_thread_and_insert_mode_render_in_every_layout() {
        let mut app = sample_app();
        app.focus = Focus::Messages;
        update(
            &mut app,
            Event::Key(crossterm::event::KeyEvent::from(
                crossterm::event::KeyCode::Char('t'),
            )),
        );
        app.mode = Mode::Insert;
        app.input.insert_str("une réponse en cours");
        for layout in Layout::ALL {
            app.settings.layout = *layout;
            let text = screen(&app, 120, 30);
            assert!(text.contains("une réponse en cours"), "{layout:?}:\n{text}");
            assert!(text.contains("INSERTION"), "{layout:?}:\n{text}");
        }
    }

    #[test]
    fn search_overlay_lists_results() {
        let mut app = sample_app();
        let found: crate::slack::SearchMatch = serde_json::from_value(serde_json::json!({
            "ts": "1700000100.000000",
            "user": "U2",
            "text": "le \u{e000}déploiement\u{e001} est passé :rocket:",
            "permalink": "https://x/p?thread_ts=1700000000.000000",
            "channel": {"id": "C1", "name": "general"}
        }))
        .unwrap();
        app.overlay = Some(Overlay::Search(crate::tui::state::Search {
            query: Default::default(),
            submitted: "déploiement".into(),
            status: crate::tui::state::SearchStatus::Done(vec![found]),
            cursor: 0,
        }));
        let text = screen(&app, 110, 30);
        assert!(text.contains("chercher dans les messages"), "{text}");
        assert!(text.contains("1 résultat"), "{text}");
        assert!(text.contains("#general · camille"), "{text}");
        assert!(text.contains("le déploiement est passé 🚀"), "{text}");
        assert!(text.contains("↳ fil"), "{text}");
    }

    #[test]
    fn sidebar_shows_sections_and_their_emoji() {
        let mut app = sample_app();
        app.set_sections(vec![
            serde_json::from_value(serde_json::json!({
                "channel_section_id": "S1",
                "type": "standard",
                "name": "Tooling",
                "emoji": "toolbox",
                "channel_ids_page": {"channel_ids": ["C2"]}
            }))
            .unwrap(),
        ]);
        let text = screen(&app, 120, 30);
        assert!(text.contains("▾ 🧰 Tooling"), "{text}");

        app.collapsed.insert("S1".into());
        let text = screen(&app, 120, 30);
        assert!(text.contains("▸ 🧰 Tooling 1"), "{text}");
    }

    #[test]
    fn sidebar_tabs_switch_to_private_messages() {
        let mut app = sample_app();
        app.channel_mut("D1").unwrap().mentions = 2;
        let text = screen(&app, 120, 30);
        assert!(text.contains("Canaux │ Privés 2"), "{text}");

        app.sidebar_tab = crate::config::SidebarTab::Direct;
        app.channel_mut("D1").unwrap().mentions = 0;
        app.channel_mut("D1").unwrap().listed = true;
        app.channel_mut("D1").unwrap().latest = "1700000000.000000".into();
        let text = screen(&app, 120, 30);
        assert!(text.contains("@ camille"), "{text}");
        assert!(!text.contains("# deploys"), "{text}");
    }

    #[test]
    fn status_hints_never_cover_the_breadcrumb() {
        let mut app = sample_app();
        for width in [80, 100, 120, 160] {
            for focus in [Focus::Sidebar, Focus::Messages] {
                app.focus = focus;
                let text = screen(&app, width, 20);
                let status = text.lines().last().unwrap();
                assert!(
                    status.contains("acme › #general "),
                    "{width} {focus:?}: {status}"
                );
            }
        }
    }

    #[test]
    fn the_emoji_picker_shows_glyphs_and_own_reactions() {
        let mut app = sample_app();
        let me = app.my_id.clone();
        app.histories.get_mut("C1").unwrap().messages[1].add_reaction("+1", &me);
        app.focus = Focus::Messages;
        update(
            &mut app,
            Event::Key(crossterm::event::KeyEvent::from(
                crossterm::event::KeyCode::Char('r'),
            )),
        );
        let text = screen(&app, 100, 30);
        assert!(text.contains("réagir"), "{text}");
        assert!(text.contains("message de camille"), "{text}");
        assert!(text.contains("👍 :+1:  ✓ déjà mise"), "{text}");
    }

    /// `cargo test preview -- --ignored --nocapture` prints every layout.
    #[test]
    #[ignore = "aperçu visuel, à lancer à la main"]
    fn preview() {
        let mut app = sample_app();
        app.focus = Focus::Messages;
        update(
            &mut app,
            Event::Key(crossterm::event::KeyEvent::from(
                crossterm::event::KeyCode::Char('t'),
            )),
        );
        for layout in Layout::ALL {
            app.settings.layout = *layout;
            println!("{layout:?}\n{}", screen(&app, 110, 22));
        }
    }

    #[test]
    fn survives_tiny_terminals() {
        let mut app = sample_app();
        app.overlay = Some(Overlay::Settings { row: 0 });
        for layout in Layout::ALL {
            app.settings.layout = *layout;
            screen(&app, 20, 6);
            screen(&app, 1, 1);
        }
    }
}

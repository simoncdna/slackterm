mod layouts;
mod overlays;
mod text;
mod widgets;

use ratatui::Frame;
use ratatui::style::Style;
use ratatui::widgets::Block;

use crate::config::Layout;

use super::pictures::Pictures;
use super::state::{App, Overlay};
use super::theme::Theme;

pub fn draw(frame: &mut Frame, app: &App, pictures: &mut Pictures) {
    let theme = Theme::from_settings(&app.settings);
    frame.render_widget(
        Block::new().style(Style::new().bg(theme.bg).fg(theme.fg)),
        frame.area(),
    );
    match app.settings.layout {
        Layout::Panels => layouts::panels(frame, app, &theme, pictures),
        Layout::Stream => layouts::stream(frame, app, &theme, pictures),
        Layout::Focus => layouts::focus(frame, app, &theme, pictures),
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
            &pictures.describe(),
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
        terminal
            .draw(|frame| draw(frame, app, &mut Pictures::disabled()))
            .unwrap();
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

    #[test]
    fn headers_start_with_initials_unless_avatars_are_off() {
        let mut app = sample_app();
        let text = screen(&app, 120, 30);
        assert!(text.contains(" CA  camille  23:13"), "{text}");

        app.settings.avatars = crate::config::AvatarStyle::Off;
        let text = screen(&app, 120, 30);
        assert!(text.contains("23:13  camille"), "{text}");
        assert!(!text.contains(" CA "), "{text}");
    }

    #[test]
    fn the_settings_list_avatars() {
        let mut app = sample_app();
        app.overlay = Some(Overlay::Settings { row: 3 });
        let text = screen(&app, 100, 30);
        assert!(text.contains("Avatars"), "{text}");
        assert!(text.contains("‹ photos ›  terminal sans images"), "{text}");
    }

    fn app_with_video() -> App {
        let mut app = sample_app();
        let video: crate::slack::File = serde_json::from_value(serde_json::json!({
            "id": "F1", "name": "demo.mp4", "mimetype": "video/mp4", "size": 10747897,
            "duration_ms": 19686, "thumb_video": "https://files.slack.com/thumb.jpg"
        }))
        .unwrap();
        app.histories.get_mut("C1").unwrap().messages[1]
            .files
            .push(video);
        app
    }

    #[test]
    fn files_show_their_kind_duration_and_size() {
        let app = app_with_video();
        let text = screen(&app, 120, 30);
        assert!(text.contains("▶ demo.mp4 · 0:19 · 10,7 Mo"), "{text}");
    }

    #[test]
    fn previews_get_room_only_when_pictures_can_be_drawn() {
        let mut app = app_with_video();
        let theme = crate::tui::theme::Theme::from_settings(&app.settings);
        let messages = app.current_messages().to_vec();
        let style = widgets::MessageStyle::Panels;
        let without = widgets::message_lines(&app, &theme, &messages, 80, None, style, false);
        assert!(without.previews.is_empty());

        app.graphics = true;
        let with = widgets::message_lines(&app, &theme, &messages, 80, None, style, false);
        let [preview] = &with.previews[..] else {
            panic!("one preview expected");
        };
        assert_eq!(preview.key, "file:F1");
        assert_eq!((preview.size.width, preview.size.height), (36, 10));
        assert_eq!(with.lines.len(), without.lines.len() + 10);
    }

    #[test]
    fn youtube_links_show_a_card_with_room_for_the_thumbnail() {
        let mut app = sample_app();
        let msg = &mut app.histories.get_mut("C1").unwrap().messages[1];
        msg.text = "<https://youtu.be/dQw4w9WgXcQ>".into();
        msg.attachments.push(crate::slack::Attachment {
            service_name: Some("YouTube".into()),
            title: Some("Never Gonna Give You Up".into()),
            author_name: Some("Rick Astley".into()),
            original_url: Some("https://youtu.be/dQw4w9WgXcQ".into()),
            video_html: Some("<iframe>".into()),
            ..Default::default()
        });
        let text = screen(&app, 120, 40);
        assert!(
            text.contains("▶ YouTube · Never Gonna Give You Up — Rick Astley"),
            "{text}"
        );

        app.graphics = true;
        let theme = crate::tui::theme::Theme::from_settings(&app.settings);
        let messages = app.current_messages().to_vec();
        let rendered = widgets::message_lines(
            &app,
            &theme,
            &messages,
            100,
            None,
            widgets::MessageStyle::Panels,
            false,
        );
        assert_eq!(rendered.previews[0].key, "link:youtube:dQw4w9WgXcQ");
    }

    #[test]
    fn a_row_added_on_top_moves_the_pictures_down() {
        let mut app = app_with_video();
        app.graphics = true;
        let theme = crate::tui::theme::Theme::from_settings(&app.settings);
        let messages = app.current_messages().to_vec();
        let mut rendered = widgets::message_lines(
            &app,
            &theme,
            &messages,
            80,
            Some(1),
            widgets::MessageStyle::Panels,
            false,
        );
        let avatar = rendered.avatars[0].0;
        let preview = rendered.previews[0].line;
        let selected = rendered.selected.unwrap();

        rendered.push_front(ratatui::text::Line::raw("début de la conversation"));

        assert_eq!(rendered.avatars[0].0, avatar + 1);
        assert_eq!(rendered.previews[0].line, preview + 1);
        assert_eq!(rendered.selected, Some((selected.0 + 1, selected.1 + 1)));
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

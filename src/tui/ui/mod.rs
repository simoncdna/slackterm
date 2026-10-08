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
        Some(Overlay::Settings { row }) => overlays::settings(frame, &theme, &app.settings, *row),
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
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
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

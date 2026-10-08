mod executor;
mod fuzzy;
mod input;
mod state;
mod theme;
mod ui;
mod update;

use std::io::stdout;

use anyhow::{Context, Result};
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event as TermEvent, EventStream, KeyEventKind,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::supports_keyboard_enhancement;
use futures_util::StreamExt;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::config::{Settings, SidebarState};
use crate::session::Session;
use crate::slack::{DEFAULT_API_BASE, SlackClient, rtm};

use state::App;
use update::{Event, initial_commands, update};

pub async fn run(session: Session) -> Result<()> {
    let workspace = session
        .workspaces
        .first()
        .context("aucun workspace enregistré, relance `slackterm login`")?;
    let client = SlackClient::new(DEFAULT_API_BASE, &workspace.token, &session.cookie_d)?;
    let (settings, settings_error) = match Settings::load() {
        Ok(settings) => (settings, None),
        Err(e) => (Settings::default(), Some(format!("{e:#}"))),
    };
    let mut app = App::new(workspace, settings);
    app.notice = settings_error;
    let sidebar = SidebarState::load();
    app.collapsed = sidebar.collapsed;
    app.sidebar_tab = sidebar.tab;

    let (events_tx, mut events_rx) = mpsc::unbounded_channel();
    let (rtm_tx, mut rtm_rx) = mpsc::unbounded_channel();
    let rtm_task = rtm::spawn(client.clone(), rtm_tx);
    let forward = events_tx.clone();
    tokio::spawn(async move {
        while let Some(event) = rtm_rx.recv().await {
            if forward.send(Event::Rtm(event)).is_err() {
                return;
            }
        }
    });
    for command in initial_commands() {
        executor::execute(command, &client, &events_tx);
    }

    let mut terminal = ratatui::try_init()?;
    let _ = execute!(stdout(), EnableBracketedPaste);
    // Lets terminals that support it report Shift+Enter distinctly.
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }

    let result = event_loop(&mut terminal, &mut app, &mut events_rx, &client, &events_tx).await;

    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout(), DisableBracketedPaste);
    ratatui::restore();
    rtm_task.abort();
    result
}

async fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut App,
    events: &mut UnboundedReceiver<Event>,
    client: &SlackClient,
    events_tx: &UnboundedSender<Event>,
) -> Result<()> {
    let mut terminal_events = EventStream::new();
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        let event = tokio::select! {
            Some(event) = terminal_events.next() => match event? {
                TermEvent::Key(key) if key.kind != KeyEventKind::Release => Event::Key(key),
                TermEvent::Paste(text) => Event::Paste(text),
                _ => continue,
            },
            Some(event) = events.recv() => event,
            else => return Ok(()),
        };

        for command in update(app, event) {
            executor::execute(command, client, events_tx);
        }
        if app.should_quit {
            return Ok(());
        }
    }
}

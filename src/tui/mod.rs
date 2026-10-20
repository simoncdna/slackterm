mod demo;
mod executor;
mod fuzzy;
mod input;
mod pictures;
mod player;
mod state;
mod terminal_events;
mod theme;
mod ui;
mod update;

use std::io::stdout;

use anyhow::{Context, Result};
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event as TermEvent, KeyEventKind,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::supports_keyboard_enhancement;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc::{self, UnboundedReceiver};

use crate::config::{Settings, SidebarState};
use crate::session::Session;
use crate::slack::{DEFAULT_API_BASE, SlackClient, rtm};

use state::App;
use terminal_events::TerminalEvents;
use update::{Command, Event, initial_commands, update};

pub use pictures::reveal_tmux;

pub async fn run(session: Session) -> Result<()> {
    let workspace = session
        .workspaces
        .first()
        .context("no saved workspace, run `slackterm login` again")?;
    let client = SlackClient::new(DEFAULT_API_BASE, &workspace.token, &session.cookie_d)?;
    let (settings, settings_error) = match Settings::load() {
        Ok(settings) => (settings, None),
        Err(e) => (Settings::default(), Some(format!("{e:#}"))),
    };
    let mut app = App::new(workspace, settings);
    app.notice = settings_error;
    app.settings_path = Settings::path().ok();
    let sidebar = SidebarState::load();
    app.collapsed = sidebar.collapsed;
    app.sidebar_tab = sidebar.tab;

    let (events_tx, events_rx) = mpsc::unbounded_channel();
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

    let execute = |command| executor::execute(command, &client, &events_tx);
    let result = run_app(&mut app, events_rx, execute).await;
    rtm_task.abort();
    result
}

/// Opens the interface on a made-up workspace, without network access and
/// without reading or writing the user's settings.
pub async fn run_demo() -> Result<()> {
    let demo = demo::Demo::new();
    let mut app = App::new(&demo::workspace(), Settings::default());

    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let live = demo.spawn_live(events_tx.clone());
    let execute = |command| demo.execute(command, &events_tx);
    let result = run_app(&mut app, events_rx, execute).await;
    live.abort();
    result
}

/// Takes over the terminal until the user quits, handing every side effect
/// to `execute`, which reports back through `events`.
async fn run_app(
    app: &mut App,
    mut events: UnboundedReceiver<Event>,
    execute: impl Fn(Command),
) -> Result<()> {
    for command in initial_commands() {
        execute(command);
    }

    let mut terminal = ratatui::try_init()?;
    let mut enhanced = enter_interface_mode();
    let mut pictures = pictures::Pictures::detect();
    app.graphics = pictures.available();
    let mut screen = Screen {
        terminal: &mut terminal,
        enhanced: &mut enhanced,
        pictures: &mut pictures,
    };
    let result = event_loop(&mut screen, app, &mut events, &execute).await;

    leave_interface_mode(enhanced);
    ratatui::restore();
    result
}

/// The terminal while the interface owns it.
struct Screen<'a> {
    terminal: &'a mut DefaultTerminal,
    /// Keys are reported with their modifiers (Shift+Enter).
    enhanced: &'a mut bool,
    pictures: &'a mut pictures::Pictures,
}

/// Bracketed paste, and distinct Shift+Enter where the terminal supports
/// it. Returns whether the latter is on.
fn enter_interface_mode() -> bool {
    let _ = execute!(stdout(), EnableBracketedPaste);
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    enhanced
}

fn leave_interface_mode(enhanced: bool) {
    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout(), DisableBracketedPaste);
}

async fn event_loop(
    screen: &mut Screen<'_>,
    app: &mut App,
    events: &mut UnboundedReceiver<Event>,
    execute: &impl Fn(Command),
) -> Result<()> {
    let mut terminal_events = TerminalEvents::spawn();
    loop {
        screen
            .terminal
            .draw(|frame| ui::draw(frame, app, screen.pictures))?;

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
            match command {
                Command::Play(target) => {
                    if let Err(notice) = play(&target, screen, &terminal_events).await {
                        app.notice = Some(notice);
                    }
                }
                command => execute(command),
            }
        }
        if app.should_quit {
            return Ok(());
        }
    }
}

/// Hands the terminal to mpv to play `target` (a downloaded file or a web
/// link), then takes it back.
async fn play(
    target: &str,
    screen: &mut Screen<'_>,
    terminal_events: &TerminalEvents,
) -> Result<(), String> {
    let mpv = crate::platform::find_program("mpv")
        .ok_or("to play videos in the terminal, install mpv: https://mpv.io")?;
    if player::is_link(target) && crate::platform::find_program("yt-dlp").is_none() {
        return Err("to play video links, install yt-dlp: https://github.com/yt-dlp/yt-dlp".into());
    }
    let term = std::env::var("TERM").unwrap_or_default();
    let output = player::video_output(std::env::var_os("TMUX").is_some(), &term);

    // Stop reading the keyboard: the keys are mpv's until it quits.
    terminal_events.pause();
    leave_interface_mode(*screen.enhanced);
    ratatui::restore();

    let status = tokio::process::Command::new(&mpv)
        .args(player::mpv_args(target, output))
        .status()
        .await;

    let resumed = crossterm::terminal::enable_raw_mode()
        .and_then(|()| execute!(stdout(), crossterm::terminal::EnterAlternateScreen))
        .and_then(|()| {
            *screen.enhanced = enter_interface_mode();
            screen.terminal.clear()
        });
    terminal_events.resume();
    // Pictures sent to the terminal may be gone after the screen switch.
    screen.pictures.forget();
    resumed.map_err(|e| format!("terminal not restored: {e}"))?;

    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("mpv stopped ({status})")),
        Err(e) => Err(format!("could not launch mpv: {e}")),
    }
}

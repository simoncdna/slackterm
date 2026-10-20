mod compose;
mod demo;
mod executor;
mod fuzzy;
mod input;
mod keymap;
mod pictures;
mod player;
mod state;
mod template;
mod terminal_events;
mod theme;
mod ui;
mod update;

use std::io::stdout;
use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event as TermEvent, KeyEventKind,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::supports_keyboard_enhancement;
use ratatui::DefaultTerminal;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;

use crate::config::{Config, Settings, SidebarFile};
use crate::session::Session;
use crate::slack::{DEFAULT_API_BASE, SlackClient, rtm};

use keymap::Keymap;
use state::App;
use terminal_events::TerminalEvents;
use update::{Command, Event, initial_commands, update};

pub use pictures::reveal_tmux;
pub use template::default_config;

/// What the terminal can do, for `slackterm doctor`.
pub struct TerminalSupport {
    /// How pictures are drawn, if they are.
    pub pictures: Option<String>,
    /// Shift+Enter differs from Enter.
    pub keyboard_enhancement: bool,
}

/// Asks the terminal, which needs raw mode. `None` when not run in one.
pub fn probe_terminal() -> Option<TerminalSupport> {
    use std::io::IsTerminal;
    if !stdout().is_terminal() {
        return None;
    }
    crossterm::terminal::enable_raw_mode().ok()?;
    let pictures = pictures::Pictures::detect();
    let keyboard_enhancement = supports_keyboard_enhancement().unwrap_or(false);
    let _ = crossterm::terminal::disable_raw_mode();
    Some(TerminalSupport {
        pictures: pictures.available().then(|| pictures.describe()),
        keyboard_enhancement,
    })
}

/// What is wrong with the `[keys]` of a config file.
pub fn check_keys(config: &Config) -> Vec<String> {
    Keymap::new(&config.keys).1
}

pub async fn run(session: Session, config_path: PathBuf) -> Result<()> {
    anyhow::ensure!(
        !session.workspaces.is_empty(),
        "no saved workspace, run `slackterm login` again"
    );
    let (config, config_error) = match Config::load(&config_path) {
        Ok(config) => (config, None),
        Err(e) => (Config::default(), Some(format!("{e:#}"))),
    };
    let (keymap, key_problems) = Keymap::new(&config.keys);
    if let Some(error) = &config_error {
        tracing::warn!(%error, "config");
    }
    for problem in &key_problems {
        tracing::warn!(%problem, "config");
    }
    let sidebar = SidebarFile::load();

    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let mut connections = Vec::new();
    let mut apps = Vec::new();
    for (index, workspace) in session.workspaces.iter().enumerate() {
        let client = SlackClient::new(DEFAULT_API_BASE, &workspace.token, &session.cookie_d)?;
        let events = tagged(index, &events_tx);
        let real_time = spawn_real_time(client.clone(), events.clone());
        connections.push((client, events, real_time));

        let mut app = App::new(workspace, config.settings);
        app.keymap = keymap.clone();
        app.colors = config.colors;
        app.settings_path = Some(config_path.clone());
        let state = sidebar.state(&workspace.team_id);
        app.collapsed = state.collapsed;
        app.sidebar_tab = state.tab;
        apps.push(app);
    }
    let active = sidebar
        .workspace
        .and_then(|team| session.workspaces.iter().position(|w| w.team_id == team))
        .unwrap_or(0);
    apps[active].notice =
        config_error.or_else(|| (!key_problems.is_empty()).then(|| key_problems.join(" · ")));

    let instances = apps
        .into_iter()
        .zip(&connections)
        .map(|(app, (client, events, _))| Instance {
            app,
            execute: Box::new(move |command| executor::execute(command, client, events)),
        })
        .collect();
    let result = run_app(instances, active, events_rx).await;
    for (_, _, real_time) in connections {
        real_time.abort();
    }
    result
}

/// Opens the interface on a made-up workspace, without network access and
/// without reading or writing the user's settings.
pub async fn run_demo() -> Result<()> {
    let demos = [
        (demo::Demo::new(), demo::workspace()),
        (demo::Demo::side(), demo::side_workspace()),
    ];
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let senders: Vec<_> = (0..demos.len()).map(|i| tagged(i, &events_tx)).collect();
    let live: Vec<_> = demos
        .iter()
        .zip(&senders)
        .map(|((demo, _), events)| demo.spawn_live(events.clone()))
        .collect();
    let instances = demos
        .iter()
        .zip(&senders)
        .map(|((demo, workspace), events)| Instance {
            app: App::new(workspace, Settings::default()),
            execute: Box::new(move |command| demo.execute(command, events)),
        })
        .collect();
    let result = run_app(instances, 0, events_rx).await;
    for task in live {
        task.abort();
    }
    result
}

/// A workspace the user is signed in to: its state, and what runs its
/// commands.
struct Instance<'a> {
    app: App,
    execute: Box<dyn Fn(Command) + 'a>,
}

/// A sender for one workspace, whose events reach the event loop tagged
/// with that workspace's place.
fn tagged(index: usize, events: &UnboundedSender<(usize, Event)>) -> UnboundedSender<Event> {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let events = events.clone();
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            if events.send((index, event)).is_err() {
                return;
            }
        }
    });
    sender
}

fn spawn_real_time(client: SlackClient, events: UnboundedSender<Event>) -> JoinHandle<()> {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let connection = rtm::spawn(client, sender);
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            if events.send(Event::Rtm(event)).is_err() {
                break;
            }
        }
        connection.abort();
    })
}

/// Takes over the terminal until the user quits, showing the workspace at
/// `active` and handing every side effect to its instance, which reports
/// back through `events`.
async fn run_app(
    mut instances: Vec<Instance<'_>>,
    active: usize,
    mut events: UnboundedReceiver<(usize, Event)>,
) -> Result<()> {
    for instance in &instances {
        for command in initial_commands() {
            (instance.execute)(command);
        }
    }

    let mut terminal = ratatui::try_init()?;
    let mut enhanced = enter_interface_mode();
    let mut pictures = pictures::Pictures::detect();
    for instance in &mut instances {
        instance.app.graphics = pictures.available();
    }
    tracing::info!(
        pictures = %pictures.describe(),
        keyboard_enhancement = enhanced,
        layout = ?instances[active].app.settings.layout,
        workspaces = instances.len(),
        "interface started"
    );
    let mut screen = Screen {
        terminal: &mut terminal,
        enhanced: &mut enhanced,
        pictures: &mut pictures,
    };
    let result = event_loop(&mut screen, &mut instances, active, &mut events).await;

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
    instances: &mut [Instance<'_>],
    mut active: usize,
    events: &mut UnboundedReceiver<(usize, Event)>,
) -> Result<()> {
    let mut terminal_events = TerminalEvents::spawn();
    loop {
        share_workspaces(instances, active);
        screen
            .terminal
            .draw(|frame| ui::draw(frame, &instances[active].app, screen.pictures))?;

        // Keys go to the workspace on screen.
        let (index, event) = tokio::select! {
            Some(event) = terminal_events.next() => match event? {
                TermEvent::Key(key) if key.kind != KeyEventKind::Release => (active, Event::Key(key)),
                TermEvent::Paste(text) => (active, Event::Paste(text)),
                _ => continue,
            },
            Some(tagged) = events.recv() => tagged,
            else => return Ok(()),
        };

        let commands = update(&mut instances[index].app, event);
        // Settings are the same in every workspace.
        let settings = instances[index].app.settings;
        for instance in instances.iter_mut() {
            instance.app.settings = settings;
        }
        for command in commands {
            match command {
                Command::Play(target) => {
                    if let Err(notice) = play(&target, screen, &terminal_events).await {
                        instances[index].app.notice = Some(notice);
                    }
                }
                Command::NextWorkspace => {
                    let next = (active + 1) % instances.len();
                    active = show(instances, active, next);
                }
                Command::SwitchWorkspace(to) => active = show(instances, active, to),
                command => (instances[index].execute)(command),
            }
        }
        if instances[index].app.should_quit {
            return Ok(());
        }
    }
}

/// Tells every workspace about the others, and which one is on screen.
fn share_workspaces(instances: &mut [Instance<'_>], active: usize) {
    let tabs: Vec<_> = instances.iter().map(|i| i.app.tab()).collect();
    for (index, instance) in instances.iter_mut().enumerate() {
        instance.app.workspaces = tabs.clone();
        instance.app.workspace = index;
        instance.app.visible = index == active;
    }
}

/// Puts the workspace at `to` on screen, and returns where it is.
fn show(instances: &mut [Instance<'_>], active: usize, to: usize) -> usize {
    if to == active || to >= instances.len() {
        return active;
    }
    let instance = &mut instances[to];
    tracing::info!(workspace = to, "workspace shown");
    let team = instance.app.team_id.clone();
    for command in update::on_shown(&mut instance.app) {
        (instance.execute)(command);
    }
    (instance.execute)(Command::RememberWorkspace(team));
    to
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

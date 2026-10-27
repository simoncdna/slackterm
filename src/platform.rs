use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use base64::Engine;

/// Finds `program` in the `PATH` the way a shell would, adding `.exe` on
/// Windows.
pub fn find_program(program: &str) -> Option<PathBuf> {
    let file = format!("{program}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&file))
        .find(|path| path.is_file())
}

/// Puts `text` on the user's clipboard. Over SSH, the machine's clipboard
/// is not the user's, so the terminal is asked to (OSC 52); elsewhere the
/// system's tool does it, the terminal being the fallback.
pub fn copy_to_clipboard(text: &str) -> io::Result<()> {
    let remote = ["SSH_CONNECTION", "SSH_TTY"]
        .iter()
        .any(|name| std::env::var_os(name).is_some());
    if !remote {
        for (program, args) in clipboard_tools() {
            if let Some(path) = find_program(program)
                && pipe_to(&path, args, text).is_ok()
            {
                return Ok(());
            }
        }
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let mut stdout = io::stdout();
    write!(stdout, "\x1b]52;c;{encoded}\x07")?;
    stdout.flush()
}

fn clipboard_tools() -> &'static [(&'static str, &'static [&'static str])] {
    if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(windows) {
        // clip.exe garbles anything outside the console's code page.
        &[]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    }
}

/// How a notification reaches the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notifier {
    /// The terminal shows it, from an escape sequence: over SSH too, and
    /// clicking it brings the terminal forward.
    Terminal {
        protocol: TerminalProtocol,
        /// Wrapped for tmux, which lets it through when `allow-passthrough`
        /// is on.
        tmux: bool,
    },
    /// The system's tool: `osascript` on macOS, `notify-send` on Linux.
    System(PathBuf),
    /// The bell, which terminals and tmux turn into a mark on the window.
    Bell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalProtocol {
    /// iTerm2's: a single line.
    Osc9,
    /// A title and a body: WezTerm, Ghostty, foot.
    Osc777,
    /// kitty's.
    Osc99,
}

impl Notifier {
    /// Picks the best way from the environment.
    pub fn detect() -> Self {
        Self::choose(
            |name| std::env::var(name).ok().filter(|value| !value.is_empty()),
            find_program,
        )
    }

    fn choose(
        var: impl Fn(&str) -> Option<String>,
        find: impl Fn(&str) -> Option<PathBuf>,
    ) -> Self {
        let remote = var("SSH_CONNECTION").is_some() || var("SSH_TTY").is_some();
        let tmux = var("TMUX").is_some();
        // Over SSH, the system's notifications would show on the server.
        let system = if remote {
            None
        } else if cfg!(target_os = "macos") {
            find("osascript")
        } else if cfg!(windows) {
            None
        } else {
            find("notify-send")
        };
        match (terminal_protocol(&var), system) {
            // tmux drops the sequences it does not know, unless told not to.
            (_, Some(tool)) if tmux => Self::System(tool),
            (Some(protocol), _) => Self::Terminal { protocol, tmux },
            (None, Some(tool)) => Self::System(tool),
            (None, None) => Self::Bell,
        }
    }

    /// How notifications are shown, for `slackterm doctor`.
    pub fn describe(&self) -> String {
        match self {
            Self::Terminal { protocol, tmux } => {
                let name = match protocol {
                    TerminalProtocol::Osc9 => "OSC 9",
                    TerminalProtocol::Osc777 => "OSC 777",
                    TerminalProtocol::Osc99 => "OSC 99",
                };
                if *tmux {
                    format!("by the terminal ({name}), through tmux if allow-passthrough is on")
                } else {
                    format!("by the terminal ({name})")
                }
            }
            Self::System(tool) => format!("by {}", tool.display()),
            Self::Bell => "as the terminal's bell".to_string(),
        }
    }

    /// Shows a notification. Its text comes from other people: control
    /// characters are taken out, so that it cannot talk to the terminal.
    pub fn notify(&self, title: &str, body: &str) -> io::Result<()> {
        let title = printable(title);
        let body = printable(body);
        match self {
            Self::Terminal { protocol, tmux } => {
                let sequence = match protocol {
                    TerminalProtocol::Osc9 => format!("\x1b]9;{title}: {body}\x07"),
                    // `;` separates the title from the body.
                    TerminalProtocol::Osc777 => {
                        format!("\x1b]777;notify;{};{body}\x07", title.replace(';', ","))
                    }
                    TerminalProtocol::Osc99 => {
                        let id = NOTIFICATIONS.fetch_add(1, Ordering::Relaxed);
                        format!(
                            "\x1b]99;i={id}:d=0;{title}\x1b\\\x1b]99;i={id}:d=1:p=body;{body}\x1b\\"
                        )
                    }
                };
                write_to_terminal(&sequence, *tmux)
            }
            Self::System(tool) => {
                let args: Vec<String> = if cfg!(target_os = "macos") {
                    // Passed as arguments, never inside the script.
                    [
                        "-e",
                        "on run argv",
                        "-e",
                        "display notification (item 2 of argv) with title (item 1 of argv)",
                        "-e",
                        "end run",
                    ]
                    .iter()
                    .map(|arg| arg.to_string())
                    .chain([title, body])
                    .collect()
                } else {
                    vec!["--app-name=slackterm".into(), "--".into(), title, body]
                };
                let mut child = Command::new(tool)
                    .args(args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?;
                // Waited for elsewhere, so the interface does not.
                std::thread::spawn(move || child.wait());
                Ok(())
            }
            Self::Bell => write_to_terminal("\x07", false),
        }
    }
}

/// Numbers kitty's notifications, which are sent in two parts.
static NOTIFICATIONS: AtomicU32 = AtomicU32::new(1);

/// Which notifications the terminal shows itself, from what it sets in the
/// environment. `TERM` and `LC_*` go through SSH, the others do not.
fn terminal_protocol(var: &impl Fn(&str) -> Option<String>) -> Option<TerminalProtocol> {
    let term = var("TERM").unwrap_or_default();
    let program = var("TERM_PROGRAM").unwrap_or_default();
    if term == "xterm-kitty" || var("KITTY_WINDOW_ID").is_some() {
        Some(TerminalProtocol::Osc99)
    } else if program == "WezTerm"
        || term == "wezterm"
        || var("WEZTERM_PANE").is_some()
        || program == "ghostty"
        || term == "xterm-ghostty"
        || var("GHOSTTY_RESOURCES_DIR").is_some()
        || term.starts_with("foot")
    {
        Some(TerminalProtocol::Osc777)
    } else if program == "iTerm.app" || var("LC_TERMINAL").as_deref() == Some("iTerm2") {
        Some(TerminalProtocol::Osc9)
    } else {
        None
    }
}

fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn write_to_terminal(sequence: &str, tmux: bool) -> io::Result<()> {
    let mut stdout = io::stdout();
    if tmux {
        // tmux passes on what is wrapped like this, its escapes doubled.
        write!(
            stdout,
            "\x1bPtmux;{}\x1b\\",
            sequence.replace('\x1b', "\x1b\x1b")
        )?;
    } else {
        stdout.write_all(sequence.as_bytes())?;
    }
    stdout.flush()
}

fn pipe_to(program: &std::path::Path, args: &[&str], text: &str) -> io::Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("no input"))?
        .write_all(text.as_bytes())?;
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{} failed", program.display())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choose(vars: &[(&str, &str)], tools: &[&str]) -> Notifier {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        Notifier::choose(
            |name| {
                vars.iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, value)| value.clone())
            },
            |program| tools.contains(&program).then(|| PathBuf::from(program)),
        )
    }

    #[test]
    fn terminals_that_notify_do_it_themselves() {
        let ghostty = [("TERM", "xterm-ghostty"), ("TERM_PROGRAM", "ghostty")];
        assert_eq!(
            choose(&ghostty, &["osascript", "notify-send"]),
            Notifier::Terminal {
                protocol: TerminalProtocol::Osc777,
                tmux: false
            }
        );
        // Over SSH, only what SSH passes on tells the terminal.
        let ssh = [("SSH_TTY", "/dev/pts/1"), ("LC_TERMINAL", "iTerm2")];
        assert_eq!(
            choose(&ssh, &["osascript", "notify-send"]),
            Notifier::Terminal {
                protocol: TerminalProtocol::Osc9,
                tmux: false
            }
        );
        let kitty = [
            ("SSH_TTY", "/dev/pts/1"),
            ("TERM", "xterm-kitty"),
            ("TMUX", "/tmp/t"),
        ];
        assert_eq!(
            choose(&kitty, &[]),
            Notifier::Terminal {
                protocol: TerminalProtocol::Osc99,
                tmux: true
            }
        );
    }

    #[test]
    fn otherwise_the_system_or_the_bell() {
        let tool = if cfg!(target_os = "macos") {
            "osascript"
        } else {
            "notify-send"
        };
        let expected = if cfg!(windows) {
            Notifier::Bell
        } else {
            Notifier::System(PathBuf::from(tool))
        };
        assert_eq!(choose(&[("TERM", "xterm-256color")], &[tool]), expected);
        // In tmux, the system's tool needs nothing from tmux's settings.
        // There is no tmux on Windows, nor a tool.
        if !cfg!(windows) {
            let tmux = [("TERM_PROGRAM", "WezTerm"), ("TMUX", "/tmp/t")];
            assert_eq!(choose(&tmux, &[tool]), expected);
        }
        // Over SSH, the system is the server's.
        let ssh = [
            ("SSH_CONNECTION", "1.2.3.4 5 6.7.8.9 22"),
            ("TERM", "xterm"),
        ];
        assert_eq!(choose(&ssh, &[tool]), Notifier::Bell);
    }

    #[test]
    fn text_from_others_cannot_talk_to_the_terminal() {
        assert_eq!(
            printable("hi\x1b]52;c;bad\x07\nthere"),
            "hi ]52;c;bad  there"
        );
    }
}

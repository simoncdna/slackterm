use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

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

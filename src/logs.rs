//! A log file for bug reports. The interface owns the terminal, so this is
//! where slackterm says what it did: which Slack methods it called, how the
//! real-time connection went, and why it crashed.
//!
//! Never logged: tokens, cookies, nor what people write.

use std::fs::File;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use tracing_subscriber::EnvFilter;

const FILE: &str = "slackterm.log";
/// The previous run's log, kept when it is the one that crashed.
const PREVIOUS: &str = "slackterm.old.log";

/// Where the log of this run goes.
pub fn path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "slackterm")?;
    Some(dirs.data_local_dir().join(FILE))
}

/// Starts logging to the file, keeping the previous run's next to it.
/// `debug` adds every call to Slack; `SLACKTERM_LOG` picks any level, as
/// `RUST_LOG` does.
pub fn init(debug: bool) -> Result<PathBuf> {
    let path = path().context("could not find the data directory")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("could not create {}", dir.display()))?;
    }
    let _ = std::fs::rename(&path, path.with_file_name(PREVIOUS));
    let file =
        File::create(&path).with_context(|| format!("could not create {}", path.display()))?;

    let level = if debug {
        "slackterm=debug"
    } else {
        "slackterm=info"
    };
    let filter = EnvFilter::try_from_env("SLACKTERM_LOG").unwrap_or_else(|_| EnvFilter::new(level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(Mutex::new(file))
        .with_ansi(false)
        .with_target(false)
        .init();

    let log = path.clone();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("crashed: {info}");
        previous(info);
        eprintln!(
            "slackterm crashed. Details are in {}: please attach them to an issue at \
             https://github.com/simoncdna/slackterm/issues",
            log.display()
        );
    }));

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        term = std::env::var("TERM").unwrap_or_default(),
        term_program = std::env::var("TERM_PROGRAM").unwrap_or_default(),
        tmux = std::env::var_os("TMUX").is_some(),
        "started"
    );
    Ok(path)
}

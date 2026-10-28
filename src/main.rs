use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::Shell;

use slackterm::commands::{self, ConfigOptions, LoginOptions};
use slackterm::config::config_path;
use slackterm::slack::DEFAULT_API_BASE;
use slackterm::store::{SessionStore, SystemStore};

#[derive(Parser)]
#[command(
    name = "slackterm",
    version,
    about = "Slack in your terminal. With no command, opens the interface."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Config file to use instead of the usual one
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Log every call to Slack, for a bug report
    #[arg(long, global = true)]
    debug: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Sign in to Slack through your browser
    Login {
        /// Paste the xoxc token and the d cookie yourself
        #[arg(long)]
        manual: bool,
        /// Path to the browser executable (Chrome, Chromium, Brave, Edge)
        #[arg(long, value_name = "PATH")]
        browser: Option<PathBuf>,
        /// How long to wait for you to sign in, in seconds
        #[arg(long, value_name = "SECONDS", default_value_t = 300)]
        timeout: u64,
    },
    /// Delete the saved credentials
    Logout,
    /// List the saved workspaces and check that their session is valid
    Status,
    /// Try the interface on a made-up workspace, no Slack account needed
    Demo,
    /// Check what slackterm needs, and where its log is
    Doctor,
    /// Print the shell completion script: bash, zsh, fish, elvish or powershell
    Completions {
        #[arg(value_name = "SHELL")]
        shell: Shell,
    },
    /// Make your own themes, and list them
    Theme {
        #[command(subcommand)]
        action: ThemeAction,
    },
    /// Open the config file in your editor ($VISUAL or $EDITOR)
    Config {
        /// Only print where the config file is
        #[arg(long, conflicts_with = "default")]
        path: bool,
        /// Print every option with its default value
        #[arg(long)]
        default: bool,
    },
}

#[derive(Subcommand)]
enum ThemeAction {
    /// Write a theme with every color of another one, and open it in your editor
    New {
        /// Its name, which is also its file's
        name: String,
        /// The theme whose colors it starts with
        #[arg(long, value_name = "THEME", default_value = "night")]
        from: String,
    },
    /// List the themes, and where yours go
    List,
}

fn main() -> Result<()> {
    // Changes the environment, so it must run before the runtime's threads.
    slackterm::tui::reveal_tmux();
    tokio::runtime::Runtime::new()?.block_on(run())
}

async fn run() -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cli = Cli::parse();
    // Only what can go wrong over time logs: running `doctor` after a crash
    // must not push the crash's log out.
    let logs = matches!(
        cli.command,
        None | Some(Command::Login { .. } | Command::Demo)
    );
    if logs && let Err(e) = slackterm::logs::init(cli.debug) {
        eprintln!("⚠ no log file: {e:#}");
    }
    let store = SystemStore::new()?;

    let Some(command) = cli.command else {
        return match store.load()? {
            Some(session) => slackterm::tui::run(session, config_path(cli.config)?).await,
            None => {
                println!("Not signed in. Run `slackterm login`.");
                Ok(())
            }
        };
    };
    match command {
        Command::Login {
            manual,
            browser,
            timeout,
        } => {
            let options = LoginOptions {
                manual,
                browser,
                timeout: Duration::from_secs(timeout),
            };
            commands::login(&store, DEFAULT_API_BASE, options).await
        }
        Command::Logout => commands::logout(&store),
        Command::Status => commands::status(&store, DEFAULT_API_BASE).await,
        Command::Demo => slackterm::tui::run_demo(config_path(cli.config)?).await,
        Command::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "slackterm",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Command::Doctor => {
            commands::doctor(&store, DEFAULT_API_BASE, &config_path(cli.config)?).await
        }
        Command::Config { path, default } => {
            commands::config(&config_path(cli.config)?, ConfigOptions { path, default })
        }
        Command::Theme { action } => {
            let path = config_path(cli.config)?;
            match action {
                ThemeAction::New { name, from } => commands::theme_new(&path, &name, &from),
                ThemeAction::List => commands::theme_list(&path),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_line_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn writes_completions_for_every_shell() {
        for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut Cli::command(), "slackterm", &mut script);
            assert!(String::from_utf8(script).unwrap().contains("doctor"));
        }
    }
}

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};

use slackterm::commands::{self, LoginOptions};
use slackterm::slack::DEFAULT_API_BASE;
use slackterm::store::{KeyringStore, SessionStore};

#[derive(Parser)]
#[command(
    name = "slackterm",
    version,
    about = "Slack in your terminal. With no command, opens the interface."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
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
}

fn main() -> Result<()> {
    // Changes the environment, so it must run before the runtime's threads.
    slackterm::tui::reveal_tmux();
    tokio::runtime::Runtime::new()?.block_on(run())
}

async fn run() -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cli = Cli::parse();
    let store = KeyringStore;

    let Some(command) = cli.command else {
        return match store.load()? {
            Some(session) => slackterm::tui::run(session).await,
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
        Command::Demo => slackterm::tui::run_demo().await,
    }
}

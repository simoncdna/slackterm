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
    about = "Slack dans le terminal. Sans commande, ouvre l'interface."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Se connecter à Slack via le navigateur
    Login {
        /// Saisir le token xoxc et le cookie d à la main
        #[arg(long)]
        manual: bool,
        /// Chemin vers l'exécutable du navigateur (Chrome, Chromium, Brave, Edge)
        #[arg(long, value_name = "CHEMIN")]
        browser: Option<PathBuf>,
        /// Délai maximum pour se connecter, en secondes
        #[arg(long, value_name = "SECONDES", default_value_t = 300)]
        timeout: u64,
    },
    /// Effacer les identifiants enregistrés
    Logout,
    /// Afficher les workspaces enregistrés et vérifier la connexion
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cli = Cli::parse();
    let store = KeyringStore;

    let Some(command) = cli.command else {
        return match store.load()? {
            Some(session) => slackterm::tui::run(session).await,
            None => {
                println!("Pas connecté. Lance `slackterm login`.");
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
    }
}

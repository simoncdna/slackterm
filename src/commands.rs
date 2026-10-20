use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::auth::{browser, manual};
use crate::config::Config;
use crate::platform::find_program;
use crate::session::{RawCredentials, Session, Workspace, normalize_cookie_d};
use crate::slack::{SlackClient, SlackError};
use crate::store::{Saved, SessionStore, SystemStore};

pub struct LoginOptions {
    pub manual: bool,
    pub browser: Option<PathBuf>,
    pub timeout: Duration,
}

pub async fn login(store: &dyn SessionStore, api_base: &str, options: LoginOptions) -> Result<()> {
    let raw = if options.manual {
        manual::prompt()?
    } else {
        capture_from_browser(options).await?
    };

    let (session, rejected) = verify(api_base, raw).await?;
    let saved = store.save(&session).context("could not save the session")?;

    for error in rejected {
        eprintln!("⚠ skipped workspace: {error}");
    }
    for workspace in &session.workspaces {
        println!(
            "✓ Signed in: {} @ {}",
            workspace.user_name, workspace.team_name
        );
    }
    match saved {
        Saved::Keychain => println!("  Credentials saved to the system keychain."),
        Saved::File {
            path,
            keychain_error,
        } => println!(
            "  No system keychain ({keychain_error}): credentials saved to {}, \
             readable only by you.",
            path.display()
        ),
    }
    Ok(())
}

async fn capture_from_browser(options: LoginOptions) -> Result<RawCredentials> {
    let executable = options.browser.or_else(browser::find_browser).context(
        "no compatible browser found (Chrome, Chromium, Brave, Edge). \
         Pass its path with --browser, or use --manual",
    )?;
    println!("→ Opening your browser… sign in to your Slack workspace.");
    browser::capture(&executable, &profile_dir()?, options.timeout).await
}

/// The browser profile is kept between logins so that an expired session
/// usually only needs a click to renew.
fn profile_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "slackterm")
        .context("could not find the data directory")?;
    Ok(dirs.data_dir().join("browser-profile"))
}

/// Checks every captured token with `auth.test`. Rejected tokens are returned
/// alongside the session rather than failing the whole login.
pub async fn verify(api_base: &str, raw: RawCredentials) -> Result<(Session, Vec<SlackError>)> {
    let cookie_d = normalize_cookie_d(&raw.cookie_d);
    let mut workspaces = Vec::new();
    let mut rejected = Vec::new();

    for token in raw.tokens {
        let client = SlackClient::new(api_base, &token, &cookie_d)?;
        match client.auth_test().await {
            Ok(auth) => workspaces.push(Workspace {
                team_id: auth.team_id,
                team_name: auth.team,
                url: auth.url,
                user_id: auth.user_id,
                user_name: auth.user,
                token,
            }),
            Err(e) => rejected.push(e),
        }
    }

    if workspaces.is_empty() {
        let reasons: Vec<String> = rejected.iter().map(ToString::to_string).collect();
        bail!("no workspace could be verified ({})", reasons.join(", "));
    }
    Ok((
        Session {
            cookie_d,
            workspaces,
        },
        rejected,
    ))
}

pub fn logout(store: &dyn SessionStore) -> Result<()> {
    if store.clear()? {
        println!("Credentials deleted.");
    } else {
        println!("No saved credentials.");
    }
    Ok(())
}

pub async fn status(store: &dyn SessionStore, api_base: &str) -> Result<()> {
    let Some(session) = store.load()? else {
        println!("Not signed in. Run `slackterm login`.");
        return Ok(());
    };

    for workspace in &session.workspaces {
        let who = format!("{} @ {}", workspace.user_name, workspace.team_name);
        let client = SlackClient::new(api_base, &workspace.token, &session.cookie_d)?;
        match client.auth_test().await {
            Ok(_) => println!("✓ {who} ({})", workspace.url),
            Err(SlackError::Api(reason)) => {
                println!("✕ {who}: session rejected ({reason}). Run `slackterm login` again.")
            }
            Err(e) => println!("? {who}: {e}"),
        }
    }
    Ok(())
}

enum Check {
    Ok,
    /// Optional, or with a way around it.
    Missing,
    Problem,
}

fn report(check: Check, name: &str, detail: impl std::fmt::Display) {
    let mark = match check {
        Check::Ok => "✓",
        Check::Missing => "–",
        Check::Problem => "✕",
    };
    println!("{mark} {name:<12} {detail}");
}

/// Checks what slackterm needs, one line each, to fix a setup or to paste
/// in a bug report.
pub async fn doctor(store: &SystemStore, api_base: &str, config_path: &Path) -> Result<()> {
    println!(
        "slackterm {} · {} {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );

    let term: Vec<String> = ["TERM_PROGRAM", "TERM"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .collect();
    let tmux = if std::env::var_os("TMUX").is_some() {
        ", in tmux"
    } else {
        ""
    };
    match crate::tui::probe_terminal() {
        Some(support) => {
            report(Check::Ok, "Terminal", format!("{}{tmux}", term.join(", ")));
            match support.pictures {
                Some(protocol) => report(Check::Ok, "Pictures", protocol),
                None => report(
                    Check::Missing,
                    "Pictures",
                    "not supported here: initials instead (Kitty, Ghostty, iTerm2, \
                     WezTerm and sixel terminals show them)",
                ),
            }
            if support.keyboard_enhancement {
                report(Check::Ok, "Shift+Enter", "a new line");
            } else {
                report(
                    Check::Missing,
                    "Shift+Enter",
                    "same as Enter here: Alt+Enter or Ctrl+J for a new line",
                );
            }
        }
        None => report(
            Check::Missing,
            "Terminal",
            "not a terminal: run doctor in the one you use",
        ),
    }

    match store.check_keychain() {
        Ok(()) => report(Check::Ok, "Keychain", "the system's"),
        Err(e) => report(
            Check::Missing,
            "Keychain",
            format!(
                "unavailable ({e:#}): the session is kept in {}",
                store.file().display()
            ),
        ),
    }
    match store.load()? {
        None => report(
            Check::Problem,
            "Session",
            "not signed in: run `slackterm login`",
        ),
        Some(session) => {
            for workspace in &session.workspaces {
                let who = format!("{} @ {}", workspace.user_name, workspace.team_name);
                let client = SlackClient::new(api_base, &workspace.token, &session.cookie_d)?;
                match client.auth_test().await {
                    Ok(_) => report(Check::Ok, "Session", who),
                    Err(e) if e.is_session_expired() => report(
                        Check::Problem,
                        "Session",
                        format!("{who}: expired, run `slackterm login`"),
                    ),
                    Err(e) => report(Check::Problem, "Session", format!("{who}: {e}")),
                }
            }
        }
    }

    if !config_path.exists() {
        report(
            Check::Ok,
            "Config",
            format!("defaults ({} not created)", config_path.display()),
        );
    } else {
        match Config::load(config_path) {
            Ok(config) => {
                let problems = crate::tui::check_keys(&config);
                if problems.is_empty() {
                    report(Check::Ok, "Config", config_path.display());
                }
                for problem in problems {
                    report(Check::Problem, "Config", problem);
                }
            }
            Err(e) => report(Check::Problem, "Config", format!("{e:#}")),
        }
    }

    match browser::find_browser() {
        Some(path) => report(Check::Ok, "Browser", path.display()),
        None => report(
            Check::Missing,
            "Browser",
            "no Chrome, Chromium, Brave or Edge: sign in with `slackterm login --manual`",
        ),
    }
    let programs = [
        ("mpv", "to play videos in the terminal: https://mpv.io"),
        (
            "yt-dlp",
            "to play video links: https://github.com/yt-dlp/yt-dlp",
        ),
    ];
    for (program, purpose) in programs {
        match find_program(program) {
            Some(path) => report(Check::Ok, program, path.display()),
            None => report(
                Check::Missing,
                program,
                format!("not installed, needed {purpose}"),
            ),
        }
    }

    if let Some(log) = crate::logs::path() {
        report(Check::Ok, "Logs", log.display());
    }
    Ok(())
}

pub struct ConfigOptions {
    pub path: bool,
    pub default: bool,
}

/// Opens the config file in the user's editor, creating it with every
/// option commented out the first time, then checks what was written.
pub fn config(path: &Path, options: ConfigOptions) -> Result<()> {
    if options.path {
        println!("{}", path.display());
        return Ok(());
    }
    if options.default {
        print!("{}", crate::tui::default_config());
        return Ok(());
    }
    if !path.exists() {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .with_context(|| format!("could not create {}", dir.display()))?;
        }
        std::fs::write(path, crate::tui::default_config())
            .with_context(|| format!("could not write {}", path.display()))?;
        println!(
            "Created {}, with every option commented out.",
            path.display()
        );
    }
    let Some(editor) = editor() else {
        println!(
            "Edit {} (set $EDITOR to open it from here).",
            path.display()
        );
        return Ok(());
    };
    let mut words = editor.split_whitespace();
    let program = words.next().context("$EDITOR is empty")?;
    let status = std::process::Command::new(program)
        .args(words)
        .arg(path)
        .status()
        .with_context(|| format!("could not launch {program}"))?;
    if !status.success() {
        bail!("{program} stopped ({status})");
    }
    check_config(path);
    Ok(())
}

/// `$VISUAL`, `$EDITOR`, or Notepad on Windows.
fn editor() -> Option<String> {
    ["VISUAL", "EDITOR"]
        .into_iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|editor| !editor.trim().is_empty())
        .or_else(|| cfg!(windows).then(|| "notepad".to_string()))
}

/// Says right away what is wrong with the file, rather than at the next
/// launch.
fn check_config(path: &Path) {
    match Config::load(path) {
        Ok(config) => {
            let problems = crate::tui::check_keys(&config);
            if problems.is_empty() {
                println!("✓ {} is valid.", path.display());
            }
            for problem in problems {
                println!("⚠ {problem}");
            }
        }
        Err(e) => println!("✕ {e:#}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;
    use wiremock::matchers::{header, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn slack_with_one_valid_token() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(path("/auth.test"))
            .and(header("authorization", "Bearer xoxc-ok"))
            .and(header("cookie", "d=xoxd-a%2Fb"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "url": "https://acme.slack.com/",
                "team": "Acme",
                "user": "simon",
                "team_id": "T1",
                "user_id": "U1"
            })))
            .mount(&server)
            .await;
        Mock::given(path("/auth.test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"ok": false, "error": "invalid_auth"})),
            )
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn verify_keeps_valid_workspaces_and_reports_the_rest() {
        let server = slack_with_one_valid_token().await;
        let raw = RawCredentials {
            cookie_d: "xoxd-a/b".into(),
            tokens: vec!["xoxc-ok".into(), "xoxc-expired".into()],
        };

        let (session, rejected) = verify(&server.uri(), raw).await.unwrap();

        assert_eq!(session.cookie_d, "xoxd-a%2Fb");
        assert_eq!(session.workspaces.len(), 1);
        assert_eq!(session.workspaces[0].team_name, "Acme");
        assert_eq!(session.workspaces[0].token, "xoxc-ok");
        assert_eq!(rejected.len(), 1);
    }

    #[tokio::test]
    async fn verify_fails_when_no_token_is_accepted() {
        let server = slack_with_one_valid_token().await;
        let raw = RawCredentials {
            cookie_d: "xoxd-a/b".into(),
            tokens: vec!["xoxc-expired".into()],
        };

        let err = verify(&server.uri(), raw).await.unwrap_err();

        assert!(err.to_string().contains("invalid_auth"));
    }

    #[tokio::test]
    async fn logout_clears_the_stored_session() {
        let server = slack_with_one_valid_token().await;
        let raw = RawCredentials {
            cookie_d: "xoxd-a/b".into(),
            tokens: vec!["xoxc-ok".into()],
        };
        let (session, _) = verify(&server.uri(), raw).await.unwrap();
        let store = MemoryStore::default();
        store.save(&session).unwrap();

        logout(&store).unwrap();

        assert!(store.load().unwrap().is_none());
    }
}

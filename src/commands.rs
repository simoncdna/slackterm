use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use crate::auth::{browser, manual};
use crate::session::{RawCredentials, Session, Workspace, normalize_cookie_d};
use crate::slack::{SlackClient, SlackError};
use crate::store::SessionStore;

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
    store
        .save(&session)
        .context("impossible d'enregistrer la session dans le trousseau")?;

    for error in rejected {
        eprintln!("⚠ workspace ignoré : {error}");
    }
    for workspace in &session.workspaces {
        println!(
            "✓ Connecté : {} @ {}",
            workspace.user_name, workspace.team_name
        );
    }
    println!("  Identifiants enregistrés dans le trousseau.");
    Ok(())
}

async fn capture_from_browser(options: LoginOptions) -> Result<RawCredentials> {
    let executable = options.browser.or_else(browser::find_browser).context(
        "aucun navigateur compatible trouvé (Chrome, Chromium, Brave, Edge). \
         Indique son chemin avec --browser, ou utilise --manual",
    )?;
    println!("→ Ouverture du navigateur… connecte-toi à ton workspace Slack.");
    browser::capture(&executable, &profile_dir()?, options.timeout).await
}

/// The browser profile is kept between logins so that an expired session
/// usually only needs a click to renew.
fn profile_dir() -> Result<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "slackterm")
        .context("impossible de déterminer le dossier de données")?;
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
        bail!(
            "aucun workspace n'a pu être vérifié ({})",
            reasons.join(", ")
        );
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
        println!("Identifiants effacés du trousseau.");
    } else {
        println!("Aucun identifiant enregistré.");
    }
    Ok(())
}

pub async fn status(store: &dyn SessionStore, api_base: &str) -> Result<()> {
    let Some(session) = store.load()? else {
        println!("Pas connecté. Lance `slackterm login`.");
        return Ok(());
    };

    for workspace in &session.workspaces {
        let who = format!("{} @ {}", workspace.user_name, workspace.team_name);
        let client = SlackClient::new(api_base, &workspace.token, &session.cookie_d)?;
        match client.auth_test().await {
            Ok(_) => println!("✓ {who} ({})", workspace.url),
            Err(SlackError::Api(reason)) => {
                println!("✕ {who} : session refusée ({reason}). Relance `slackterm login`.")
            }
            Err(e) => println!("? {who} : {e}"),
        }
    }
    Ok(())
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

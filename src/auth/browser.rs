use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::json;
use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout};

use super::cdp::Cdp;
use super::local_config::tokens_from_local_config;
use crate::session::RawCredentials;

const SLACK_URL: &str = "https://app.slack.com/";
const CLIENT_URL_PREFIX: &str = "https://app.slack.com/client/";
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
/// Consecutive polls with no open tab before assuming the user closed the window
/// (on macOS the process keeps running after its last window is closed).
const MAX_POLLS_WITHOUT_PAGE: u32 = 3;

#[cfg(target_os = "macos")]
const CANDIDATES: &[&str] = &[
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
];

#[cfg(not(target_os = "macos"))]
const CANDIDATES: &[&str] = &[
    "google-chrome",
    "google-chrome-stable",
    "chromium",
    "chromium-browser",
    "brave-browser",
    "microsoft-edge",
];

pub fn find_browser() -> Option<PathBuf> {
    CANDIDATES.iter().find_map(|candidate| resolve(candidate))
}

fn resolve(candidate: &str) -> Option<PathBuf> {
    let path = Path::new(candidate);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(candidate))
        .find(|path| path.is_file())
}

struct DevTools {
    http_base: String,
    browser_ws: String,
}

#[derive(Deserialize)]
struct Target {
    #[serde(rename = "type")]
    kind: String,
    url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    ws_url: Option<String>,
}

/// Opens Slack in a dedicated browser profile, waits for the user to sign in,
/// then reads the `xoxc` tokens and the `d` cookie the web client uses.
pub async fn capture(
    executable: &Path,
    profile_dir: &Path,
    login_timeout: Duration,
) -> Result<RawCredentials> {
    std::fs::create_dir_all(profile_dir)
        .with_context(|| format!("impossible de créer {}", profile_dir.display()))?;
    let port_file = profile_dir.join("DevToolsActivePort");
    let _ = std::fs::remove_file(&port_file);

    // Chrome refuses remote debugging on the default profile since v136, hence
    // the dedicated --user-data-dir. No automation flags are passed so that
    // sign-in pages (Google SSO in particular) see a regular browser.
    let mut child = Command::new(executable)
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg("--remote-debugging-port=0")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .arg(SLACK_URL)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("impossible de lancer {}", executable.display()))?;

    let devtools = match wait_for_devtools(&port_file, &mut child).await {
        Ok(devtools) => devtools,
        Err(e) => {
            let _ = child.kill().await;
            return Err(e);
        }
    };

    let result = match timeout(login_timeout, wait_for_credentials(&devtools, &mut child)).await {
        Ok(result) => result,
        Err(_) => Err(anyhow!(
            "délai dépassé : pas de connexion à Slack au bout de {} s",
            login_timeout.as_secs()
        )),
    };
    close_browser(&devtools, &mut child).await;
    result
}

async fn wait_for_devtools(port_file: &Path, child: &mut Child) -> Result<DevTools> {
    let deadline = tokio::time::Instant::now() + STARTUP_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        if let Some(status) = child.try_wait()? {
            bail!(
                "le navigateur s'est fermé tout de suite ({status}). Une fenêtre utilisant \
                 le profil slackterm est peut-être déjà ouverte : ferme-la et réessaie"
            );
        }
        if let Ok(contents) = std::fs::read_to_string(port_file)
            && let Some((port, path)) = parse_devtools_active_port(&contents)
        {
            return Ok(DevTools {
                http_base: format!("http://127.0.0.1:{port}"),
                browser_ws: format!("ws://127.0.0.1:{port}{path}"),
            });
        }
        sleep(Duration::from_millis(100)).await;
    }
    bail!("le navigateur n'a pas ouvert son port DevTools")
}

/// `DevToolsActivePort` holds the port on its first line and the browser
/// websocket path on the second.
fn parse_devtools_active_port(contents: &str) -> Option<(u16, String)> {
    let mut lines = contents.lines();
    let port: u16 = lines.next()?.trim().parse().ok()?;
    let path = lines.next()?.trim();
    (port != 0 && path.starts_with('/')).then(|| (port, path.to_string()))
}

async fn wait_for_credentials(devtools: &DevTools, child: &mut Child) -> Result<RawCredentials> {
    let http = reqwest::Client::new();
    let mut polls_without_page = 0;
    loop {
        if child.try_wait()?.is_some() {
            bail!("navigateur fermé avant la fin de la connexion");
        }
        let targets: Vec<Target> = http
            .get(format!("{}/json/list", devtools.http_base))
            .send()
            .await?
            .json()
            .await?;

        let pages: Vec<&Target> = targets.iter().filter(|t| t.kind == "page").collect();
        if pages.is_empty() {
            polls_without_page += 1;
            if polls_without_page >= MAX_POLLS_WITHOUT_PAGE {
                bail!("fenêtre fermée avant la fin de la connexion");
            }
        } else {
            polls_without_page = 0;
        }

        for page in pages
            .iter()
            .filter(|p| p.url.starts_with(CLIENT_URL_PREFIX))
        {
            let Some(page_ws) = &page.ws_url else {
                continue;
            };
            if let Some(credentials) = read_credentials(page_ws, &devtools.browser_ws).await? {
                return Ok(credentials);
            }
        }
        sleep(POLL_INTERVAL).await;
    }
}

/// Returns `None` while the web client has not finished writing its config,
/// so the caller keeps polling.
async fn read_credentials(page_ws: &str, browser_ws: &str) -> Result<Option<RawCredentials>> {
    let mut page = Cdp::connect(page_ws).await?;
    let evaluated = page
        .call(
            "Runtime.evaluate",
            json!({"expression": "localStorage.getItem('localConfig_v2')", "returnByValue": true}),
        )
        .await?;
    let Some(local_config) = evaluated["result"]["value"].as_str() else {
        return Ok(None);
    };
    let tokens = tokens_from_local_config(local_config)?;
    if tokens.is_empty() {
        return Ok(None);
    }

    let mut browser = Cdp::connect(browser_ws).await?;
    let cookies = browser.call("Storage.getCookies", json!({})).await?;
    let cookie_d = cookies["cookies"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|c| {
            c["name"] == "d"
                && c["domain"]
                    .as_str()
                    .is_some_and(|d| d.ends_with("slack.com"))
        })
        .and_then(|c| c["value"].as_str());

    Ok(cookie_d.map(|cookie_d| RawCredentials {
        cookie_d: cookie_d.to_string(),
        tokens,
    }))
}

async fn close_browser(devtools: &DevTools, child: &mut Child) {
    let close = async {
        if let Ok(mut browser) = Cdp::connect(&devtools.browser_ws).await {
            // The browser usually drops the connection before answering.
            let _ = browser.call("Browser.close", json!({})).await;
        }
        let _ = child.wait().await;
    };
    if timeout(Duration::from_secs(5), close).await.is_err() {
        let _ = child.kill().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_devtools_active_port() {
        let contents = "53412\n/devtools/browser/0b7c-42\n";
        assert_eq!(
            parse_devtools_active_port(contents),
            Some((53412, "/devtools/browser/0b7c-42".to_string()))
        );
    }

    #[test]
    fn ignores_a_partially_written_port_file() {
        assert_eq!(parse_devtools_active_port("53412\n"), None);
        assert_eq!(parse_devtools_active_port(""), None);
    }

    #[tokio::test]
    #[ignore = "lance un vrai navigateur (cargo test -- --ignored)"]
    async fn drives_a_real_headless_browser() {
        let executable = find_browser().expect("aucun navigateur compatible installé");
        let profile = tempfile::tempdir().unwrap();
        let mut child = Command::new(&executable)
            .arg(format!("--user-data-dir={}", profile.path().display()))
            .arg("--remote-debugging-port=0")
            .arg("--headless=new")
            .arg("--no-first-run")
            .arg("about:blank")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();

        let devtools = wait_for_devtools(&profile.path().join("DevToolsActivePort"), &mut child)
            .await
            .unwrap();
        let targets: Vec<Target> = reqwest::Client::new()
            .get(format!("{}/json/list", devtools.http_base))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let page = targets.iter().find(|t| t.kind == "page").unwrap();

        let mut cdp = Cdp::connect(page.ws_url.as_deref().unwrap()).await.unwrap();
        let evaluated = cdp
            .call(
                "Runtime.evaluate",
                json!({"expression": "6 * 7", "returnByValue": true}),
            )
            .await
            .unwrap();
        assert_eq!(evaluated["result"]["value"], 42);

        let mut browser = Cdp::connect(&devtools.browser_ws).await.unwrap();
        let cookies = browser.call("Storage.getCookies", json!({})).await.unwrap();
        assert!(cookies["cookies"].is_array());

        close_browser(&devtools, &mut child).await;
        assert!(
            child.try_wait().unwrap().is_some(),
            "le navigateur aurait dû se fermer"
        );
    }
}

use std::fmt;

use serde::{Deserialize, Serialize};

/// Secrets captured from a browser session, not yet verified against Slack.
pub struct RawCredentials {
    pub cookie_d: String,
    pub tokens: Vec<String>,
}

/// A verified login: the shared `d` cookie plus one `xoxc` token per workspace.
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub cookie_d: String,
    pub workspaces: Vec<Workspace>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub team_id: String,
    pub team_name: String,
    pub url: String,
    pub user_id: String,
    pub user_name: String,
    pub token: String,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("cookie_d", &"<redacted>")
            .field("workspaces", &self.workspaces)
            .finish()
    }
}

impl fmt::Debug for Workspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Workspace")
            .field("team_id", &self.team_id)
            .field("team_name", &self.team_name)
            .field("url", &self.url)
            .field("user_id", &self.user_id)
            .field("user_name", &self.user_name)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Slack expects the `d` cookie URL-encoded, exactly as the browser stores it.
/// Values copied from DevTools with "show URL-decoded" ticked contain raw `/+=`,
/// so those get re-encoded here.
pub fn normalize_cookie_d(raw: &str) -> String {
    let value = raw.trim().trim_end_matches(';');
    let value = value.strip_prefix("d=").unwrap_or(value);
    if value.contains('%') {
        return value.to_string();
    }
    let mut encoded = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '/' => encoded.push_str("%2F"),
            '+' => encoded.push_str("%2B"),
            '=' => encoded.push_str("%3D"),
            c => encoded.push(c),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_an_already_encoded_cookie() {
        assert_eq!(normalize_cookie_d("xoxd-abc%2Fdef%3D"), "xoxd-abc%2Fdef%3D");
    }

    #[test]
    fn encodes_a_decoded_cookie() {
        assert_eq!(normalize_cookie_d("xoxd-ab/c+d="), "xoxd-ab%2Fc%2Bd%3D");
    }

    #[test]
    fn strips_prefix_and_whitespace() {
        assert_eq!(normalize_cookie_d("  d=xoxd-abc; \n"), "xoxd-abc");
    }

    #[test]
    fn debug_output_never_contains_secrets() {
        let session = Session {
            cookie_d: "xoxd-secret-cookie".into(),
            workspaces: vec![Workspace {
                team_id: "T1".into(),
                team_name: "acme".into(),
                url: "https://acme.slack.com/".into(),
                user_id: "U1".into(),
                user_name: "simon".into(),
                token: "xoxc-secret-token".into(),
            }],
        };
        let debug = format!("{session:?}");
        assert!(!debug.contains("xoxd-secret-cookie"));
        assert!(!debug.contains("xoxc-secret-token"));
        assert!(debug.contains("acme"));
    }
}

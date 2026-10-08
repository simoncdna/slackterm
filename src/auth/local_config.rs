use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Deserialize;

/// The web client keeps one entry per signed-in workspace in
/// `localStorage.localConfig_v2`, each with its own `xoxc` token.
#[derive(Deserialize)]
struct LocalConfig {
    #[serde(default)]
    teams: BTreeMap<String, LocalTeam>,
}

#[derive(Deserialize)]
struct LocalTeam {
    token: Option<String>,
}

pub fn tokens_from_local_config(json: &str) -> Result<Vec<String>> {
    let config: LocalConfig =
        serde_json::from_str(json).context("localConfig_v2 n'est pas au format attendu")?;
    Ok(config
        .teams
        .into_values()
        .filter_map(|team| team.token)
        .filter(|token| token.starts_with("xoxc-"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_one_token_per_team() {
        let json = r#"{
            "teams": {
                "T1": {"id": "T1", "name": "Acme", "token": "xoxc-1", "user_id": "U1"},
                "T2": {"id": "T2", "name": "Side", "token": "xoxc-2"}
            },
            "lastActiveTeamId": "T1"
        }"#;
        assert_eq!(
            tokens_from_local_config(json).unwrap(),
            vec!["xoxc-1", "xoxc-2"]
        );
    }

    #[test]
    fn skips_teams_without_a_web_token() {
        let json = r#"{"teams": {"T1": {"name": "Acme"}, "T2": {"token": "xoxp-legacy"}}}"#;
        assert!(tokens_from_local_config(json).unwrap().is_empty());
    }

    #[test]
    fn rejects_garbage() {
        assert!(tokens_from_local_config("not json").is_err());
    }
}

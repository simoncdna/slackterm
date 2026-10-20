use std::collections::{HashMap, HashSet};
use std::time::Duration;

use reqwest::StatusCode;
use reqwest::header::{COOKIE, RETRY_AFTER};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::types::{ChannelSection, Conversation, ConversationCount, Message, SearchMatch, User};

pub const DEFAULT_API_BASE: &str = "https://slack.com/api";
const MAX_RATE_LIMIT_RETRIES: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum SlackError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("unreadable response: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("Slack rejected the request: {0}")]
    Api(String),
}

/// What Slack answers once it no longer accepts the session.
const SESSION_EXPIRED: &[&str] = &[
    "invalid_auth",
    "not_authed",
    "token_revoked",
    "token_expired",
    "account_inactive",
];

impl SlackError {
    /// Slack no longer accepts the session: only signing in again helps.
    pub fn is_session_expired(&self) -> bool {
        matches!(self, Self::Api(code) if SESSION_EXPIRED.contains(&code.as_str()))
    }
}

/// Web API client authenticated as the web client is: `xoxc` token + `d` cookie.
#[derive(Clone)]
pub struct SlackClient {
    http: reqwest::Client,
    api_base: String,
    token: String,
    cookie_d: String,
}

#[derive(Debug, Deserialize)]
pub struct AuthTest {
    pub url: String,
    pub team: String,
    pub user: String,
    pub team_id: String,
    pub user_id: String,
}

/// Muted conversations, from the user's preferences: the current
/// notification settings, a JSON document in a string, and the older
/// `muted_channels` list.
fn muted_in(prefs: &Value) -> HashSet<String> {
    let mut muted: HashSet<String> = prefs["muted_channels"]
        .as_str()
        .unwrap_or_default()
        .split(',')
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .collect();
    let notifications = prefs["all_notifications_prefs"]
        .as_str()
        .and_then(|json| serde_json::from_str::<Value>(json).ok());
    if let Some(channels) = notifications
        .as_ref()
        .and_then(|n| n["channels"].as_object())
    {
        for (id, settings) in channels {
            if settings["muted"] == true {
                muted.insert(id.clone());
            }
        }
    }
    muted
}

/// Whether `url` is one of Slack's, which may receive the session.
pub fn is_slack_file_url(url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(url) else {
        return false;
    };
    url.scheme() == "https"
        && url.host_str().is_some_and(|host| {
            ["slack.com", "slack-edge.com"]
                .iter()
                .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
        })
}

pub struct HistoryPage {
    pub messages: Vec<Message>,
    pub has_more: bool,
}

#[derive(Debug, Deserialize)]
pub struct RtmConnect {
    pub url: String,
}

type Params<'a> = Vec<(&'a str, String)>;

#[derive(Deserialize)]
struct Counts {
    #[serde(default)]
    channels: Vec<ConversationCount>,
    #[serde(default)]
    ims: Vec<ConversationCount>,
    #[serde(default)]
    mpims: Vec<ConversationCount>,
}

impl SlackClient {
    pub fn new(api_base: &str, token: &str, cookie_d: &str) -> Result<Self, SlackError> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("slackterm/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self {
            http,
            api_base: api_base.trim_end_matches('/').to_string(),
            token: token.to_string(),
            cookie_d: cookie_d.to_string(),
        })
    }

    pub fn cookie_d(&self) -> &str {
        &self.cookie_d
    }

    pub async fn auth_test(&self) -> Result<AuthTest, SlackError> {
        self.call("auth.test", Vec::new()).await
    }

    pub async fn rtm_connect(&self) -> Result<RtmConnect, SlackError> {
        self.call("rtm.connect", Vec::new()).await
    }

    pub async fn users_list(&self) -> Result<Vec<User>, SlackError> {
        self.paginate("users.list", vec![("limit", "200".into())], "members")
            .await
    }

    /// Conversations the user is a member of, including DMs.
    pub async fn user_conversations(&self) -> Result<Vec<Conversation>, SlackError> {
        let params = vec![
            ("types", "public_channel,private_channel,mpim,im".into()),
            ("exclude_archived", "true".into()),
            ("limit", "200".into()),
        ];
        self.paginate("users.conversations", params, "channels")
            .await
    }

    /// Read state of the conversations the web client shows in its sidebar.
    /// Undocumented: only answers sessions captured from the browser.
    /// Every public channel of the workspace, joined or not.
    pub async fn public_channels(&self) -> Result<Vec<Conversation>, SlackError> {
        let params = vec![
            ("types", "public_channel".into()),
            ("exclude_archived", "true".into()),
            ("limit", "1000".into()),
        ];
        self.paginate("conversations.list", params, "channels")
            .await
    }

    /// The direct conversation with `user`, created if there is none yet.
    pub async fn conversations_open(&self, user: &str) -> Result<Conversation, SlackError> {
        let params = vec![("users", user.into()), ("return_im", "true".into())];
        let body = self.call_value("conversations.open", &params).await?;
        let mut conversation: Conversation = serde_json::from_value(body["channel"].clone())?;
        // Only the id is sure to come back.
        conversation.is_im = true;
        conversation.user.get_or_insert_with(|| user.to_string());
        Ok(conversation)
    }

    pub async fn conversations_join(&self, channel: &str) -> Result<Conversation, SlackError> {
        let params = vec![("channel", channel.into())];
        let body = self.call_value("conversations.join", &params).await?;
        Ok(serde_json::from_value(body["channel"].clone())?)
    }

    pub async fn client_counts(&self) -> Result<Vec<ConversationCount>, SlackError> {
        let counts: Counts = self.call("client.counts", Vec::new()).await?;
        Ok(counts
            .channels
            .into_iter()
            .chain(counts.ims)
            .chain(counts.mpims)
            .collect())
    }

    /// The user's sidebar sections, in their display order. Undocumented.
    pub async fn channel_sections(&self) -> Result<Vec<ChannelSection>, SlackError> {
        let body = self
            .call_value("users.channelSections.list", &Vec::new())
            .await?;
        let sections = serde_json::from_value(body["channel_sections"].clone())?;
        Ok(super::types::order_sections(sections))
    }

    /// The latest messages of a conversation, or the ones before `before`,
    /// oldest first.
    pub async fn conversations_history(
        &self,
        channel: &str,
        before: Option<&str>,
        limit: u32,
    ) -> Result<HistoryPage, SlackError> {
        let mut params = vec![("channel", channel.into()), ("limit", limit.to_string())];
        if let Some(before) = before {
            params.push(("latest", before.into()));
        }
        let body = self.call_value("conversations.history", &params).await?;
        let mut messages: Vec<Message> = serde_json::from_value(body["messages"].clone())?;
        messages.reverse();
        Ok(HistoryPage {
            messages,
            has_more: body["has_more"] == true,
        })
    }

    /// Messages matching a Slack search query, newest first.
    pub async fn search_messages(&self, query: &str) -> Result<Vec<SearchMatch>, SlackError> {
        let params = vec![
            ("query", query.into()),
            ("count", "50".into()),
            ("highlight", "true".into()),
            ("sort", "timestamp".into()),
            ("sort_dir", "desc".into()),
        ];
        let body = self.call_value("search.messages", &params).await?;
        Ok(serde_json::from_value(body["messages"]["matches"].clone())?)
    }

    /// A thread's parent followed by its replies, oldest first.
    pub async fn conversations_replies(
        &self,
        channel: &str,
        ts: &str,
    ) -> Result<Vec<Message>, SlackError> {
        let params = vec![
            ("channel", channel.into()),
            ("ts", ts.into()),
            ("limit", "200".into()),
        ];
        let body = self.call_value("conversations.replies", &params).await?;
        Ok(serde_json::from_value(body["messages"].clone())?)
    }

    pub async fn chat_post_message(
        &self,
        channel: &str,
        text: &str,
        thread_ts: Option<&str>,
    ) -> Result<Message, SlackError> {
        let mut params = vec![("channel", channel.into()), ("text", text.into())];
        if let Some(thread_ts) = thread_ts {
            params.push(("thread_ts", thread_ts.into()));
        }
        let body = self.call_value("chat.postMessage", &params).await?;
        Ok(serde_json::from_value(body["message"].clone())?)
    }

    pub async fn chat_update(&self, channel: &str, ts: &str, text: &str) -> Result<(), SlackError> {
        let params = vec![
            ("channel", channel.into()),
            ("ts", ts.into()),
            ("text", text.into()),
        ];
        self.call_value("chat.update", &params).await?;
        Ok(())
    }

    pub async fn chat_delete(&self, channel: &str, ts: &str) -> Result<(), SlackError> {
        let params = vec![("channel", channel.into()), ("ts", ts.into())];
        self.call_value("chat.delete", &params).await?;
        Ok(())
    }

    pub async fn reactions_add(
        &self,
        channel: &str,
        ts: &str,
        name: &str,
    ) -> Result<(), SlackError> {
        let params = vec![
            ("channel", channel.into()),
            ("timestamp", ts.into()),
            ("name", name.into()),
        ];
        self.call_value("reactions.add", &params).await?;
        Ok(())
    }

    pub async fn reactions_remove(
        &self,
        channel: &str,
        ts: &str,
        name: &str,
    ) -> Result<(), SlackError> {
        let params = vec![
            ("channel", channel.into()),
            ("timestamp", ts.into()),
            ("name", name.into()),
        ];
        self.call_value("reactions.remove", &params).await?;
        Ok(())
    }

    /// The conversations the user muted.
    pub async fn muted_channels(&self) -> Result<HashSet<String>, SlackError> {
        let body = self.call_value("users.prefs.get", &Vec::new()).await?;
        Ok(muted_in(&body["prefs"]))
    }

    /// The workspace's custom emoji: name → image URL, or `alias:<name>`.
    pub async fn emoji_list(&self) -> Result<HashMap<String, String>, SlackError> {
        let body = self.call_value("emoji.list", &Vec::new()).await?;
        Ok(serde_json::from_value(body["emoji"].clone())?)
    }

    /// Starts downloading a private file (`url_private…`, thumbnails), which
    /// needs the session. Refuses hosts other than Slack's, so the session
    /// never leaves for another site.
    pub async fn download(&self, url: &str) -> Result<reqwest::Response, SlackError> {
        if !is_slack_file_url(url) {
            return Err(SlackError::Api(format!("not a Slack address: {url}")));
        }
        Ok(self
            .http
            .get(url)
            .bearer_auth(&self.token)
            .header(COOKIE, format!("d={}", self.cookie_d))
            .send()
            .await?
            .error_for_status()?)
    }

    pub async fn conversations_mark(&self, channel: &str, ts: &str) -> Result<(), SlackError> {
        let params = vec![("channel", channel.into()), ("ts", ts.into())];
        self.call_value("conversations.mark", &params).await?;
        Ok(())
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Params<'_>,
    ) -> Result<T, SlackError> {
        Ok(serde_json::from_value(
            self.call_value(method, &params).await?,
        )?)
    }

    async fn paginate<T: DeserializeOwned>(
        &self,
        method: &str,
        params: Params<'_>,
        field: &str,
    ) -> Result<Vec<T>, SlackError> {
        let mut items = Vec::new();
        let mut cursor = String::new();
        loop {
            let mut page_params = params.clone();
            if !cursor.is_empty() {
                page_params.push(("cursor", cursor.clone()));
            }
            let body = self.call_value(method, &page_params).await?;
            items.extend(serde_json::from_value::<Vec<T>>(body[field].clone())?);
            cursor = body["response_metadata"]["next_cursor"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            if cursor.is_empty() {
                return Ok(items);
            }
        }
    }

    /// Logs the method, how long it took and why Slack refused it, but
    /// never the parameters: they hold messages.
    async fn call_value(&self, method: &str, params: &Params<'_>) -> Result<Value, SlackError> {
        let started = std::time::Instant::now();
        let mut attempt = 0;
        loop {
            let sent = self
                .http
                .post(format!("{}/{method}", self.api_base))
                .bearer_auth(&self.token)
                .header(COOKIE, format!("d={}", self.cookie_d))
                .form(params)
                .send()
                .await;
            let response = match sent {
                Ok(response) => response,
                Err(e) => {
                    tracing::warn!(method, error = %e, "slack unreachable");
                    return Err(e.into());
                }
            };

            if response.status() == StatusCode::TOO_MANY_REQUESTS
                && attempt < MAX_RATE_LIMIT_RETRIES
            {
                attempt += 1;
                let wait = response
                    .headers()
                    .get(RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1);
                tracing::warn!(method, wait, "rate limited by slack");
                tokio::time::sleep(Duration::from_secs(wait)).await;
                continue;
            }

            let body: Value = response.error_for_status()?.json().await?;
            let ms = started.elapsed().as_millis() as u64;
            if body["ok"] != true {
                let error = body["error"].as_str().unwrap_or("unknown_error");
                tracing::warn!(method, error, ms, "slack refused");
                return Err(SlackError::Api(error.to_string()));
            }
            tracing::debug!(method, ms, "slack");
            return Ok(body);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(server: &MockServer) -> SlackClient {
        SlackClient::new(&server.uri(), "xoxc-1", "xoxd-abc").unwrap()
    }

    #[tokio::test]
    async fn auth_test_sends_token_and_cookie() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth.test"))
            .and(header("authorization", "Bearer xoxc-1"))
            .and(header("cookie", "d=xoxd-abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "url": "https://acme.slack.com/",
                "team": "Acme",
                "user": "simon",
                "team_id": "T1",
                "user_id": "U1"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let auth = client(&server).auth_test().await.unwrap();

        assert_eq!(auth.team, "Acme");
        assert_eq!(auth.user_id, "U1");
    }

    #[tokio::test]
    async fn opening_a_direct_conversation_knows_its_member() {
        let server = MockServer::start().await;
        Mock::given(path("/conversations.open"))
            .and(body_string_contains("users=U2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true, "channel": {"id": "D9"}
            })))
            .mount(&server)
            .await;

        let conversation = client(&server).conversations_open("U2").await.unwrap();

        assert_eq!(conversation.id, "D9");
        assert!(conversation.is_im);
        assert_eq!(conversation.user.as_deref(), Some("U2"));
    }

    #[test]
    fn reads_muted_conversations_from_both_preferences() {
        let prefs = serde_json::json!({
            "muted_channels": "C1,C2",
            "all_notifications_prefs":
                "{\"channels\":{\"C3\":{\"muted\":true},\"C4\":{\"muted\":false}}}"
        });
        let mut muted: Vec<String> = muted_in(&prefs).into_iter().collect();
        muted.sort();
        assert_eq!(muted, ["C1", "C2", "C3"]);
        assert!(muted_in(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn knows_when_the_session_is_over() {
        assert!(SlackError::Api("invalid_auth".into()).is_session_expired());
        assert!(SlackError::Api("token_revoked".into()).is_session_expired());
        assert!(!SlackError::Api("channel_not_found".into()).is_session_expired());
    }

    #[tokio::test]
    async fn api_errors_are_surfaced() {
        let server = MockServer::start().await;
        Mock::given(path("/auth.test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"ok": false, "error": "invalid_auth"})),
            )
            .mount(&server)
            .await;

        let err = client(&server).auth_test().await.unwrap_err();

        assert!(matches!(err, SlackError::Api(ref e) if e == "invalid_auth"));
    }

    #[tokio::test]
    async fn follows_pagination_cursors() {
        let server = MockServer::start().await;
        Mock::given(path("/users.list"))
            .and(body_string_contains("cursor=next"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "members": [{"id": "U2", "name": "b"}],
                "response_metadata": {"next_cursor": ""}
            })))
            .mount(&server)
            .await;
        Mock::given(path("/users.list"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "members": [{"id": "U1", "name": "a"}],
                "response_metadata": {"next_cursor": "next"}
            })))
            .mount(&server)
            .await;

        let users = client(&server).users_list().await.unwrap();

        let ids: Vec<&str> = users.iter().map(|u| u.id.as_str()).collect();
        assert_eq!(ids, ["U1", "U2"]);
    }

    #[tokio::test]
    async fn history_is_returned_oldest_first() {
        let server = MockServer::start().await;
        Mock::given(path("/conversations.history"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "messages": [
                    {"ts": "2.0", "user": "U1", "text": "second"},
                    {"ts": "1.0", "user": "U1", "text": "first"}
                ]
            })))
            .mount(&server)
            .await;

        let page = client(&server)
            .conversations_history("C1", None, 50)
            .await
            .unwrap();

        assert_eq!(page.messages[0].text, "first");
        assert!(!page.has_more);
    }

    #[test]
    fn only_slack_hosts_get_the_session() {
        assert!(is_slack_file_url(
            "https://files.slack.com/files-pri/T1-F1/demo.mp4"
        ));
        assert!(is_slack_file_url("https://acme.slack.com/files/x"));
        assert!(!is_slack_file_url("https://evilslack.com/x"));
        assert!(!is_slack_file_url("https://files.slack.com.evil.dev/x"));
        assert!(!is_slack_file_url("http://files.slack.com/x"));
    }

    #[tokio::test]
    async fn retries_after_a_rate_limit() {
        let server = MockServer::start().await;
        Mock::given(path("/auth.test"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(path("/auth.test"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true, "url": "u", "team": "Acme", "user": "s", "team_id": "T", "user_id": "U"
            })))
            .mount(&server)
            .await;

        assert!(client(&server).auth_test().await.is_ok());
    }
}

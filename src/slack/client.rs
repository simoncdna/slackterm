use std::collections::HashMap;
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
    #[error("erreur réseau : {0}")]
    Http(#[from] reqwest::Error),
    #[error("réponse illisible : {0}")]
    Decode(#[from] serde_json::Error),
    #[error("Slack a refusé la requête : {0}")]
    Api(String),
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

    /// The workspace's custom emoji: name → image URL, or `alias:<name>`.
    pub async fn emoji_list(&self) -> Result<HashMap<String, String>, SlackError> {
        let body = self.call_value("emoji.list", &Vec::new()).await?;
        Ok(serde_json::from_value(body["emoji"].clone())?)
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

    async fn call_value(&self, method: &str, params: &Params<'_>) -> Result<Value, SlackError> {
        let mut attempt = 0;
        loop {
            let response = self
                .http
                .post(format!("{}/{method}", self.api_base))
                .bearer_auth(&self.token)
                .header(COOKIE, format!("d={}", self.cookie_d))
                .form(params)
                .send()
                .await?;

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
                tokio::time::sleep(Duration::from_secs(wait)).await;
                continue;
            }

            let body: Value = response.error_for_status()?.json().await?;
            if body["ok"] != true {
                let error = body["error"].as_str().unwrap_or("unknown_error");
                return Err(SlackError::Api(error.to_string()));
            }
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

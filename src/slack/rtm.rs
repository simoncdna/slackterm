//! Real-time events over the websocket returned by `rtm.connect`, with
//! automatic reconnection.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use reqwest::header::COOKIE;
use serde_json::{Value, json};
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use super::{Message, SlackClient};

const PING_INTERVAL: Duration = Duration::from_secs(30);
const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub enum RtmEvent {
    Connected,
    Reconnecting {
        in_secs: u64,
        reason: String,
    },
    Message {
        channel: String,
        message: Message,
    },
    MessageChanged {
        channel: String,
        message: Message,
    },
    MessageDeleted {
        channel: String,
        ts: String,
    },
    ReactionAdded {
        channel: String,
        ts: String,
        name: String,
        user: String,
    },
    ReactionRemoved {
        channel: String,
        ts: String,
        name: String,
        user: String,
    },
}

pub fn spawn(client: SlackClient, events: UnboundedSender<RtmEvent>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff = MIN_BACKOFF;
        loop {
            let (greeted, reason) = run(&client, &events).await;
            if events.is_closed() {
                return;
            }
            if greeted {
                backoff = MIN_BACKOFF;
            }
            let _ = events.send(RtmEvent::Reconnecting {
                in_secs: backoff.as_secs(),
                reason,
            });
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(MAX_BACKOFF);
        }
    })
}

/// Runs one connection until it drops. Returns whether Slack said hello (the
/// connection was healthy, so the backoff can reset) and why it ended.
async fn run(client: &SlackClient, events: &UnboundedSender<RtmEvent>) -> (bool, String) {
    let rtm = match client.rtm_connect().await {
        Ok(rtm) => rtm,
        Err(e) => return (false, e.to_string()),
    };
    let mut request = match rtm.url.as_str().into_client_request() {
        Ok(request) => request,
        Err(e) => return (false, e.to_string()),
    };
    if let Ok(cookie) = format!("d={}", client.cookie_d()).parse() {
        request.headers_mut().insert(COOKIE, cookie);
    }
    let mut socket = match connect_async(request).await {
        Ok((socket, _)) => socket,
        Err(e) => return (false, e.to_string()),
    };

    let mut greeted = false;
    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.tick().await;
    let mut next_id: u64 = 1;

    loop {
        tokio::select! {
            incoming = socket.next() => {
                let message = match incoming {
                    Some(Ok(message)) => message,
                    Some(Err(e)) => return (greeted, e.to_string()),
                    None => return (greeted, "connexion fermée".into()),
                };
                let WsMessage::Text(text) = message else { continue };
                let Ok(value) = serde_json::from_str::<Value>(&text) else { continue };
                let event = match value["type"].as_str() {
                    Some("hello") => {
                        greeted = true;
                        Some(RtmEvent::Connected)
                    }
                    Some("goodbye") => return (greeted, "Slack a demandé une reconnexion".into()),
                    _ => parse(&value),
                };
                if let Some(event) = event
                    && events.send(event).is_err()
                {
                    return (greeted, "application fermée".into());
                }
            }
            _ = ping.tick() => {
                let ping = json!({"id": next_id, "type": "ping"}).to_string();
                next_id += 1;
                if let Err(e) = socket.send(WsMessage::text(ping)).await {
                    return (greeted, e.to_string());
                }
            }
        }
    }
}

pub fn parse(event: &Value) -> Option<RtmEvent> {
    match event["type"].as_str()? {
        "message" => {
            let channel = event["channel"].as_str()?.to_string();
            match event["subtype"].as_str() {
                Some("message_changed") | Some("message_replied") => {
                    Some(RtmEvent::MessageChanged {
                        channel,
                        message: serde_json::from_value(event["message"].clone()).ok()?,
                    })
                }
                Some("message_deleted") => Some(RtmEvent::MessageDeleted {
                    channel,
                    ts: event["deleted_ts"].as_str()?.to_string(),
                }),
                _ => Some(RtmEvent::Message {
                    channel,
                    message: serde_json::from_value(event.clone()).ok()?,
                }),
            }
        }
        kind @ ("reaction_added" | "reaction_removed") => {
            let item = &event["item"];
            if item["type"] != "message" {
                return None;
            }
            let channel = item["channel"].as_str()?.to_string();
            let ts = item["ts"].as_str()?.to_string();
            let name = event["reaction"].as_str()?.to_string();
            let user = event["user"].as_str()?.to_string();
            Some(if kind == "reaction_added" {
                RtmEvent::ReactionAdded {
                    channel,
                    ts,
                    name,
                    user,
                }
            } else {
                RtmEvent::ReactionRemoved {
                    channel,
                    ts,
                    name,
                    user,
                }
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_new_message() {
        let event =
            json!({"type": "message", "channel": "C1", "user": "U1", "text": "hi", "ts": "1.0"});
        let Some(RtmEvent::Message { channel, message }) = parse(&event) else {
            panic!("expected a message");
        };
        assert_eq!(channel, "C1");
        assert_eq!(message.text, "hi");
    }

    #[test]
    fn parses_edits_and_deletions() {
        let edit = json!({
            "type": "message", "subtype": "message_changed", "channel": "C1",
            "message": {"ts": "1.0", "user": "U1", "text": "fixed", "edited": {"user": "U1"}}
        });
        assert!(matches!(
            parse(&edit),
            Some(RtmEvent::MessageChanged { message, .. }) if message.text == "fixed"
        ));

        let deletion = json!({"type": "message", "subtype": "message_deleted", "channel": "C1", "deleted_ts": "1.0"});
        assert!(matches!(
            parse(&deletion),
            Some(RtmEvent::MessageDeleted { ts, .. }) if ts == "1.0"
        ));
    }

    #[test]
    fn parses_reactions_on_messages_only() {
        let added = json!({
            "type": "reaction_added", "user": "U1", "reaction": "+1",
            "item": {"type": "message", "channel": "C1", "ts": "1.0"}
        });
        assert!(
            matches!(parse(&added), Some(RtmEvent::ReactionAdded { name, .. }) if name == "+1")
        );

        let on_file = json!({
            "type": "reaction_added", "user": "U1", "reaction": "+1",
            "item": {"type": "file", "file": "F1"}
        });
        assert!(parse(&on_file).is_none());
    }

    #[test]
    fn ignores_other_events() {
        assert!(parse(&json!({"type": "user_typing", "channel": "C1"})).is_none());
    }
}

//! A made-up workspace that answers the interface's commands as Slack
//! would, to try slackterm or film it without an account. Nothing leaves
//! the process and nothing is written to disk.

mod content;

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

use crate::session::Workspace;
use crate::slack::rtm::RtmEvent;
use crate::slack::{HIGHLIGHT_END, HIGHLIGHT_START, Message, SearchMatch};

use super::update::{ApiEvent, Command, Event};
use content::{Content, ME};

/// Slack answers in a blink; the demo too, but loading states still show.
const LATENCY: Duration = Duration::from_millis(120);
/// How long others take to react to what the user sends.
const REACTION_DELAY: Duration = Duration::from_millis(1500);
const REPLY_DELAY: Duration = Duration::from_millis(2500);
const REACTIONS: [&str; 4] = ["raised_hands", "+1", "heart", "rocket"];

pub fn workspace() -> Workspace {
    Workspace {
        team_id: "T0DEMO".into(),
        team_name: "Atelier Nova".into(),
        url: "https://atelier-nova.slack.com/".into(),
        user_id: ME.into(),
        user_name: "alex".into(),
        token: String::new(),
    }
}

#[derive(Clone)]
pub struct Demo {
    state: Arc<Mutex<State>>,
}

struct State {
    content: Content,
    /// Messages the user sent, to vary the reactions they get.
    sent: usize,
}

impl Demo {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                content: content::build(),
                sent: 0,
            })),
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        // The state stays consistent even if a task panicked holding it.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs a command in the background and reports its result as an
    /// event, like the Slack executor.
    pub fn execute(&self, command: Command, events: &UnboundedSender<Event>) {
        let demo = self.clone();
        let events = events.clone();
        tokio::spawn(async move {
            tokio::time::sleep(LATENCY).await;
            demo.run(command, &events).await;
        });
    }

    /// Connects, then lets the scripted messages arrive one by one.
    pub fn spawn_live(&self, events: UnboundedSender<Event>) -> JoinHandle<()> {
        let demo = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            let _ = events.send(Event::Rtm(RtmEvent::Connected));
            let script = std::mem::take(&mut demo.state().content.live);
            let mut elapsed = Duration::ZERO;
            for scheduled in script {
                tokio::time::sleep(scheduled.after.saturating_sub(elapsed)).await;
                elapsed = scheduled.after;
                let mut message = scheduled.message;
                message.ts = now_ts();
                let stored = demo.state().store(&scheduled.channel, message);
                for event in stored {
                    let _ = events.send(event);
                }
            }
        })
    }

    async fn run(&self, command: Command, events: &UnboundedSender<Event>) {
        let send = |event: ApiEvent| {
            let _ = events.send(Event::Api(event));
        };
        match command {
            Command::LoadUsers => send(ApiEvent::Users(self.state().content.users.clone())),
            Command::LoadConversations => send(ApiEvent::Conversations(
                self.state().content.conversations.clone(),
            )),
            Command::LoadCounts => send(ApiEvent::Counts(self.state().content.counts.clone())),
            Command::LoadSections => {
                send(ApiEvent::Sections(self.state().content.sections.clone()))
            }
            Command::LoadHistory(channel) => {
                let messages = self
                    .state()
                    .messages(&channel)
                    .iter()
                    .filter(|m| !m.is_thread_reply())
                    .cloned()
                    .collect();
                send(ApiEvent::History {
                    channel,
                    messages,
                    has_more: false,
                });
            }
            // The whole history is sent at once.
            Command::LoadOlder { channel, .. } => send(ApiEvent::Older {
                channel,
                messages: Vec::new(),
                has_more: false,
            }),
            Command::LoadReplies { channel, ts } => {
                let messages = self
                    .state()
                    .messages(&channel)
                    .iter()
                    .filter(|m| m.ts == ts || m.thread_ts.as_deref() == Some(&ts))
                    .cloned()
                    .collect();
                send(ApiEvent::Replies {
                    channel,
                    ts,
                    messages,
                });
            }
            Command::Search(query) => {
                let result = Ok(self.state().search(&query));
                send(ApiEvent::SearchResults { query, result });
            }
            Command::Send {
                channel,
                text,
                thread_ts,
            } => self.send(channel, text, thread_ts, events).await,
            Command::LoadEmoji => {
                let custom = ["atelier", "shipit", "party-parrot"]
                    .into_iter()
                    .map(|name| {
                        let url = format!("https://emoji.atelier-nova.fr/{name}.png");
                        (name.to_string(), url)
                    })
                    .collect();
                send(ApiEvent::CustomEmoji(custom));
            }
            // The interface already shows the reaction: keep it for reloads.
            Command::React {
                channel,
                ts,
                name,
                add,
            } => {
                if let Some(message) = self.state().find(&channel, &ts) {
                    if add {
                        message.add_reaction(&name, ME);
                    } else {
                        message.remove_reaction(&name, ME);
                    }
                }
            }
            Command::MarkRead { .. } | Command::SaveSettings(_) | Command::SaveSidebar(_) => {}
        }
    }

    /// Posts the user's message, then has someone answer it: the other
    /// member of a direct message replies, elsewhere someone reacts.
    async fn send(
        &self,
        channel: String,
        text: String,
        thread_ts: Option<String>,
        events: &UnboundedSender<Event>,
    ) {
        let mut message = content::message(&now_ts(), ME, &text);
        message.thread_ts = thread_ts.clone();
        let (stored, partner, reactor, sent) = {
            let mut state = self.state();
            let stored = state.store(&channel, message.clone());
            state.sent += 1;
            let partner = state.direct_partner(&channel);
            let reactor = state.last_other_author(&channel, thread_ts.as_deref());
            (stored, partner, reactor, state.sent)
        };
        // The interface shows its own message from `Sent`, not from the
        // real-time event Slack would also send.
        let _ = events.send(Event::Api(ApiEvent::Sent {
            channel: channel.clone(),
            message: message.clone(),
        }));
        for event in stored.into_iter().skip(1) {
            let _ = events.send(event);
        }

        match partner {
            Some(partner) => {
                tokio::time::sleep(REPLY_DELAY).await;
                let mut reply =
                    content::message(&now_ts(), &partner, content::reply_from(&partner));
                reply.thread_ts = thread_ts;
                let stored = self.state().store(&channel, reply);
                for event in stored {
                    let _ = events.send(event);
                }
            }
            None => {
                tokio::time::sleep(REACTION_DELAY).await;
                let name = REACTIONS[(sent - 1) % REACTIONS.len()].to_string();
                if let Some(target) = self.state().find(&channel, &message.ts) {
                    target.add_reaction(&name, &reactor);
                }
                let _ = events.send(Event::Rtm(RtmEvent::ReactionAdded {
                    channel,
                    ts: message.ts,
                    name,
                    user: reactor,
                }));
            }
        }
    }
}

impl State {
    fn messages(&self, channel: &str) -> &[Message] {
        self.content
            .messages
            .get(channel)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn find(&mut self, channel: &str, ts: &str) -> Option<&mut Message> {
        self.content
            .messages
            .get_mut(channel)?
            .iter_mut()
            .find(|m| m.ts == ts)
    }

    /// Keeps a new message and returns what Slack would announce: the
    /// message, then its thread's parent with one more reply.
    fn store(&mut self, channel: &str, message: Message) -> Vec<Event> {
        let mut events = vec![Event::Rtm(RtmEvent::Message {
            channel: channel.to_string(),
            message: message.clone(),
        })];
        let parent_ts = message.thread_ts.clone();
        self.content
            .messages
            .entry(channel.to_string())
            .or_default()
            .push(message);
        if let Some(parent_ts) = parent_ts
            && let Some(parent) = self.find(channel, &parent_ts)
        {
            parent.thread_ts = Some(parent.ts.clone());
            parent.reply_count += 1;
            events.push(Event::Rtm(RtmEvent::MessageChanged {
                channel: channel.to_string(),
                message: parent.clone(),
            }));
        }
        events
    }

    fn direct_partner(&self, channel: &str) -> Option<String> {
        self.content
            .conversations
            .iter()
            .find(|c| c.id == channel && c.is_im)
            .and_then(|c| c.user.clone())
    }

    /// The latest person other than the user to speak in the conversation,
    /// or in the thread when `thread_ts` is given.
    fn last_other_author(&self, channel: &str, thread_ts: Option<&str>) -> String {
        self.messages(channel)
            .iter()
            .rev()
            .filter(|m| match thread_ts {
                Some(ts) => m.ts == ts || m.thread_ts.as_deref() == Some(ts),
                None => !m.is_thread_reply(),
            })
            .filter_map(|m| m.user.clone())
            .find(|user| user != ME)
            .unwrap_or_else(|| content::FALLBACK_REACTOR.to_string())
    }

    /// Messages holding every word of the query, newest first. Supports
    /// `in:#channel` and `from:@person`, the way Slack does.
    fn search(&self, query: &str) -> Vec<SearchMatch> {
        let mut terms = Vec::new();
        let mut place = None;
        let mut author = None;
        for word in query.split_whitespace() {
            if let Some(name) = word.strip_prefix("in:") {
                place = Some(name.trim_start_matches(['#', '@']).to_lowercase());
            } else if let Some(name) = word.strip_prefix("from:") {
                author = Some(name.trim_start_matches('@').to_lowercase());
            } else {
                terms.push(word.trim_matches('"').to_lowercase());
            }
        }

        let mut found = Vec::new();
        for conversation in &self.content.conversations {
            let name = conversation.name.clone().unwrap_or_default();
            let partner = conversation.user.as_deref().map(|u| self.user_handle(u));
            let shown = partner.clone().unwrap_or_else(|| name.clone());
            if place.as_ref().is_some_and(|place| *place != shown) {
                continue;
            }
            for message in self.messages(&conversation.id) {
                let handle = message.user.as_deref().map(|u| self.user_handle(u));
                if author.is_some() && handle != author {
                    continue;
                }
                let Some(text) = highlight(&message.text, &terms) else {
                    continue;
                };
                found.push(self.search_match(conversation, message, text));
            }
        }
        found.sort_by(|a, b| b.ts.cmp(&a.ts));
        found
    }

    fn user_handle(&self, id: &str) -> String {
        self.content
            .users
            .iter()
            .find(|u| u.id == id)
            .map(|u| u.name.clone())
            .unwrap_or_default()
    }

    fn search_match(
        &self,
        conversation: &crate::slack::Conversation,
        message: &Message,
        text: String,
    ) -> SearchMatch {
        let id = &conversation.id;
        let mut permalink = format!(
            "https://atelier-nova.slack.com/archives/{id}/p{}",
            message.ts.replace('.', "")
        );
        if let Some(thread) = &message.thread_ts {
            permalink.push_str(&format!("?thread_ts={thread}&cid={id}"));
        }
        let channel_name = match &conversation.user {
            // Search names direct messages after the other member's id.
            Some(user) => user.clone(),
            None => conversation.name.clone().unwrap_or_default(),
        };
        serde_json::from_value(serde_json::json!({
            "ts": message.ts,
            "user": message.user,
            "username": message.username,
            "text": text,
            "permalink": permalink,
            "channel": {
                "id": id,
                "name": channel_name,
                "is_im": conversation.is_im,
                "is_mpim": conversation.is_mpim,
                "is_private": conversation.is_private,
            },
        }))
        .expect("a search match has Slack's shape")
    }
}

/// Wraps each term found in `text` in highlight markers, or `None` when a
/// term is missing.
fn highlight(text: &str, terms: &[String]) -> Option<String> {
    let lower = text.to_lowercase();
    let mut ranges = Vec::new();
    for term in terms.iter().filter(|t| !t.is_empty()) {
        let found: Vec<(usize, usize)> = lower
            .match_indices(term.as_str())
            .map(|(start, matched)| (start, start + matched.len()))
            .collect();
        if found.is_empty() {
            return None;
        }
        ranges.extend(found);
    }
    // Lowercasing can change byte lengths: highlight only when it did not.
    if lower.len() != text.len() {
        return Some(text.to_string());
    }
    ranges.sort();
    let mut out = String::with_capacity(text.len() + ranges.len() * 6);
    let mut at = 0;
    for (start, end) in ranges {
        if start < at {
            continue;
        }
        out.push_str(&text[at..start]);
        out.push(HIGHLIGHT_START);
        out.push_str(&text[start..end]);
        out.push(HIGHLIGHT_END);
        at = end;
    }
    out.push_str(&text[at..]);
    Some(out)
}

/// A Slack timestamp for the current instant.
fn now_ts() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:06}", now.as_secs(), now.subsec_micros())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_every_term_case_insensitively() {
        let terms = vec!["migration".to_string(), "prod".to_string()];
        let text = highlight("La Migration passe en prod", &terms).unwrap();
        assert_eq!(
            text,
            "La \u{E000}Migration\u{E001} passe en \u{E000}prod\u{E001}"
        );
        assert!(highlight("rien à voir", &terms).is_none());
    }

    #[test]
    fn search_filters_by_channel_and_author() {
        let state = State {
            content: content::build(),
            sent: 0,
        };
        let in_dev = state.search("migration in:#dev");
        assert!(!in_dev.is_empty());
        assert!(in_dev.iter().all(|m| m.channel.name == "dev"));

        let from_hugo = state.search("from:@hugo");
        assert!(!from_hugo.is_empty());
        assert!(
            from_hugo
                .iter()
                .all(|m| m.user.as_deref() == Some("U02HUG"))
        );
    }

    #[test]
    fn search_points_thread_replies_to_their_parent() {
        let state = State {
            content: content::build(),
            sent: 0,
        };
        let found = state.search("notifications push");
        assert_eq!(found.len(), 1);
        assert!(found[0].thread_ts().is_some());
    }

    #[test]
    fn a_reply_bumps_its_parent() {
        let mut state = State {
            content: content::build(),
            sent: 0,
        };
        let parent = state
            .messages("C04DEV")
            .iter()
            .find(|m| m.reply_count == 0 && m.user.as_deref() == Some("U02HUG"))
            .cloned()
            .unwrap();
        let mut reply = content::message("9999999999.000001", ME, "ok");
        reply.thread_ts = Some(parent.ts.clone());

        let events = state.store("C04DEV", reply);

        assert_eq!(events.len(), 2);
        assert_eq!(state.find("C04DEV", &parent.ts).unwrap().reply_count, 1);
    }
}

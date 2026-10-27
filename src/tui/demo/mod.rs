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
use crate::slack::{Conversation, File, HIGHLIGHT_END, HIGHLIGHT_START, Message, SearchMatch};

use super::update::{ApiEvent, Command, Event};
use content::{Content, ME};

/// Slack answers in a blink; the demo too, but loading states still show.
const LATENCY: Duration = Duration::from_millis(120);
/// How long others take to react to what the user sends.
const REACTION_DELAY: Duration = Duration::from_millis(1500);
const REPLY_DELAY: Duration = Duration::from_millis(2500);
const REACTIONS: [&str; 4] = ["raised_hands", "+1", "heart", "rocket"];

pub fn side_workspace() -> Workspace {
    Workspace {
        team_id: "T0SIDE".into(),
        team_name: "Weekend Builders".into(),
        url: "https://weekend-builders.slack.com/".into(),
        user_id: ME.into(),
        user_name: "alex".into(),
        token: String::new(),
    }
}

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
        Self::with(content::build())
    }

    /// The second workspace, to switch to.
    pub fn side() -> Self {
        Self::with(content::build_side())
    }

    fn with(content: Content) -> Self {
        Self {
            state: Arc::new(Mutex::new(State { content, sent: 0 })),
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
            } => {
                self.send(channel, text, thread_ts, Vec::new(), events)
                    .await
            }
            Command::Upload {
                channel,
                text,
                thread_ts,
                files,
            } => {
                let files = files
                    .iter()
                    .map(|path| File {
                        name: path
                            .file_name()
                            .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                        size: std::fs::metadata(path).map_or(0, |m| m.len()),
                        ..File::default()
                    })
                    .collect::<Vec<_>>();
                let names: Vec<String> = files.iter().map(|file| file.name.clone()).collect();
                send(ApiEvent::Notice(format!("sent {}", names.join(", "))));
                self.send(channel, text, thread_ts, files, events).await;
            }
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
            // Shrunk like the Slack executor's, so they look the same.
            Command::LoadAvatars(pictures) => {
                for (user, _) in pictures {
                    let image = content::avatar(&user)
                        .and_then(|bytes| image::load_from_memory(bytes).ok())
                        .map(|image| image.thumbnail(64, 64));
                    if let Some(image) = image {
                        send(ApiEvent::Picture { key: user, image });
                    }
                }
            }
            // No files in the demo to preview.
            Command::LoadPreviews(_) => {}
            Command::OpenFiles(_) | Command::OpenUrl(_) | Command::DownloadToPlay(_) => {
                send(ApiEvent::Notice("files in the demo can't be opened".into()))
            }
            // Run by the event loop, which never hands it to the demo.
            Command::Play(_) => {}
            Command::MarkRead { .. }
            | Command::SaveSettings { .. }
            | Command::SaveSidebar { .. }
            | Command::NextWorkspace
            | Command::SwitchWorkspace(_)
            | Command::RememberWorkspace(_)
            | Command::NextUnreadWorkspace
            | Command::Copy { .. }
            | Command::Notify { .. } => {}
            Command::LoadMuted => send(ApiEvent::Muted(content::muted())),
            Command::LoadPublicChannels => send(ApiEvent::PublicChannels(
                self.state().content.public_channels.clone(),
            )),
            Command::OpenDirect(user) => send(ApiEvent::Opened(self.state().open_direct(&user))),
            Command::Edit { channel, ts, text } => {
                if let Some(message) = self.state().find(&channel, &ts) {
                    message.text = text.clone();
                    message.edited = Some(serde_json::json!({ "user": ME }));
                }
                send(ApiEvent::Edited { channel, ts, text });
            }
            Command::Delete { channel, ts } => {
                if let Some(messages) = self.state().content.messages.get_mut(&channel) {
                    messages.retain(|m| m.ts != ts);
                }
                send(ApiEvent::Deleted { channel, ts });
            }
            Command::Join(channel) => {
                if let Some(conversation) = self.state().join(&channel) {
                    send(ApiEvent::Opened(conversation));
                }
            }
        }
    }

    /// Posts the user's message, then has someone answer it: the other
    /// member of a direct message replies, elsewhere someone reacts.
    async fn send(
        &self,
        channel: String,
        text: String,
        thread_ts: Option<String>,
        files: Vec<File>,
        events: &UnboundedSender<Event>,
    ) {
        let mut message = content::message(&now_ts(), ME, &text);
        message.thread_ts = thread_ts.clone();
        message.files = files;
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

    /// The direct conversation with `user`, started the first time.
    fn open_direct(&mut self, user: &str) -> Conversation {
        let conversations = &mut self.content.conversations;
        if let Some(existing) = conversations
            .iter()
            .find(|c| c.is_im && c.user.as_deref() == Some(user))
        {
            return existing.clone();
        }
        let conversation = content::direct_conversation(user);
        conversations.push(conversation.clone());
        conversation
    }

    fn join(&mut self, channel: &str) -> Option<Conversation> {
        let conversation = self
            .content
            .public_channels
            .iter()
            .find(|c| c.id == channel)?
            .clone();
        if !self.content.conversations.iter().any(|c| c.id == channel) {
            self.content.conversations.push(conversation.clone());
        }
        Some(conversation)
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
        let text = highlight("The Migration goes to prod", &terms).unwrap();
        assert_eq!(
            text,
            "The \u{E000}Migration\u{E001} goes to \u{E000}prod\u{E001}"
        );
        assert!(highlight("nothing to see here", &terms).is_none());
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
        let found = state.search("push notifications");
        assert_eq!(found.len(), 1);
        assert!(found[0].thread_ts().is_some());
    }

    #[test]
    fn every_member_has_a_profile_picture() {
        for user in content::build().users {
            assert!(user.profile.image_72.is_some(), "{}", user.id);
            let bytes = content::avatar(&user.id).unwrap();
            assert!(image::load_from_memory(bytes).is_ok(), "{}", user.id);
        }
    }

    #[test]
    fn the_second_workspace_waits_for_the_user() {
        let side = content::build_side();
        let ideas = side.counts.iter().find(|c| c.id == "C12IDE").unwrap();
        assert_eq!(ideas.mention_count, 1);
        assert!(
            side.messages["C12IDE"]
                .iter()
                .any(|m| m.text.contains("<@U0ME>"))
        );
        assert!(side.users.iter().any(|u| u.name == "maya"));
    }

    #[test]
    fn starts_a_direct_conversation_once() {
        let demo = Demo::new();
        let first = demo.state().open_direct("U05SAR");
        let again = demo.state().open_direct("U05SAR");
        assert_eq!(first.id, again.id);
        let copies = demo
            .state()
            .content
            .conversations
            .iter()
            .filter(|c| c.id == first.id)
            .count();
        assert_eq!(copies, 1);
        // Camille already has one.
        assert_eq!(demo.state().open_direct("U01CAM").id, "D01CAM");
    }

    #[test]
    fn joins_the_channels_not_joined_yet() {
        let demo = Demo::new();
        let joinable: Vec<String> = {
            let state = demo.state();
            let joined: Vec<&str> = state
                .content
                .conversations
                .iter()
                .map(|c| c.id.as_str())
                .collect();
            state
                .content
                .public_channels
                .iter()
                .filter(|c| !joined.contains(&c.id.as_str()))
                .map(|c| c.id.clone())
                .collect()
        };
        assert_eq!(joinable.len(), 2);
        let joined = demo.state().join(&joinable[0]).unwrap();
        assert!(
            demo.state()
                .content
                .conversations
                .iter()
                .any(|c| c.id == joined.id)
        );
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

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub real_name: Option<String>,
    #[serde(default)]
    pub profile: UserProfile,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct UserProfile {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub real_name: String,
}

impl User {
    /// The name Slack shows in the UI: display name, else real name, else handle.
    pub fn display_name(&self) -> &str {
        [
            self.profile.display_name.as_str(),
            self.profile.real_name.as_str(),
            self.real_name.as_deref().unwrap_or(""),
        ]
        .into_iter()
        .find(|name| !name.is_empty())
        .unwrap_or(&self.name)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Conversation {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub is_im: bool,
    #[serde(default)]
    pub is_mpim: bool,
    #[serde(default)]
    pub is_private: bool,
    /// The other member of a direct message.
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub topic: Option<Topic>,
}

/// A message found by `search.messages`. Matched terms are wrapped in
/// [`HIGHLIGHT_START`] and [`HIGHLIGHT_END`].
#[derive(Debug, Clone, Deserialize)]
pub struct SearchMatch {
    pub ts: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub permalink: String,
    pub channel: SearchChannel,
}

pub const HIGHLIGHT_START: char = '\u{E000}';
pub const HIGHLIGHT_END: char = '\u{E001}';

#[derive(Debug, Clone, Deserialize)]
pub struct SearchChannel {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub is_im: bool,
    #[serde(default)]
    pub is_mpim: bool,
    #[serde(default)]
    pub is_private: bool,
}

impl SearchMatch {
    /// The parent of the thread the message belongs to, read from its
    /// permalink (`…?thread_ts=1700000000.000100&cid=C123`).
    pub fn thread_ts(&self) -> Option<String> {
        let query = self.permalink.split_once('?')?.1;
        query
            .split('&')
            .find_map(|pair| pair.strip_prefix("thread_ts="))
            .map(str::to_string)
            .filter(|thread| *thread != self.ts)
    }

    pub fn epoch_seconds(&self) -> i64 {
        self.ts
            .split('.')
            .next()
            .and_then(|secs| secs.parse().ok())
            .unwrap_or(0)
    }
}

/// A sidebar section arranged by the user in the web client.
#[derive(Debug, Clone, Deserialize)]
pub struct ChannelSection {
    pub channel_section_id: String,
    /// `standard` for the user's own sections, else `channels`,
    /// `direct_messages`, `stars`…
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub emoji: String,
    #[serde(default)]
    pub channel_ids_page: ChannelIdsPage,
    #[serde(default)]
    pub next_channel_section_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChannelIdsPage {
    #[serde(default)]
    pub channel_ids: Vec<String>,
}

/// Sections form a linked list through `next_channel_section_id`; returns
/// them in that order, followed by any the chain does not reach.
pub fn order_sections(mut sections: Vec<ChannelSection>) -> Vec<ChannelSection> {
    let pointed: std::collections::HashSet<String> = sections
        .iter()
        .filter_map(|s| s.next_channel_section_id.clone())
        .collect();
    let mut ordered = Vec::with_capacity(sections.len());
    let mut next = sections
        .iter()
        .find(|s| !pointed.contains(&s.channel_section_id))
        .map(|s| s.channel_section_id.clone());
    while let Some(id) = next {
        let Some(index) = sections.iter().position(|s| s.channel_section_id == id) else {
            break;
        };
        let section = sections.remove(index);
        next = section.next_channel_section_id.clone();
        ordered.push(section);
    }
    ordered.extend(sections);
    ordered
}

/// Read state of a conversation, from the web client's `client.counts`.
#[derive(Debug, Clone, Deserialize)]
pub struct ConversationCount {
    pub id: String,
    #[serde(default)]
    pub has_unreads: bool,
    #[serde(default)]
    pub mention_count: u32,
    /// Timestamp of the latest message.
    #[serde(default)]
    pub latest: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Topic {
    #[serde(default)]
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Message {
    pub ts: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub bot_profile: Option<BotProfile>,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub thread_ts: Option<String>,
    #[serde(default)]
    pub reply_count: u32,
    #[serde(default)]
    pub edited: Option<serde_json::Value>,
    #[serde(default)]
    pub reactions: Vec<Reaction>,
    #[serde(default)]
    pub files: Vec<File>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BotProfile {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Reaction {
    pub name: String,
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub users: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct File {
    #[serde(default)]
    pub name: String,
}

impl Message {
    /// A reply inside a thread, as opposed to the thread's parent or a
    /// reply also broadcast to the channel.
    pub fn is_thread_reply(&self) -> bool {
        self.thread_ts
            .as_deref()
            .is_some_and(|thread| thread != self.ts)
            && self.subtype.as_deref() != Some("thread_broadcast")
    }

    pub fn has_thread(&self) -> bool {
        self.reply_count > 0
    }

    pub fn is_edited(&self) -> bool {
        self.edited.is_some()
    }

    /// Seconds since the epoch, from Slack's `"1700000000.123456"` timestamps.
    pub fn epoch_seconds(&self) -> i64 {
        self.ts
            .split('.')
            .next()
            .and_then(|secs| secs.parse().ok())
            .unwrap_or(0)
    }

    pub fn add_reaction(&mut self, name: &str, user: &str) {
        match self.reactions.iter_mut().find(|r| r.name == name) {
            Some(reaction) if !reaction.users.iter().any(|u| u == user) => {
                reaction.count += 1;
                reaction.users.push(user.to_string());
            }
            Some(_) => {}
            None => self.reactions.push(Reaction {
                name: name.to_string(),
                count: 1,
                users: vec![user.to_string()],
            }),
        }
    }

    pub fn remove_reaction(&mut self, name: &str, user: &str) {
        if let Some(reaction) = self.reactions.iter_mut().find(|r| r.name == name)
            && let Some(pos) = reaction.users.iter().position(|u| u == user)
        {
            reaction.users.remove(pos);
            reaction.count = reaction.count.saturating_sub(1);
        }
        self.reactions.retain(|r| r.count > 0);
    }
}

#[cfg(test)]
pub(crate) fn message(ts: &str, user: &str, text: &str) -> Message {
    Message {
        ts: ts.to_string(),
        user: Some(user.to_string()),
        username: None,
        bot_profile: None,
        text: text.to_string(),
        subtype: None,
        thread_ts: None,
        reply_count: 0,
        edited: None,
        reactions: Vec::new(),
        files: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_falls_back_to_real_name_then_handle() {
        let mut user: User = serde_json::from_value(serde_json::json!({
            "id": "U1", "name": "jdoe", "profile": {"display_name": "", "real_name": "Jane Doe"}
        }))
        .unwrap();
        assert_eq!(user.display_name(), "Jane Doe");
        user.profile.real_name.clear();
        assert_eq!(user.display_name(), "jdoe");
    }

    #[test]
    fn thread_replies_are_told_apart_from_parents_and_broadcasts() {
        let mut parent = message("1.0", "U1", "parent");
        parent.thread_ts = Some("1.0".into());
        let mut reply = message("2.0", "U1", "reply");
        reply.thread_ts = Some("1.0".into());
        let mut broadcast = reply.clone();
        broadcast.subtype = Some("thread_broadcast".into());

        assert!(!parent.is_thread_reply());
        assert!(reply.is_thread_reply());
        assert!(!broadcast.is_thread_reply());
    }

    #[test]
    fn search_matches_know_their_thread() {
        let found = |ts: &str, permalink: &str| -> SearchMatch {
            serde_json::from_value(serde_json::json!({
                "ts": ts, "permalink": permalink, "channel": {"id": "C1"}
            }))
            .unwrap()
        };
        let reply = found(
            "2.0",
            "https://acme.slack.com/archives/C1/p2?thread_ts=1.0&cid=C1",
        );
        assert_eq!(reply.thread_ts().as_deref(), Some("1.0"));
        assert!(
            found("2.0", "https://acme.slack.com/archives/C1/p2")
                .thread_ts()
                .is_none()
        );
        let parent = found(
            "1.0",
            "https://acme.slack.com/archives/C1/p1?thread_ts=1.0&cid=C1",
        );
        assert!(parent.thread_ts().is_none());
    }

    #[test]
    fn sections_follow_their_linked_order() {
        let section = |id: &str, next: Option<&str>| ChannelSection {
            channel_section_id: id.into(),
            kind: "standard".into(),
            name: id.into(),
            emoji: String::new(),
            channel_ids_page: ChannelIdsPage::default(),
            next_channel_section_id: next.map(str::to_string),
        };
        let ordered = order_sections(vec![
            section("c", None),
            section("a", Some("b")),
            section("b", Some("c")),
        ]);
        let ids: Vec<&str> = ordered
            .iter()
            .map(|s| s.channel_section_id.as_str())
            .collect();
        assert_eq!(ids, ["a", "b", "c"]);
    }

    #[test]
    fn reactions_are_counted_once_per_user() {
        let mut msg = message("1.0", "U1", "hi");
        msg.add_reaction("+1", "U1");
        msg.add_reaction("+1", "U1");
        msg.add_reaction("+1", "U2");
        assert_eq!(msg.reactions[0].count, 2);

        msg.remove_reaction("+1", "U1");
        msg.remove_reaction("+1", "U2");
        assert!(msg.reactions.is_empty());
    }
}

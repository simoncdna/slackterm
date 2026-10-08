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

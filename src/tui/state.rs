use std::collections::{HashMap, HashSet};

use crate::config::{Layout, Settings};
use crate::session::Workspace;
use crate::slack::{Conversation, ConversationCount, Message, User};

use super::fuzzy::fuzzy_match;
use super::input::Input;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    Public,
    Private,
    Direct,
    Group,
}

#[derive(Debug, Clone)]
pub struct Channel {
    pub id: String,
    pub kind: ChannelKind,
    pub name: String,
    /// The other member of a direct message.
    pub dm_user: Option<String>,
    pub topic: String,
    pub unread: bool,
    /// Mentions of the user, or messages received in a direct conversation.
    pub mentions: u32,
    /// Shown in the sidebar. Every channel is; direct messages only when the
    /// web client lists them as open, or once they see activity.
    pub listed: bool,
    /// Timestamp of the latest message, to order direct messages by recency.
    pub latest: String,
}

impl Channel {
    pub fn from_conversation(conversation: Conversation) -> Self {
        let kind = if conversation.is_im {
            ChannelKind::Direct
        } else if conversation.is_mpim {
            ChannelKind::Group
        } else if conversation.is_private {
            ChannelKind::Private
        } else {
            ChannelKind::Public
        };
        Self {
            id: conversation.id,
            kind,
            name: conversation.name.unwrap_or_default(),
            dm_user: conversation.user,
            topic: conversation.topic.map(|t| t.value).unwrap_or_default(),
            unread: false,
            mentions: 0,
            listed: !matches!(kind, ChannelKind::Direct | ChannelKind::Group),
            latest: String::new(),
        }
    }

    pub fn is_direct(&self) -> bool {
        matches!(self.kind, ChannelKind::Direct | ChannelKind::Group)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sidebar,
    Messages,
    Thread,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Connection {
    Connecting,
    Connected,
    Reconnecting { in_secs: u64, reason: String },
}

#[derive(Debug, Default)]
pub struct History {
    pub messages: Vec<Message>,
    pub loaded: bool,
}

#[derive(Debug)]
pub struct Thread {
    pub channel: String,
    pub ts: String,
    /// The parent first, then the replies.
    pub messages: Vec<Message>,
    pub loaded: bool,
}

#[derive(Debug)]
pub enum Overlay {
    Switcher { query: Input, cursor: usize },
    Settings { row: usize },
}

pub const SETTINGS_ROWS: usize = 3;

pub struct SwitcherResult {
    pub channel: usize,
    pub label: String,
    pub matched: Vec<usize>,
}

pub struct App {
    pub team_name: String,
    pub my_id: String,
    pub my_name: String,
    pub settings: Settings,
    pub users: HashMap<String, User>,
    pub channels: Vec<Channel>,
    pub channels_loaded: bool,
    /// Read state from `client.counts`, kept until the channels are loaded.
    pub counts: Option<HashMap<String, ConversationCount>>,
    /// `client.counts` failed: fall back to listing every direct message.
    pub counts_unavailable: bool,
    /// Channels that received messages before they were known.
    pub pending_unread: HashSet<String>,
    pub current: Option<String>,
    pub sidebar_cursor: usize,
    pub histories: HashMap<String, History>,
    /// Selected message in the current channel; `None` follows the latest.
    pub selected: Option<usize>,
    pub thread: Option<Thread>,
    pub focus: Focus,
    pub mode: Mode,
    pub input: Input,
    pub overlay: Option<Overlay>,
    pub connection: Connection,
    pub notice: Option<String>,
    pub should_quit: bool,
}

impl App {
    pub fn new(workspace: &Workspace, settings: Settings) -> Self {
        let focus = if settings.layout == Layout::Focus {
            Focus::Messages
        } else {
            Focus::Sidebar
        };
        Self {
            team_name: workspace.team_name.clone(),
            my_id: workspace.user_id.clone(),
            my_name: workspace.user_name.clone(),
            settings,
            users: HashMap::new(),
            channels: Vec::new(),
            channels_loaded: false,
            counts: None,
            counts_unavailable: false,
            pending_unread: HashSet::new(),
            current: None,
            sidebar_cursor: 0,
            histories: HashMap::new(),
            selected: None,
            thread: None,
            focus,
            mode: Mode::Normal,
            input: Input::default(),
            overlay: None,
            connection: Connection::Connecting,
            notice: None,
            should_quit: false,
        }
    }

    pub fn user_name(&self, id: &str) -> String {
        match self.users.get(id) {
            Some(user) => user.display_name().to_string(),
            None if id == "USLACKBOT" => "Slackbot".to_string(),
            None => id.to_string(),
        }
    }

    pub fn author(&self, message: &Message) -> String {
        if let Some(user) = &message.user {
            return self.user_name(user);
        }
        message
            .username
            .clone()
            .or_else(|| message.bot_profile.as_ref().map(|b| b.name.clone()))
            .unwrap_or_else(|| "?".to_string())
    }

    /// The channel name without its `#`/`@` prefix.
    pub fn channel_name(&self, channel: &Channel) -> String {
        match channel.kind {
            ChannelKind::Public | ChannelKind::Private => channel.name.clone(),
            ChannelKind::Direct => channel
                .dm_user
                .as_deref()
                .map(|id| self.user_name(id))
                .unwrap_or_else(|| channel.name.clone()),
            // Group DMs are named like `mpdm-alice--bob--carol-1`.
            ChannelKind::Group => channel
                .name
                .trim_start_matches("mpdm-")
                .trim_end_matches("-1")
                .split("--")
                .collect::<Vec<_>>()
                .join(", "),
        }
    }

    pub fn channel_label(&self, channel: &Channel) -> String {
        let prefix = match channel.kind {
            ChannelKind::Public => "#",
            ChannelKind::Private => "◇",
            ChannelKind::Direct => "@",
            ChannelKind::Group => "&",
        };
        format!("{prefix}{}", self.channel_name(channel))
    }

    pub fn channel(&self, id: &str) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == id)
    }

    pub fn channel_mut(&mut self, id: &str) -> Option<&mut Channel> {
        self.channels.iter_mut().find(|c| c.id == id)
    }

    pub fn current_channel(&self) -> Option<&Channel> {
        self.current.as_deref().and_then(|id| self.channel(id))
    }

    pub fn current_history(&self) -> Option<&History> {
        self.current
            .as_deref()
            .and_then(|id| self.histories.get(id))
    }

    pub fn current_messages(&self) -> &[Message] {
        self.current_history()
            .map(|h| h.messages.as_slice())
            .unwrap_or_default()
    }

    /// Channels first, then direct messages, each alphabetically. The
    /// sidebar cursor stays on the same channel.
    pub fn sort_channels(&mut self) {
        let cursor_id = self.channels.get(self.sidebar_cursor).map(|c| c.id.clone());
        let mut keyed: Vec<(String, Channel)> = std::mem::take(&mut self.channels)
            .into_iter()
            .map(|c| (self.channel_name(&c).to_lowercase(), c))
            .collect();
        keyed.sort_by(|(a_name, a), (b_name, b)| {
            a.is_direct()
                .cmp(&b.is_direct())
                .then_with(|| match a.is_direct() {
                    true => b.latest.cmp(&a.latest),
                    false => std::cmp::Ordering::Equal,
                })
                .then_with(|| a_name.cmp(b_name))
        });
        self.channels = keyed.into_iter().map(|(_, c)| c).collect();
        if let Some(id) = cursor_id {
            self.sidebar_cursor = self.channels.iter().position(|c| c.id == id).unwrap_or(0);
        }
    }

    pub fn is_listed(&self, channel: &Channel) -> bool {
        channel.listed
            || channel.unread
            || channel.mentions > 0
            || self.current.as_deref() == Some(channel.id.as_str())
    }

    /// Indices in `channels` of the ones shown in the sidebar.
    pub fn listed_indices(&self) -> Vec<usize> {
        (0..self.channels.len())
            .filter(|&i| self.is_listed(&self.channels[i]))
            .collect()
    }

    /// Applies the read state from `client.counts` once both it and the
    /// channel list are known. It is only valid at startup, so it is used once.
    pub fn apply_counts(&mut self) {
        if !self.channels_loaded {
            return;
        }
        if self.counts_unavailable {
            for channel in &mut self.channels {
                channel.listed |= channel.kind == ChannelKind::Direct;
            }
        }
        let Some(counts) = self.counts.take() else {
            return;
        };
        for channel in &mut self.channels {
            let Some(count) = counts.get(&channel.id) else {
                continue;
            };
            channel.listed = true;
            channel.unread |= count.has_unreads;
            channel.mentions = channel.mentions.max(count.mention_count);
            if let Some(latest) = &count.latest
                && *latest > channel.latest
            {
                channel.latest = latest.clone();
            }
        }
        self.sort_channels();
    }

    pub fn switcher_results(&self, query: &str) -> Vec<SwitcherResult> {
        let mut results: Vec<(i32, bool, SwitcherResult)> = self
            .channels
            .iter()
            .enumerate()
            .filter_map(|(index, channel)| {
                let label = self.channel_label(channel);
                let (score, matched) = fuzzy_match(query, &label)?;
                Some((
                    score,
                    channel.unread,
                    SwitcherResult {
                        channel: index,
                        label,
                        matched,
                    },
                ))
            })
            .collect();
        results.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then(b.1.cmp(&a.1))
                .then_with(|| a.2.label.cmp(&b.2.label))
        });
        results.into_iter().take(50).map(|(_, _, r)| r).collect()
    }

    /// Where typed text goes: the open thread when it has focus, else the channel.
    pub fn compose_target(&self) -> Option<(String, Option<String>)> {
        match (&self.thread, self.focus) {
            (Some(thread), Focus::Thread) => {
                Some((thread.channel.clone(), Some(thread.ts.clone())))
            }
            _ => self.current.clone().map(|channel| (channel, None)),
        }
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    pub fn workspace() -> Workspace {
        Workspace {
            team_id: "T1".into(),
            team_name: "acme".into(),
            url: "https://acme.slack.com/".into(),
            user_id: "UME".into(),
            user_name: "simon".into(),
            token: "xoxc-test".into(),
        }
    }

    pub fn conversation(id: &str, name: &str) -> Conversation {
        serde_json::from_value(serde_json::json!({"id": id, "name": name})).unwrap()
    }

    pub fn direct(id: &str, user: &str) -> Conversation {
        serde_json::from_value(serde_json::json!({"id": id, "is_im": true, "user": user})).unwrap()
    }

    pub fn user(id: &str, name: &str) -> User {
        serde_json::from_value(serde_json::json!({"id": id, "name": name})).unwrap()
    }

    pub fn app() -> App {
        App::new(&workspace(), Settings::default())
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;

    #[test]
    fn sorts_channels_before_direct_messages() {
        let mut app = app();
        app.users.insert("U2".into(), user("U2", "alice"));
        app.channels = vec![
            Channel::from_conversation(direct("D1", "U2")),
            Channel::from_conversation(conversation("C2", "random")),
            Channel::from_conversation(conversation("C1", "deploys")),
        ];
        app.sort_channels();

        let labels: Vec<String> = app.channels.iter().map(|c| app.channel_label(c)).collect();
        assert_eq!(labels, ["#deploys", "#random", "@alice"]);
    }

    #[test]
    fn names_group_messages_after_their_members() {
        let app = app();
        let mut group = Channel::from_conversation(conversation("G1", "mpdm-alice--bob--carol-1"));
        group.kind = ChannelKind::Group;
        assert_eq!(app.channel_name(&group), "alice, bob, carol");
    }

    #[test]
    fn switcher_ranks_best_matches_first() {
        let mut app = app();
        app.channels = vec![
            Channel::from_conversation(conversation("C1", "data-exports")),
            Channel::from_conversation(conversation("C2", "deploys")),
            Channel::from_conversation(conversation("C3", "random")),
        ];
        let results = app.switcher_results("dep");
        let labels: Vec<&str> = results.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["#deploys", "#data-exports"]);
    }
}

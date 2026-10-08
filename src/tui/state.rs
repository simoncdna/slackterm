use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use crate::config::{Layout, Settings, SidebarState, SidebarTab};
use crate::session::Workspace;
use crate::slack::{
    ChannelSection, Conversation, ConversationCount, Message, SearchChannel, SearchMatch, User,
    emoji,
};

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

    /// A conversation known only from a search result, such as a public
    /// channel the user is not a member of.
    pub fn from_search(channel: &SearchChannel) -> Self {
        let kind = if channel.is_im {
            ChannelKind::Direct
        } else if channel.is_mpim {
            ChannelKind::Group
        } else if channel.is_private {
            ChannelKind::Private
        } else {
            ChannelKind::Public
        };
        Self {
            id: channel.id.clone(),
            kind,
            name: channel.name.clone(),
            // Search names direct messages after the other member's id.
            dm_user: channel.is_im.then(|| channel.name.clone()),
            topic: String::new(),
            unread: false,
            mentions: 0,
            listed: false,
            latest: String::new(),
        }
    }

    pub fn is_direct(&self) -> bool {
        matches!(self.kind, ChannelKind::Direct | ChannelKind::Group)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    /// Created by the user, with an explicit list of conversations.
    Custom,
    /// Every other channel.
    Channels,
    /// Every other direct message.
    Direct,
    Starred,
}

#[derive(Debug, Clone)]
pub struct Section {
    pub id: String,
    pub kind: SectionKind,
    pub name: String,
    pub emoji: Option<String>,
    pub channel_ids: Vec<String>,
}

impl Section {
    /// Sections of the web client that hold conversations; others (apps,
    /// Slack Connect, agents…) are skipped unless they list conversations.
    pub fn from_slack(section: ChannelSection) -> Option<Self> {
        let channel_ids = section.channel_ids_page.channel_ids;
        let (kind, default_name) = match section.kind.as_str() {
            "standard" => (SectionKind::Custom, ""),
            "channels" => (SectionKind::Channels, "Canaux"),
            "direct_messages" => (SectionKind::Direct, "Messages directs"),
            "stars" => (SectionKind::Starred, "Favoris"),
            _ if !channel_ids.is_empty() => (SectionKind::Custom, ""),
            _ => return None,
        };
        Some(Self {
            id: section.channel_section_id,
            kind,
            name: if section.name.is_empty() {
                default_name.to_string()
            } else {
                section.name
            },
            emoji: (!section.emoji.is_empty())
                .then(|| emoji::lookup(&section.emoji))
                .flatten(),
            channel_ids,
        })
    }

    /// Used until the user's sections are loaded, or if they cannot be.
    pub fn defaults() -> Vec<Self> {
        let section = |id: &str, kind, name: &str| Self {
            id: id.to_string(),
            kind,
            name: name.to_string(),
            emoji: None,
            channel_ids: Vec::new(),
        };
        vec![
            section("channels", SectionKind::Channels, "Canaux"),
            section("direct_messages", SectionKind::Direct, "Messages directs"),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarItem {
    Section(String),
    Channel(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarRow {
    Section {
        index: usize,
        collapsed: bool,
        /// Conversations hidden by the collapse.
        hidden: usize,
    },
    Channel(usize),
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
    /// Older messages exist on Slack.
    pub has_more: bool,
    pub loading_older: bool,
}

/// A message to select once it is loaded, after opening a search result.
#[derive(Debug, Clone)]
pub struct Jump {
    pub channel: String,
    pub ts: String,
    /// Pages of older history fetched so far to reach it.
    pub pages: u32,
}

#[derive(Debug)]
pub enum SearchStatus {
    /// Nothing submitted yet.
    Idle,
    Loading,
    Done(Vec<SearchMatch>),
    Failed(String),
}

#[derive(Debug)]
pub struct Search {
    pub query: Input,
    /// The query whose results are shown.
    pub submitted: String,
    pub status: SearchStatus,
    pub cursor: usize,
}

#[derive(Debug)]
pub struct Thread {
    pub channel: String,
    pub ts: String,
    /// The parent first, then the replies.
    pub messages: Vec<Message>,
    pub loaded: bool,
    /// Selected message; `None` follows the latest.
    pub selected: Option<usize>,
}

/// The message a reaction goes to.
#[derive(Debug, Clone)]
pub struct ReactionTarget {
    pub channel: String,
    pub ts: String,
    pub author: String,
    pub text: String,
}

#[derive(Debug)]
pub struct EmojiPicker {
    pub query: Input,
    pub cursor: usize,
    pub target: ReactionTarget,
}

pub struct EmojiChoice {
    /// The name sent to Slack.
    pub name: String,
    /// `None` for a custom emoji, which is an image.
    pub glyph: Option<String>,
    /// Char indices of `name` matching the query.
    pub matched: Vec<usize>,
}

/// Offered while nothing is typed, after the user's most used ones.
const DEFAULT_REACTIONS: [&str; 12] = [
    "+1",
    "heart",
    "joy",
    "tada",
    "eyes",
    "rocket",
    "white_check_mark",
    "pray",
    "fire",
    "100",
    "clap",
    "raised_hands",
];
const MAX_EMOJI_CHOICES: usize = 60;

#[derive(Debug)]
pub enum Overlay {
    Switcher { query: Input, cursor: usize },
    Settings { row: usize },
    Search(Search),
    Emoji(EmojiPicker),
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
    /// Where settings changes are saved, if they are.
    pub settings_path: Option<PathBuf>,
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
    pub sections: Vec<Section>,
    /// Ids of the collapsed sections.
    pub collapsed: BTreeSet<String>,
    pub sidebar_tab: SidebarTab,
    /// `None` until the user moves in the sidebar: it then follows the
    /// current channel.
    pub sidebar_cursor: Option<SidebarItem>,
    pub jump: Option<Jump>,
    /// Names of the workspace's custom emoji.
    pub custom_emoji: Vec<String>,
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
            settings_path: None,
            users: HashMap::new(),
            channels: Vec::new(),
            channels_loaded: false,
            counts: None,
            counts_unavailable: false,
            pending_unread: HashSet::new(),
            current: None,
            sections: Section::defaults(),
            collapsed: BTreeSet::new(),
            sidebar_tab: SidebarTab::Channels,
            sidebar_cursor: None,
            jump: None,
            custom_emoji: Vec::new(),
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

    /// Channels alphabetically, then direct messages by recency.
    pub fn sort_channels(&mut self) {
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
    }

    pub fn is_listed(&self, channel: &Channel) -> bool {
        channel.listed
            || channel.unread
            || channel.mentions > 0
            || self.current.as_deref() == Some(channel.id.as_str())
    }

    /// The section a conversation appears in: the user's own section that
    /// lists it, else the default one for its kind.
    fn section_of(&self, channel: &Channel) -> Option<usize> {
        let explicit = self.sections.iter().position(|s| {
            matches!(s.kind, SectionKind::Custom | SectionKind::Starred)
                && s.channel_ids.contains(&channel.id)
        });
        let fallback = if channel.is_direct() {
            SectionKind::Direct
        } else {
            SectionKind::Channels
        };
        explicit.or_else(|| self.sections.iter().position(|s| s.kind == fallback))
    }

    /// The sidebar, top to bottom, for the selected tab.
    pub fn sidebar_rows(&self) -> Vec<SidebarRow> {
        match self.sidebar_tab {
            SidebarTab::Channels => self.section_rows(),
            // `channels` already orders direct messages by recency.
            SidebarTab::Direct => (0..self.channels.len())
                .filter(|&i| {
                    let channel = &self.channels[i];
                    channel.is_direct() && self.is_listed(channel)
                })
                .map(SidebarRow::Channel)
                .collect(),
        }
    }

    /// Whether a tab holds unread conversations, and how many messages or
    /// mentions await, to badge the tab that is not shown.
    pub fn tab_activity(&self, tab: SidebarTab) -> (bool, u32) {
        self.channels
            .iter()
            .filter(|c| c.is_direct() == (tab == SidebarTab::Direct))
            .fold((false, 0), |(unread, mentions), c| {
                (unread || c.unread, mentions + c.mentions)
            })
    }

    pub fn sidebar_state(&self) -> SidebarState {
        SidebarState {
            collapsed: self.collapsed.clone(),
            tab: self.sidebar_tab,
        }
    }

    /// Sections with their conversations. Collapsed sections still show the
    /// conversations that need attention, as the web client does.
    fn section_rows(&self) -> Vec<SidebarRow> {
        let mut members: Vec<Vec<usize>> = vec![Vec::new(); self.sections.len()];
        for (index, channel) in self.channels.iter().enumerate() {
            let Some(section) = self.section_of(channel) else {
                continue;
            };
            if self.is_listed(channel) || self.sections[section].kind != SectionKind::Direct {
                members[section].push(index);
            }
        }

        let mut rows = Vec::new();
        for (index, section) in self.sections.iter().enumerate() {
            let channels = &members[index];
            if channels.is_empty() {
                continue;
            }
            let collapsed = self.collapsed.contains(&section.id);
            let shown: Vec<usize> = channels
                .iter()
                .copied()
                .filter(|&i| !collapsed || self.needs_attention(&self.channels[i]))
                .collect();
            rows.push(SidebarRow::Section {
                index,
                collapsed,
                hidden: channels.len() - shown.len(),
            });
            rows.extend(shown.into_iter().map(SidebarRow::Channel));
        }
        rows
    }

    fn needs_attention(&self, channel: &Channel) -> bool {
        channel.unread
            || channel.mentions > 0
            || self.current.as_deref() == Some(channel.id.as_str())
    }

    pub fn sidebar_items(&self) -> Vec<SidebarItem> {
        self.sidebar_rows()
            .into_iter()
            .map(|row| match row {
                SidebarRow::Section { index, .. } => {
                    SidebarItem::Section(self.sections[index].id.clone())
                }
                SidebarRow::Channel(index) => SidebarItem::Channel(self.channels[index].id.clone()),
            })
            .collect()
    }

    /// Where the sidebar cursor is: the item last moved to if still shown,
    /// else the current channel.
    pub fn sidebar_position(&self, items: &[SidebarItem]) -> usize {
        let current = self.current.clone().map(SidebarItem::Channel);
        [self.sidebar_cursor.as_ref(), current.as_ref()]
            .into_iter()
            .flatten()
            .find_map(|wanted| items.iter().position(|item| item == wanted))
            .unwrap_or(0)
    }

    /// Indices in `channels` of the ones shown in the sidebar, in order.
    #[cfg(test)]
    pub fn listed_indices(&self) -> Vec<usize> {
        self.sidebar_rows()
            .into_iter()
            .filter_map(|row| match row {
                SidebarRow::Channel(index) => Some(index),
                SidebarRow::Section { .. } => None,
            })
            .collect()
    }

    /// Replaces the default sections with the user's, keeping a home for
    /// channels and direct messages they did not file anywhere.
    pub fn set_sections(&mut self, sections: Vec<ChannelSection>) {
        let mut sections: Vec<Section> = sections
            .into_iter()
            .filter_map(Section::from_slack)
            .collect();
        for default in Section::defaults() {
            if !sections.iter().any(|s| s.kind == default.kind) {
                sections.push(default);
            }
        }
        self.sections = sections;
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

    /// Emoji to react with: the most used ones while `query` is empty, else
    /// the best matches among standard and custom emoji.
    pub fn emoji_choices(&self, query: &str) -> Vec<EmojiChoice> {
        let query = query.trim_start_matches(':');
        if query.is_empty() {
            return self.frequent_reactions();
        }
        let mut scored: Vec<(i32, EmojiChoice)> = Vec::new();
        for entry in emoji::all() {
            let best = entry
                .names
                .iter()
                .filter_map(|name| fuzzy_match(query, name).map(|(score, m)| (score, name, m)))
                .max_by_key(|(score, name, _)| (*score, -(name.len() as i32)));
            if let Some((score, name, matched)) = best {
                scored.push((
                    score,
                    EmojiChoice {
                        name: name.to_string(),
                        glyph: Some(entry.glyph.to_string()),
                        matched,
                    },
                ));
            }
        }
        for name in &self.custom_emoji {
            if let Some((score, matched)) = fuzzy_match(query, name) {
                scored.push((
                    score,
                    EmojiChoice {
                        name: name.clone(),
                        glyph: None,
                        matched,
                    },
                ));
            }
        }
        scored.sort_by(|(a_score, a), (b_score, b)| {
            b_score
                .cmp(a_score)
                .then(a.name.len().cmp(&b.name.len()))
                .then_with(|| a.name.cmp(&b.name))
        });
        scored
            .into_iter()
            .take(MAX_EMOJI_CHOICES)
            .map(|(_, choice)| choice)
            .collect()
    }

    /// Reactions seen in the loaded conversations, most used first,
    /// completed with common ones.
    fn frequent_reactions(&self) -> Vec<EmojiChoice> {
        let mut counts: HashMap<&str, u32> = HashMap::new();
        let thread = self.thread.iter().flat_map(|t| t.messages.iter());
        for message in self
            .histories
            .values()
            .flat_map(|h| h.messages.iter())
            .chain(thread)
        {
            for reaction in &message.reactions {
                *counts.entry(reaction.name.as_str()).or_default() += reaction.count;
            }
        }
        let mut names: Vec<(&str, u32)> = counts.into_iter().collect();
        names.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let mut chosen: Vec<&str> = names.into_iter().map(|(name, _)| name).take(24).collect();
        for name in DEFAULT_REACTIONS {
            if !chosen.contains(&name) {
                chosen.push(name);
            }
        }
        chosen
            .into_iter()
            .map(|name| EmojiChoice {
                name: name.to_string(),
                glyph: emoji::lookup(name),
                matched: Vec::new(),
            })
            .collect()
    }

    /// The message `r` reacts to: the selected one in the focused list, else
    /// the latest.
    pub fn reaction_target(&self) -> Option<ReactionTarget> {
        let (channel, messages, selected) = match (&self.thread, self.focus) {
            (Some(thread), Focus::Thread) => {
                (&thread.channel, &thread.messages[..], thread.selected)
            }
            _ => (
                self.current.as_ref()?,
                self.current_messages(),
                self.selected,
            ),
        };
        let message = selected.map_or(messages.last(), |i| messages.get(i))?;
        Some(ReactionTarget {
            channel: channel.clone(),
            ts: message.ts.clone(),
            author: self.author(message),
            text: message.text.clone(),
        })
    }

    /// The loaded copy of a message, wherever it is shown.
    pub fn find_message(&self, channel: &str, ts: &str) -> Option<&Message> {
        let history = self.histories.get(channel).map(|h| &h.messages[..]);
        let thread = self
            .thread
            .as_ref()
            .filter(|t| t.channel == channel)
            .map(|t| &t.messages[..]);
        history
            .into_iter()
            .chain(thread)
            .flatten()
            .find(|m| m.ts == ts)
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

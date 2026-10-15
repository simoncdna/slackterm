mod client;
pub mod emoji;
pub mod media;
pub mod mrkdwn;
pub mod rtm;
mod types;

pub use client::{
    AuthTest, DEFAULT_API_BASE, HistoryPage, RtmConnect, SlackClient, SlackError, is_slack_file_url,
};
pub use types::{
    Attachment, ChannelSection, Conversation, ConversationCount, File, FileKind, HIGHLIGHT_END,
    HIGHLIGHT_START, Message, Reaction, SearchChannel, SearchMatch, User, order_sections,
};

#[cfg(test)]
pub(crate) use types::message as test_message;

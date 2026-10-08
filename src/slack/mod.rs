mod client;
pub mod emoji;
pub mod mrkdwn;
pub mod rtm;
mod types;

pub use client::{AuthTest, DEFAULT_API_BASE, HistoryPage, RtmConnect, SlackClient, SlackError};
pub use types::{
    ChannelSection, Conversation, ConversationCount, File, HIGHLIGHT_END, HIGHLIGHT_START, Message,
    Reaction, SearchChannel, SearchMatch, User, order_sections,
};

#[cfg(test)]
pub(crate) use types::message as test_message;

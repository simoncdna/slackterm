use tokio::sync::mpsc::UnboundedSender;

use crate::slack::{SlackClient, SlackError};

use super::update::{ApiEvent, Command, Event};

/// Runs a command in the background and reports its result as an event.
pub fn execute(command: Command, client: &SlackClient, events: &UnboundedSender<Event>) {
    let client = client.clone();
    let events = events.clone();
    tokio::spawn(async move {
        let result = run(command, &client).await;
        let event = match result {
            Ok(Some(event)) => event,
            Ok(None) => return,
            Err(reason) => ApiEvent::Failed(reason),
        };
        let _ = events.send(Event::Api(event));
    });
}

async fn run(command: Command, client: &SlackClient) -> Result<Option<ApiEvent>, String> {
    let event = match command {
        Command::LoadUsers => client
            .users_list()
            .await
            .map(ApiEvent::Users)
            .map_err(|e| format!("utilisateurs : {e}"))?,
        Command::LoadConversations => client
            .user_conversations()
            .await
            .map(ApiEvent::Conversations)
            .map_err(|e| format!("liste des canaux : {e}"))?,
        Command::LoadCounts => match client.client_counts().await {
            Ok(counts) => ApiEvent::Counts(counts),
            Err(_) => ApiEvent::CountsUnavailable,
        },
        Command::LoadHistory(channel) => client
            .conversations_history(&channel, None, 100)
            .await
            .map(|page| ApiEvent::History {
                channel,
                messages: page.messages,
                has_more: page.has_more,
            })
            .map_err(|e| format!("historique : {e}"))?,
        Command::LoadOlder { channel, before } => client
            .conversations_history(&channel, Some(&before), 200)
            .await
            .map(|page| ApiEvent::Older {
                channel,
                messages: page.messages,
                has_more: page.has_more,
            })
            .map_err(|e| format!("historique : {e}"))?,
        // The default sections stay if the user's cannot be read.
        Command::LoadSections => match client.channel_sections().await {
            Ok(sections) => ApiEvent::Sections(sections),
            Err(_) => return Ok(None),
        },
        Command::Search(query) => {
            let result = client
                .search_messages(&query)
                .await
                .map_err(|e| e.to_string());
            ApiEvent::SearchResults { query, result }
        }
        Command::LoadReplies { channel, ts } => client
            .conversations_replies(&channel, &ts)
            .await
            .map(|messages| ApiEvent::Replies {
                channel,
                ts,
                messages,
            })
            .map_err(|e| format!("fil : {e}"))?,
        Command::Send {
            channel,
            text,
            thread_ts,
        } => client
            .chat_post_message(&channel, &text, thread_ts.as_deref())
            .await
            .map(|message| ApiEvent::Sent { channel, message })
            .map_err(|e| format!("envoi : {e}"))?,
        Command::MarkRead { channel, ts } => {
            client
                .conversations_mark(&channel, &ts)
                .await
                .map_err(|e| format!("marquer comme lu : {e}"))?;
            return Ok(None);
        }
        Command::LoadEmoji => match client.emoji_list().await {
            Ok(emoji) => ApiEvent::CustomEmoji(emoji),
            // Only custom emoji go missing from the picker.
            Err(_) => return Ok(None),
        },
        Command::React {
            channel,
            ts,
            name,
            add,
        } => {
            let result = if add {
                client.reactions_add(&channel, &ts, &name).await
            } else {
                client.reactions_remove(&channel, &ts, &name).await
            };
            match result {
                // Already in the requested state: nothing to undo.
                Ok(()) => return Ok(None),
                Err(SlackError::Api(code))
                    if code == "already_reacted" || code == "no_reaction" =>
                {
                    return Ok(None);
                }
                Err(e) => ApiEvent::ReactionFailed {
                    channel,
                    ts,
                    name,
                    added: add,
                    reason: e.to_string(),
                },
            }
        }
        Command::SaveSidebar(state) => {
            state
                .save()
                .map_err(|e| format!("barre latérale : {e:#}"))?;
            return Ok(None);
        }
        Command::SaveSettings(settings) => {
            settings.save().map_err(|e| format!("réglages : {e:#}"))?;
            return Ok(None);
        }
    };
    Ok(Some(event))
}

use tokio::sync::mpsc::UnboundedSender;

use crate::slack::SlackClient;

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
            .conversations_history(&channel, 100)
            .await
            .map(|messages| ApiEvent::History { channel, messages })
            .map_err(|e| format!("historique : {e}"))?,
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
        Command::SaveSettings(settings) => {
            settings.save().map_err(|e| format!("réglages : {e:#}"))?;
            return Ok(None);
        }
    };
    Ok(Some(event))
}

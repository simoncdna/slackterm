use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};

use futures_util::{StreamExt, stream};
use tokio::sync::mpsc::UnboundedSender;

use crate::platform::find_program;
use crate::slack::{SlackClient, SlackError, is_slack_file_url};

use super::update::{ApiEvent, Command, Event, FileToOpen};

/// Runs a command in the background and reports its result as an event.
pub fn execute(command: Command, client: &SlackClient, events: &UnboundedSender<Event>) {
    let client = client.clone();
    let events = events.clone();
    tokio::spawn(async move {
        match command {
            Command::LoadAvatars(pictures) => {
                load_pictures(pictures, None, &events).await;
                return;
            }
            Command::LoadPreviews(pictures) => {
                load_pictures(pictures, Some(&client), &events).await;
                return;
            }
            Command::DownloadToPlay(file) => {
                let event = match cached_file(&client, &file, &events).await {
                    Ok(path) => ApiEvent::PlayReady(path.to_string_lossy().into_owned()),
                    Err(reason) => ApiEvent::Failed(format!("{}: {reason}", file.name)),
                };
                let _ = events.send(Event::Api(event));
                return;
            }
            Command::OpenUrl(url) => {
                if let Err(reason) = open_url(&url) {
                    let _ = events.send(Event::Api(ApiEvent::Failed(reason)));
                }
                return;
            }
            Command::OpenFiles(files) => {
                for file in files {
                    let event = match open_file(&client, &file, &events).await {
                        Ok(()) => ApiEvent::Notice(format!("opened: {}", file.name)),
                        Err(reason) => ApiEvent::Failed(format!("{}: {reason}", file.name)),
                    };
                    let _ = events.send(Event::Api(event));
                }
                return;
            }
            _ => {}
        }
        let result = run(command, &client).await;
        let event = match result {
            Ok(Some(event)) => event,
            Ok(None) => return,
            Err(Failure::SessionExpired) => ApiEvent::SessionExpired,
            Err(Failure::Other(reason)) => ApiEvent::Failed(reason),
        };
        let _ = events.send(Event::Api(event));
    });
}

/// Why a command failed. An expired session makes every command fail, and
/// is told once rather than as one error each.
enum Failure {
    SessionExpired,
    Other(String),
}

/// Turns an error of the Slack API into a failure, naming what failed.
fn failed(what: &'static str) -> impl FnOnce(SlackError) -> Failure {
    move |e| {
        if e.is_session_expired() {
            Failure::SessionExpired
        } else {
            Failure::Other(format!("{what}: {e}"))
        }
    }
}

async fn run(command: Command, client: &SlackClient) -> Result<Option<ApiEvent>, Failure> {
    let event = match command {
        Command::LoadUsers => client
            .users_list()
            .await
            .map(ApiEvent::Users)
            .map_err(failed("users"))?,
        Command::LoadConversations => client
            .user_conversations()
            .await
            .map(ApiEvent::Conversations)
            .map_err(failed("channel list"))?,
        Command::LoadCounts => match client.client_counts().await {
            Ok(counts) => ApiEvent::Counts(counts),
            Err(e) if e.is_session_expired() => return Err(Failure::SessionExpired),
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
            .map_err(failed("history"))?,
        Command::LoadOlder { channel, before } => client
            .conversations_history(&channel, Some(&before), 200)
            .await
            .map(|page| ApiEvent::Older {
                channel,
                messages: page.messages,
                has_more: page.has_more,
            })
            .map_err(failed("history"))?,
        // The default sections stay if the user's cannot be read.
        Command::LoadSections => match client.channel_sections().await {
            Ok(sections) => ApiEvent::Sections(sections),
            Err(_) => return Ok(None),
        },
        Command::Search(query) => {
            let result = match client.search_messages(&query).await {
                Err(e) if e.is_session_expired() => return Err(Failure::SessionExpired),
                result => result.map_err(|e| e.to_string()),
            };
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
            .map_err(failed("thread"))?,
        Command::Send {
            channel,
            text,
            thread_ts,
        } => client
            .chat_post_message(&channel, &text, thread_ts.as_deref())
            .await
            .map(|message| ApiEvent::Sent { channel, message })
            .map_err(failed("send"))?,
        Command::MarkRead { channel, ts } => {
            client
                .conversations_mark(&channel, &ts)
                .await
                .map_err(failed("mark as read"))?;
            return Ok(None);
        }
        // Several events each: answered in `execute`.
        Command::LoadAvatars(_)
        | Command::LoadPreviews(_)
        | Command::OpenFiles(_)
        | Command::OpenUrl(_)
        | Command::DownloadToPlay(_)
        // Run by the event loop itself.
        | Command::Play(_) => return Ok(None),
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
                Err(e) if e.is_session_expired() => return Err(Failure::SessionExpired),
                Err(e) => ApiEvent::ReactionFailed {
                    channel,
                    ts,
                    name,
                    added: add,
                    reason: e.to_string(),
                },
            }
        }
        Command::LoadPublicChannels => client
            .public_channels()
            .await
            .map(ApiEvent::PublicChannels)
            .map_err(failed("channels"))?,
        Command::OpenDirect(user) => client
            .conversations_open(&user)
            .await
            .map(ApiEvent::Opened)
            .map_err(failed("direct message"))?,
        Command::Join(channel) => client
            .conversations_join(&channel)
            .await
            .map(ApiEvent::Opened)
            .map_err(failed("join"))?,
        Command::Edit { channel, ts, text } => {
            client
                .chat_update(&channel, &ts, &text)
                .await
                .map_err(failed("edit"))?;
            ApiEvent::Edited { channel, ts, text }
        }
        Command::Delete { channel, ts } => {
            client
                .chat_delete(&channel, &ts)
                .await
                .map_err(failed("delete"))?;
            ApiEvent::Deleted { channel, ts }
        }
        Command::SaveSidebar(state) => {
            state
                .save()
                .map_err(|e| Failure::Other(format!("sidebar: {e:#}")))?;
            return Ok(None);
        }
        Command::SaveSettings { settings, path } => {
            settings
                .save(&path)
                .map_err(|e| Failure::Other(format!("settings: {e:#}")))?;
            return Ok(None);
        }
    };
    Ok(Some(event))
}

/// Sends each picture as soon as it is ready. Profile pictures are public;
/// previews need the session (`client`). A picture that cannot be had is
/// skipped: initials or the file name stay instead.
async fn load_pictures(
    pictures: Vec<(String, String)>,
    client: Option<&SlackClient>,
    events: &UnboundedSender<Event>,
) {
    let http = reqwest::Client::new();
    let (folder, max_size) = match client {
        None => ("avatars", 64),
        Some(_) => ("previews", 480),
    };
    stream::iter(pictures)
        .for_each_concurrent(6, |(key, url)| {
            let http = &http;
            async move {
                let path = cache_path(folder, &url);
                let bytes = match cached(&path) {
                    Some(bytes) => Some(bytes),
                    None => {
                        // Slack's previews need the session; video thumbnails
                        // come from their own sites and are public.
                        let response = match client {
                            Some(client) if is_slack_file_url(&url) => {
                                client.download(&url).await.ok()
                            }
                            _ => http.get(&url).send().await.ok(),
                        };
                        let bytes = match response.and_then(|r| r.error_for_status().ok()) {
                            Some(response) => response.bytes().await.ok().map(|b| b.to_vec()),
                            None => None,
                        };
                        if let Some(bytes) = &bytes {
                            store(&path, bytes);
                        }
                        bytes
                    }
                };
                let image = bytes
                    .and_then(|bytes| image::load_from_memory(&bytes).ok())
                    .map(|image| image.thumbnail(max_size, max_size));
                if let Some(image) = image {
                    let _ = events.send(Event::Api(ApiEvent::Picture { key, image }));
                }
            }
        })
        .await;
}

/// Named after the URL, which changes when the picture does.
fn cache_path(folder: &str, url: &str) -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "slackterm")?;
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    Some(
        dirs.cache_dir()
            .join(folder)
            .join(format!("{:016x}", hasher.finish())),
    )
}

fn cached(path: &Option<PathBuf>) -> Option<Vec<u8>> {
    std::fs::read(path.as_ref()?).ok()
}

fn store(path: &Option<PathBuf>, bytes: &[u8]) {
    if let Some(path) = path
        && let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let _ = std::fs::write(path, bytes);
    }
}

/// Downloads the file once into the cache, reporting progress, then opens
/// it: videos in mpv when installed, everything else with the system's
/// default application.
async fn open_file(
    client: &SlackClient,
    file: &FileToOpen,
    events: &UnboundedSender<Event>,
) -> Result<(), String> {
    let path = cached_file(client, file, events).await?;
    let opened = match file.video.then(|| find_program("mpv")).flatten() {
        Some(mpv) => std::process::Command::new(mpv)
            .arg("--force-window=immediate")
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ()),
        None => open::that_detached(&path),
    };
    opened.map_err(|e| format!("could not open {}: {e}", file.name))
}

/// The file's path in the cache, downloading it the first time.
async fn cached_file(
    client: &SlackClient,
    file: &FileToOpen,
    events: &UnboundedSender<Event>,
) -> Result<PathBuf, String> {
    let dirs =
        directories::ProjectDirs::from("", "", "slackterm").ok_or("cache directory not found")?;
    let path = dirs
        .cache_dir()
        .join("files")
        .join(file_name(&file.id, &file.name));
    if !path.exists() {
        download(client, file, &path, events).await?;
    }
    Ok(path)
}

/// Opens a web link in the default browser. Only web links: anything else
/// could make the system run a program.
fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("ignored a non-web link: {url}"));
    }
    open::that_detached(url).map_err(|e| format!("could not open the link: {e}"))
}

async fn download(
    client: &SlackClient,
    file: &FileToOpen,
    path: &Path,
    events: &UnboundedSender<Event>,
) -> Result<(), String> {
    let notice = |text: String| {
        let _ = events.send(Event::Api(ApiEvent::Notice(text)));
    };
    notice(format!("downloading {}…", file.name));
    let mut response = client
        .download(&file.url)
        .await
        .map_err(|e| e.to_string())?;
    let total = response.content_length().unwrap_or(0);
    let dir = path.parent().ok_or("cache directory not found")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // Written aside then renamed, so an interrupted download is never
    // taken for a complete file.
    let partial = path.with_extension("part");
    let mut out = std::fs::File::create(&partial).map_err(|e| e.to_string())?;
    let (mut received, mut reported) = (0u64, 0u64);
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        std::io::Write::write_all(&mut out, &chunk).map_err(|e| e.to_string())?;
        received += chunk.len() as u64;
        let percent = (received * 100).checked_div(total).unwrap_or(0);
        if percent >= reported + 25 {
            reported = percent - percent % 25;
            notice(format!("downloading {}: {percent}%", file.name));
        }
    }
    std::fs::rename(&partial, path).map_err(|e| e.to_string())
}

/// `F123-rapport final.pdf`, without characters that cannot go in a path.
fn file_name(id: &str, name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect();
    format!("{id}-{}", clean.trim_start_matches('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tells_an_expired_session_apart() {
        let expired = failed("users")(SlackError::Api("invalid_auth".into()));
        assert!(matches!(expired, Failure::SessionExpired));
        let other = failed("users")(SlackError::Api("ratelimited".into()));
        assert!(
            matches!(other, Failure::Other(m) if m == "users: Slack rejected the request: ratelimited")
        );
    }

    #[test]
    fn names_cached_files_safely() {
        assert_eq!(file_name("F1", "final report.pdf"), "F1-final report.pdf");
        assert_eq!(file_name("F2", "../../etc/passwd"), "F2-_.._etc_passwd");
        assert_eq!(file_name("F3", "a:b"), "F3-a_b");
    }
}

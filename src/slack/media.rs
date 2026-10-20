//! What can be shown or played from a message besides its text: uploaded
//! files and video links (YouTube…), with their preview pictures.

use super::mrkdwn;
use super::types::{FileKind, Message};

/// A picture to show under a message, keyed for the picture cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// `file:<id>` or `link:<video>`.
    pub key: String,
    pub url: String,
    pub dimensions: Option<(u32, u32)>,
}

/// A video link, from Slack's card under it or from the text alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoLink {
    pub url: String,
    pub service: String,
    pub title: Option<String>,
    pub author: Option<String>,
    pub preview: Option<Preview>,
}

/// The video id of a YouTube link: `watch?v=`, `youtu.be/`, `shorts/`,
/// `embed/` and `live/` forms.
pub fn youtube_id(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    let host = parsed
        .host_str()?
        .trim_start_matches("www.")
        .trim_start_matches("m.");
    let id = match host {
        "youtu.be" => parsed.path_segments()?.next()?.to_string(),
        "youtube.com" | "music.youtube.com" => {
            let mut segments = parsed.path_segments()?;
            match segments.next()? {
                "watch" => parsed
                    .query_pairs()
                    .find(|(key, _)| key == "v")
                    .map(|(_, value)| value.into_owned())?,
                "shorts" | "embed" | "live" => segments.next()?.to_string(),
                _ => return None,
            }
        }
        _ => return None,
    };
    let valid = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    valid.then_some(id)
}

fn youtube_preview(id: &str) -> Preview {
    // `mqdefault` is 16:9, unlike the letterboxed `hqdefault` Slack points to.
    Preview {
        key: format!("link:youtube:{id}"),
        url: format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"),
        dimensions: Some((320, 180)),
    }
}

/// Video links of a message: those Slack made a card for, then YouTube
/// links in the text it did not.
pub fn video_links(message: &Message) -> Vec<VideoLink> {
    let mut videos: Vec<VideoLink> = message
        .attachments
        .iter()
        .filter(|card| card.video_html.is_some())
        .filter_map(|card| {
            let url = card
                .original_url
                .clone()
                .or_else(|| card.from_url.clone())
                .or_else(|| card.title_link.clone())?;
            let preview = match youtube_id(&url) {
                Some(id) => Some(youtube_preview(&id)),
                None => card.thumb_url.clone().map(|thumb| Preview {
                    key: format!("link:{url}"),
                    url: thumb,
                    dimensions: card.thumb_width.zip(card.thumb_height),
                }),
            };
            Some(VideoLink {
                service: card.service_name.clone().unwrap_or_else(|| "Video".into()),
                title: card.title.clone(),
                author: card.author_name.clone(),
                url,
                preview,
            })
        })
        .collect();

    for url in mrkdwn::links(&message.text) {
        let Some(id) = youtube_id(&url) else {
            continue;
        };
        if videos
            .iter()
            .any(|v| youtube_id(&v.url).as_deref() == Some(id.as_str()))
        {
            continue;
        }
        videos.push(VideoLink {
            url,
            service: "YouTube".into(),
            title: None,
            author: None,
            preview: Some(youtube_preview(&id)),
        });
    }
    videos
}

/// Every picture to show under a message: file previews, then videos'.
pub fn previews(message: &Message) -> Vec<Preview> {
    let files = message
        .files
        .iter()
        .filter(|f| !f.id.is_empty())
        .filter_map(|file| {
            let (url, dimensions) = file.preview()?;
            Some(Preview {
                key: format!("file:{}", file.id),
                url: url.to_string(),
                dimensions,
            })
        });
    let videos = video_links(message)
        .into_iter()
        .filter_map(|video| video.preview);
    files.chain(videos).collect()
}

/// What `p` plays: the first video file, else the first video link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Playable {
    File(usize),
    Link(String),
}

pub fn playable(message: &Message) -> Option<Playable> {
    if let Some(index) = message
        .files
        .iter()
        .position(|f| f.kind() == FileKind::Video)
    {
        return Some(Playable::File(index));
    }
    video_links(message)
        .into_iter()
        .next()
        .map(|v| Playable::Link(v.url))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slack::types::{Attachment, message};

    #[test]
    fn finds_youtube_ids_in_every_link_form() {
        for url in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42",
            "https://youtu.be/dQw4w9WgXcQ?si=abc",
            "https://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/shorts/dQw4w9WgXcQ",
            "https://www.youtube.com/embed/dQw4w9WgXcQ",
        ] {
            assert_eq!(youtube_id(url).as_deref(), Some("dQw4w9WgXcQ"), "{url}");
        }
        assert!(youtube_id("https://www.youtube.com/@channel").is_none());
        assert!(youtube_id("https://example.com/watch?v=dQw4w9WgXcQ").is_none());
    }

    #[test]
    fn uses_slack_cards_and_bare_youtube_links() {
        let mut msg = message(
            "1.0",
            "U1",
            "<https://youtu.be/dQw4w9WgXcQ> and <https://youtu.be/aaaaaaaaaaa>",
        );
        msg.attachments.push(Attachment {
            service_name: Some("YouTube".into()),
            title: Some("Never Gonna Give You Up".into()),
            author_name: Some("Rick Astley".into()),
            original_url: Some("https://youtu.be/dQw4w9WgXcQ".into()),
            thumb_url: Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg".into()),
            video_html: Some("<iframe…>".into()),
            ..Attachment::default()
        });

        let videos = video_links(&msg);
        assert_eq!(videos.len(), 2, "the carded link is not counted twice");
        assert_eq!(videos[0].title.as_deref(), Some("Never Gonna Give You Up"));
        assert_eq!(
            videos[0].preview.as_ref().map(|p| p.url.as_str()),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg")
        );
        assert_eq!(videos[1].url, "https://youtu.be/aaaaaaaaaaa");
        assert_eq!(
            playable(&msg),
            Some(Playable::Link("https://youtu.be/dQw4w9WgXcQ".into()))
        );
    }

    #[test]
    fn plays_video_files_before_links() {
        let mut msg = message("1.0", "U1", "<https://youtu.be/dQw4w9WgXcQ>");
        msg.files.push(
            serde_json::from_value(serde_json::json!({"id": "F1", "mimetype": "video/mp4"}))
                .unwrap(),
        );
        assert_eq!(playable(&msg), Some(Playable::File(0)));
        assert!(playable(&message("1.0", "U1", "nothing")).is_none());
    }
}

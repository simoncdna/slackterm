//! Turns Slack's message markup (`<@U123>`, `<https://…|label>`, `&lt;`,
//! backticks…) into plain-text segments the UI can style.

use super::emoji::{self, Piece};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Mention {
        me: bool,
    },
    Channel,
    Link,
    Code,
    /// A custom workspace emoji, shown by name since it is an image.
    Emoji,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub kind: Kind,
}

const ENTITIES: [(&str, char); 3] = [("&amp;", '&'), ("&lt;", '<'), ("&gt;", '>')];

/// The names behind the ids a message refers to.
pub trait Names {
    fn user(&self, id: &str) -> Option<String>;
    fn channel(&self, id: &str) -> Option<String>;
}

pub fn parse(text: &str, my_id: &str, names: &impl Names) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut buffer = String::new();
    let mut in_code = false;
    let mut rest = text;

    'scan: while let Some(c) = rest.chars().next() {
        if rest.starts_with("```") || c == '`' {
            flush(&mut segments, &mut buffer, code_kind(in_code));
            in_code = !in_code;
            rest = &rest[if rest.starts_with("```") { 3 } else { 1 }..];
            continue;
        }
        if c == '<'
            && !in_code
            && let Some(end) = rest.find('>')
        {
            flush(&mut segments, &mut buffer, Kind::Text);
            segments.push(reference(&rest[1..end], my_id, names));
            rest = &rest[end + 1..];
            continue;
        }
        if c == '&' {
            for (entity, decoded) in ENTITIES {
                if rest.starts_with(entity) {
                    buffer.push(decoded);
                    rest = &rest[entity.len()..];
                    continue 'scan;
                }
            }
        }
        buffer.push(c);
        rest = &rest[c.len_utf8()..];
    }
    flush(&mut segments, &mut buffer, code_kind(in_code));
    segments
}

/// The web links of a raw message, in order: `<https://…|label>` and
/// `<https://…>` references.
pub fn links(text: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        let Some(end) = rest[start..].find('>') else {
            break;
        };
        let inner = &rest[start + 1..start + end];
        let target = inner.split_once('|').map_or(inner, |(target, _)| target);
        if target.starts_with("https://") || target.starts_with("http://") {
            links.push(target.to_string());
        }
        rest = &rest[start + end + 1..];
    }
    links
}

/// Whether a raw message notifies the user: a direct mention or a broadcast.
pub fn mentions(text: &str, my_id: &str) -> bool {
    text.contains(&format!("<@{my_id}"))
        || ["<!here", "<!channel", "<!everyone"]
            .iter()
            .any(|broadcast| text.contains(broadcast))
}

fn code_kind(in_code: bool) -> Kind {
    if in_code { Kind::Code } else { Kind::Text }
}

fn flush(segments: &mut Vec<Segment>, buffer: &mut String, kind: Kind) {
    if buffer.is_empty() {
        return;
    }
    let text = std::mem::take(buffer);
    if kind != Kind::Text {
        push(segments, text, kind);
        return;
    }
    for piece in emoji::split(&text) {
        match piece {
            Piece::Text(text) => push(segments, text, Kind::Text),
            Piece::Custom(name) => push(segments, format!(":{name}:"), Kind::Emoji),
        }
    }
}

fn push(segments: &mut Vec<Segment>, text: String, kind: Kind) {
    match segments.last_mut() {
        Some(last) if last.kind == kind => last.text.push_str(&text),
        _ => segments.push(Segment { text, kind }),
    }
}

/// The inside of a `<…>` reference: user, channel, special mention or link.
fn reference(inner: &str, my_id: &str, names: &impl Names) -> Segment {
    // Slack sometimes leaves the label empty: `<#C123|>`.
    let (target, label) = match inner.split_once('|') {
        Some((target, label)) => (target, Some(label).filter(|l| !l.is_empty())),
        None => (inner, None),
    };
    let segment = |text: String, kind| Segment { text, kind };

    if let Some(id) = target.strip_prefix('@') {
        let name = names
            .user(id)
            .or(label.map(str::to_string))
            .unwrap_or_else(|| id.to_string());
        return segment(format!("@{name}"), Kind::Mention { me: id == my_id });
    }
    if let Some(id) = target.strip_prefix('#') {
        let name = label
            .map(str::to_string)
            .or_else(|| names.channel(id))
            .unwrap_or_else(|| id.to_string());
        return segment(format!("#{name}"), Kind::Channel);
    }
    if let Some(special) = target.strip_prefix('!') {
        return match special {
            "here" | "channel" | "everyone" => {
                segment(format!("@{special}"), Kind::Mention { me: true })
            }
            _ if special.starts_with("subteam^") => segment(
                label.unwrap_or("@group").to_string(),
                Kind::Mention { me: false },
            ),
            _ => segment(label.unwrap_or(special).to_string(), Kind::Text),
        };
    }
    let shown = label.unwrap_or(target);
    segment(shown.trim_start_matches("mailto:").to_string(), Kind::Link)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Team;

    impl Names for Team {
        fn user(&self, id: &str) -> Option<String> {
            (id == "U2").then(|| "Camille".to_string())
        }
        fn channel(&self, id: &str) -> Option<String> {
            (id == "C2").then(|| "dev".to_string())
        }
    }

    fn render(text: &str) -> Vec<(String, Kind)> {
        parse(text, "U1", &Team)
            .into_iter()
            .map(|s| (s.text, s.kind))
            .collect()
    }

    #[test]
    fn resolves_user_mentions() {
        assert_eq!(
            render("hi <@U2> and <@U1>"),
            vec![
                ("hi ".into(), Kind::Text),
                ("@Camille".into(), Kind::Mention { me: false }),
                (" and ".into(), Kind::Text),
                ("@U1".into(), Kind::Mention { me: true }),
            ]
        );
    }

    #[test]
    fn renders_channels_links_and_broadcasts() {
        assert_eq!(
            render("<#C1|deploys> <https://x.dev|the PR> <!here>"),
            vec![
                ("#deploys".into(), Kind::Channel),
                (" ".into(), Kind::Text),
                ("the PR".into(), Kind::Link),
                (" ".into(), Kind::Text),
                ("@here".into(), Kind::Mention { me: true }),
            ]
        );
    }

    #[test]
    fn names_channels_slack_left_unlabeled() {
        assert_eq!(
            render("<#C2> <#C2|> <#C9>"),
            vec![
                ("#dev".into(), Kind::Channel),
                (" ".into(), Kind::Text),
                ("#dev".into(), Kind::Channel),
                (" ".into(), Kind::Text),
                ("#C9".into(), Kind::Channel),
            ]
        );
    }

    #[test]
    fn decodes_entities_and_code() {
        assert_eq!(
            render("a &lt; b `x &amp;&amp; y`"),
            vec![("a < b ".into(), Kind::Text), ("x && y".into(), Kind::Code),]
        );
    }

    #[test]
    fn keeps_references_inside_code_blocks_literal() {
        assert_eq!(
            render("```\n<div>\n```"),
            vec![("\n<div>\n".into(), Kind::Code)]
        );
    }

    #[test]
    fn renders_emoji_but_not_inside_code() {
        assert_eq!(
            render("go :rocket: :pictaheart: `:tada:`"),
            vec![
                ("go 🚀 ".into(), Kind::Text),
                (":pictaheart:".into(), Kind::Emoji),
                (" ".into(), Kind::Text),
                (":tada:".into(), Kind::Code),
            ]
        );
    }

    #[test]
    fn lists_web_links() {
        assert_eq!(
            links(
                "see <https://youtu.be/abc|this> and <https://x.dev> but not <@U1> nor <#C1|dev>"
            ),
            ["https://youtu.be/abc", "https://x.dev"]
        );
    }

    #[test]
    fn detects_mentions_of_the_user() {
        assert!(mentions("hey <@U1>", "U1"));
        assert!(mentions("<!channel> deploy", "U1"));
        assert!(!mentions("hey <@U2>", "U1"));
    }
}

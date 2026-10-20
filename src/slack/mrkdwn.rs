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

/// How Slack's mrkdwn dresses text: `*bold*`, `_italic_`, `~strike~`, and
/// `>` quotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Emphasis {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub quote: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub text: String,
    pub kind: Kind,
    pub emphasis: Emphasis,
}

/// Drawn at the start of every quoted line.
const QUOTE_BAR: &str = "▎ ";

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
    let mut emphasis = Emphasis::default();
    // `>>>` quotes the rest of the message, `>` its line.
    let mut quote_rest = false;
    let mut line_start = true;
    let mut previous: Option<char> = None;
    let mut rest = text;

    'scan: while let Some(c) = rest.chars().next() {
        if line_start && !in_code {
            line_start = false;
            let quoted = if let Some(after) = rest.strip_prefix("&gt;&gt;&gt;") {
                quote_rest = true;
                Some(after)
            } else {
                rest.strip_prefix("&gt;")
            };
            if quoted.is_some() || quote_rest {
                flush(&mut segments, &mut buffer, Kind::Text, emphasis);
                emphasis.quote = true;
                push(&mut segments, QUOTE_BAR.to_string(), Kind::Text, emphasis);
                if let Some(after) = quoted {
                    rest = after.strip_prefix(' ').unwrap_or(after);
                    continue;
                }
            }
        }
        if rest.starts_with("```") || c == '`' {
            let kind = code_kind(in_code);
            flush(
                &mut segments,
                &mut buffer,
                kind,
                emphasis_of(kind, emphasis),
            );
            in_code = !in_code;
            rest = &rest[if rest.starts_with("```") { 3 } else { 1 }..];
            previous = Some('`');
            continue;
        }
        if c == '\n' && !in_code {
            // Emphasis ends with the line; so does a `>` quote.
            flush(&mut segments, &mut buffer, Kind::Text, emphasis);
            emphasis = Emphasis {
                quote: quote_rest,
                ..Emphasis::default()
            };
            line_start = true;
        }
        if let Some(marker) = emphasis_marker(c).filter(|_| !in_code) {
            let on = marker(&mut emphasis);
            let next = rest[1..].chars().next();
            if (*on && closes(previous, next)) || (!*on && opens(previous, rest)) {
                flush(&mut segments, &mut buffer, Kind::Text, emphasis);
                let on = marker(&mut emphasis);
                *on = !*on;
                previous = Some(c);
                rest = &rest[1..];
                continue;
            }
        }
        if c == '<'
            && !in_code
            && let Some(end) = rest.find('>')
        {
            flush(&mut segments, &mut buffer, Kind::Text, emphasis);
            let mut segment = reference(&rest[1..end], my_id, names);
            segment.emphasis = emphasis;
            segments.push(segment);
            rest = &rest[end + 1..];
            previous = Some('>');
            continue;
        }
        if c == '&' {
            for (entity, decoded) in ENTITIES {
                if rest.starts_with(entity) {
                    buffer.push(decoded);
                    rest = &rest[entity.len()..];
                    previous = Some(decoded);
                    continue 'scan;
                }
            }
        }
        buffer.push(c);
        previous = Some(c);
        rest = &rest[c.len_utf8()..];
    }
    let kind = code_kind(in_code);
    flush(
        &mut segments,
        &mut buffer,
        kind,
        emphasis_of(kind, emphasis),
    );
    segments
}

/// The emphasis a marker character switches: `*`, `_` or `~`.
fn emphasis_marker(c: char) -> Option<fn(&mut Emphasis) -> &mut bool> {
    match c {
        '*' => Some(|e| &mut e.bold),
        '_' => Some(|e| &mut e.italic),
        '~' => Some(|e| &mut e.strike),
        _ => None,
    }
}

/// A marker opens at the start of a word, before text, when the same
/// marker closes later on the line: `_init_`, not `snake_case` nor `2 * 3`.
fn opens(previous: Option<char>, rest: &str) -> bool {
    let mut chars = rest.chars();
    let Some(marker) = chars.next() else {
        return false;
    };
    let starts_word = previous.is_none_or(|p| !p.is_alphanumeric());
    let line: Vec<char> = chars.take_while(|c| *c != '\n').collect();
    let before_text = line
        .first()
        .is_some_and(|c| !c.is_whitespace() && *c != marker);
    let closed = (1..line.len())
        .any(|i| line[i] == marker && closes(Some(line[i - 1]), line.get(i + 1).copied()));
    starts_word && before_text && closed
}

/// A marker closes at the end of a word: after text, before a space or
/// punctuation.
fn closes(previous: Option<char>, next: Option<char>) -> bool {
    previous.is_some_and(|p| !p.is_whitespace()) && next.is_none_or(|n| !n.is_alphanumeric())
}

/// Code shows as typed, without emphasis but in a quote.
fn emphasis_of(kind: Kind, emphasis: Emphasis) -> Emphasis {
    match kind {
        Kind::Code => Emphasis {
            quote: emphasis.quote,
            ..Emphasis::default()
        },
        _ => emphasis,
    }
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

fn flush(segments: &mut Vec<Segment>, buffer: &mut String, kind: Kind, emphasis: Emphasis) {
    if buffer.is_empty() {
        return;
    }
    let text = std::mem::take(buffer);
    if kind != Kind::Text {
        push(segments, text, kind, emphasis);
        return;
    }
    for piece in emoji::split(&text) {
        match piece {
            Piece::Text(text) => push(segments, text, Kind::Text, emphasis),
            Piece::Custom(name) => push(segments, format!(":{name}:"), Kind::Emoji, emphasis),
        }
    }
}

fn push(segments: &mut Vec<Segment>, text: String, kind: Kind, emphasis: Emphasis) {
    match segments.last_mut() {
        Some(last) if last.kind == kind && last.emphasis == emphasis => last.text.push_str(&text),
        _ => segments.push(Segment {
            text,
            kind,
            emphasis,
        }),
    }
}

/// The inside of a `<…>` reference: user, channel, special mention or link.
fn reference(inner: &str, my_id: &str, names: &impl Names) -> Segment {
    // Slack sometimes leaves the label empty: `<#C123|>`.
    let (target, label) = match inner.split_once('|') {
        Some((target, label)) => (target, Some(label).filter(|l| !l.is_empty())),
        None => (inner, None),
    };
    let segment = |text: String, kind| Segment {
        text,
        kind,
        emphasis: Emphasis::default(),
    };

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

    fn dressed(text: &str) -> Vec<(String, Emphasis)> {
        parse(text, "U1", &Team)
            .into_iter()
            .map(|s| (s.text, s.emphasis))
            .collect()
    }

    const PLAIN: Emphasis = Emphasis {
        bold: false,
        italic: false,
        strike: false,
        quote: false,
    };

    #[test]
    fn dresses_bold_italic_and_strike() {
        let bold = Emphasis {
            bold: true,
            ..PLAIN
        };
        let italic = Emphasis {
            italic: true,
            ..PLAIN
        };
        let strike = Emphasis {
            strike: true,
            ..PLAIN
        };
        assert_eq!(
            dressed("*ship it*, _now_ ~later~"),
            vec![
                ("ship it".into(), bold),
                (", ".into(), PLAIN),
                ("now".into(), italic),
                (" ".into(), PLAIN),
                ("later".into(), strike),
            ]
        );
        let both = Emphasis {
            bold: true,
            italic: true,
            ..PLAIN
        };
        assert_eq!(dressed("*_both_*"), vec![("both".into(), both)]);
    }

    #[test]
    fn leaves_markers_that_dress_nothing() {
        for text in [
            "snake_case_name",
            "2*3*4 and 5 * 6",
            "a lone _ and * and ~",
            "*not closed",
            "** nothing",
            ":white_check_mark: done",
        ] {
            assert!(dressed(text).iter().all(|(_, e)| *e == PLAIN), "{text}");
        }
        // Emphasis stops at the end of the line.
        assert!(dressed("*a\nb*").iter().all(|(_, e)| !e.bold));
    }

    #[test]
    fn quotes_a_line_or_the_rest() {
        let quote = Emphasis {
            quote: true,
            ..PLAIN
        };
        assert_eq!(
            dressed("&gt; said\nreplied"),
            vec![("▎ said".into(), quote), ("\nreplied".into(), PLAIN)]
        );
        assert_eq!(
            dressed("&gt;&gt;&gt; one\ntwo"),
            vec![("▎ one\n▎ two".into(), quote)]
        );
    }

    #[test]
    fn code_stays_as_typed() {
        assert_eq!(
            render("`*not bold*`"),
            vec![("*not bold*".into(), Kind::Code)]
        );
        let bold_mention = parse("*<@U2>*", "U1", &Team);
        assert_eq!(bold_mention[0].text, "@Camille");
        assert!(bold_mention[0].emphasis.bold);
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

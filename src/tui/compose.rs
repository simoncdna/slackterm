//! The message box: what the user typed turned into Slack's markup, and
//! people or channels suggested while a name is typed after `@` or `#`.

use std::collections::HashMap;

use super::fuzzy::fuzzy_match;
use super::input::Input;
use super::state::{App, ChannelKind, Mode};

/// Suggestions shown at most.
const SHOWN: usize = 6;

/// Broadcasts, with what they do.
const BROADCASTS: [(&str, &str); 3] = [
    ("here", "Everyone active in the channel"),
    ("channel", "Everyone in the channel"),
    ("everyone", "Everyone in the workspace"),
];

/// What a handle or a channel name is made of.
fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '.' | '_' | '-')
}

/// Slack's markup for what the user typed: `<`, `>` and `&` escaped, and
/// `@name`, `@here` and `#channel` turned into mentions Slack notifies.
/// Names inside code, and unknown ones, stay as typed.
pub fn encode(text: &str, app: &App) -> String {
    let names = Names::new(app);
    let text = escape(text);
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    let mut in_code = false;
    let mut previous: Option<char> = None;

    'text: while let Some(c) = rest.chars().next() {
        if c == '`' {
            in_code = !in_code;
        }
        // Not the `@` of an e-mail address.
        let starts_name =
            matches!(c, '@' | '#') && !in_code && previous.is_none_or(|p| !p.is_alphanumeric());
        if starts_name {
            let length: usize = rest[1..]
                .chars()
                .take_while(|c| is_name_char(*c))
                .map(char::len_utf8)
                .sum();
            let mut name = &rest[1..1 + length];
            // `@hugo.` may end a sentence rather than a handle.
            loop {
                if let Some(markup) = names.markup(c, name) {
                    out.push_str(&markup);
                    rest = &rest[1 + name.len()..];
                    previous = name.chars().next_back();
                    continue 'text;
                }
                match name.strip_suffix(['.', '_', '-']) {
                    Some(shorter) => name = shorter,
                    None => break,
                }
            }
        }
        out.push(c);
        previous = Some(c);
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// A message as the user would type it again, to edit it: mentions back
/// to `@handle` and `#channel`, links to their address. The way back from
/// [`encode`].
pub fn decode(text: &str, app: &App) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    // Typed `<` are escaped, so every `<` opens a reference.
    while let Some(start) = rest.find('<') {
        let Some(length) = rest[start..].find('>') else {
            break;
        };
        out.push_str(&rest[..start]);
        out.push_str(&typed_reference(&rest[start + 1..start + length], app));
        rest = &rest[start + length + 1..];
    }
    out.push_str(rest);
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// The inside of a `<…>` reference, as it would be typed.
fn typed_reference(inner: &str, app: &App) -> String {
    let (target, label) = match inner.split_once('|') {
        Some((target, label)) => (target, Some(label).filter(|l| !l.is_empty())),
        None => (inner, None),
    };
    if let Some(id) = target.strip_prefix('@') {
        let handle = app.users.get(id).map(|u| u.name.clone());
        let name = handle.or(label.map(str::to_string));
        return format!("@{}", name.unwrap_or_else(|| id.to_string()));
    }
    if let Some(id) = target.strip_prefix('#') {
        let name = label
            .map(str::to_string)
            .or_else(|| app.channel_name_of(id));
        return format!("#{}", name.unwrap_or_else(|| id.to_string()));
    }
    if let Some(special) = target.strip_prefix('!') {
        return match special {
            "here" | "channel" | "everyone" => format!("@{special}"),
            _ => label.unwrap_or_default().to_string(),
        };
    }
    target.trim_start_matches("mailto:").to_string()
}

/// Slack reads `<`, `>` and `&` as markup, so typed text must escape them.
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Who and what a name typed after `@` or `#` can mean, without case.
struct Names<'a> {
    /// `None` when two people go by that name.
    people: HashMap<String, Option<&'a str>>,
    /// Id and name.
    channels: HashMap<String, (&'a str, &'a str)>,
}

impl<'a> Names<'a> {
    fn new(app: &'a App) -> Self {
        let users = app.users.values().filter(|u| !u.deleted);
        let mut people: HashMap<String, Option<&str>> = HashMap::new();
        for user in users.clone() {
            let shown = user.display_name().to_lowercase();
            if !shown.contains(' ') {
                people
                    .entry(shown)
                    .and_modify(|id| {
                        if *id != Some(user.id.as_str()) {
                            *id = None;
                        }
                    })
                    .or_insert(Some(&user.id));
            }
        }
        // Handles are unique, and win over the names people show.
        for user in users {
            people.insert(user.name.to_lowercase(), Some(&user.id));
        }

        let joined = app
            .channels
            .iter()
            .filter(|c| matches!(c.kind, ChannelKind::Public | ChannelKind::Private))
            .map(|c| (c.name.to_lowercase(), (c.id.as_str(), c.name.as_str())));
        let others = app.public_channels.iter().filter_map(|c| {
            let name = c.name.as_deref()?;
            Some((name.to_lowercase(), (c.id.as_str(), name)))
        });
        Self {
            people,
            channels: others.chain(joined).collect(),
        }
    }

    fn markup(&self, sigil: char, name: &str) -> Option<String> {
        let name = name.to_lowercase();
        if sigil == '#' {
            let (id, name) = self.channels.get(&name)?;
            return Some(format!("<#{id}|{name}>"));
        }
        if BROADCASTS.iter().any(|(broadcast, _)| *broadcast == name) {
            return Some(format!("<!{name}>"));
        }
        let id = self.people.get(&name).copied().flatten()?;
        Some(format!("<@{id}>"))
    }
}

/// A name being typed after `@` or `#`, with the suggestion picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Byte offset of the `@` or `#` in the message.
    pub start: usize,
    pub sigil: char,
    /// What follows the sigil, up to the cursor.
    pub query: String,
    pub selected: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// What replaces the typed name: `@hugo.bernard`, `#dev`.
    pub text: String,
    /// Said after it: the person's name, what a broadcast does.
    pub detail: String,
}

/// Where the `@` or `#` of the name the cursor ends is, if it ends one.
pub fn name_before_cursor(input: &Input) -> Option<usize> {
    let before = &input.text()[..input.cursor()];
    let length: usize = before
        .chars()
        .rev()
        .take_while(|c| is_name_char(*c))
        .map(char::len_utf8)
        .sum();
    let name_start = before.len() - length;
    let sigil = before[..name_start].chars().next_back()?;
    let sigil_at = name_start - sigil.len_utf8();
    let starts_word = before[..sigil_at]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric());
    (matches!(sigil, '@' | '#') && starts_word).then_some(sigil_at)
}

/// Follows the cursor: suggests while it ends a name, unless the user
/// closed the suggestions for that name.
pub fn refresh(app: &mut App) {
    let start = (app.mode == Mode::Insert && app.overlay.is_none())
        .then(|| name_before_cursor(&app.input))
        .flatten();
    let Some(start) = start else {
        app.completion = None;
        app.completion_closed = None;
        return;
    };
    if app.completion_closed == Some(start) {
        app.completion = None;
        return;
    }
    let text = app.input.text();
    let sigil = text[start..].chars().next().unwrap_or('@');
    let query = text[start + 1..app.input.cursor()].to_string();
    let selected = match &app.completion {
        Some(old) if old.start == start && old.query == query => old.selected,
        _ => 0,
    };
    app.completion = Some(Completion {
        start,
        sigil,
        query,
        selected,
    });
}

/// The people or channels that fit what is typed, best first.
pub fn suggestions(app: &App, completion: &Completion) -> Vec<Suggestion> {
    let query = completion.query.as_str();
    let mut found: Vec<(i32, Suggestion)> = Vec::new();
    if completion.sigil == '@' {
        for (name, detail) in BROADCASTS {
            if name.starts_with(&query.to_lowercase())
                && let Some((score, _)) = fuzzy_match(query, name)
            {
                let text = format!("@{name}");
                let detail = detail.to_string();
                found.push((score, Suggestion { text, detail }));
            }
        }
        for user in app.users.values().filter(|u| !u.deleted) {
            let score = [user.name.as_str(), user.display_name()]
                .into_iter()
                .filter_map(|name| fuzzy_match(query, name).map(|(score, _)| score))
                .max();
            if let Some(score) = score {
                let text = format!("@{}", user.name);
                let detail = user.display_name().to_string();
                found.push((score, Suggestion { text, detail }));
            }
        }
    } else {
        let joined = app
            .channels
            .iter()
            .filter(|c| matches!(c.kind, ChannelKind::Public | ChannelKind::Private))
            .map(|c| (c.id.as_str(), c.name.as_str(), c.topic.as_str()));
        let others = app.public_channels.iter().filter_map(|c| {
            let topic = c.topic.as_ref().map_or("", |t| t.value.as_str());
            Some((c.id.as_str(), c.name.as_deref()?, topic))
        });
        let mut seen = Vec::new();
        for (id, name, topic) in joined.chain(others) {
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            if let Some((score, _)) = fuzzy_match(query, name) {
                let text = format!("#{name}");
                let detail = topic.to_string();
                found.push((score, Suggestion { text, detail }));
            }
        }
    }
    // The closest names first: `#dev` before `#design` for `de`.
    found.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.1.text.len().cmp(&b.1.text.len()))
            .then_with(|| a.1.text.cmp(&b.1.text))
    });
    found.into_iter().take(SHOWN).map(|(_, s)| s).collect()
}

/// Puts the picked suggestion in place of the name being typed.
pub fn accept(app: &mut App, completion: &Completion, suggestion: &Suggestion) {
    let range = completion.start..app.input.cursor();
    app.input.replace(range, &format!("{} ", suggestion.text));
    app.completion = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::slack::User;
    use crate::tui::state::Channel;
    use crate::tui::state::fixtures::*;

    fn person(id: &str, handle: &str, shown: &str) -> User {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": handle, "profile": {"display_name": shown}
        }))
        .unwrap()
    }

    fn team() -> App {
        let mut app = app();
        for user in [
            person("U2", "hugo.bernard", "Hugo"),
            person("U3", "camille", "Camille"),
            person("U4", "camille.roux", "Camille"),
            person("U5", "ines", "Inès Lefèvre"),
        ] {
            app.users.insert(user.id.clone(), user);
        }
        app.channels = vec![
            Channel::from_conversation(conversation("C1", "dev")),
            Channel::from_conversation(direct("D1", "U2")),
        ];
        app.public_channels = vec![conversation("C1", "dev"), conversation("C9", "design")];
        app
    }

    #[test]
    fn turns_names_into_mentions() {
        let app = team();
        let encode = |text| encode(text, &app);
        assert_eq!(encode("hi @hugo.bernard!"), "hi <@U2>!");
        assert_eq!(encode("@Hugo, see #dev."), "<@U2>, see <#C1|dev>.");
        assert_eq!(
            encode("@here deploy in #design"),
            "<!here> deploy in <#C9|design>"
        );
        assert_eq!(encode("@camille.roux and @ines"), "<@U4> and <@U5>");
    }

    #[test]
    fn leaves_what_is_not_a_known_name() {
        let app = team();
        let encode = |text| encode(text, &app);
        assert_eq!(encode("mail alex@hugo.bernard"), "mail alex@hugo.bernard");
        assert_eq!(encode("run `@hugo.bernard` now"), "run `@hugo.bernard` now");
        assert_eq!(encode("@nobody in #nowhere"), "@nobody in #nowhere");
        // Two people show as Camille: only the handle says which.
        assert_eq!(encode("@Camille"), "<@U3>");
        assert_eq!(encode("a < b && @hugo"), "a &lt; b &amp;&amp; <@U2>");
    }

    #[test]
    fn gives_back_what_was_typed_to_edit_it() {
        let app = team();
        for typed in [
            "hi @hugo.bernard, see #dev & <b>",
            "@here `code` in #design",
            "nothing special",
        ] {
            assert_eq!(decode(&encode(typed, &app), &app), typed);
        }
        let link = "read <https://x.dev|the doc> or <mailto:a@b.c|a@b.c> <@U9|ghost>";
        assert_eq!(decode(link, &app), "read https://x.dev or a@b.c @ghost");
    }

    #[test]
    fn finds_the_name_before_the_cursor() {
        let typed = |text: &str| {
            let mut input = Input::default();
            input.insert_str(text);
            name_before_cursor(&input)
        };
        assert_eq!(typed("hi @hu"), Some(3));
        assert_eq!(typed("@"), Some(0));
        assert_eq!(typed("in #de"), Some(3));
        assert_eq!(typed("é@hu"), None);
        assert_eq!(typed("mail alex@hu"), None);
        assert_eq!(typed("hi @hugo "), None);
        assert_eq!(typed("hi hugo"), None);
    }

    #[test]
    fn suggests_people_broadcasts_and_channels() {
        let app = team();
        let completion = |sigil, query: &str| Completion {
            start: 0,
            sigil,
            query: query.into(),
            selected: 0,
        };
        let texts = |sigil, query| -> Vec<String> {
            suggestions(&app, &completion(sigil, query))
                .into_iter()
                .map(|s| s.text)
                .collect()
        };
        assert_eq!(texts('@', "hu"), ["@hugo.bernard"]);
        assert_eq!(texts('@', "her")[0], "@here");
        assert_eq!(texts('@', "cam"), ["@camille", "@camille.roux"]);
        assert_eq!(texts('#', "de"), ["#dev", "#design"]);
        assert_eq!(texts('@', "").len(), SHOWN);
    }
}

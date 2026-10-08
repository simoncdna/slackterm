//! Slack emoji shortcodes (`:tada:`, `:+1::skin-tone-3:`) to Unicode.
//!
//! Names come from emoji-data, the table Slack itself uses, embedded as
//! `emoji_names.tsv`. Custom workspace emoji are images and have no Unicode
//! equivalent.

use std::collections::HashMap;
use std::sync::LazyLock;

use emojis::SkinTone;

/// One emoji per line: `<emoji>\t<name>,<alias>,…`, in Slack's picker order.
const TABLE: &str = include_str!("emoji_names.tsv");

pub struct Entry {
    pub glyph: &'static str,
    /// The first one is the name Slack prefers.
    pub names: Vec<&'static str>,
}

struct Index {
    entries: Vec<Entry>,
    by_name: HashMap<&'static str, usize>,
}

static INDEX: LazyLock<Index> = LazyLock::new(|| {
    let entries: Vec<Entry> = TABLE
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (glyph, names) = line.split_once('\t')?;
            Some(Entry {
                glyph,
                names: names.split(',').collect(),
            })
        })
        .collect();
    let by_name = entries
        .iter()
        .enumerate()
        .flat_map(|(index, entry)| entry.names.iter().map(move |name| (*name, index)))
        .collect();
    Index { entries, by_name }
});

/// A piece of message text: plain text with known shortcodes already
/// replaced, or the name of a custom emoji.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Custom(String),
}

/// Every standard emoji, in Slack's order.
pub fn all() -> &'static [Entry] {
    &INDEX.entries
}

/// The Unicode emoji for a Slack name, including reaction names with a skin
/// tone such as `+1::skin-tone-3`.
pub fn lookup(name: &str) -> Option<String> {
    let (base, tone) = match name.split_once("::skin-tone-") {
        Some((base, tone)) => (base, skin_tone(tone)),
        None => (name, None),
    };
    let glyph = INDEX.by_name.get(base).map(|&i| INDEX.entries[i].glyph)?;
    let toned = tone
        .and_then(|tone| emojis::get(glyph)?.with_skin_tone(tone))
        .map(|emoji| emoji.as_str());
    Some(toned.unwrap_or(glyph).to_string())
}

fn skin_tone(level: &str) -> Option<SkinTone> {
    match level {
        "2" => Some(SkinTone::Light),
        "3" => Some(SkinTone::MediumLight),
        "4" => Some(SkinTone::Medium),
        "5" => Some(SkinTone::MediumDark),
        "6" => Some(SkinTone::Dark),
        _ => None,
    }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '\'')
}

/// Replaces the shortcodes of `text` that have a Unicode equivalent and
/// splits out the custom ones.
pub fn split(text: &str) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut plain = String::new();
    let mut rest = text;

    while let Some(start) = rest.find(':') {
        plain.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let name_len = after
            .find(':')
            .filter(|&len| len > 0 && after[..len].chars().all(is_name_char));
        let Some(len) = name_len else {
            plain.push(':');
            rest = after;
            continue;
        };
        let name = &after[..len];
        rest = &after[len + 1..];

        // `:+1::skin-tone-3:` is one emoji.
        let mut full_name = name.to_string();
        if let Some(tone) = rest
            .strip_prefix(":skin-tone-")
            .and_then(|t| t.get(..2))
            .filter(|t| t.ends_with(':'))
        {
            full_name = format!("{name}::skin-tone-{}", &tone[..1]);
            rest = &rest[":skin-tone-".len() + 2..];
        }

        match lookup(&full_name) {
            Some(glyph) => plain.push_str(&glyph),
            None => {
                if !plain.is_empty() {
                    pieces.push(Piece::Text(std::mem::take(&mut plain)));
                }
                pieces.push(Piece::Custom(name.to_string()));
            }
        }
    }
    plain.push_str(rest);
    if !plain.is_empty() {
        pieces.push(Piece::Text(plain));
    }
    pieces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_slack_names_and_their_aliases() {
        assert_eq!(lookup("tada").as_deref(), Some("🎉"));
        assert_eq!(lookup("+1").as_deref(), Some("👍"));
        assert_eq!(lookup("thumbsup").as_deref(), Some("👍"));
        assert_eq!(lookup("thinking_face").as_deref(), Some("🤔"));
        assert_eq!(lookup("star-struck").as_deref(), Some("🤩"));
        assert_eq!(lookup("flag-mx").as_deref(), Some("🇲🇽"));
        assert!(lookup("woman-running").is_some());
        assert!(lookup("male-technologist").is_some());
        assert!(lookup("pictaheart").is_none());
    }

    #[test]
    fn the_table_is_complete_and_ordered() {
        assert!(all().len() > 1800);
        assert_eq!(all()[0].names[0], "grinning");
    }

    #[test]
    fn applies_skin_tones() {
        let toned = lookup("+1::skin-tone-4").unwrap();
        assert_ne!(toned, lookup("+1").unwrap());
        assert!(toned.starts_with('👍'));
    }

    #[test]
    fn replaces_shortcodes_in_text() {
        assert_eq!(
            split("bravo :tada: :+1::skin-tone-2: et :pictaheart: !"),
            vec![
                Piece::Text(format!(
                    "bravo 🎉 {} et ",
                    lookup("+1::skin-tone-2").unwrap()
                )),
                Piece::Custom("pictaheart".into()),
                Piece::Text(" !".into()),
            ]
        );
    }

    #[test]
    fn leaves_times_and_lone_colons_alone() {
        let text = "rdv à 10:30 : ok, ratio 3:2";
        assert_eq!(split(text), vec![Piece::Text(text.into())]);
    }
}

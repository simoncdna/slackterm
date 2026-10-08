//! Slack emoji shortcodes (`:tada:`, `:+1::skin-tone-3:`) to Unicode.
//!
//! The `emojis` crate knows GitHub's shortcodes, which match Slack's for the
//! most part; the rest is covered by a few naming rules and aliases. Custom
//! workspace emoji are images and have no Unicode equivalent.

use emojis::SkinTone;

/// Slack names whose GitHub equivalent cannot be derived by a rule.
const ALIASES: &[(&str, &str)] = &[
    ("thinking_face", "thinking"),
    ("face_with_rolling_eyes", "roll_eyes"),
    ("simple_smile", "slightly_smiling_face"),
    ("plus1", "+1"),
    ("rolling_on_the_floor_laughing", "rofl"),
    ("beach_with_umbrella", "beach_umbrella"),
    ("shopping_bags", "shopping"),
    ("large_blue_square", "blue_square"),
    ("ballot_box_with_ballot", "ballot_box"),
    ("face_palm", "facepalm"),
    ("hugging_face", "hugs"),
    ("white_frowning_face", "frowning_face"),
    ("face_with_hand_over_mouth", "hand_over_mouth"),
    ("robot_face", "robot"),
    ("the_horns", "metal"),
    ("spock-hand", "vulcan_salute"),
    ("i_love_you_hand_sign", "love_you_gesture"),
    ("face_with_cowboy_hat", "cowboy_hat_face"),
    (
        "smiling_face_with_3_hearts",
        "smiling_face_with_three_hearts",
    ),
    ("person_climbing", "climbing"),
];

/// A piece of message text: plain text with known shortcodes already
/// replaced, or the name of a custom emoji.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Custom(String),
}

/// The Unicode emoji for a Slack name, including reaction names with a skin
/// tone such as `+1::skin-tone-3`.
pub fn lookup(name: &str) -> Option<String> {
    let (base, tone) = match name.split_once("::skin-tone-") {
        Some((base, tone)) => (base, skin_tone(tone)),
        None => (name, None),
    };
    if let Some(flag) = country_flag(base) {
        return Some(flag);
    }
    let emoji = find(base)?;
    let toned = tone.and_then(|tone| emoji.with_skin_tone(tone));
    Some(toned.unwrap_or(emoji).as_str().to_string())
}

fn find(name: &str) -> Option<&'static emojis::Emoji> {
    let alias = ALIASES
        .iter()
        .find(|(slack, _)| *slack == name)
        .map(|(_, github)| *github);
    let underscored = name.replace('-', "_");
    // Slack's `woman-running` / `male-technologist` are GitHub's
    // `running_woman` / `man_technologist`.
    let gendered = name.split_once('-').and_then(|(who, what)| {
        let what = what.replace('-', "_");
        match who {
            "man" | "woman" => Some(format!("{what}_{who}")),
            "male" => Some(format!("man_{what}")),
            "female" => Some(format!("woman_{what}")),
            _ => None,
        }
    });
    [
        Some(name),
        alias,
        Some(underscored.as_str()),
        gendered.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find_map(emojis::get_by_shortcode)
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

/// `flag-mx` → 🇲🇽, from the two-letter country code.
fn country_flag(name: &str) -> Option<String> {
    let code = name.strip_prefix("flag-")?;
    if code.len() != 2 || !code.chars().all(|c| c.is_ascii_alphabetic()) {
        return find(code).map(|e| e.as_str().to_string());
    }
    code.to_ascii_uppercase()
        .chars()
        .map(|c| char::from_u32(0x1F1E6 + (c as u32 - 'A' as u32)))
        .collect()
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
    fn every_alias_points_to_a_known_emoji() {
        for (slack, github) in ALIASES {
            assert!(
                emojis::get_by_shortcode(github).is_some(),
                "{slack} → {github} is unknown"
            );
        }
    }

    #[test]
    fn looks_up_common_slack_names() {
        assert_eq!(lookup("tada").as_deref(), Some("🎉"));
        assert_eq!(lookup("+1").as_deref(), Some("👍"));
        assert_eq!(lookup("thinking_face").as_deref(), Some("🤔"));
        assert_eq!(lookup("star-struck").as_deref(), Some("🤩"));
        assert!(lookup("woman-running").is_some());
        assert!(lookup("male-technologist").is_some());
        assert!(lookup("pictaheart").is_none());
    }

    #[test]
    fn applies_skin_tones() {
        let toned = lookup("+1::skin-tone-4").unwrap();
        assert_ne!(toned, lookup("+1").unwrap());
        assert!(toned.starts_with('👍'));
    }

    #[test]
    fn builds_country_flags() {
        assert_eq!(lookup("flag-mx").as_deref(), Some("🇲🇽"));
        assert_eq!(lookup("flag-fr").as_deref(), Some("🇫🇷"));
        assert!(lookup("flag-scotland").is_some());
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

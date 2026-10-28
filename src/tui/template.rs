//! The config file with every option commented out, written from the code
//! so that it always lists what slackterm reads.

use ratatui::style::Color;
use serde::Serialize;

use crate::config::{
    Accent, AvatarStyle, Choice, Clock, Colors, Layout, Mouse, Notifications, Settings, ThemeName,
};
use crate::themes::UserTheme;

use super::keymap::ACTIONS;
use super::theme::Theme;

const HEADER: &str = "\
# slackterm configuration.
#
# Every option is commented out with its default value: uncomment a line to
# change it. The settings screen (`,` in slackterm) writes here too, and
# keeps your comments.

";

const KEYS: &str = "
# Keys of normal mode. Give one key or a list of keys, or [] for none.
# A key is a character (\"q\", \"G\", \"?\") or one of enter, esc, tab, space,
# backspace, delete, up, down, left, right, home, end, pageup, pagedown and
# f1 to f12, after ctrl+, alt+ or shift+ if needed: \"ctrl+k\", \"shift+tab\".
# The keys of the message box do not change.
[keys]
";

const COLORS: &str = "
# Colors that replace the theme's: \"#rrggbb\", a terminal color name such as
# \"red\" or \"lightblue\", a palette index such as \"42\", or \"reset\" for the
# terminal's own color. Shown here with the colors of the Night theme.
[colors]
";

pub fn default_config() -> String {
    let mut out = HEADER.to_string();
    option::<Layout>(&mut out, "layout");
    line(
        &mut out,
        "theme = \"night\"",
        "night | light | gruvbox | nord, or one of yours in themes/",
    );
    option::<Accent>(&mut out, "accent");
    option::<AvatarStyle>(&mut out, "avatars");
    option::<Clock>(&mut out, "clock");
    option::<Mouse>(&mut out, "mouse");
    option::<Notifications>(&mut out, "notifications");

    out.push_str(KEYS);
    for definition in ACTIONS {
        let keys = match definition.keys {
            [key] => quote(key),
            keys => format!(
                "[{}]",
                keys.iter()
                    .map(|key| quote(key))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        line(
            &mut out,
            &format!("{} = {keys}", definition.name),
            definition.help,
        );
    }

    out.push_str(COLORS);
    for (option, help) in color_options(&Theme::from_settings(&Settings::default())) {
        line(&mut out, &option, help);
    }
    out
}

/// A theme file with every color of `from`, for the user to change.
pub fn theme_file(name: &str, from: ThemeName, themes: &[UserTheme]) -> String {
    let settings = Settings {
        theme: from,
        ..Settings::default()
    };
    let base = themes
        .iter()
        .find(|theme| theme.name == from)
        .map_or(from, |theme| theme.base);
    let theme = Theme::new(&settings, themes, &Colors::default());
    let mut out = format!(
        "# The {name} theme of slackterm, picked in the settings (`,`) or with\n\
         # `theme = {}` in config.toml. Colors are \"#rrggbb\", a terminal color\n\
         # name such as \"red\", a palette index such as \"42\", or \"reset\" for\n\
         # the terminal's own. Those left out come from `base`.\n\n",
        quote(name)
    );
    out.push_str(&format!("base = {}\n\n", quote(base.key())));
    for (option, help) in color_options(&theme) {
        out.push_str(&format!("{option:<36} # {help}\n"));
    }
    out
}

/// Each color of `theme` as written in a file, with what it colors.
fn color_options(theme: &Theme) -> Vec<(String, &'static str)> {
    let colors = [
        ("bg", theme.bg, "Background"),
        ("surface", theme.surface, "Selected rows"),
        ("panel", theme.panel, "Bars, code blocks and popups"),
        ("border", theme.border, "Borders"),
        ("fg", theme.fg, "Text"),
        ("muted", theme.muted, "Secondary text"),
        ("accent", theme.accent, "Titles, selection, mentions of you"),
        ("on_accent", theme.on_accent, "Text on the accent color"),
        ("unread", theme.unread, "Unread counts and errors"),
        ("link", theme.link, "Links, channels and mentions"),
        ("success", theme.success, "Connected"),
    ];
    let mut options: Vec<(String, &'static str)> = colors
        .into_iter()
        .map(|(name, color, help)| (format!("{name} = {}", quote(&hex(color))), help))
        .collect();
    let nicks: Vec<String> = theme.nicks.iter().map(|c| quote(&hex(*c))).collect();
    options.push((
        format!("nicks = [{}]", nicks.join(", ")),
        "Names, one color per person",
    ));
    options
}

/// A setting with its default value and the values it takes.
fn option<T: Choice + Default + Serialize>(out: &mut String, key: &str) {
    let values: Vec<String> = T::ALL.iter().map(|value| name(*value)).collect();
    line(
        out,
        &format!("{key} = {}", quote(&name(T::default()))),
        &values.join(" | "),
    );
}

/// A commented-out option, with a comment lined up after it.
fn line(out: &mut String, option: &str, comment: &str) {
    out.push_str(&format!("# {option:<34} # {comment}\n"));
}

/// How a setting's value is written in the file: `panels`, `12h`.
fn name<T: Serialize>(value: T) -> String {
    match toml::Value::try_from(value) {
        Ok(toml::Value::String(name)) => name,
        _ => unreachable!("settings are written as strings"),
    }
}

fn quote(text: &str) -> String {
    toml::Value::String(text.to_string()).to_string()
}

fn hex(color: Color) -> String {
    match color {
        Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tui::keymap::Keymap;

    /// `# layout = "panels"  # …` becomes `layout = "panels"  # …`.
    fn uncomment(text: &str) -> String {
        text.lines()
            .map(|line| match line.strip_prefix("# ") {
                Some(rest)
                    if rest.split_once(" = ").is_some_and(|(key, _)| {
                        !key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                    }) =>
                {
                    rest
                }
                _ => line,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn a_new_theme_file_gives_back_its_starting_theme() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mine.toml");
        std::fs::write(&path, theme_file("mine", ThemeName::Gruvbox, &[])).unwrap();
        let mine = crate::themes::read(&path).unwrap();
        assert_eq!(mine.base, ThemeName::Gruvbox);

        let theme = |name| {
            let settings = Settings {
                theme: name,
                ..Settings::default()
            };
            Theme::new(&settings, std::slice::from_ref(&mine), &Colors::default())
        };
        assert_eq!(theme(ThemeName::named("mine")), theme(ThemeName::Gruvbox));
    }

    #[test]
    fn as_written_it_changes_nothing() {
        assert_eq!(
            Config::from_toml(&default_config()).unwrap(),
            Config::default()
        );
    }

    #[test]
    fn uncommented_it_holds_the_defaults() {
        let config = Config::from_toml(&uncomment(&default_config())).unwrap();

        assert_eq!(config.settings, Settings::default());
        let (keymap, problems) = Keymap::new(&config.keys);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(keymap, Keymap::default());
        assert_eq!(config.keys.len(), ACTIONS.len());
        let theme = Theme::new(&config.settings, &[], &config.colors);
        assert_eq!(theme, Theme::from_settings(&Settings::default()));
    }
}

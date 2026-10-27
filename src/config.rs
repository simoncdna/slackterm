use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use ratatui::style::Color;
use serde::{Deserialize, Deserializer, Serialize};
use toml_edit::{DocumentMut, Item};

const FILE: &str = "config.toml";

/// A setting with a fixed list of values the settings screen cycles through.
pub trait Choice: Copy + PartialEq + 'static {
    const ALL: &'static [Self];
    fn label(self) -> &'static str;

    fn cycle(self, step: isize) -> Self {
        let len = Self::ALL.len() as isize;
        let index = Self::ALL.iter().position(|v| *v == self).unwrap_or(0) as isize;
        Self::ALL[(index + step).rem_euclid(len) as usize]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    #[default]
    Panels,
    Stream,
    Focus,
}

impl Choice for Layout {
    const ALL: &'static [Self] = &[Self::Panels, Self::Stream, Self::Focus];
    fn label(self) -> &'static str {
        match self {
            Self::Panels => "A · three panels",
            Self::Stream => "B · dense stream (IRC)",
            Self::Focus => "C · focus + ^k",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    // The aliases keep reading config files written by the French versions.
    #[default]
    #[serde(alias = "nuit")]
    Night,
    #[serde(alias = "clair")]
    Light,
    Gruvbox,
    Nord,
}

impl Choice for ThemeName {
    const ALL: &'static [Self] = &[Self::Night, Self::Light, Self::Gruvbox, Self::Nord];
    fn label(self) -> &'static str {
        match self {
            Self::Night => "Night",
            Self::Light => "Light",
            Self::Gruvbox => "Gruvbox",
            Self::Nord => "Nord",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    /// The theme's own accent color.
    #[default]
    Theme,
    #[serde(alias = "ambre")]
    Amber,
    #[serde(alias = "bleu")]
    Blue,
    #[serde(alias = "lilas")]
    Lilac,
    #[serde(alias = "vert")]
    Green,
}

impl Choice for Accent {
    const ALL: &'static [Self] = &[
        Self::Theme,
        Self::Amber,
        Self::Blue,
        Self::Lilac,
        Self::Green,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Theme => "the theme's own",
            Self::Amber => "Amber",
            Self::Blue => "Blue",
            Self::Lilac => "Lilac",
            Self::Green => "Green",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AvatarStyle {
    /// Profile pictures, when the terminal can draw images.
    #[default]
    Images,
    Initials,
    Off,
}

impl Choice for AvatarStyle {
    const ALL: &'static [Self] = &[Self::Images, Self::Initials, Self::Off];
    fn label(self) -> &'static str {
        match self {
            Self::Images => "pictures",
            Self::Initials => "initials",
            Self::Off => "hidden",
        }
    }
}

/// How times of day are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Clock {
    /// `14:02`
    #[default]
    #[serde(rename = "24h")]
    TwentyFourHour,
    /// `2:02 PM`
    #[serde(rename = "12h")]
    TwelveHour,
}

impl Choice for Clock {
    const ALL: &'static [Self] = &[Self::TwentyFourHour, Self::TwelveHour];
    fn label(self) -> &'static str {
        match self {
            Self::TwentyFourHour => "24-hour",
            Self::TwelveHour => "12-hour",
        }
    }
}

/// Whether slackterm takes the mouse: the wheel scrolls and clicks open.
/// Off, the terminal selects text as usual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mouse {
    #[default]
    On,
    Off,
}

impl Choice for Mouse {
    const ALL: &'static [Self] = &[Self::On, Self::Off];
    fn label(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off: the terminal selects text",
        }
    }
}

/// Whether direct messages, mentions and replies in the user's threads are
/// announced on the desktop while they look elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Notifications {
    #[default]
    On,
    Off,
}

impl Choice for Notifications {
    const ALL: &'static [Self] = &[Self::On, Self::Off];
    fn label(self) -> &'static str {
        match self {
            Self::On => "direct messages, mentions, threads",
            Self::Off => "off",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub layout: Layout,
    #[serde(default)]
    pub theme: ThemeName,
    #[serde(default)]
    pub accent: Accent,
    #[serde(default)]
    pub avatars: AvatarStyle,
    #[serde(default)]
    pub clock: Clock,
    #[serde(default)]
    pub mouse: Mouse,
    #[serde(default)]
    pub notifications: Notifications,
}

impl Settings {
    /// Writes the settings into the config file, keeping everything else in
    /// it: comments, keys and colors.
    pub fn save(&self, path: &Path) -> Result<()> {
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e).with_context(|| format!("could not read {}", path.display())),
        };
        let updated = self
            .write_into(&contents)
            .with_context(|| format!("{} is invalid", path.display()))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, updated).with_context(|| format!("could not write {}", path.display()))
    }

    fn write_into(&self, contents: &str) -> Result<String> {
        let mut document: DocumentMut = contents.parse()?;
        let settings: DocumentMut = toml::to_string(self)?.parse()?;
        for (key, item) in settings.iter() {
            let Some(value) = item.as_value() else {
                continue;
            };
            match document.get_mut(key).and_then(Item::as_value_mut) {
                // Keep the comment written after the old value.
                Some(old) => {
                    let decor = old.decor().clone();
                    *old = value.clone();
                    *old.decor_mut() = decor;
                }
                None => {
                    document.insert(key, Item::Value(value.clone()));
                }
            }
        }
        Ok(document.to_string())
    }
}

/// Everything the config file holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    /// What the settings screen changes.
    pub settings: Settings,
    /// Keys by action name, as written: the interface checks them.
    pub keys: BTreeMap<String, KeyList>,
    pub colors: Colors,
}

/// The file as written. Unknown keys are refused, so that a typo does not
/// go unnoticed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    layout: Layout,
    #[serde(default)]
    theme: ThemeName,
    #[serde(default)]
    accent: Accent,
    #[serde(default)]
    avatars: AvatarStyle,
    #[serde(default)]
    clock: Clock,
    #[serde(default)]
    mouse: Mouse,
    #[serde(default)]
    notifications: Notifications,
    #[serde(default)]
    keys: BTreeMap<String, KeyList>,
    #[serde(default)]
    colors: Colors,
}

impl Config {
    /// Missing file means defaults; a file that cannot be read is an error.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                Self::from_toml(&contents).with_context(|| format!("{} is invalid", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
        }
    }

    pub fn from_toml(contents: &str) -> Result<Self> {
        let file: File = toml::from_str(contents)?;
        Ok(Self {
            settings: Settings {
                layout: file.layout,
                theme: file.theme,
                accent: file.accent,
                avatars: file.avatars,
                clock: file.clock,
                mouse: file.mouse,
                notifications: file.notifications,
            },
            keys: file.keys,
            colors: file.colors,
        })
    }
}

/// Where the config file is: the one given with `--config`, else in
/// `$XDG_CONFIG_HOME`, else in `~/.config` on macOS if there is one there,
/// else in the system's config directory.
pub fn config_path(chosen: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = chosen {
        return Ok(path);
    }
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir).join("slackterm").join(FILE));
    }
    if cfg!(target_os = "macos")
        && let Some(home) = std::env::var_os("HOME")
    {
        let path = PathBuf::from(home).join(".config/slackterm").join(FILE);
        if path.exists() {
            return Ok(path);
        }
    }
    let dirs = directories::ProjectDirs::from("", "", "slackterm")
        .context("could not find the config directory")?;
    Ok(dirs.config_dir().join(FILE))
}

/// The keys of an action: one key, or a list.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(from = "OneOrMany")]
pub struct KeyList(pub Vec<String>);

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl From<OneOrMany> for KeyList {
    fn from(keys: OneOrMany) -> Self {
        match keys {
            OneOrMany::One(key) => Self(vec![key]),
            OneOrMany::Many(keys) => Self(keys),
        }
    }
}

/// Colors that replace the theme's own.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Colors {
    pub bg: Option<ColorValue>,
    pub surface: Option<ColorValue>,
    pub panel: Option<ColorValue>,
    pub border: Option<ColorValue>,
    pub fg: Option<ColorValue>,
    pub muted: Option<ColorValue>,
    pub accent: Option<ColorValue>,
    pub on_accent: Option<ColorValue>,
    pub unread: Option<ColorValue>,
    pub link: Option<ColorValue>,
    pub success: Option<ColorValue>,
    pub nicks: Option<[ColorValue; 5]>,
}

/// `#rrggbb`, an ANSI color name such as `red`, a palette index such as
/// `42`, or `reset` for the terminal's own color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorValue(pub Color);

impl<'de> Deserialize<'de> for ColorValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Color::from_str(&text).map(Self).map_err(|_| {
            serde::de::Error::custom(format!(
                "unknown color `{text}`: use \"#rrggbb\", a name such as \"red\", \
                 a palette index or \"reset\""
            ))
        })
    }
}

/// The two lists the sidebar switches between.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarTab {
    /// The user's sections, as in Slack's home.
    #[default]
    Channels,
    /// Direct and group messages, most recent first.
    Direct,
}

/// What the user changed in the sidebar, kept between launches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarState {
    #[serde(default)]
    pub collapsed: BTreeSet<String>,
    #[serde(default)]
    pub tab: SidebarTab,
}

/// What the sidebar remembers, for each workspace, and which workspace was
/// open last. Kept in `sidebar.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarFile {
    /// Team id of the workspace open last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// By team id.
    #[serde(default)]
    pub teams: BTreeMap<String, SidebarState>,
    /// The one state of the versions before several workspaces, for the
    /// workspaces not saved since.
    #[serde(skip)]
    earlier: Option<SidebarState>,
}

impl SidebarFile {
    fn path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "slackterm")
            .context("could not find the data directory")?;
        Ok(dirs.data_dir().join("sidebar.toml"))
    }

    /// Falls back to an empty state: losing it only re-expands sections.
    pub fn load() -> Self {
        Self::path()
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|contents| Self::from_toml(&contents))
            .unwrap_or_default()
    }

    fn from_toml(contents: &str) -> Self {
        match toml::from_str::<Self>(contents) {
            Ok(file) if file.workspace.is_some() || !file.teams.is_empty() => file,
            _ => Self {
                earlier: toml::from_str(contents).ok(),
                ..Self::default()
            },
        }
    }

    pub fn state(&self, team: &str) -> SidebarState {
        self.teams
            .get(team)
            .or(self.earlier.as_ref())
            .cloned()
            .unwrap_or_default()
    }

    /// Saves one workspace's state, keeping the others'.
    pub fn save_state(team: &str, state: SidebarState) -> Result<()> {
        let mut file = Self::load();
        file.teams.insert(team.to_string(), state);
        file.save()
    }

    pub fn save_workspace(team: &str) -> Result<()> {
        let mut file = Self::load();
        file.workspace = Some(team.to_string());
        file.save()
    }

    fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, toml::to_string(self)?)
            .with_context(|| format!("could not write {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let settings = Settings {
            layout: Layout::Stream,
            theme: ThemeName::Gruvbox,
            accent: Accent::Lilac,
            avatars: AvatarStyle::Initials,
            clock: Clock::TwelveHour,
            mouse: Mouse::Off,
            notifications: Notifications::Off,
        };
        let toml = settings.write_into("").unwrap();
        assert!(toml.contains("layout = \"stream\""));
        assert!(toml.contains("clock = \"12h\""));
        assert_eq!(Config::from_toml(&toml).unwrap().settings, settings);
    }

    #[test]
    fn missing_keys_fall_back_to_defaults() {
        let config = Config::from_toml("theme = \"nord\"").unwrap();
        assert_eq!(config.settings.theme, ThemeName::Nord);
        assert_eq!(config.settings.layout, Layout::Panels);
    }

    #[test]
    fn reads_the_french_names_of_earlier_versions() {
        let config = Config::from_toml("theme = \"clair\"\naccent = \"vert\"").unwrap();
        assert_eq!(config.settings.theme, ThemeName::Light);
        assert_eq!(config.settings.accent, Accent::Green);
    }

    #[test]
    fn refuses_unknown_options_with_their_line() {
        let error = Config::from_toml("theme = \"nord\"\nlyout = \"stream\"").unwrap_err();
        let message = format!("{error:#}");
        assert!(
            message.contains("lyout") && message.contains("line 2"),
            "{message}"
        );
        assert!(Config::from_toml("[colors]\nbackground = \"red\"").is_err());
    }

    #[test]
    fn reads_keys_and_colors() {
        let config = Config::from_toml(
            "[keys]\nreact = \"e\"\nsearch = [\"/\", \"ctrl+s\"]\n\
             [colors]\naccent = \"#88C0D0\"\nbg = \"reset\"\nunread = \"red\"",
        )
        .unwrap();
        assert_eq!(config.keys["react"], KeyList(vec!["e".into()]));
        assert_eq!(config.keys["search"].0.len(), 2);
        assert_eq!(
            config.colors.accent,
            Some(ColorValue(Color::Rgb(0x88, 0xC0, 0xD0)))
        );
        assert_eq!(config.colors.bg, Some(ColorValue(Color::Reset)));
        assert_eq!(config.colors.unread, Some(ColorValue(Color::Red)));
        let error = Config::from_toml("[colors]\nfg = \"blu\"").unwrap_err();
        assert!(format!("{error:#}").contains("unknown color `blu`"));
    }

    #[test]
    fn saving_settings_keeps_the_rest_of_the_file() {
        let written = "# My setup\ntheme = \"nord\" # cold\n\n[keys]\nreact = \"e\"\n";
        let settings = Settings {
            theme: ThemeName::Gruvbox,
            ..Settings::default()
        };

        let saved = settings.write_into(written).unwrap();

        assert!(
            saved.starts_with("# My setup\ntheme = \"gruvbox\" # cold\n"),
            "{saved}"
        );
        assert!(saved.contains("[keys]\nreact = \"e\"\n"), "{saved}");
        let config = Config::from_toml(&saved).unwrap();
        assert_eq!(config.settings, settings);
        assert_eq!(config.keys["react"], KeyList(vec!["e".into()]));
        assert!(settings.write_into("theme = ").is_err());
    }

    #[test]
    fn sidebar_state_round_trips_through_toml() {
        let state = SidebarState {
            collapsed: ["S1".to_string()].into(),
            tab: SidebarTab::Direct,
        };
        let toml = toml::to_string(&state).unwrap();
        assert_eq!(toml::from_str::<SidebarState>(&toml).unwrap(), state);
        assert_eq!(
            toml::from_str::<SidebarState>("").unwrap(),
            SidebarState::default()
        );
    }

    #[test]
    fn the_sidebar_file_keeps_each_workspace() {
        let mut file = SidebarFile::default();
        let state = SidebarState {
            collapsed: ["S1".to_string()].into(),
            tab: SidebarTab::Direct,
        };
        file.teams.insert("T1".into(), state.clone());
        file.workspace = Some("T1".into());

        let read = SidebarFile::from_toml(&toml::to_string(&file).unwrap());

        assert_eq!(read.workspace.as_deref(), Some("T1"));
        assert_eq!(read.state("T1"), state);
        assert_eq!(read.state("T2"), SidebarState::default());
    }

    #[test]
    fn the_sidebar_file_of_one_workspace_still_reads() {
        let read = SidebarFile::from_toml("collapsed = [\"S1\"]\ntab = \"direct\"");
        assert_eq!(read.workspace, None);
        assert_eq!(read.state("T1").tab, SidebarTab::Direct);
        assert!(read.state("T9").collapsed.contains("S1"));
    }

    #[test]
    fn choices_cycle_in_both_directions() {
        assert_eq!(Layout::Panels.cycle(1), Layout::Stream);
        assert_eq!(Layout::Panels.cycle(-1), Layout::Focus);
        assert_eq!(Layout::Focus.cycle(1), Layout::Panels);
    }
}

use std::collections::BTreeSet;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

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
}

impl Settings {
    pub fn path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "slackterm")
            .context("could not find the config directory")?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// Missing file means defaults; a file that cannot be read is an error.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        match std::fs::read_to_string(&path) {
            Ok(contents) => {
                Self::from_toml(&contents).with_context(|| format!("{} is invalid", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, self.to_toml()?)
            .with_context(|| format!("could not write {}", path.display()))
    }

    fn from_toml(contents: &str) -> Result<Self> {
        Ok(toml::from_str(contents)?)
    }

    fn to_toml(self) -> Result<String> {
        Ok(toml::to_string(&self)?)
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

impl SidebarState {
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
            .and_then(|contents| toml::from_str(&contents).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
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
        };
        let toml = settings.to_toml().unwrap();
        assert!(toml.contains("layout = \"stream\""));
        assert!(toml.contains("clock = \"12h\""));
        assert_eq!(Settings::from_toml(&toml).unwrap(), settings);
    }

    #[test]
    fn missing_keys_fall_back_to_defaults() {
        let settings = Settings::from_toml("theme = \"nord\"").unwrap();
        assert_eq!(settings.theme, ThemeName::Nord);
        assert_eq!(settings.layout, Layout::Panels);
    }

    #[test]
    fn reads_the_french_names_of_earlier_versions() {
        let settings = Settings::from_toml("theme = \"clair\"\naccent = \"vert\"").unwrap();
        assert_eq!(settings.theme, ThemeName::Light);
        assert_eq!(settings.accent, Accent::Green);
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
    fn choices_cycle_in_both_directions() {
        assert_eq!(Layout::Panels.cycle(1), Layout::Stream);
        assert_eq!(Layout::Panels.cycle(-1), Layout::Focus);
        assert_eq!(Layout::Focus.cycle(1), Layout::Panels);
    }
}

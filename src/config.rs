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
            Self::Panels => "A · trois panneaux",
            Self::Stream => "B · flux dense (IRC)",
            Self::Focus => "C · focus + ^k",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    #[default]
    Nuit,
    Clair,
    Gruvbox,
    Nord,
}

impl Choice for ThemeName {
    const ALL: &'static [Self] = &[Self::Nuit, Self::Clair, Self::Gruvbox, Self::Nord];
    fn label(self) -> &'static str {
        match self {
            Self::Nuit => "Nuit",
            Self::Clair => "Clair",
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
    Ambre,
    Bleu,
    Lilas,
    Vert,
}

impl Choice for Accent {
    const ALL: &'static [Self] = &[
        Self::Theme,
        Self::Ambre,
        Self::Bleu,
        Self::Lilas,
        Self::Vert,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Theme => "celui du thème",
            Self::Ambre => "Ambre",
            Self::Bleu => "Bleu",
            Self::Lilas => "Lilas",
            Self::Vert => "Vert",
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
}

impl Settings {
    pub fn path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "slackterm")
            .context("impossible de déterminer le dossier de configuration")?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// Missing file means defaults; a file that cannot be read is an error.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        match std::fs::read_to_string(&path) {
            Ok(contents) => Self::from_toml(&contents)
                .with_context(|| format!("{} est invalide", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("impossible de lire {}", path.display())),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, self.to_toml()?)
            .with_context(|| format!("impossible d'écrire {}", path.display()))
    }

    fn from_toml(contents: &str) -> Result<Self> {
        Ok(toml::from_str(contents)?)
    }

    fn to_toml(self) -> Result<String> {
        Ok(toml::to_string(&self)?)
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
            accent: Accent::Lilas,
        };
        let toml = settings.to_toml().unwrap();
        assert!(toml.contains("layout = \"stream\""));
        assert_eq!(Settings::from_toml(&toml).unwrap(), settings);
    }

    #[test]
    fn missing_keys_fall_back_to_defaults() {
        let settings = Settings::from_toml("theme = \"nord\"").unwrap();
        assert_eq!(settings.theme, ThemeName::Nord);
        assert_eq!(settings.layout, Layout::Panels);
    }

    #[test]
    fn choices_cycle_in_both_directions() {
        assert_eq!(Layout::Panels.cycle(1), Layout::Stream);
        assert_eq!(Layout::Panels.cycle(-1), Layout::Focus);
        assert_eq!(Layout::Focus.cycle(1), Layout::Panels);
    }
}

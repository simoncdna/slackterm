//! The user's themes: TOML files in the `themes` folder next to the config
//! file, each named after its file. A theme gives the colors that differ
//! from a built-in theme, its `base`, with the names of `[colors]`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::config::{Choice, ColorValue, Colors, ThemeName};

/// A theme of the user's, as its file has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserTheme {
    /// Always [`ThemeName::Custom`].
    pub name: ThemeName,
    /// The built-in theme the colors not given come from.
    pub base: ThemeName,
    pub colors: Colors,
}

/// The folder of the user's themes, next to the config file.
pub fn themes_dir(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map_or_else(|| PathBuf::from("themes"), |dir| dir.join("themes"))
}

/// The themes in `dir`, sorted by name, with what is wrong with the files
/// that could not be read. No folder means no themes.
pub fn load(dir: &Path) -> (Vec<UserTheme>, Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (Vec::new(), Vec::new());
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    let mut themes = Vec::new();
    let mut problems = Vec::new();
    for path in paths {
        match read(&path) {
            Ok(theme) => themes.push(theme),
            Err(e) => problems.push(format!("{e:#}")),
        }
    }
    (themes, problems)
}

/// Reads one theme file.
pub fn read(path: &Path) -> Result<UserTheme> {
    let file = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .with_context(|| format!("{file}: name the file with letters"))?;
    let name = ThemeName::named(stem);
    if !matches!(name, ThemeName::Custom(_)) {
        bail!(
            "{file}: {} is a built-in theme, name yours otherwise",
            name.label()
        );
    }
    let text = std::fs::read_to_string(path).with_context(|| format!("could not read {file}"))?;
    let theme: File = toml::from_str(&text).map_err(|e| {
        // One line, for the status bar.
        let line = e
            .span()
            .map_or(1, |span| text[..span.start].lines().count().max(1));
        anyhow::anyhow!("{file}, line {line}: {}", e.message().trim())
    })?;
    let base = match theme.base {
        None => ThemeName::Night,
        Some(ThemeName::Custom(other)) => {
            bail!("{file}: `base` is night, light, gruvbox or nord, not {other}")
        }
        Some(base) => base,
    };
    Ok(UserTheme {
        name,
        base,
        colors: theme.colors(),
    })
}

/// A theme file: `[colors]`, with `base` on top.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    base: Option<ThemeName>,
    bg: Option<ColorValue>,
    surface: Option<ColorValue>,
    panel: Option<ColorValue>,
    border: Option<ColorValue>,
    fg: Option<ColorValue>,
    muted: Option<ColorValue>,
    accent: Option<ColorValue>,
    on_accent: Option<ColorValue>,
    unread: Option<ColorValue>,
    link: Option<ColorValue>,
    success: Option<ColorValue>,
    nicks: Option<[ColorValue; 5]>,
}

impl File {
    fn colors(&self) -> Colors {
        Colors {
            bg: self.bg,
            surface: self.surface,
            panel: self.panel,
            border: self.border,
            fg: self.fg,
            muted: self.muted,
            accent: self.accent,
            on_accent: self.on_accent,
            unread: self.unread,
            link: self.link,
            success: self.success,
            nicks: self.nicks,
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;

    fn write(dir: &Path, file: &str, contents: &str) {
        std::fs::write(dir.join(file), contents).unwrap();
    }

    #[test]
    fn reads_the_themes_of_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "mocha.toml",
            "base = \"light\"\nbg = \"#1e1e2e\"\naccent = \"red\"\n",
        );
        write(dir.path(), "plain.toml", "");
        write(dir.path(), "notes.txt", "not a theme");

        let (themes, problems) = load(dir.path());
        assert_eq!(problems, Vec::<String>::new());
        let names: Vec<&str> = themes.iter().map(|t| t.name.key()).collect();
        assert_eq!(names, ["mocha", "plain"]);
        assert_eq!(themes[0].base, ThemeName::Light);
        assert_eq!(
            themes[0].colors.bg,
            Some(ColorValue(Color::Rgb(0x1e, 0x1e, 0x2e)))
        );
        assert_eq!(themes[0].colors.accent, Some(ColorValue(Color::Red)));
        assert_eq!(themes[1].base, ThemeName::Night);
    }

    #[test]
    fn says_what_is_wrong_with_a_theme() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "typo.toml", "bgg = \"#000000\"\n");
        write(dir.path(), "color.toml", "fg = \"blurple\"\n");
        write(dir.path(), "base.toml", "base = \"typo\"\n");
        write(dir.path(), "nord.toml", "fg = \"red\"\n");
        write(dir.path(), "fine.toml", "fg = \"red\"\n");

        let (themes, problems) = load(dir.path());
        assert_eq!(themes.len(), 1);
        let problems = problems.join("\n");
        assert!(
            problems.contains("base.toml: `base` is night"),
            "{problems}"
        );
        assert!(problems.contains("unknown color `blurple`"), "{problems}");
        assert!(
            problems.contains("nord.toml: Nord is a built-in theme"),
            "{problems}"
        );
        assert!(problems.contains("bgg"), "{problems}");
        assert!(problems.contains("line 1"), "{problems}");
    }

    #[test]
    fn no_folder_no_themes() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(&dir.path().join("themes")), (Vec::new(), Vec::new()));
    }
}

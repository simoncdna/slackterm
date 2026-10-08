use ratatui::style::Color;

use crate::config::{Accent, Settings, ThemeName};

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    /// Selected rows.
    pub surface: Color,
    /// Bars, code blocks and popups.
    pub panel: Color,
    pub border: Color,
    pub fg: Color,
    pub muted: Color,
    pub accent: Color,
    /// Text drawn on top of `accent`.
    pub on_accent: Color,
    pub unread: Color,
    pub link: Color,
    pub success: Color,
    pub nicks: [Color; 5],
}

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const NUIT: Theme = Theme {
    bg: rgb(0x141318),
    surface: rgb(0x2A2633),
    panel: rgb(0x1C1A22),
    border: rgb(0x3A3644),
    fg: rgb(0xE6E1EA),
    muted: rgb(0x8E8799),
    accent: rgb(0xF0B35A),
    on_accent: rgb(0x141318),
    unread: rgb(0xEE8E7A),
    link: rgb(0x8FB8F0),
    success: rgb(0x7CCB8A),
    nicks: [
        rgb(0x8FB8F0),
        rgb(0xC59BEA),
        rgb(0x7CCB8A),
        rgb(0xEE8E7A),
        rgb(0x6FC7C1),
    ],
};

const CLAIR: Theme = Theme {
    bg: rgb(0xFAF8F5),
    surface: rgb(0xE9E4EE),
    panel: rgb(0xF0ECE6),
    border: rgb(0xD3CDD8),
    fg: rgb(0x23202A),
    muted: rgb(0x625B6C),
    accent: rgb(0xA65A12),
    on_accent: rgb(0xFFFFFF),
    unread: rgb(0xB23A1E),
    link: rgb(0x2F5FB3),
    success: rgb(0x2E7D4F),
    nicks: [
        rgb(0x2F5FB3),
        rgb(0x7B4FB0),
        rgb(0x2E7D4F),
        rgb(0xB5452F),
        rgb(0x1F7A74),
    ],
};

const GRUVBOX: Theme = Theme {
    bg: rgb(0x282828),
    surface: rgb(0x3C3836),
    panel: rgb(0x32302F),
    border: rgb(0x504945),
    fg: rgb(0xEBDBB2),
    muted: rgb(0xA89984),
    accent: rgb(0xFABD2F),
    on_accent: rgb(0x282828),
    unread: rgb(0xFB4934),
    link: rgb(0x83A598),
    success: rgb(0xB8BB26),
    nicks: [
        rgb(0x83A598),
        rgb(0xD3869B),
        rgb(0xB8BB26),
        rgb(0xFE8019),
        rgb(0x8EC07C),
    ],
};

const NORD: Theme = Theme {
    bg: rgb(0x2E3440),
    surface: rgb(0x434C5E),
    panel: rgb(0x3B4252),
    border: rgb(0x4C566A),
    fg: rgb(0xECEFF4),
    muted: rgb(0xA3ACBD),
    accent: rgb(0x88C0D0),
    on_accent: rgb(0x2E3440),
    unread: rgb(0xD08770),
    link: rgb(0x81A1C1),
    success: rgb(0xA3BE8C),
    nicks: [
        rgb(0x81A1C1),
        rgb(0xB48EAD),
        rgb(0xA3BE8C),
        rgb(0xD08770),
        rgb(0x8FBCBB),
    ],
};

impl Theme {
    pub fn from_settings(settings: &Settings) -> Self {
        let mut theme = match settings.theme {
            ThemeName::Nuit => NUIT,
            ThemeName::Clair => CLAIR,
            ThemeName::Gruvbox => GRUVBOX,
            ThemeName::Nord => NORD,
        };
        let light = settings.theme == ThemeName::Clair;
        // Light backgrounds need darker accents to keep text readable.
        let accent = match (settings.accent, light) {
            (Accent::Theme, _) => None,
            (Accent::Ambre, false) => Some(0xF0B35A),
            (Accent::Ambre, true) => Some(0xA65A12),
            (Accent::Bleu, false) => Some(0x8FB8F0),
            (Accent::Bleu, true) => Some(0x2F5FB3),
            (Accent::Lilas, false) => Some(0xC59BEA),
            (Accent::Lilas, true) => Some(0x7B4FB0),
            (Accent::Vert, false) => Some(0x7CCB8A),
            (Accent::Vert, true) => Some(0x2E7D4F),
        };
        if let Some(accent) = accent {
            theme.accent = rgb(accent);
        }
        theme
    }

    /// A stable color per user, so a name keeps its color across messages.
    pub fn nick(&self, user_key: &str) -> Color {
        let hash = user_key
            .bytes()
            .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
        self.nicks[hash as usize % self.nicks.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_override_replaces_the_theme_accent() {
        let settings = Settings {
            accent: Accent::Bleu,
            ..Settings::default()
        };
        assert_eq!(Theme::from_settings(&settings).accent, rgb(0x8FB8F0));
    }

    #[test]
    fn nick_colors_are_stable() {
        let theme = NUIT;
        assert_eq!(theme.nick("U123"), theme.nick("U123"));
    }
}

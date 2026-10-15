//! Pictures (profile pictures, previews of videos and images) drawn with
//! the terminal's graphics protocol (kitty, iTerm2 or sixel), through tmux
//! when needed.

use std::collections::HashMap;

use image::DynamicImage;
use image::imageops::FilterType;
use ratatui::Frame;
use ratatui::layout::{Rect, Size};
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::{Image, Resize};

/// Cells a picture covers: about square, as cells are twice as tall as wide.
pub const AVATAR_SIZE: Size = Size {
    width: 4,
    height: 2,
};

/// ratatui-image recognises tmux only from `TERM` or `TERM_PROGRAM`, which
/// tmux configurations often replace (`default-terminal "${TERM}"`,
/// `update-environment "TERM_PROGRAM"`). Unrecognised, it neither wraps its
/// sequences for tmux, which then drops them, nor queries the real terminal.
fn hides_tmux(tmux: Option<&str>, term: Option<&str>, term_program: Option<&str>) -> bool {
    tmux.is_some_and(|t| !t.is_empty())
        && !term.is_some_and(|t| t.starts_with("tmux"))
        && term_program != Some("tmux")
}

/// Lets ratatui-image recognise tmux. Must run before any other thread
/// exists, since it changes the environment.
pub fn reveal_tmux() {
    let var = |name| std::env::var(name).ok();
    let (tmux, term, term_program) = (var("TMUX"), var("TERM"), var("TERM_PROGRAM"));
    if hides_tmux(tmux.as_deref(), term.as_deref(), term_program.as_deref()) {
        // SAFETY: called at the start of `main`, while no other thread runs.
        unsafe { std::env::set_var("TERM_PROGRAM", "tmux") };
    }
}

pub struct Pictures {
    /// `None` when the terminal cannot draw images.
    picker: Option<Picker>,
    /// Encoded pictures, by key and size in cells.
    protocols: HashMap<(String, Size), Protocol>,
}

impl Pictures {
    /// Asks the terminal which graphics protocol it speaks. Must run once
    /// the terminal is in raw mode, before input events are read.
    pub fn detect() -> Self {
        // Half blocks would draw a 4×4-pixel blur: initials read better.
        let picker = Picker::from_query_stdio()
            .ok()
            .filter(|picker| picker.protocol_type() != ProtocolType::Halfblocks);
        Self {
            picker,
            protocols: HashMap::new(),
        }
    }

    #[cfg(test)]
    pub fn disabled() -> Self {
        Self {
            picker: None,
            protocols: HashMap::new(),
        }
    }

    /// How pictures are drawn, for the settings: `kitty via tmux`…
    pub fn describe(&self) -> String {
        match &self.picker {
            None => "terminal sans images".to_string(),
            Some(picker) => {
                let protocol = format!("{:?}", picker.protocol_type()).to_lowercase();
                if picker.tmux_detected() {
                    format!("{protocol} via tmux")
                } else {
                    protocol
                }
            }
        }
    }

    /// Drops the pictures already encoded, so they are sent to the terminal
    /// again, after another program had the screen.
    pub fn forget(&mut self) {
        self.protocols.clear();
    }

    pub fn available(&self) -> bool {
        self.picker.is_some()
    }

    /// Draws the picture stored under `key` over `area`, once downloaded.
    /// Returns whether it did, what is underneath staying visible otherwise.
    pub fn draw(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        key: &str,
        images: &HashMap<String, DynamicImage>,
    ) -> bool {
        let Some(picker) = &self.picker else {
            return false;
        };
        let size = Size::new(area.width, area.height);
        let slot = (key.to_string(), size);
        if !self.protocols.contains_key(&slot) {
            let Some(image) = images.get(key) else {
                return false;
            };
            let resize = Resize::Scale(Some(FilterType::Triangle));
            let Ok(protocol) = picker.new_protocol(image.clone(), size, resize) else {
                return false;
            };
            self.protocols.insert(slot.clone(), protocol);
        }
        frame.render_widget(Image::new(&self.protocols[&slot]), area);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_tmux_hidden_by_the_configuration() {
        let tmux = Some("/private/tmp/tmux-501/default,2594,12");
        assert!(hides_tmux(tmux, Some("xterm-ghostty"), Some("WezTerm")));
        assert!(hides_tmux(tmux, Some("xterm-ghostty"), None));
        assert!(!hides_tmux(tmux, Some("tmux-256color"), Some("ghostty")));
        assert!(!hides_tmux(tmux, Some("xterm-ghostty"), Some("tmux")));
        assert!(!hides_tmux(None, Some("xterm-ghostty"), Some("ghostty")));
    }
}

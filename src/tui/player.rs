//! Playing videos inside the terminal with mpv.

/// How mpv draws the video: kitty images where the terminal speaks the
/// protocol directly, colored text cells otherwise (mpv's kitty output does
/// not go through tmux).
pub fn video_output(in_tmux: bool, term: &str) -> &'static str {
    let kitty = term.contains("kitty") || term.contains("ghostty");
    if kitty && !in_tmux { "kitty" } else { "tct" }
}

/// mpv's arguments for a file path or a web link. Links go through yt-dlp,
/// limited to 480p since a terminal shows little more.
pub fn mpv_args(target: &str, output: &str) -> Vec<String> {
    vec![
        format!("--vo={output}"),
        "--really-quiet".into(),
        "--ytdl-format=bestvideo[height<=?480]+bestaudio/best".into(),
        target.into(),
    ]
}

pub fn is_link(target: &str) -> bool {
    target.starts_with("https://") || target.starts_with("http://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_text_cells_inside_tmux() {
        assert_eq!(video_output(false, "xterm-ghostty"), "kitty");
        assert_eq!(video_output(true, "xterm-ghostty"), "tct");
        assert_eq!(video_output(false, "xterm-256color"), "tct");
    }

    #[test]
    fn passes_the_target_last() {
        let args = mpv_args("https://youtu.be/dQw4w9WgXcQ", "tct");
        assert_eq!(args[0], "--vo=tct");
        assert_eq!(args.last().unwrap(), "https://youtu.be/dQw4w9WgXcQ");
        assert!(is_link(&args[3]));
        assert!(!is_link("/cache/F1-demo.mp4"));
    }
}

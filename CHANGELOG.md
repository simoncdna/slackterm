# Changelog

What changes from one version of slackterm to the next. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Several workspaces at once: `w` or Ctrl+K switches between them, the status bar tells what waits in the others, and slackterm opens the last one you used. The others stay connected, and what arrives there stays unread until you look.
- The demo has a second workspace to switch to.
- `n` opens the next conversation with unread messages, in any workspace.
- `y` copies a message's text and `Y` its link, through the terminal over SSH.
- The mouse: the wheel scrolls, a click opens, selects or starts writing. `mouse = "off"` gives text selection back to the terminal.

### Changed

- Folded sections and the open tab are remembered for each workspace.
- The other workspaces show above the sidebar rather than in the status bar, which keeps room for the keys.

## [0.2.0] - 2026-10-20

### Added

- Mentions that notify: `@name`, `@here` and `#channel` become real mentions, suggested as you type `@` or `#`.
- Ctrl+K reaches everyone in the workspace and every public channel: pick someone to start a conversation, or a channel to join it.
- Edit (`e`) and delete (`d`) your own messages.
- A draft per conversation and per thread, marked ✎ in the sidebar.
- Bold, italic, strikethrough and quotes are drawn as such.
- Every key of normal mode can be changed in `[keys]`, and `?` lists the keys in use.
- Any color of the theme can be changed in `[colors]`, `"reset"` giving the terminal's own.
- A 12-hour clock, in the settings.
- `slackterm config` opens the config file in your editor, creating it with every option commented out, and checks it afterwards. `--config` uses another file, and `$XDG_CONFIG_HOME` is followed.
- `slackterm doctor` checks your setup, one line each.
- A log file for bug reports, with `--debug` for every call to Slack.
- `slackterm completions <shell>` for bash, zsh, fish, elvish and PowerShell.

### Changed

- Conversations muted in Slack stay quiet: only mentions mark them unread.
- Unknown options in the config file are reported with their line instead of being ignored.
- Saving from the settings screen keeps your comments and the rest of the config file.

### Fixed

- An expired session is said once, in the status bar, rather than as an error for every request, and real time stops trying to reconnect.
- Something typed in a conversation no longer follows you to the next one.
- Channels Slack names only by their id show their name.
- Popups no longer leave their text over profile pictures.

## [0.1.0] - 2026-10-20

The first release.

- Slack in the terminal: real time, threads, your sections, search through the whole history, reactions, Ctrl+K to jump anywhere, three layouts and four themes, profile pictures and file previews.
- Sign in through your browser. Credentials go to the system keychain, or to a file only you can read where there is none.
- Runs on macOS, Linux and Windows; installs with Homebrew, the install scripts or Cargo.

[Unreleased]: https://github.com/simoncdna/slackterm/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/simoncdna/slackterm/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/simoncdna/slackterm/releases/tag/v0.1.0

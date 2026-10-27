# Contributing to slackterm

Thanks for your interest. Bug reports, ideas and pull requests are all welcome.

## Reporting a bug or asking for a feature

Open an [issue](https://github.com/simoncdna/slackterm/issues/new/choose): the forms ask for what helps, like the output of `slackterm doctor` for a bug. If it is about how something looks or behaves, try it in `slackterm demo` too: if it happens there, anyone can reproduce it.

## Working on the code

You need [Rust](https://rustup.rs) 1.90 or later.

```sh
git clone https://github.com/simoncdna/slackterm.git
cd slackterm
cargo run -- demo        # the interface, on a made-up workspace
cargo run                # on your own workspaces, after `cargo run -- login`
cargo test
```

`slackterm demo` needs no account and no network, which makes it the quickest way to see a change. Its workspaces live in `src/tui/demo/content.rs`.

### How the code is organized

```
src/
├── auth/        sign-in through the browser, or by hand
├── slack/       HTTP client, real-time websocket, message markup, emoji
└── tui/
    ├── update/  all the logic: keys, mouse, popups, events
    ├── ui/      drawing: the three layouts, sidebar, popups
    ├── demo/    the made-up workspaces of `slackterm demo`
    └── executor.rs  runs commands against Slack's API
```

All the logic goes through `update(app, event) -> Vec<Command>`: it changes the state and returns the side effects to run, without running them. A change in behavior is therefore tested by feeding events to `update` and checking the state and the commands, without Slack; `src/tui/update/tests.rs` has many examples. Drawing is tested by rendering to a buffer and reading the text, in `src/tui/ui/mod.rs`.

### Before opening a pull request

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test
```

CI runs the same on Linux, macOS and Windows. A few more things keep the code consistent:

- Comments say why, in plain sentences; the code says what.
- Text shown to people is in English, short and concrete: say what happened and what to do.
- New keys go through the keymap (`src/tui/keymap.rs`), so people can change them and `?` lists them.
- If people will notice the change, add a line to the "Unreleased" part of [CHANGELOG.md](CHANGELOG.md), and update the README when it describes the behavior.

### The videos in the README

They are [VHS](https://github.com/charmbracelet/vhs) scripts in `docs/tapes`, played on the demo. After a change to the interface, `docs/record.sh` records them again.

## License

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in slackterm by you, as defined in the Apache-2.0 license, shall be dual licensed under the MIT and Apache-2.0 licenses, without any additional terms or conditions.

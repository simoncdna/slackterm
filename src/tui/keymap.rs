//! Which key does what in normal mode: the defaults, then the `[keys]`
//! table of the config file.

use std::collections::BTreeMap;
use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::state::Focus;
use crate::config::KeyList;

/// What a key does in normal mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Quit,
    GoTo,
    Search,
    Settings,
    Help,
    NextPane,
    PreviousPane,
    FocusSidebar,
    FocusMessages,
    FocusThread,
    NextWorkspace,
    Write,
    Down,
    Up,
    PageDown,
    PageUp,
    Top,
    Bottom,
    Left,
    Right,
    Open,
    Thread,
    React,
    OpenFiles,
    Play,
    Edit,
    Delete,
}

/// Where an action works, as the help screen groups them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Anywhere,
    /// Moving through the sidebar, the messages and the thread.
    Lists,
    /// On the selected message.
    Message,
}

pub struct Definition {
    pub action: Action,
    /// Its name in the `[keys]` table.
    pub name: &'static str,
    pub scope: Scope,
    /// In the syntax of the config file.
    pub keys: &'static [&'static str],
    pub help: &'static str,
}

const fn define(
    action: Action,
    name: &'static str,
    scope: Scope,
    keys: &'static [&'static str],
    help: &'static str,
) -> Definition {
    Definition {
        action,
        name,
        scope,
        keys,
        help,
    }
}

use Action::*;
use Scope::*;

/// Every action, in the order of the help screen and the config file.
pub const ACTIONS: &[Definition] = &[
    define(Quit, "quit", Anywhere, &["q"], "Quit"),
    define(
        GoTo,
        "go_to",
        Anywhere,
        &["ctrl+k"],
        "Go to a channel or a person",
    ),
    define(
        Search,
        "search",
        Anywhere,
        &["/", "ctrl+f"],
        "Search messages",
    ),
    define(Settings, "settings", Anywhere, &[","], "Settings"),
    define(Help, "help", Anywhere, &["?"], "This list of keys"),
    define(NextPane, "next_pane", Anywhere, &["tab"], "Next pane"),
    define(
        PreviousPane,
        "previous_pane",
        Anywhere,
        &["shift+tab"],
        "Previous pane",
    ),
    define(FocusSidebar, "focus_sidebar", Anywhere, &["1"], "Sidebar"),
    define(
        FocusMessages,
        "focus_messages",
        Anywhere,
        &["2"],
        "Messages",
    ),
    define(FocusThread, "focus_thread", Anywhere, &["3"], "Thread"),
    define(
        NextWorkspace,
        "next_workspace",
        Anywhere,
        &["w"],
        "Next workspace, when signed in to several",
    ),
    define(
        Write,
        "write",
        Anywhere,
        &["i", "a", "enter"],
        "Write a message",
    ),
    define(Down, "down", Lists, &["j", "down"], "Down"),
    define(
        Up,
        "up",
        Lists,
        &["k", "up"],
        "Up, loading older messages at the top",
    ),
    define(
        PageDown,
        "page_down",
        Lists,
        &["pagedown"],
        "Next page of messages",
    ),
    define(
        PageUp,
        "page_up",
        Lists,
        &["pageup"],
        "Previous page of messages",
    ),
    define(Top, "top", Lists, &["g", "home"], "Top"),
    define(
        Bottom,
        "bottom",
        Lists,
        &["G", "end"],
        "Bottom, back to the latest",
    ),
    define(
        Left,
        "left",
        Lists,
        &["h", "left", "["],
        "Channels tab, or the pane on the left",
    ),
    define(Right, "right", Lists, &["l", "right", "]"], "DMs tab"),
    define(
        Open,
        "open",
        Lists,
        &["enter", "space"],
        "Open a conversation, fold a section",
    ),
    define(
        Thread,
        "thread",
        Message,
        &["t"],
        "Open the message's thread",
    ),
    define(React, "react", Message, &["r"], "React to the message"),
    define(
        OpenFiles,
        "open_files",
        Message,
        &["o"],
        "Open the message's files or link",
    ),
    define(
        Play,
        "play",
        Message,
        &["p"],
        "Play the message's video in the terminal",
    ),
    define(Edit, "edit", Message, &["e"], "Edit your message"),
    define(Delete, "delete", Message, &["d"], "Delete your message"),
];

/// What works in each pane, before the actions that work anywhere: a key
/// bound to both does the pane's action.
fn pane_actions(focus: Focus) -> &'static [Action] {
    match focus {
        Focus::Sidebar => &[Down, Up, Top, Bottom, Left, Right, Open],
        Focus::Messages => &[
            Down, Up, PageDown, PageUp, Top, Bottom, Left, Thread, React, OpenFiles, Play, Edit,
            Delete,
        ],
        Focus::Thread => &[Down, Up, Bottom, Left, React, OpenFiles, Play, Edit, Delete],
    }
}

fn definition(action: Action) -> &'static Definition {
    ACTIONS
        .iter()
        .find(|d| d.action == action)
        .expect("every action is defined")
}

/// A key with its modifiers, as the terminal reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl Key {
    /// Shift is part of a character (`G`, `?`), and turns Tab into BackTab.
    /// With Ctrl, letters have no case: most terminals cannot tell
    /// Ctrl+Shift+K from Ctrl+K.
    fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        let mut modifiers =
            modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        let shift = modifiers.contains(KeyModifiers::SHIFT);
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        let code = match code {
            KeyCode::Char(c) => {
                modifiers.remove(KeyModifiers::SHIFT);
                KeyCode::Char(match (ctrl, shift) {
                    (true, _) => c.to_ascii_lowercase(),
                    (false, true) => c.to_ascii_uppercase(),
                    (false, false) => c,
                })
            }
            KeyCode::Tab if shift => {
                modifiers.remove(KeyModifiers::SHIFT);
                KeyCode::BackTab
            }
            KeyCode::BackTab => {
                modifiers.remove(KeyModifiers::SHIFT);
                KeyCode::BackTab
            }
            code => code,
        };
        Self { code, modifiers }
    }

    pub fn from_event(event: KeyEvent) -> Self {
        Self::new(event.code, event.modifiers)
    }

    /// `q`, `G`, `enter`, `ctrl+k`, `shift+tab`…
    pub fn parse(spec: &str) -> Result<Self, String> {
        let mut modifiers = KeyModifiers::NONE;
        let mut rest = spec;
        loop {
            let lower = rest.to_ascii_lowercase();
            let (modifier, prefix) = if lower.starts_with("ctrl+") {
                (KeyModifiers::CONTROL, "ctrl+")
            } else if lower.starts_with("alt+") {
                (KeyModifiers::ALT, "alt+")
            } else if lower.starts_with("shift+") {
                (KeyModifiers::SHIFT, "shift+")
            } else {
                break;
            };
            // `ctrl++` is Ctrl and the `+` key.
            if rest.len() == prefix.len() {
                break;
            }
            modifiers |= modifier;
            rest = &rest[prefix.len()..];
        }
        let code = match rest.to_ascii_lowercase().as_str() {
            "enter" | "return" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "backtab" => KeyCode::BackTab,
            "space" => KeyCode::Char(' '),
            "backspace" => KeyCode::Backspace,
            "delete" | "del" => KeyCode::Delete,
            "insert" => KeyCode::Insert,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" | "pgup" => KeyCode::PageUp,
            "pagedown" | "pgdn" => KeyCode::PageDown,
            name => match name.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                Some(n @ 1..=12) => KeyCode::F(n),
                _ => {
                    let mut chars = rest.chars();
                    match (chars.next(), chars.next()) {
                        (Some(c), None) => KeyCode::Char(c),
                        _ => return Err(format!("unknown key `{spec}`")),
                    }
                }
            },
        };
        Ok(Self::new(code, modifiers))
    }

    /// In the syntax of the config file: `ctrl+k`, `shift+tab`, `G`.
    pub fn spec(self) -> String {
        let mut out = String::new();
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            out.push_str("ctrl+");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            out.push_str("alt+");
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            out.push_str("shift+");
        }
        match self.code {
            KeyCode::Char(' ') => out.push_str("space"),
            KeyCode::Char(c) => out.push(c),
            KeyCode::BackTab => out.push_str("shift+tab"),
            KeyCode::F(n) => out.push_str(&format!("f{n}")),
            code => out.push_str(&format!("{code:?}").to_ascii_lowercase()),
        }
        out
    }
}

/// Short, for hints: `^k`, `⏎`, `⇧tab`, `↑`.
impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            f.write_str("^")?;
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            f.write_str("alt+")?;
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            f.write_str("⇧")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("space"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::Enter => f.write_str("⏎"),
            KeyCode::Esc => f.write_str("esc"),
            KeyCode::Tab => f.write_str("tab"),
            KeyCode::BackTab => f.write_str("⇧tab"),
            KeyCode::Backspace => f.write_str("⌫"),
            KeyCode::Delete => f.write_str("del"),
            KeyCode::Up => f.write_str("↑"),
            KeyCode::Down => f.write_str("↓"),
            KeyCode::Left => f.write_str("←"),
            KeyCode::Right => f.write_str("→"),
            KeyCode::PageUp => f.write_str("pgup"),
            KeyCode::PageDown => f.write_str("pgdn"),
            KeyCode::F(n) => write!(f, "f{n}"),
            code => f.write_str(&format!("{code:?}").to_ascii_lowercase()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    keys: BTreeMap<Action, Vec<Key>>,
}

impl Default for Keymap {
    fn default() -> Self {
        let keys = ACTIONS
            .iter()
            .map(|d| {
                let keys = d.keys.iter().map(|spec| Key::parse(spec).expect(spec));
                (d.action, keys.collect())
            })
            .collect();
        Self { keys }
    }
}

impl Keymap {
    /// The defaults with the `[keys]` table applied, and what is wrong with
    /// that table.
    pub fn new(table: &BTreeMap<String, KeyList>) -> (Self, Vec<String>) {
        let mut keymap = Self::default();
        let mut problems = Vec::new();
        for (name, list) in table {
            let Some(definition) = ACTIONS.iter().find(|d| d.name == name) else {
                problems.push(format!("[keys]: unknown action `{name}`"));
                continue;
            };
            let mut keys = Vec::new();
            for spec in &list.0 {
                match Key::parse(spec) {
                    Ok(key) => keys.push(key),
                    Err(e) => problems.push(format!("[keys] {name}: {e}")),
                }
            }
            keymap.keys.insert(definition.action, keys);
        }
        // The defaults share some keys on purpose (Enter opens in the
        // sidebar and writes elsewhere): only report new clashes.
        let expected = Self::default().clashes();
        for (key, winner, loser) in keymap.clashes() {
            if !expected.contains(&(key, winner, loser)) {
                problems.push(format!(
                    "[keys]: `{}` is bound to both {} and {}, and does {} where both apply",
                    key.spec(),
                    definition(winner).name,
                    definition(loser).name,
                    definition(winner).name,
                ));
            }
        }
        (keymap, problems)
    }

    /// The action of a key pressed in normal mode.
    pub fn action(&self, focus: Focus, event: KeyEvent) -> Option<Action> {
        let key = Key::from_event(event);
        Self::actions_in(focus).find(|action| self.keys(*action).contains(&key))
    }

    /// The action of a key pressed while writing: only shortcuts with Ctrl
    /// or Alt that leave the message, so that typing stays typing.
    pub fn action_while_writing(&self, event: KeyEvent) -> Option<Action> {
        if !event
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return None;
        }
        let key = Key::from_event(event);
        [GoTo, Search]
            .into_iter()
            .find(|action| self.keys(*action).contains(&key))
    }

    pub fn keys(&self, action: Action) -> &[Key] {
        self.keys.get(&action).map_or(&[], Vec::as_slice)
    }

    /// The first key of an action, for hints; `None` when it has none.
    pub fn hint(&self, action: Action) -> Option<String> {
        self.keys(action).first().map(Key::to_string)
    }

    /// `j/k`: the first keys of two actions that go together.
    pub fn pair(&self, first: Action, second: Action) -> Option<String> {
        Some(format!("{}/{}", self.hint(first)?, self.hint(second)?))
    }

    fn actions_in(focus: Focus) -> impl Iterator<Item = Action> {
        let anywhere = ACTIONS
            .iter()
            .filter(|d| d.scope == Anywhere)
            .map(|d| d.action);
        pane_actions(focus).iter().copied().chain(anywhere)
    }

    /// Keys bound to two actions of one pane, with the action that wins.
    fn clashes(&self) -> Vec<(Key, Action, Action)> {
        let mut clashes = Vec::new();
        for focus in [Focus::Sidebar, Focus::Messages, Focus::Thread] {
            let actions: Vec<Action> = Self::actions_in(focus).collect();
            for (i, &winner) in actions.iter().enumerate() {
                for &loser in &actions[i + 1..] {
                    for &key in self.keys(winner) {
                        let clash = (key, winner, loser);
                        if self.keys(loser).contains(&key) && !clashes.contains(&clash) {
                            clashes.push(clash);
                        }
                    }
                }
            }
        }
        clashes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn table(entries: &[(&str, &[&str])]) -> BTreeMap<String, KeyList> {
        entries
            .iter()
            .map(|(name, keys)| {
                let keys = keys.iter().map(|k| k.to_string()).collect();
                (name.to_string(), KeyList(keys))
            })
            .collect()
    }

    #[test]
    fn parses_the_key_syntax() {
        let key = |code, modifiers| Key::new(code, modifiers);
        let none = KeyModifiers::NONE;
        assert_eq!(Key::parse("q"), Ok(key(KeyCode::Char('q'), none)));
        assert_eq!(Key::parse("G"), Ok(key(KeyCode::Char('G'), none)));
        assert_eq!(Key::parse("shift+g"), Ok(key(KeyCode::Char('G'), none)));
        assert_eq!(
            Key::parse("Ctrl+K"),
            Ok(key(KeyCode::Char('k'), KeyModifiers::CONTROL))
        );
        assert_eq!(Key::parse("ctrl++").unwrap().spec(), "ctrl++");
        assert_eq!(Key::parse("shift+tab"), Ok(key(KeyCode::BackTab, none)));
        assert_eq!(Key::parse("space"), Ok(key(KeyCode::Char(' '), none)));
        assert_eq!(Key::parse("f5"), Ok(key(KeyCode::F(5), none)));
        assert_eq!(Key::parse("F"), Ok(key(KeyCode::Char('F'), none)));
        assert!(Key::parse("hyper+x").is_err());
        assert!(Key::parse("f13").is_err());
    }

    #[test]
    fn shows_keys_in_both_syntaxes() {
        for spec in [
            "q",
            "G",
            "ctrl+k",
            "alt+enter",
            "shift+tab",
            "space",
            "pagedown",
        ] {
            assert_eq!(Key::parse(spec).unwrap().spec(), spec);
        }
        assert_eq!(Key::parse("ctrl+k").unwrap().to_string(), "^k");
        assert_eq!(Key::parse("enter").unwrap().to_string(), "⏎");
        assert_eq!(Key::parse("shift+tab").unwrap().to_string(), "⇧tab");
    }

    #[test]
    fn the_defaults_parse_and_only_share_keys_on_purpose() {
        let (keymap, problems) = Keymap::new(&BTreeMap::new());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(keymap, Keymap::default());
        // Enter opens in the sidebar, and writes everywhere else.
        assert_eq!(keymap.clashes().len(), 1, "{:?}", keymap.clashes());
    }

    #[test]
    fn finds_actions_pane_first() {
        let keymap = Keymap::default();
        let enter = press(KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(keymap.action(Focus::Sidebar, enter), Some(Open));
        assert_eq!(keymap.action(Focus::Messages, enter), Some(Write));
        let shift_g = press(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(keymap.action(Focus::Thread, shift_g), Some(Bottom));
        let back_tab = press(KeyCode::BackTab, KeyModifiers::SHIFT);
        assert_eq!(keymap.action(Focus::Messages, back_tab), Some(PreviousPane));
        let t = press(KeyCode::Char('t'), KeyModifiers::NONE);
        assert_eq!(keymap.action(Focus::Sidebar, t), None);
    }

    #[test]
    fn only_shortcuts_with_modifiers_work_while_writing() {
        let keymap = Keymap::default();
        let ctrl_k = press(KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_eq!(keymap.action_while_writing(ctrl_k), Some(GoTo));
        let slash = press(KeyCode::Char('/'), KeyModifiers::NONE);
        assert_eq!(keymap.action_while_writing(slash), None);
    }

    #[test]
    fn the_table_replaces_keys_and_reports_problems() {
        let (keymap, problems) = Keymap::new(&table(&[
            ("react", &["z"]),
            ("play", &[]),
            ("quit", &["x", "hyper+q"]),
            ("fly", &["f"]),
        ]));
        let z = press(KeyCode::Char('z'), KeyModifiers::NONE);
        assert_eq!(keymap.action(Focus::Messages, z), Some(React));
        let p = press(KeyCode::Char('p'), KeyModifiers::NONE);
        assert_eq!(keymap.action(Focus::Messages, p), None);
        assert_eq!(keymap.hint(Quit).as_deref(), Some("x"));
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("unknown key `hyper+q`")));
        assert!(problems.iter().any(|p| p.contains("unknown action `fly`")));
    }

    #[test]
    fn reports_keys_that_shadow_another_action() {
        let (keymap, problems) = Keymap::new(&table(&[("react", &["q"])]));
        let q = press(KeyCode::Char('q'), KeyModifiers::NONE);
        assert_eq!(keymap.action(Focus::Messages, q), Some(React));
        assert_eq!(keymap.action(Focus::Sidebar, q), Some(Quit));
        assert_eq!(
            problems,
            ["[keys]: `q` is bound to both react and quit, and does react where both apply"]
        );
    }
}

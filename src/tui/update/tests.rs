use crossterm::event::{KeyCode, KeyModifiers};

use super::*;
use crate::slack::test_message as message;
use crate::tui::state::fixtures::*;

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn ctrl(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL))
}

fn typed(app: &mut App, text: &str) {
    for c in text.chars() {
        update(app, key(KeyCode::Char(c)));
    }
}

/// An app showing #general with two messages, and #random unread-free.
fn loaded_app() -> App {
    let mut app = app();
    update(
        &mut app,
        Event::Api(ApiEvent::Conversations(vec![
            conversation("C1", "general"),
            conversation("C2", "random"),
            direct("D1", "U2"),
        ])),
    );
    update(
        &mut app,
        Event::Api(ApiEvent::History {
            channel: "C1".into(),
            messages: vec![
                message("1.0", "U2", "hi"),
                message("2.0", "U2", "how are you?"),
            ],
            has_more: false,
        }),
    );
    app
}

#[test]
fn opens_the_general_channel_once_conversations_load() {
    let mut app = app();
    let commands = update(
        &mut app,
        Event::Api(ApiEvent::Conversations(vec![
            conversation("C2", "random"),
            conversation("C1", "general"),
        ])),
    );
    assert_eq!(app.current.as_deref(), Some("C1"));
    assert_eq!(commands, [Command::LoadHistory("C1".into())]);
}

#[test]
fn marks_the_channel_read_when_its_history_arrives() {
    let mut app = app();
    update(
        &mut app,
        Event::Api(ApiEvent::Conversations(vec![conversation("C1", "general")])),
    );
    let commands = update(
        &mut app,
        Event::Api(ApiEvent::History {
            channel: "C1".into(),
            messages: vec![message("1.0", "U2", "hi")],
            has_more: false,
        }),
    );
    assert_eq!(
        commands,
        [Command::MarkRead {
            channel: "C1".into(),
            ts: "1.0".into()
        }]
    );
}

#[test]
fn new_messages_elsewhere_mark_channels_unread() {
    let mut app = loaded_app();
    update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "C2".into(),
            message: message("3.0", "U2", "hey <@UME>"),
        }),
    );
    let random = app.channel("C2").unwrap();
    assert!(random.unread);
    assert_eq!(random.mentions, 1);
}

#[test]
fn own_messages_do_not_mark_channels_unread() {
    let mut app = loaded_app();
    update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "C2".into(),
            message: message("3.0", "UME", "from my phone"),
        }),
    );
    assert!(!app.channel("C2").unwrap().unread);
}

#[test]
fn messages_in_the_current_channel_are_appended_once() {
    let mut app = loaded_app();
    let event = || {
        Event::Rtm(RtmEvent::Message {
            channel: "C1".into(),
            message: message("3.0", "U2", "nouveau"),
        })
    };
    let commands = update(&mut app, event());
    update(&mut app, event());

    assert_eq!(app.current_messages().len(), 3);
    assert_eq!(
        commands,
        [Command::MarkRead {
            channel: "C1".into(),
            ts: "3.0".into()
        }]
    );
}

#[test]
fn messages_from_unknown_channels_reload_the_list() {
    let mut app = loaded_app();
    let commands = update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "D9".into(),
            message: message("3.0", "U3", "hi"),
        }),
    );
    assert_eq!(
        commands,
        [
            Command::LoadConversations,
            Command::Notify {
                title: "U3".into(),
                body: "hi".into()
            }
        ]
    );

    update(
        &mut app,
        Event::Api(ApiEvent::Conversations(vec![
            conversation("C1", "general"),
            direct("D9", "U3"),
        ])),
    );
    assert!(app.channel("D9").unwrap().unread);
}

#[test]
fn edits_and_deletions_apply_to_the_history() {
    let mut app = loaded_app();
    let mut edited = message("1.0", "U2", "hi all");
    edited.edited = Some(serde_json::json!({}));
    update(
        &mut app,
        Event::Rtm(RtmEvent::MessageChanged {
            channel: "C1".into(),
            message: edited,
        }),
    );
    assert_eq!(app.current_messages()[0].text, "hi all");

    update(
        &mut app,
        Event::Rtm(RtmEvent::MessageDeleted {
            channel: "C1".into(),
            ts: "1.0".into(),
        }),
    );
    assert_eq!(app.current_messages().len(), 1);
}

#[test]
fn typing_and_enter_sends_an_escaped_message() {
    let mut app = loaded_app();
    update(&mut app, key(KeyCode::Char('i')));
    typed(&mut app, "a < b & c");
    let commands = update(&mut app, key(KeyCode::Enter));

    assert_eq!(
        commands,
        [Command::Send {
            channel: "C1".into(),
            text: "a &lt; b &amp; c".into(),
            thread_ts: None
        }]
    );
    assert!(app.input.is_empty());
    assert_eq!(app.mode, Mode::Insert);
}

#[test]
fn replies_go_to_the_open_thread() {
    let mut app = loaded_app();
    app.focus = Focus::Messages;
    update(&mut app, key(KeyCode::Char('t')));
    assert_eq!(app.focus, Focus::Thread);

    update(&mut app, key(KeyCode::Char('i')));
    typed(&mut app, "ok");
    let commands = update(&mut app, key(KeyCode::Enter));

    assert_eq!(
        commands,
        [Command::Send {
            channel: "C1".into(),
            text: "ok".into(),
            thread_ts: Some("2.0".into())
        }]
    );
}

#[test]
fn thread_replies_stay_out_of_the_channel() {
    let mut app = loaded_app();
    let mut reply = message("3.0", "U2", "reply");
    reply.thread_ts = Some("1.0".into());
    update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "C1".into(),
            message: reply,
        }),
    );
    assert_eq!(app.current_messages().len(), 2);
}

#[test]
fn selection_moves_through_messages_and_back_to_live() {
    let mut app = loaded_app();
    app.focus = Focus::Messages;
    update(&mut app, key(KeyCode::Char('k')));
    assert_eq!(app.selected, Some(1));
    update(&mut app, key(KeyCode::Char('k')));
    assert_eq!(app.selected, Some(0));
    update(&mut app, key(KeyCode::Char('j')));
    update(&mut app, key(KeyCode::Char('j')));
    assert_eq!(app.selected, None);
}

#[test]
fn switcher_opens_the_chosen_channel() {
    let mut app = loaded_app();
    update(&mut app, ctrl('k'));
    typed(&mut app, "rand");
    let commands = update(&mut app, key(KeyCode::Enter));

    assert!(app.overlay.is_none());
    assert_eq!(app.current.as_deref(), Some("C2"));
    assert_eq!(commands, [Command::LoadHistory("C2".into())]);
}

#[test]
fn settings_change_live_and_save_on_close() {
    let mut app = loaded_app();
    let path = std::path::PathBuf::from("/home/alex/.config/slackterm/config.toml");
    app.settings_path = Some(path.clone());
    update(&mut app, key(KeyCode::Char(',')));
    update(&mut app, key(KeyCode::Char('l')));
    assert_eq!(app.settings.layout, Layout::Stream);

    update(&mut app, key(KeyCode::Char('j')));
    update(&mut app, key(KeyCode::Char('l')));
    let commands = update(&mut app, key(KeyCode::Esc));

    assert!(app.overlay.is_none());
    assert_eq!(
        commands,
        [Command::SaveSettings {
            settings: app.settings,
            path
        }]
    );
    assert_ne!(app.settings.theme, Settings::default().theme);
}

#[test]
fn settings_without_a_config_file_are_not_saved() {
    let mut app = loaded_app();
    update(&mut app, key(KeyCode::Char(',')));
    update(&mut app, key(KeyCode::Char('l')));
    let commands = update(&mut app, key(KeyCode::Esc));
    assert!(commands.is_empty());
}

#[test]
fn keys_follow_the_keymap() {
    let mut app = loaded_app();
    let mut keys = std::collections::BTreeMap::new();
    keys.insert("quit".to_string(), crate::config::KeyList(vec!["x".into()]));
    keys.insert(
        "help".to_string(),
        crate::config::KeyList(vec!["f1".into()]),
    );
    app.keymap = crate::tui::keymap::Keymap::new(&keys).0;

    update(&mut app, key(KeyCode::Char('q')));
    assert!(!app.should_quit);
    update(&mut app, key(KeyCode::F(1)));
    assert!(matches!(app.overlay, Some(Overlay::Help { .. })));
    update(&mut app, key(KeyCode::F(1)));
    assert!(app.overlay.is_none());
    update(&mut app, key(KeyCode::Char('x')));
    assert!(app.should_quit);
}

fn count(id: &str, unread: bool, latest: &str) -> ConversationCount {
    ConversationCount {
        id: id.into(),
        has_unreads: unread,
        mention_count: 0,
        latest: Some(latest.into()),
        last_read: None,
    }
}

fn group(id: &str) -> Conversation {
    serde_json::from_value(serde_json::json!({"id": id, "is_mpim": true, "name": "mpdm-a--b-1"}))
        .unwrap()
}

fn conversations() -> Event {
    Event::Api(ApiEvent::Conversations(vec![
        conversation("C1", "general"),
        conversation("C2", "random"),
        direct("D1", "U2"),
        direct("D2", "U3"),
        group("G1"),
    ]))
}

fn counts() -> Event {
    Event::Api(ApiEvent::Counts(vec![
        count("C2", true, "4.0"),
        count("D1", false, "3.0"),
    ]))
}

fn listed(app: &App) -> Vec<String> {
    app.listed_indices()
        .into_iter()
        .map(|i| app.channels[i].id.clone())
        .collect()
}

#[test]
fn counts_apply_whichever_arrives_first() {
    for counts_first in [true, false] {
        let mut app = app();
        if counts_first {
            update(&mut app, counts());
            update(&mut app, conversations());
        } else {
            update(&mut app, conversations());
            update(&mut app, counts());
        }
        assert!(app.channel("C2").unwrap().unread);
        assert_eq!(listed(&app), ["C1", "C2", "D1"]);
    }
}

#[test]
fn without_counts_direct_messages_are_listed_but_not_groups() {
    let mut app = app();
    update(&mut app, conversations());
    update(&mut app, Event::Api(ApiEvent::CountsUnavailable));
    assert_eq!(listed(&app), ["C1", "C2", "D1", "D2"]);
}

#[test]
fn a_new_direct_message_lists_the_conversation_first() {
    let mut app = app();
    update(&mut app, conversations());
    update(&mut app, counts());
    update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "D2".into(),
            message: message("9.0", "U3", "hi"),
        }),
    );
    assert_eq!(listed(&app), ["C1", "C2", "D2", "D1"]);
    assert_eq!(app.channel("D2").unwrap().mentions, 1);
}

#[test]
fn the_sidebar_cursor_skips_hidden_conversations() {
    let mut app = app();
    update(&mut app, conversations());
    update(&mut app, counts());
    for _ in 0..5 {
        update(&mut app, key(KeyCode::Char('j')));
    }
    assert_eq!(app.sidebar_cursor, Some(SidebarItem::Channel("D1".into())));
}

#[test]
fn the_focus_layout_has_no_sidebar_to_focus() {
    let mut app = loaded_app();
    app.settings.layout = Layout::Focus;
    app.focus = Focus::Messages;
    update(&mut app, key(KeyCode::Tab));
    assert_eq!(app.focus, Focus::Messages);
    update(&mut app, key(KeyCode::Char('1')));
    assert!(matches!(app.overlay, Some(Overlay::Switcher { .. })));
}

fn section(
    id: &str,
    kind: &str,
    name: &str,
    channels: &[&str],
    next: Option<&str>,
) -> ChannelSection {
    serde_json::from_value(serde_json::json!({
        "channel_section_id": id,
        "type": kind,
        "name": name,
        "emoji": "toolbox",
        "channel_ids_page": {"channel_ids": channels},
        "next_channel_section_id": next,
    }))
    .unwrap()
}

/// `loaded_app` plus a "Tooling" section holding #random, before the
/// default channel and direct message sections.
fn app_with_sections() -> App {
    let mut app = loaded_app();
    update(&mut app, Event::Api(ApiEvent::CountsUnavailable));
    update(
        &mut app,
        Event::Api(ApiEvent::Sections(vec![
            section("S1", "standard", "Tooling", &["C2"], Some("S2")),
            section("S2", "channels", "", &[], Some("S3")),
            section("S3", "direct_messages", "", &[], None),
            section("S4", "salesforce_records", "Salesforce channels", &[], None),
        ])),
    );
    app
}

#[test]
fn custom_sections_hold_their_channels() {
    let app = app_with_sections();
    let names: Vec<String> = app.sections.iter().map(|s| s.name.clone()).collect();
    assert_eq!(names, ["Tooling", "Channels", "Direct messages"]);
    assert_eq!(app.sections[0].emoji.as_deref(), Some("🧰"));
    assert_eq!(
        app.sidebar_items(),
        [
            SidebarItem::Section("S1".into()),
            SidebarItem::Channel("C2".into()),
            SidebarItem::Section("S2".into()),
            SidebarItem::Channel("C1".into()),
            SidebarItem::Section("S3".into()),
            SidebarItem::Channel("D1".into()),
        ]
    );
}

#[test]
fn enter_on_a_section_collapses_it_and_saves() {
    let mut app = app_with_sections();
    update(&mut app, key(KeyCode::Char('g')));
    let commands = update(&mut app, key(KeyCode::Enter));

    assert!(app.collapsed.contains("S1"));
    assert!(
        !app.sidebar_items()
            .contains(&SidebarItem::Channel("C2".into()))
    );
    assert!(
        matches!(&commands[..], [Command::SaveSidebar { state, .. }] if state.collapsed.contains("S1"))
    );

    update(&mut app, key(KeyCode::Enter));
    assert!(app.collapsed.is_empty());
}

#[test]
fn collapsed_sections_still_show_unread_channels() {
    let mut app = app_with_sections();
    app.collapsed.insert("S1".into());
    update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "C2".into(),
            message: message("5.0", "U2", "nouveau"),
        }),
    );
    assert!(
        app.sidebar_items()
            .contains(&SidebarItem::Channel("C2".into()))
    );
}

fn found(channel: &str, ts: &str, permalink: &str) -> SearchMatch {
    serde_json::from_value(serde_json::json!({
        "ts": ts,
        "user": "U2",
        "text": "the \u{e000}deploy\u{e001} went through",
        "permalink": permalink,
        "channel": {"id": channel, "name": "general"}
    }))
    .unwrap()
}

fn search_for(app: &mut App, query: &str, results: Vec<SearchMatch>) {
    update(app, key(KeyCode::Char('/')));
    typed(app, query);
    let commands = update(app, key(KeyCode::Enter));
    assert_eq!(commands, [Command::Search(query.into())]);
    update(
        app,
        Event::Api(ApiEvent::SearchResults {
            query: query.into(),
            result: Ok(results),
        }),
    );
}

#[test]
fn opening_a_search_result_selects_the_message() {
    let mut app = loaded_app();
    update(&mut app, key(KeyCode::Char('2')));
    search_for(&mut app, "deploy", vec![found("C1", "1.0", "https://x/p1")]);
    let commands = update(&mut app, key(KeyCode::Enter));

    assert!(app.overlay.is_none());
    assert_eq!(app.current.as_deref(), Some("C1"));
    assert_eq!(app.selected, Some(0));
    assert!(
        commands
            .iter()
            .all(|c| !matches!(c, Command::LoadOlder { .. }))
    );
}

#[test]
fn opening_a_thread_reply_opens_its_thread() {
    let mut app = loaded_app();
    search_for(
        &mut app,
        "deploy",
        vec![found("C1", "3.0", "https://x/p3?thread_ts=2.0&cid=C1")],
    );
    let commands = update(&mut app, key(KeyCode::Enter));

    assert_eq!(app.focus, Focus::Thread);
    assert_eq!(app.thread.as_ref().map(|t| t.ts.as_str()), Some("2.0"));
    assert!(commands.contains(&Command::LoadReplies {
        channel: "C1".into(),
        ts: "2.0".into()
    }));
}

#[test]
fn reaching_an_old_result_loads_older_history() {
    let mut app = loaded_app();
    app.histories.get_mut("C1").unwrap().has_more = true;
    search_for(
        &mut app,
        "deploy",
        vec![found("C1", "0.5", "https://x/p05")],
    );
    let commands = update(&mut app, key(KeyCode::Enter));
    assert!(commands.contains(&Command::LoadOlder {
        channel: "C1".into(),
        before: "1.0".into()
    }));

    let commands = update(
        &mut app,
        Event::Api(ApiEvent::Older {
            channel: "C1".into(),
            messages: vec![
                message("0.2", "U2", "vieux"),
                message("0.5", "U2", "deploy"),
            ],
            has_more: true,
        }),
    );
    assert!(commands.is_empty());
    assert_eq!(app.selected, Some(1));
    assert_eq!(app.current_messages()[1].ts, "0.5");
}

#[test]
fn a_new_query_replaces_the_results() {
    let mut app = loaded_app();
    search_for(&mut app, "deploy", vec![found("C1", "1.0", "https://x/p1")]);
    update(&mut app, key(KeyCode::Char('s')));
    let commands = update(&mut app, key(KeyCode::Enter));
    assert_eq!(commands, [Command::Search("deploys".into())]);
    assert!(matches!(
        &app.overlay,
        Some(Overlay::Search(search)) if matches!(search.status, SearchStatus::Loading)
    ));
}

#[test]
fn going_up_from_the_oldest_message_loads_older_ones() {
    let mut app = loaded_app();
    app.histories.get_mut("C1").unwrap().has_more = true;
    app.focus = Focus::Messages;
    update(&mut app, key(KeyCode::Char('g')));
    let commands = update(&mut app, key(KeyCode::Char('k')));
    assert_eq!(
        commands,
        [Command::LoadOlder {
            channel: "C1".into(),
            before: "1.0".into()
        }]
    );
    // A second press while loading does not fetch twice.
    assert!(update(&mut app, key(KeyCode::Char('k'))).is_empty());

    update(
        &mut app,
        Event::Api(ApiEvent::Older {
            channel: "C1".into(),
            messages: vec![message("0.5", "U2", "plus ancien")],
            has_more: false,
        }),
    );
    // The same message stays selected although indices shifted.
    assert_eq!(app.selected, Some(1));
}

#[test]
fn the_private_tab_lists_direct_messages_by_recency() {
    let mut app = app();
    update(&mut app, conversations());
    update(&mut app, counts());
    update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "D2".into(),
            message: message("9.0", "U3", "hi"),
        }),
    );

    let commands = update(&mut app, key(KeyCode::Right));
    assert_eq!(app.sidebar_tab, SidebarTab::Direct);
    assert!(
        matches!(&commands[..], [Command::SaveSidebar { state, .. }] if state.tab == SidebarTab::Direct)
    );
    assert_eq!(
        app.sidebar_items(),
        [
            SidebarItem::Channel("D2".into()),
            SidebarItem::Channel("D1".into()),
        ]
    );

    update(&mut app, key(KeyCode::Enter));
    assert_eq!(app.current.as_deref(), Some("D2"));

    update(&mut app, key(KeyCode::Char('1')));
    update(&mut app, key(KeyCode::Left));
    assert_eq!(app.sidebar_tab, SidebarTab::Channels);
    assert!(
        app.sidebar_items()
            .contains(&SidebarItem::Channel("C1".into()))
    );
}

#[test]
fn switching_to_the_current_tab_does_nothing() {
    let mut app = loaded_app();
    assert!(update(&mut app, key(KeyCode::Left)).is_empty());
}

fn react_with(app: &mut App, query: &str) -> Vec<Command> {
    update(app, key(KeyCode::Char('r')));
    typed(app, query);
    update(app, key(KeyCode::Enter))
}

fn reactions(app: &App, ts: &str) -> Vec<(String, u32)> {
    app.find_message("C1", ts)
        .unwrap()
        .reactions
        .iter()
        .map(|r| (r.name.clone(), r.count))
        .collect()
}

#[test]
fn r_opens_the_picker_on_the_selected_message() {
    let mut app = loaded_app();
    app.focus = Focus::Messages;
    update(&mut app, key(KeyCode::Char('k')));
    update(&mut app, key(KeyCode::Char('k')));
    update(&mut app, key(KeyCode::Char('r')));
    assert!(matches!(&app.overlay, Some(Overlay::Emoji(picker)) if picker.target.ts == "1.0"));
}

#[test]
fn reacting_shows_the_reaction_at_once_and_a_second_time_removes_it() {
    let mut app = loaded_app();
    app.focus = Focus::Messages;

    let commands = react_with(&mut app, ":tada");
    assert_eq!(
        commands,
        [Command::React {
            channel: "C1".into(),
            ts: "2.0".into(),
            name: "tada".into(),
            add: true
        }]
    );
    assert!(app.overlay.is_none());
    assert_eq!(reactions(&app, "2.0"), [("tada".to_string(), 1)]);

    let commands = react_with(&mut app, "tada");
    assert!(matches!(&commands[..], [Command::React { add: false, .. }]));
    assert!(reactions(&app, "2.0").is_empty());
}

#[test]
fn a_refused_reaction_is_undone() {
    let mut app = loaded_app();
    app.focus = Focus::Messages;
    react_with(&mut app, "tada");
    update(
        &mut app,
        Event::Api(ApiEvent::ReactionFailed {
            channel: "C1".into(),
            ts: "2.0".into(),
            name: "tada".into(),
            added: true,
            reason: "invalid_name".into(),
        }),
    );
    assert!(reactions(&app, "2.0").is_empty());
    assert!(
        app.notice
            .as_deref()
            .is_some_and(|n| n.contains("invalid_name"))
    );
}

#[test]
fn the_picker_starts_with_the_most_used_reactions() {
    let mut app = loaded_app();
    let history = app.histories.get_mut("C1").unwrap();
    history.messages[0].add_reaction("eyes", "U2");
    history.messages[0].add_reaction("eyes", "U3");
    history.messages[1].add_reaction("eyes", "U4");
    let choices = app.emoji_choices("");
    assert_eq!(choices[0].name, "eyes");
    assert_eq!(choices[0].glyph.as_deref(), Some("👀"));
    assert!(choices.iter().any(|c| c.name == "+1"));
}

#[test]
fn the_picker_finds_standard_and_custom_emoji() {
    let mut app = loaded_app();
    app.custom_emoji = vec!["pictaheart".into()];
    assert_eq!(app.emoji_choices("+1")[0].name, "+1");
    assert_eq!(app.emoji_choices("thinking")[0].name, "thinking_face");
    let custom = &app.emoji_choices("pictah")[0];
    assert_eq!(custom.name, "pictaheart");
    assert!(custom.glyph.is_none());
}

#[test]
fn reactions_in_a_thread_go_to_the_selected_reply() {
    let mut app = loaded_app();
    app.histories.get_mut("C1").unwrap().messages[1].reply_count = 1;
    app.focus = Focus::Messages;
    update(&mut app, key(KeyCode::Char('t')));
    let mut reply = message("3.0", "U2", "reply");
    reply.thread_ts = Some("2.0".into());
    update(
        &mut app,
        Event::Api(ApiEvent::Replies {
            channel: "C1".into(),
            ts: "2.0".into(),
            messages: vec![message("2.0", "U2", "how are you?"), reply],
        }),
    );
    update(&mut app, key(KeyCode::Char('k')));
    update(&mut app, key(KeyCode::Char('k')));
    update(&mut app, key(KeyCode::Char('j')));
    let commands = react_with(&mut app, "+1");
    assert!(matches!(&commands[..], [Command::React { ts, .. }] if ts == "3.0"));
}

fn user_with_picture(id: &str, name: &str) -> crate::slack::User {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "name": name,
        "profile": {"image_72": format!("https://avatars.example/{id}.png")}
    }))
    .unwrap()
}

#[test]
fn pictures_are_requested_once_for_authors_on_screen() {
    let mut app = app();
    let commands = update(
        &mut app,
        Event::Api(ApiEvent::Users(vec![
            user_with_picture("U2", "camille"),
            user_with_picture("U9", "absent"),
        ])),
    );
    assert!(commands.is_empty(), "no message shown yet");

    update(
        &mut app,
        Event::Api(ApiEvent::Conversations(vec![conversation("C1", "general")])),
    );
    let commands = update(
        &mut app,
        Event::Api(ApiEvent::History {
            channel: "C1".into(),
            messages: vec![message("1.0", "U2", "hi"), message("2.0", "U2", "re")],
            has_more: false,
        }),
    );
    assert!(commands.contains(&Command::LoadAvatars(vec![(
        "U2".into(),
        "https://avatars.example/U2.png".into()
    )])));

    let commands = update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "C1".into(),
            message: message("3.0", "U2", "encore"),
        }),
    );
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, Command::LoadAvatars(_)))
    );
}

#[test]
fn no_pictures_are_fetched_when_initials_are_chosen() {
    let mut app = app();
    app.settings.avatars = crate::config::AvatarStyle::Initials;
    update(
        &mut app,
        Event::Api(ApiEvent::Users(vec![user_with_picture("U2", "camille")])),
    );
    update(
        &mut app,
        Event::Api(ApiEvent::Conversations(vec![conversation("C1", "general")])),
    );
    let commands = update(
        &mut app,
        Event::Api(ApiEvent::History {
            channel: "C1".into(),
            messages: vec![message("1.0", "U2", "hi")],
            has_more: false,
        }),
    );
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, Command::LoadAvatars(_)))
    );
}

#[test]
fn the_avatar_setting_is_the_last_row() {
    let mut app = loaded_app();
    update(&mut app, key(KeyCode::Char(',')));
    for _ in 0..3 {
        update(&mut app, key(KeyCode::Char('j')));
    }
    update(&mut app, key(KeyCode::Char('l')));
    assert_eq!(app.settings.avatars, crate::config::AvatarStyle::Initials);
    assert_eq!(app.settings.layout, Layout::Panels);
}

fn video_file() -> crate::slack::File {
    serde_json::from_value(serde_json::json!({
        "id": "F1",
        "name": "demo.mp4",
        "mimetype": "video/mp4",
        "size": 10747897,
        "duration_ms": 19686,
        "thumb_video": "https://files.slack.com/thumb.jpg",
        "url_private_download": "https://files.slack.com/demo.mp4"
    }))
    .unwrap()
}

/// `loaded_app` whose latest message carries a video.
fn app_with_video(graphics: bool) -> App {
    let mut app = app();
    app.graphics = graphics;
    update(
        &mut app,
        Event::Api(ApiEvent::Conversations(vec![conversation("C1", "general")])),
    );
    let mut with_video = message("2.0", "U2", "regardez");
    with_video.files.push(video_file());
    update(
        &mut app,
        Event::Api(ApiEvent::History {
            channel: "C1".into(),
            messages: vec![message("1.0", "U2", "hi"), with_video],
            has_more: false,
        }),
    );
    app
}

#[test]
fn previews_are_requested_once_when_the_terminal_draws_pictures() {
    let mut app = app();
    app.graphics = true;
    update(
        &mut app,
        Event::Api(ApiEvent::Conversations(vec![conversation("C1", "general")])),
    );
    let mut with_video = message("2.0", "U2", "regardez");
    with_video.files.push(video_file());
    let commands = update(
        &mut app,
        Event::Api(ApiEvent::History {
            channel: "C1".into(),
            messages: vec![with_video],
            has_more: false,
        }),
    );
    assert!(commands.contains(&Command::LoadPreviews(vec![(
        "file:F1".into(),
        "https://files.slack.com/thumb.jpg".into()
    )])));

    let commands = update(&mut app, key(KeyCode::Char('j')));
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, Command::LoadPreviews(_)))
    );
}

#[test]
fn no_previews_without_graphics() {
    let mut app = app_with_video(false);
    let commands = update(&mut app, key(KeyCode::Char('j')));
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c, Command::LoadPreviews(_)))
    );
    assert!(app.pictures_requested.is_empty());
}

#[test]
fn o_downloads_and_opens_the_files_of_the_message() {
    let mut app = app_with_video(false);
    app.focus = Focus::Messages;
    let commands = update(&mut app, key(KeyCode::Char('o')));
    assert_eq!(
        commands,
        [Command::OpenFiles(vec![FileToOpen {
            id: "F1".into(),
            name: "demo.mp4".into(),
            url: "https://files.slack.com/demo.mp4".into(),
            video: true,
        }])]
    );
}

#[test]
fn o_on_a_message_without_files_says_so() {
    let mut app = app_with_video(false);
    app.focus = Focus::Messages;
    update(&mut app, key(KeyCode::Char('k')));
    update(&mut app, key(KeyCode::Char('k')));
    assert!(update(&mut app, key(KeyCode::Char('o'))).is_empty());
    assert!(
        app.notice
            .as_deref()
            .is_some_and(|n| n.contains("nothing to open"))
    );
}

fn app_with_message(build: impl FnOnce(&mut crate::slack::Message)) -> App {
    let mut app = app();
    app.graphics = true;
    app.focus = Focus::Messages;
    update(
        &mut app,
        Event::Api(ApiEvent::Conversations(vec![conversation("C1", "general")])),
    );
    let mut msg = message("1.0", "U2", "regardez <https://youtu.be/dQw4w9WgXcQ>");
    build(&mut msg);
    update(
        &mut app,
        Event::Api(ApiEvent::History {
            channel: "C1".into(),
            messages: vec![msg],
            has_more: false,
        }),
    );
    app
}

#[test]
fn youtube_thumbnails_are_requested() {
    let mut app = app();
    app.graphics = true;
    update(
        &mut app,
        Event::Api(ApiEvent::Conversations(vec![conversation("C1", "general")])),
    );
    let commands = update(
        &mut app,
        Event::Api(ApiEvent::History {
            channel: "C1".into(),
            messages: vec![message("1.0", "U2", "<https://youtu.be/dQw4w9WgXcQ>")],
            has_more: false,
        }),
    );
    assert!(commands.contains(&Command::LoadPreviews(vec![(
        "link:youtube:dQw4w9WgXcQ".into(),
        "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg".into()
    )])));
}

#[test]
fn p_plays_a_youtube_link_in_the_terminal() {
    let mut app = app_with_message(|_| {});
    let commands = update(&mut app, key(KeyCode::Char('p')));
    assert_eq!(
        commands,
        [Command::Play("https://youtu.be/dQw4w9WgXcQ".into())]
    );
}

#[test]
fn p_downloads_a_video_file_then_plays_it() {
    let mut app = app_with_message(|msg| msg.files.push(video_file()));
    let commands = update(&mut app, key(KeyCode::Char('p')));
    assert!(matches!(&commands[..], [Command::DownloadToPlay(file)] if file.id == "F1"));

    let commands = update(
        &mut app,
        Event::Api(ApiEvent::PlayReady("/cache/F1-demo.mp4".into())),
    );
    assert_eq!(commands, [Command::Play("/cache/F1-demo.mp4".into())]);
}

#[test]
fn p_without_a_video_says_so() {
    let mut app = app_with_message(|msg| msg.text = "no video here".into());
    assert!(update(&mut app, key(KeyCode::Char('p'))).is_empty());
    assert!(
        app.notice
            .as_deref()
            .is_some_and(|n| n.contains("no video"))
    );
}

#[test]
fn o_opens_the_first_link_when_there_is_no_file() {
    let mut app = app_with_message(|_| {});
    let commands = update(&mut app, key(KeyCode::Char('o')));
    assert_eq!(
        commands,
        [Command::OpenUrl("https://youtu.be/dQw4w9WgXcQ".into())]
    );
}

#[test]
fn an_expired_session_says_how_to_sign_in_again() {
    let mut app = loaded_app();
    update(&mut app, key(KeyCode::Char('/')));
    for c in "deploy".chars() {
        update(&mut app, key(KeyCode::Char(c)));
    }
    update(&mut app, key(KeyCode::Enter));

    update(&mut app, Event::Api(ApiEvent::SessionExpired));

    assert!(matches!(app.connection, Connection::SessionExpired));
    assert!(matches!(
        &app.overlay,
        Some(Overlay::Search(search)) if matches!(search.status, SearchStatus::Failed(_))
    ));
    update(&mut app, Event::Rtm(RtmEvent::SessionExpired));
    assert!(matches!(app.connection, Connection::SessionExpired));
}

#[test]
fn the_switcher_lists_public_channels_once() {
    let mut app = loaded_app();
    let commands = update(&mut app, ctrl('k'));
    assert!(commands.contains(&Command::LoadPublicChannels));
    update(&mut app, key(KeyCode::Esc));
    let commands = update(&mut app, ctrl('k'));
    assert!(!commands.contains(&Command::LoadPublicChannels));
}

#[test]
fn writes_to_someone_new_from_the_switcher() {
    let mut app = loaded_app();
    app.users.insert("U3".into(), user("U3", "sarah"));
    update(&mut app, ctrl('k'));
    typed(&mut app, "sarah");

    let commands = update(&mut app, key(KeyCode::Enter));

    assert_eq!(commands, [Command::OpenDirect("U3".into())]);
    let commands = update(&mut app, Event::Api(ApiEvent::Opened(direct("D3", "U3"))));
    assert_eq!(app.current.as_deref(), Some("D3"));
    assert_eq!(app.focus, Focus::Messages);
    assert!(app.channel("D3").is_some_and(|c| c.listed));
    assert_eq!(commands, [Command::LoadHistory("D3".into())]);
}

#[test]
fn joins_a_public_channel_from_the_switcher() {
    let mut app = loaded_app();
    update(&mut app, ctrl('k'));
    update(
        &mut app,
        Event::Api(ApiEvent::PublicChannels(vec![
            conversation("C1", "general"),
            conversation("C9", "support"),
        ])),
    );
    typed(&mut app, "support");

    let commands = update(&mut app, key(KeyCode::Enter));

    assert_eq!(commands, [Command::Join("C9".into())]);
    update(
        &mut app,
        Event::Api(ApiEvent::Opened(conversation("C9", "support"))),
    );
    assert_eq!(app.current.as_deref(), Some("C9"));
}

/// Writing in #general, where Hugo and Hélène can be mentioned.
fn writing_app() -> App {
    let mut app = loaded_app();
    app.users.insert("U2".into(), user("U2", "hugo"));
    app.users.insert("U3".into(), user("U3", "helene"));
    update(&mut app, key(KeyCode::Char('2')));
    update(&mut app, key(KeyCode::Char('i')));
    app
}

#[test]
fn completes_a_mention_and_sends_it_as_one() {
    let mut app = writing_app();
    typed(&mut app, "hi @hu");
    assert!(app.completion.is_some());

    update(&mut app, key(KeyCode::Tab));
    assert_eq!(app.input.text(), "hi @hugo ");
    assert!(app.completion.is_none());
    typed(&mut app, "& co");
    let commands = update(&mut app, key(KeyCode::Enter));

    assert_eq!(
        commands,
        [Command::Send {
            channel: "C1".into(),
            text: "hi <@U2> &amp; co".into(),
            thread_ts: None
        }]
    );
}

#[test]
fn completes_an_emoji_after_a_colon() {
    let mut app = writing_app();
    typed(&mut app, "shipped :t");
    assert!(app.completion.is_none());
    typed(&mut app, "ada");
    update(&mut app, key(KeyCode::Tab));
    assert_eq!(app.input.text(), "shipped :tada: ");

    let commands = update(&mut app, key(KeyCode::Enter));
    assert_eq!(
        commands,
        [Command::Send {
            channel: "C1".into(),
            text: "shipped :tada:".into(),
            thread_ts: None
        }]
    );
}

#[test]
fn enter_picks_a_suggestion_rather_than_sending() {
    let mut app = writing_app();
    typed(&mut app, "@h");
    update(&mut app, key(KeyCode::Down));
    let commands = update(&mut app, key(KeyCode::Enter));
    assert!(commands.is_empty());
    assert_eq!(app.input.text(), "@hugo ");
}

#[test]
fn esc_closes_the_suggestions_then_stops_writing() {
    let mut app = writing_app();
    typed(&mut app, "@hu");
    update(&mut app, key(KeyCode::Esc));
    assert!(app.completion.is_none());
    assert_eq!(app.mode, Mode::Insert);
    typed(&mut app, "g");
    assert!(app.completion.is_none(), "closed for this name");

    update(&mut app, key(KeyCode::Esc));
    assert_eq!(app.mode, Mode::Normal);
}

/// #general open with one of the user's own messages last.
fn app_with_my_message() -> App {
    let mut app = loaded_app();
    app.users.insert("U2".into(), user("U2", "hugo"));
    update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "C1".into(),
            message: message("3.0", "UME", "thanks <@U2> &amp; co"),
        }),
    );
    update(&mut app, key(KeyCode::Char('2')));
    app
}

#[test]
fn edits_one_of_my_messages() {
    let mut app = app_with_my_message();

    update(&mut app, key(KeyCode::Char('e')));
    assert_eq!(app.mode, Mode::Insert);
    assert_eq!(app.input.text(), "thanks @hugo & co");
    typed(&mut app, "!");
    let commands = update(&mut app, key(KeyCode::Enter));

    assert_eq!(
        commands,
        [Command::Edit {
            channel: "C1".into(),
            ts: "3.0".into(),
            text: "thanks <@U2> &amp; co!".into()
        }]
    );
    assert!(app.editing.is_none());
    update(
        &mut app,
        Event::Api(ApiEvent::Edited {
            channel: "C1".into(),
            ts: "3.0".into(),
            text: "thanks <@U2> &amp; co!".into(),
        }),
    );
    let edited = app.find_message("C1", "3.0").unwrap();
    assert!(edited.text.ends_with("co!") && edited.is_edited());
}

#[test]
fn cancelling_an_edit_gives_the_draft_back() {
    let mut app = app_with_my_message();
    update(&mut app, key(KeyCode::Char('i')));
    typed(&mut app, "half a thought");
    update(&mut app, key(KeyCode::Esc));

    update(&mut app, key(KeyCode::Char('e')));
    assert_eq!(app.input.text(), "thanks @hugo & co");
    update(&mut app, key(KeyCode::Esc));

    assert!(app.editing.is_none());
    assert_eq!(app.input.text(), "half a thought");
}

#[test]
fn only_my_own_messages_change() {
    let mut app = loaded_app();
    update(&mut app, key(KeyCode::Char('2')));
    update(&mut app, key(KeyCode::Char('e')));
    assert!(app.editing.is_none());
    assert!(
        app.notice
            .as_deref()
            .is_some_and(|n| n.contains("your own"))
    );
    update(&mut app, key(KeyCode::Char('d')));
    assert!(app.overlay.is_none());
}

#[test]
fn deletes_after_asking() {
    let mut app = app_with_my_message();
    update(&mut app, key(KeyCode::Char('d')));
    assert!(matches!(app.overlay, Some(Overlay::DeleteMessage(_))));
    assert!(update(&mut app, key(KeyCode::Char('n'))).is_empty());
    assert!(app.overlay.is_none());

    update(&mut app, key(KeyCode::Char('d')));
    let commands = update(&mut app, key(KeyCode::Char('y')));
    assert_eq!(
        commands,
        [Command::Delete {
            channel: "C1".into(),
            ts: "3.0".into()
        }]
    );
    update(
        &mut app,
        Event::Api(ApiEvent::Deleted {
            channel: "C1".into(),
            ts: "3.0".into(),
        }),
    );
    assert!(app.find_message("C1", "3.0").is_none());
}

#[test]
fn muted_conversations_only_count_mentions() {
    let mut app = loaded_app();
    update(
        &mut app,
        Event::Api(ApiEvent::Muted(["C2".to_string()].into())),
    );
    let random = |app: &App| {
        let c = app.channel("C2").unwrap();
        (c.muted, c.unread, c.mentions)
    };
    assert_eq!(random(&app), (true, false, 0));

    update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "C2".into(),
            message: message("5.0", "U2", "lunch?"),
        }),
    );
    assert_eq!(random(&app), (true, false, 0));

    update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "C2".into(),
            message: message("6.0", "U2", "<@UME> you coming?"),
        }),
    );
    assert_eq!(random(&app), (true, true, 1));
}

fn go_to(app: &mut App, name: &str) {
    update(app, ctrl('k'));
    typed(app, name);
    update(app, key(KeyCode::Enter));
}

fn write(app: &mut App, text: &str) {
    update(app, key(KeyCode::Char('i')));
    typed(app, text);
    update(app, key(KeyCode::Esc));
}

#[test]
fn each_conversation_keeps_its_draft() {
    let mut app = loaded_app();
    update(&mut app, key(KeyCode::Char('2')));
    write(&mut app, "for general");

    go_to(&mut app, "random");
    assert_eq!(app.current.as_deref(), Some("C2"));
    assert_eq!(app.input.text(), "");
    write(&mut app, "for random");
    go_to(&mut app, "general");

    assert_eq!(app.input.text(), "for general");
    assert_eq!(app.draft("C2", None), Some("for random"));
}

#[test]
fn a_thread_keeps_its_own_draft() {
    let mut app = loaded_app();
    update(&mut app, key(KeyCode::Char('2')));
    write(&mut app, "in the channel");
    update(&mut app, key(KeyCode::Char('t')));
    assert_eq!(app.focus, Focus::Thread);
    assert_eq!(app.input.text(), "");
    write(&mut app, "in the thread");

    update(&mut app, key(KeyCode::Char('2')));
    assert_eq!(app.input.text(), "in the channel");
    update(&mut app, key(KeyCode::Char('3')));
    assert_eq!(app.input.text(), "in the thread");
}

#[test]
fn a_sent_message_leaves_no_draft() {
    let mut app = loaded_app();
    update(&mut app, key(KeyCode::Char('2')));
    update(&mut app, key(KeyCode::Char('i')));
    typed(&mut app, "hello");
    update(&mut app, key(KeyCode::Enter));
    update(&mut app, key(KeyCode::Esc));

    go_to(&mut app, "random");
    go_to(&mut app, "general");
    assert_eq!(app.input.text(), "");
    assert!(app.draft("C1", None).is_none());
}

use crate::tui::state::WorkspaceTab;

/// Signed in to acme, shown, and to side, in the background.
fn with_two_workspaces(app: &mut App) {
    let tab = |name: &str, mentions| WorkspaceTab {
        name: name.into(),
        unread: mentions > 0,
        mentions,
    };
    app.workspaces = vec![tab("acme", 0), tab("side", 2)];
    app.workspace = 0;
}

#[test]
fn a_workspace_in_the_background_keeps_what_arrives_unread() {
    let mut app = loaded_app();
    app.visible = false;

    let commands = update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "C1".into(),
            message: message("9.0", "U2", "still there?"),
        }),
    );

    assert!(commands.is_empty(), "nothing marked read: {commands:?}");
    assert!(app.channel("C1").unwrap().unread);
    let commands = on_shown(&mut app);
    assert!(app.visible);
    assert!(!app.channel("C1").unwrap().unread);
    assert_eq!(
        commands,
        [Command::MarkRead {
            channel: "C1".into(),
            ts: "9.0".into()
        }]
    );
}

#[test]
fn w_goes_to_the_next_workspace_when_there_is_one() {
    let mut app = loaded_app();
    assert!(update(&mut app, key(KeyCode::Char('w'))).is_empty());
    with_two_workspaces(&mut app);
    assert_eq!(
        update(&mut app, key(KeyCode::Char('w'))),
        [Command::NextWorkspace]
    );
}

#[test]
fn the_switcher_goes_to_another_workspace() {
    let mut app = loaded_app();
    with_two_workspaces(&mut app);
    update(&mut app, ctrl('k'));
    typed(&mut app, "side");
    let commands = update(&mut app, key(KeyCode::Enter));
    assert!(
        commands.contains(&Command::SwitchWorkspace(1)),
        "{commands:?}"
    );
}

fn receive(app: &mut App, channel: &str, ts: &str) {
    update(
        app,
        Event::Rtm(RtmEvent::Message {
            channel: channel.into(),
            message: message(ts, "U2", "news"),
        }),
    );
}

#[test]
fn n_goes_through_unread_conversations_in_sidebar_order() {
    let mut app = loaded_app();
    receive(&mut app, "D1", "7.0");
    receive(&mut app, "C2", "8.0");

    update(&mut app, key(KeyCode::Char('n')));
    assert_eq!(app.current.as_deref(), Some("C2"));
    update(&mut app, key(KeyCode::Char('n')));
    assert_eq!(app.current.as_deref(), Some("D1"));

    let commands = update(&mut app, key(KeyCode::Char('n')));
    assert!(commands.is_empty());
    assert_eq!(app.notice.as_deref(), Some("nothing unread"));
}

#[test]
fn n_moves_on_to_another_workspace_once_this_one_is_read() {
    let mut app = loaded_app();
    with_two_workspaces(&mut app);
    assert_eq!(
        update(&mut app, key(KeyCode::Char('n'))),
        [Command::NextUnreadWorkspace]
    );
}

#[test]
fn y_copies_the_message_and_shift_y_its_link() {
    let mut app = loaded_app();
    app.team_url = "https://acme.slack.com/".into();
    app.users.insert("U2".into(), user("U2", "hugo"));
    update(
        &mut app,
        Event::Rtm(RtmEvent::Message {
            channel: "C1".into(),
            message: message("9.000100", "U2", "ask <@U2> &amp; co"),
        }),
    );
    update(&mut app, key(KeyCode::Char('2')));

    assert_eq!(
        update(&mut app, key(KeyCode::Char('y'))),
        [Command::Copy {
            text: "ask @hugo & co".into(),
            what: "message"
        }]
    );
    assert_eq!(
        update(&mut app, key(KeyCode::Char('Y'))),
        [Command::Copy {
            text: "https://acme.slack.com/archives/C1/p9000100".into(),
            what: "link"
        }]
    );
}

#[test]
fn a_reply_links_to_its_thread() {
    assert_eq!(
        permalink(
            "https://acme.slack.com/",
            "C1",
            "2.000200",
            Some("1.000100")
        ),
        "https://acme.slack.com/archives/C1/p2000200?thread_ts=1.000100&cid=C1"
    );
}

fn arrives(app: &mut App, channel: &str, message: Message) -> Vec<Command> {
    update(
        app,
        Event::Rtm(RtmEvent::Message {
            channel: channel.into(),
            message,
        }),
    )
}

fn notifications(commands: &[Command]) -> Vec<(String, String)> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::Notify { title, body } => Some((title.clone(), body.clone())),
            _ => None,
        })
        .collect()
}

fn notified(title: &str, body: &str) -> Vec<(String, String)> {
    vec![(title.to_string(), body.to_string())]
}

/// #general is on screen; Camille writes in a direct conversation.
fn app_with_camille() -> App {
    let mut app = loaded_app();
    update(
        &mut app,
        Event::Api(ApiEvent::Users(vec![
            user("U2", "camille"),
            user("UME", "simon"),
        ])),
    );
    app
}

#[test]
fn direct_messages_and_mentions_notify() {
    let mut app = app_with_camille();
    let commands = arrives(&mut app, "D1", message("3.0", "U2", "lunch?"));
    assert_eq!(notifications(&commands), notified("camille", "lunch?"));

    let commands = arrives(&mut app, "C2", message("4.0", "U2", "<@UME> can you look?"));
    assert_eq!(
        notifications(&commands),
        notified("camille in #random", "@simon can you look?")
    );

    // Other messages in channels only mark them unread.
    let commands = arrives(&mut app, "C2", message("5.0", "U2", "deploying now"));
    assert_eq!(notifications(&commands), []);
}

#[test]
fn what_the_user_reads_does_not_notify() {
    let mut app = app_with_camille();
    let commands = arrives(&mut app, "C1", message("3.0", "U2", "<@UME> look"));
    assert_eq!(notifications(&commands), []);

    // In another window, or another workspace, the user does not see it.
    app.terminal_focused = false;
    let commands = arrives(&mut app, "C1", message("4.0", "U2", "<@UME> look"));
    assert_eq!(notifications(&commands).len(), 1);
    app.terminal_focused = true;
    app.visible = false;
    let commands = arrives(&mut app, "C1", message("5.0", "U2", "<@UME> look"));
    assert_eq!(notifications(&commands).len(), 1);
}

#[test]
fn thread_replies_notify_unless_the_thread_is_open() {
    let mut app = app_with_camille();
    let mut reply = message("3.0", "U2", "<@UME> what do you think?");
    reply.thread_ts = Some("2.0".into());
    let commands = arrives(&mut app, "C1", reply.clone());
    assert_eq!(notifications(&commands).len(), 1);

    app.focus = Focus::Messages;
    update(&mut app, key(KeyCode::Char('t')));
    assert!(app.thread.as_ref().is_some_and(|t| t.ts == "2.0"));
    reply.ts = "4.0".into();
    let commands = arrives(&mut app, "C1", reply);
    assert_eq!(notifications(&commands), []);
}

#[test]
fn quiet_for_own_messages_muted_conversations_and_when_off() {
    let mut app = app_with_camille();
    let commands = arrives(&mut app, "D1", message("3.0", "UME", "sent from my phone"));
    assert_eq!(notifications(&commands), []);

    update(
        &mut app,
        Event::Api(ApiEvent::Muted(["D1".to_string(), "C2".to_string()].into())),
    );
    let commands = arrives(&mut app, "D1", message("4.0", "U2", "ping"));
    assert_eq!(notifications(&commands), []);
    // A mention gets through, as in Slack.
    let commands = arrives(&mut app, "C2", message("5.0", "U2", "<@UME> ping"));
    assert_eq!(notifications(&commands).len(), 1);

    app.settings.notifications = crate::config::Notifications::Off;
    let commands = arrives(&mut app, "C2", message("6.0", "U2", "<@UME> ping"));
    assert_eq!(notifications(&commands), []);
}

#[test]
fn notifications_name_the_workspace_when_there_are_several() {
    let mut app = app_with_camille();
    app.workspaces = vec![app.tab(), app.tab()];
    let long = "word ".repeat(100);
    let commands = arrives(&mut app, "D9", message("3.0", "U2", &long));
    let [(title, body)] = notifications(&commands).try_into().unwrap();
    assert_eq!(title, "camille · acme");
    assert_eq!(body.chars().count(), 201);
    assert!(body.ends_with('…'));
}

fn dropped(dir: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, "content").unwrap();
    path
}

#[test]
fn a_dropped_file_goes_with_the_next_message() {
    let dir = tempfile::tempdir().unwrap();
    let report = dropped(&dir, "Q3 report.pdf");
    let mut app = loaded_app();

    // Dropped from the sidebar, in normal mode: it opens the message box.
    update(&mut app, Event::Paste(format!("'{}'", report.display())));
    assert_eq!(app.mode, Mode::Insert);
    assert_eq!(app.focus, Focus::Messages);
    assert_eq!(app.input.text(), "");
    assert_eq!(app.attached()[0].name, "Q3 report.pdf");
    assert_eq!(app.attached()[0].size, 7);

    typed(&mut app, "numbers");
    let commands = update(&mut app, key(KeyCode::Enter));
    assert_eq!(
        commands,
        [Command::Upload {
            channel: "C1".into(),
            text: "numbers".into(),
            thread_ts: None,
            files: vec![report],
        }]
    );
    assert!(app.attached().is_empty());
}

#[test]
fn backspace_takes_back_a_file_and_text_stays_text() {
    let dir = tempfile::tempdir().unwrap();
    let shot = dropped(&dir, "shot.png");
    let mut app = writing_app();
    update(&mut app, Event::Paste(shot.display().to_string()));
    update(&mut app, Event::Paste("shot.png is attached".into()));
    assert_eq!(app.input.text(), "shot.png is attached");
    assert_eq!(app.attached().len(), 1);

    update(&mut app, ctrl('u'));
    update(&mut app, key(KeyCode::Backspace));
    assert!(app.attached().is_empty());
    assert_eq!(update(&mut app, key(KeyCode::Enter)), []);
}

#[test]
fn each_conversation_keeps_its_files() {
    let dir = tempfile::tempdir().unwrap();
    let shot = dropped(&dir, "shot.png");
    let mut app = writing_app();
    update(&mut app, Event::Paste(shot.display().to_string()));
    update(&mut app, key(KeyCode::Esc));

    go_to(&mut app, "random");
    update(&mut app, key(KeyCode::Char('i')));
    assert!(app.attached().is_empty());
    update(&mut app, key(KeyCode::Esc));

    go_to(&mut app, "general");
    assert_eq!(app.attached().len(), 1);
}

#[test]
fn a_path_pasted_while_editing_is_text() {
    let dir = tempfile::tempdir().unwrap();
    let shot = dropped(&dir, "shot.png");
    let mut app = writing_app();
    typed(&mut app, "mine");
    update(&mut app, key(KeyCode::Enter));
    update(
        &mut app,
        Event::Api(ApiEvent::Sent {
            channel: "C1".into(),
            message: message("5.0", "UME", "mine"),
        }),
    );
    update(&mut app, key(KeyCode::Esc));
    update(&mut app, key(KeyCode::Char('e')));
    assert!(app.editing.is_some());
    update(&mut app, Event::Paste(shot.display().to_string()));
    assert!(app.input.text().ends_with("shot.png"));
    assert!(app.attached().is_empty());
}

fn marked(channel: &str, ts: &str, unread: Option<u64>, mentions: Option<u64>) -> Event {
    Event::Rtm(RtmEvent::Marked {
        channel: channel.into(),
        ts: ts.into(),
        unread,
        mentions,
    })
}

#[test]
fn what_is_read_elsewhere_is_read_here() {
    let mut app = loaded_app();
    arrives(&mut app, "D1", message("3.0", "U2", "lunch?"));
    arrives(&mut app, "D1", message("4.0", "U2", "at noon"));
    let state = |app: &App| {
        let c = app.channel("D1").unwrap();
        (c.unread, c.mentions)
    };
    assert_eq!(state(&app), (true, 2));

    // Read on the phone up to the first one: Slack says one is left.
    update(&mut app, marked("D1", "3.0", Some(1), Some(1)));
    assert_eq!(state(&app), (true, 1));
    update(&mut app, marked("D1", "4.0", Some(0), Some(0)));
    assert_eq!(state(&app), (false, 0));

    // Without counts, the timestamp tells.
    arrives(&mut app, "D1", message("5.0", "U2", "?"));
    update(&mut app, marked("D1", "4.0", None, None));
    assert!(state(&app).0);
    update(&mut app, marked("D1", "5.0", None, None));
    assert_eq!(state(&app), (false, 0));
}

#[test]
fn changes_made_elsewhere_reload_the_list_and_mutes() {
    let mut app = loaded_app();
    assert_eq!(
        update(&mut app, Event::Rtm(RtmEvent::ConversationsChanged)),
        [Command::LoadConversations]
    );
    assert_eq!(
        update(&mut app, Event::Rtm(RtmEvent::MutedChanged)),
        [Command::LoadMuted]
    );
}

fn reply_to(parent: &str, ts: &str, user: &str, text: &str) -> Message {
    let mut reply = message(ts, user, text);
    reply.thread_ts = Some(parent.into());
    reply
}

#[test]
fn replies_in_the_users_threads_notify() {
    let mut app = app_with_camille();
    // A thread the user started.
    let mut reply = reply_to("1.0", "3.0", "U2", "done");
    reply.parent_user_id = Some("UME".into());
    let commands = arrives(&mut app, "C1", reply);
    assert_eq!(
        notifications(&commands),
        notified("camille replied in #general", "done")
    );
    assert!(app.unread_threads.contains("C1/1.0"));

    // One the user replied in.
    app.histories.get_mut("C1").unwrap().messages[1].reply_users = vec!["UME".into()];
    let commands = arrives(&mut app, "C1", reply_to("2.0", "4.0", "U2", "and?"));
    assert_eq!(notifications(&commands).len(), 1);

    // Someone else's.
    app.histories.get_mut("C1").unwrap().messages[1].reply_users = vec!["U3".into()];
    let commands = arrives(&mut app, "C1", reply_to("2.0", "5.0", "U2", "ok"));
    assert_eq!(notifications(&commands), []);

    // The user's own replies are nothing new.
    app.unread_threads.clear();
    let mut mine = reply_to("1.0", "6.0", "UME", "thanks");
    mine.parent_user_id = Some("UME".into());
    arrives(&mut app, "C1", mine);
    assert!(app.unread_threads.is_empty());
}

#[test]
fn opening_a_conversation_marks_where_the_new_messages_start() {
    let mut app = loaded_app();
    update(
        &mut app,
        Event::Api(ApiEvent::Counts(vec![
            serde_json::from_value(serde_json::json!({
                "id": "C2", "has_unreads": true, "latest": "6.0", "last_read": "4.0"
            }))
            .unwrap(),
        ])),
    );
    go_to(&mut app, "random");
    assert_eq!(app.new_since(), Some("4.0"));

    // Read up to the end now: nothing is new the next time.
    go_to(&mut app, "general");
    assert_eq!(app.new_since(), None);
    go_to(&mut app, "random");
    assert_eq!(app.new_since(), None);

    // What arrives while the user reads is read too.
    arrives(&mut app, "C2", message("7.0", "U2", "still there?"));
    go_to(&mut app, "general");
    arrives(&mut app, "C2", message("8.0", "U2", "hello?"));
    go_to(&mut app, "random");
    assert_eq!(app.new_since(), Some("7.0"));
}

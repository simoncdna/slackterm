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
                message("1.0", "U2", "salut"),
                message("2.0", "U2", "ça va ?"),
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
            messages: vec![message("1.0", "U2", "salut")],
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
            message: message("3.0", "U3", "salut"),
        }),
    );
    assert_eq!(commands, [Command::LoadConversations]);

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
    let mut edited = message("1.0", "U2", "salut à tous");
    edited.edited = Some(serde_json::json!({}));
    update(
        &mut app,
        Event::Rtm(RtmEvent::MessageChanged {
            channel: "C1".into(),
            message: edited,
        }),
    );
    assert_eq!(app.current_messages()[0].text, "salut à tous");

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
    let mut reply = message("3.0", "U2", "réponse");
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
    update(&mut app, key(KeyCode::Char(',')));
    update(&mut app, key(KeyCode::Char('l')));
    assert_eq!(app.settings.layout, Layout::Stream);

    update(&mut app, key(KeyCode::Char('j')));
    update(&mut app, key(KeyCode::Char('l')));
    let commands = update(&mut app, key(KeyCode::Esc));

    assert!(app.overlay.is_none());
    assert_eq!(commands, [Command::SaveSettings(app.settings)]);
    assert_ne!(app.settings.theme, Settings::default().theme);
}

fn count(id: &str, unread: bool, latest: &str) -> ConversationCount {
    ConversationCount {
        id: id.into(),
        has_unreads: unread,
        mention_count: 0,
        latest: Some(latest.into()),
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
            message: message("9.0", "U3", "salut"),
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
            section("S4", "salesforce_records", "Canaux Salesforce", &[], None),
        ])),
    );
    app
}

#[test]
fn custom_sections_hold_their_channels() {
    let app = app_with_sections();
    let names: Vec<String> = app.sections.iter().map(|s| s.name.clone()).collect();
    assert_eq!(names, ["Tooling", "Canaux", "Messages directs"]);
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
        matches!(&commands[..], [Command::SaveSidebar(state)] if state.collapsed.contains("S1"))
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
        "text": "le \u{e000}deploy\u{e001} est passé",
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
            message: message("9.0", "U3", "salut"),
        }),
    );

    let commands = update(&mut app, key(KeyCode::Right));
    assert_eq!(app.sidebar_tab, SidebarTab::Direct);
    assert!(
        matches!(&commands[..], [Command::SaveSidebar(state)] if state.tab == SidebarTab::Direct)
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

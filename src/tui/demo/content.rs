//! The story the demo tells: a small team getting version 2.0 of its app
//! out of the door. Times are relative to launch, so it always looks fresh.

use std::collections::HashMap;
use std::time::Duration;

use chrono::{Local, TimeZone};
use serde_json::{Value, json};

use crate::slack::{ChannelSection, Conversation, ConversationCount, File, Message, User};

pub const ME: &str = "U0ME";
const CAMILLE: &str = "U01CAM";
const HUGO: &str = "U02HUG";
const INES: &str = "U03INE";
const NATHAN: &str = "U04NAT";
const SARAH: &str = "U05SAR";

const GENERAL: &str = "C01GEN";
const RELEASE: &str = "C02REL";
const DEPLOYS: &str = "C03DEP";
const DEV: &str = "C04DEV";
const DESIGN: &str = "C05DES";
const RANDOM: &str = "C06RAN";
const PRODUCT: &str = "G07PRO";
const DM_CAMILLE: &str = "D01CAM";
const DM_HUGO: &str = "D02HUG";
const DM_INES: &str = "D03INE";
const GROUP: &str = "G08MPD";
/// Public channels the user has not joined, to show joining.
const MARKETING: &str = "C09MKT";
const SUPPORT: &str = "C10SUP";

const DEPLOY_BOT: &str = "deploy-bot";

pub struct Content {
    pub users: Vec<User>,
    pub conversations: Vec<Conversation>,
    /// Every public channel, joined or not.
    pub public_channels: Vec<Conversation>,
    pub sections: Vec<ChannelSection>,
    pub counts: Vec<ConversationCount>,
    /// Every message, thread replies included, oldest first.
    pub messages: HashMap<String, Vec<Message>>,
    /// Messages that arrive on their own once the demo is open.
    pub live: Vec<Scheduled>,
}

pub struct Scheduled {
    pub after: Duration,
    pub channel: String,
    /// Its timestamp is set when it is posted.
    pub message: Message,
}

/// What the other member of a direct message answers to the user.
pub fn reply_from(user: &str) -> &'static str {
    match user {
        CAMILLE => "Great, thanks! Let me know if anything's missing :raised_hands:",
        HUGO => "Perfect, I'll call you in 5 :telephone_receiver:",
        INES => "Awesome, sending you the files :art:",
        _ => "Got it :+1:",
    }
}

/// Someone to react when nobody else spoke in the conversation.
pub const FALLBACK_REACTOR: &str = CAMILLE;

pub fn build() -> Content {
    let mut b = Builder::new();
    general(&mut b);
    release(&mut b);
    deploys(&mut b);
    dev(&mut b);
    design(&mut b);
    random(&mut b);
    product(&mut b);
    direct(&mut b);
    let live = live(&mut b);

    let mut messages = b.messages;
    for list in messages.values_mut() {
        list.sort_by(|a, b| a.ts.cmp(&b.ts));
    }
    let conversations = conversations();
    let unread = [(RELEASE, 1), (DESIGN, 0), (DM_CAMILLE, 2)];
    let counts = counts(&conversations, &messages, &unread);
    let public_channels = public_channels(&conversations);
    Content {
        users: users(),
        conversations,
        public_channels,
        sections: sections(),
        counts,
        messages,
        live,
    }
}

fn users() -> Vec<User> {
    [
        (ME, "alex", "Alex Morel"),
        (CAMILLE, "camille", "Camille Laurent"),
        (HUGO, "hugo", "Hugo Bernard"),
        (INES, "ines", "Inès Lefèvre"),
        (NATHAN, "nathan", "Nathan Roux"),
        (SARAH, "sarah", "Sarah Dubois"),
    ]
    .into_iter()
    .map(|(id, name, real_name)| {
        let first = real_name.split(' ').next().unwrap_or(name);
        from_json(json!({
            "id": id,
            "name": name,
            "real_name": real_name,
            "profile": {
                "display_name": first,
                "real_name": real_name,
                "image_72": format!("https://avatars.atelier-nova.fr/{name}.jpg"),
            },
        }))
    })
    .collect()
}

/// A member's profile picture, as a JPEG file. The faces were generated
/// (thispersondoesnotexist.com): nobody real is pictured.
pub fn avatar(user: &str) -> Option<&'static [u8]> {
    let picture: &[u8] = match user {
        ME => include_bytes!("avatars/alex.jpg"),
        CAMILLE => include_bytes!("avatars/camille.jpg"),
        HUGO => include_bytes!("avatars/hugo.jpg"),
        INES => include_bytes!("avatars/ines.jpg"),
        NATHAN => include_bytes!("avatars/nathan.jpg"),
        SARAH => include_bytes!("avatars/sarah.jpg"),
        _ => return None,
    };
    Some(picture)
}

fn conversations() -> Vec<Conversation> {
    let channel = |id: &str, name: &str, topic: &str| json!({"id": id, "name": name, "topic": {"value": topic}});
    let direct = |id: &str, user: &str| json!({"id": id, "is_im": true, "user": user});
    [
        channel(GENERAL, "general", "Team life and announcements 📣"),
        channel(
            RELEASE,
            "release-2-0",
            "v2.0 launch · demo Thursday 2pm · checklist pinned",
        ),
        channel(
            DEPLOYS,
            "deploys",
            "Automatic deploys, staging and production",
        ),
        channel(DEV, "dev", "Tech questions and code reviews"),
        channel(DESIGN, "design", "Mockups, feedback and inspiration"),
        channel(RANDOM, "random", "Everything else 🦆"),
        json!({
            "id": PRODUCT, "name": "product", "is_private": true,
            "topic": {"value": "Roadmap and priorities"},
        }),
        direct(DM_CAMILLE, CAMILLE),
        direct(DM_HUGO, HUGO),
        direct(DM_INES, INES),
        json!({"id": GROUP, "name": "mpdm-alex--ines--nathan-1", "is_mpim": true}),
    ]
    .into_iter()
    .map(from_json)
    .collect()
}

fn public_channels(joined: &[Conversation]) -> Vec<Conversation> {
    let others = [
        (
            MARKETING,
            "marketing",
            "Campaigns, launches and the newsletter",
        ),
        (SUPPORT, "support", "Questions from customers"),
    ]
    .map(|(id, name, topic)| from_json(json!({"id": id, "name": name, "topic": {"value": topic}})));
    joined
        .iter()
        .filter(|c| !c.is_im && !c.is_mpim && !c.is_private)
        .cloned()
        .chain(others)
        .collect()
}

/// Muted, to show how muted conversations look.
pub fn muted() -> std::collections::HashSet<String> {
    [RANDOM.to_string()].into()
}

/// A direct conversation with `user`, as `conversations.open` starts one.
pub fn direct_conversation(user: &str) -> Conversation {
    from_json(json!({"id": format!("D{user}"), "is_im": true, "user": user}))
}

fn sections() -> Vec<ChannelSection> {
    let section = |id: &str, kind: &str, name: &str, emoji: &str, ids: &[&str]| {
        from_json(json!({
            "channel_section_id": id,
            "type": kind,
            "name": name,
            "emoji": emoji,
            "channel_ids_page": {"channel_ids": ids},
        }))
    };
    vec![
        section(
            "S1",
            "standard",
            "v2 launch",
            "rocket",
            &[RELEASE, DEPLOYS, PRODUCT],
        ),
        section("S2", "stars", "Starred", "star", &[DEV]),
        section("S3", "channels", "", "", &[]),
        section("S4", "standard", "Chill", "palm_tree", &[RANDOM]),
        section("S5", "direct_messages", "", "", &[]),
    ]
}

/// The read state at launch. Every conversation is listed, as the web
/// client would, so direct messages show in the sidebar.
fn counts(
    conversations: &[Conversation],
    messages: &HashMap<String, Vec<Message>>,
    unread: &[(&str, u32)],
) -> Vec<ConversationCount> {
    let unread: HashMap<&str, u32> = unread.iter().copied().collect();
    conversations
        .iter()
        .map(|conversation| {
            let latest = messages
                .get(&conversation.id)
                .and_then(|list| list.iter().rev().find(|m| !m.is_thread_reply()))
                .map(|m| m.ts.clone());
            let mentions = unread.get(conversation.id.as_str());
            from_json(json!({
                "id": conversation.id,
                "has_unreads": mentions.is_some(),
                "mention_count": mentions.copied().unwrap_or(0),
                "latest": latest,
            }))
        })
        .collect()
}

fn general(b: &mut Builder) {
    let welcome = b.post(
        GENERAL,
        b.day(2, 9, 15),
        CAMILLE,
        "<@U04NAT> joins the mobile team on Monday, give him a warm welcome :wave:",
    );
    b.react(&welcome, "wave", &[HUGO, INES, SARAH]);
    let figures = b.post(
        GENERAL,
        b.day(2, 14, 30),
        SARAH,
        "September numbers are in the drive: +18% active users \
         :chart_with_upwards_trend:",
    );
    b.react(&figures, "tada", &[CAMILLE, HUGO, ME]);
    let site = b.post(
        GENERAL,
        b.day(1, 9, 2),
        SARAH,
        "Morning all :wave: the new website is live: \
         <https://atelier-nova.fr|atelier-nova.fr>",
    );
    b.react(&site, "tada", &[CAMILLE, HUGO, NATHAN]);
    b.react(&site, "heart", &[INES]);
    let joined = b.post(
        GENERAL,
        b.day(1, 9, 30),
        NATHAN,
        "<@U04NAT> has joined the channel",
    );
    b.message(&joined).subtype = Some("channel_join".into());
    let lunch = b.post(
        GENERAL,
        b.day(1, 11, 48),
        HUGO,
        "Who's up for Italian for lunch? :pizza:",
    );
    b.react(&lunch, "raising_hand", &[CAMILLE, ME, NATHAN]);

    let demo = b.post(
        GENERAL,
        b.ago(190),
        CAMILLE,
        "Reminder: v2 demo Thursday at 2pm for the whole team :rocket: \
         the run-through is in <#C02REL|release-2-0>",
    );
    b.react(&demo, "rocket", &[HUGO, INES, NATHAN, SARAH]);
    b.react(&demo, "+1", &[ME]);
    b.post(
        GENERAL,
        b.ago(187),
        NATHAN,
        "Can't wait! The iOS build goes out to TestFlight tonight :sparkles:",
    );
    let stickers = b.post(
        GENERAL,
        b.ago(120),
        INES,
        "Fun news: we finally have Atelier Nova stickers :art: come grab some",
    );
    b.attach(&stickers, &["nova-stickers.png"]);
    let blog = b.post(
        GENERAL,
        b.ago(42),
        SARAH,
        "<!here> the v2 blog post is ready for review: \
         <https://docs.atelier-nova.fr/blog/v2|v2 draft>",
    );
    b.reply(
        &blog,
        b.ago(39),
        CAMILLE,
        "I'll read it this afternoon :eyes:",
    );
    b.reply(
        &blog,
        b.ago(20),
        ME,
        "Great work! Flagged two small typos in the comments",
    );
    let coffee = b.post(
        GENERAL,
        b.ago(8),
        HUGO,
        "The coffee machine on the 3rd floor is finally fixed :tada:",
    );
    b.react(&coffee, "100", &[INES, SARAH, ME]);
}

fn release(b: &mut Builder) {
    let checklist = b.post(
        RELEASE,
        b.day(1, 16, 10),
        CAMILLE,
        "v2.0 launch checklist :clipboard:\n\
         • Release notes — <@U0ME>\n\
         • Signed Android build — <@U04NAT>\n\
         • Database migration — <@U02HUG>\n\
         • Store visuals — <@U03INE>",
    );
    b.react(&checklist, "white_check_mark", &[HUGO, NATHAN, INES]);
    let migration = b.post(
        RELEASE,
        b.day(1, 16, 45),
        HUGO,
        "For the migration, I suggest we run it Wednesday night:\n\
         ```cargo run --bin migrate -- --env prod --dry-run```\n\
         The dry run passes on the prod copy :white_check_mark:",
    );
    b.react(&migration, "+1", &[CAMILLE, ME]);
    b.reply(
        &migration,
        b.day(1, 16, 52),
        CAMILLE,
        "Wednesday night works for me. Should we give support a heads-up?",
    );
    b.reply(
        &migration,
        b.day(1, 16, 55),
        HUGO,
        "Yep, I'll email them tomorrow morning",
    );
    b.reply(
        &migration,
        b.day(1, 17, 20),
        NATHAN,
        "Remember to turn off push notifications during the migration :pray:",
    );
    // Followed, so that a reply arriving live shows as new.
    b.message(&migration).subscribed = true;

    let visuals = b.post(
        RELEASE,
        b.ago(150),
        INES,
        "The store visuals are ready :sparkles:",
    );
    b.attach(&visuals, &["store-ios.png", "store-android.png"]);
    b.react(&visuals, "heart_eyes", &[CAMILLE, SARAH]);
    b.react(&visuals, "fire", &[NATHAN]);
    let android = b.post(
        RELEASE,
        b.ago(60),
        NATHAN,
        "The Android build is signed and submitted for Play Store review :robot_face:",
    );
    b.react(&android, "raised_hands", &[CAMILLE, HUGO]);
    b.post(
        RELEASE,
        b.ago(25),
        CAMILLE,
        "<@U0ME> the release notes are the only thing left, \
         think you can have them by tomorrow? :pray:",
    );
}

fn deploys(b: &mut Builder) {
    b.bot(
        DEPLOYS,
        b.day(1, 11, 2),
        ":white_check_mark: `v1.9.4` deployed to production (2m 10s)",
    );
    let failed = b.bot(
        DEPLOYS,
        b.day(1, 18, 31),
        ":x: `v2.0.0-rc.2`: integration tests failed on staging · \
         <https://ci.atelier-nova.fr/runs/4182|run #4182>",
    );
    b.react(&failed, "eyes", &[HUGO]);
    b.reply(
        &failed,
        b.day(1, 18, 40),
        HUGO,
        "Flaky payment test, fixing it",
    );
    b.reply(
        &failed,
        b.day(1, 19, 5),
        HUGO,
        "Fixed in <https://github.com/atelier-nova/app/pull/482|#482> :wrench:",
    );
    b.bot(
        DEPLOYS,
        b.ago(95),
        ":white_check_mark: `v2.0.0-rc.3` deployed to staging (3m 42s)",
    );
}

fn dev(b: &mut Builder) {
    let pods = b.post(
        DEV,
        b.day(1, 10, 15),
        NATHAN,
        "Anyone know why `pod install` takes 10 minutes on CI? :thinking_face:",
    );
    b.reply(
        &pods,
        b.day(1, 10, 31),
        HUGO,
        "The CocoaPods cache isn't restored: the key depends on `Podfile.lock`",
    );
    b.reply(
        &pods,
        b.day(1, 10, 58),
        NATHAN,
        "Good catch, down to 40s :zap:",
    );
    let pr = b.post(
        DEV,
        b.day(1, 15, 40),
        HUGO,
        "Migration PR ready for review: \
         <https://github.com/atelier-nova/app/pull/478|#478 Move accounts to the new schema>",
    );
    b.react(&pr, "eyes", &[ME]);
    let review = b.post(
        DEV,
        b.ago(70),
        ME,
        "Reviewed and approved :white_check_mark: just one comment on the `accounts.email` index",
    );
    b.message(&review).edited = Some(json!({"user": ME}));
    let fixed = b.post(DEV, b.ago(66), HUGO, "Thanks! Fixed :+1:");
    b.react(&fixed, "raised_hands", &[ME]);
}

fn design(b: &mut Builder) {
    let palette = b.post(
        DESIGN,
        b.day(1, 14, 0),
        INES,
        "New palette for the home screen :art:",
    );
    b.attach(&palette, &["home-v2.fig"]);
    b.react(&palette, "heart", &[CAMILLE, SARAH]);
    let onboarding = b.post(
        DESIGN,
        b.ago(35),
        INES,
        "I updated the onboarding mockups after user testing",
    );
    b.attach(&onboarding, &["onboarding-v3.fig"]);
    b.post(
        DESIGN,
        b.ago(33),
        INES,
        "The main changes:\n\
         1. one less screen\n\
         2. the “Skip” button stands out more\n\
         3. lighter illustrations",
    );
}

fn random(b: &mut Builder) {
    let games = b.post(
        RANDOM,
        b.day(2, 17, 0),
        NATHAN,
        "Board games Friday after work? :game_die:",
    );
    b.react(&games, "+1", &[HUGO, SARAH, INES]);
    let photo = b.post(
        RANDOM,
        b.day(1, 13, 10),
        SARAH,
        "Team photo from dinner :spaghetti:",
    );
    b.attach(&photo, &["team-dinner.jpg"]);
    b.react(&photo, "heart", &[CAMILLE, INES, NATHAN]);
    let duck = b.post(
        RANDOM,
        b.ago(15),
        HUGO,
        "The rubber duck on my desk just solved a bug :duck:",
    );
    b.react(&duck, "joy", &[NATHAN, INES]);
}

fn product(b: &mut Builder) {
    let roadmap = b.post(
        PRODUCT,
        b.day(1, 11, 0),
        CAMILLE,
        "Q4 roadmap: offline mode first, then widgets :dart:",
    );
    b.reply(
        &roadmap,
        b.day(1, 11, 20),
        SARAH,
        "Lines up with customer feedback :+1:",
    );
    b.post(
        PRODUCT,
        b.ago(80),
        CAMILLE,
        "Today's product sync moved to 4pm",
    );
}

fn direct(b: &mut Builder) {
    b.post(
        DM_CAMILLE,
        b.day(1, 18, 2),
        CAMILLE,
        "Thanks for your help with the checklist :pray:",
    );
    b.post(DM_CAMILLE, b.day(1, 18, 5), ME, "Anytime!");
    b.post(
        DM_CAMILLE,
        b.ago(12),
        CAMILLE,
        "Got 5 minutes to look over the demo run-through?",
    );
    b.post(
        DM_CAMILLE,
        b.ago(11),
        CAMILLE,
        "It's pinned in <#C02REL|release-2-0> :pushpin:",
    );

    b.post(
        DM_HUGO,
        b.ago(240),
        HUGO,
        "Can you review my PR when you get a minute?",
    );
    b.post(DM_HUGO, b.ago(72), ME, "Done, approved :+1:");

    let icons = b.post(
        DM_INES,
        b.day(1, 10, 0),
        INES,
        "Which version of the icon do you like best?",
    );
    b.attach(&icons, &["icon-a.png", "icon-b.png"]);
    b.post(DM_INES, b.day(1, 10, 20), ME, "B, no question :heart_eyes:");

    b.post(
        GROUP,
        b.day(2, 16, 0),
        NATHAN,
        "Shall we book an Android testing session tomorrow?",
    );
    b.post(GROUP, b.day(2, 16, 4), INES, "10am works :+1:");
}

fn live(b: &mut Builder) -> Vec<Scheduled> {
    let migration = b
        .messages
        .get(RELEASE)
        .and_then(|list| {
            list.iter()
                .find(|m| m.text.starts_with("For the migration"))
        })
        .map(|m| m.ts.clone());
    let mut beta = message("", INES, "I'll let the beta testers know too :bell:");
    beta.thread_ts = migration;

    let scheduled = |secs: u64, channel: &str, message: Message| Scheduled {
        after: Duration::from_secs(secs),
        channel: channel.to_string(),
        message,
    };
    let mut deployed = message(
        "",
        "",
        ":white_check_mark: `v2.0.0-rc.4` deployed to staging (3m 18s)",
    );
    deployed.user = None;
    deployed.username = Some(DEPLOY_BOT.into());
    vec![
        scheduled(9, DEPLOYS, deployed),
        scheduled(
            17,
            DM_HUGO,
            message(
                "",
                HUGO,
                "Free for a quick 5-min call after stand-up? :telephone_receiver:",
            ),
        ),
        scheduled(
            30,
            RANDOM,
            message("", SARAH, "Who's in for lunch out on Thursday? :ramen:"),
        ),
        scheduled(45, RELEASE, beta),
    ]
}

/// A message posted by someone in a conversation, once built.
struct Posted {
    channel: &'static str,
    ts: String,
}

struct Builder {
    messages: HashMap<String, Vec<Message>>,
    /// Keeps timestamps unique when two messages share a second.
    serial: u32,
}

impl Builder {
    fn new() -> Self {
        Self {
            messages: HashMap::new(),
            serial: 0,
        }
    }

    fn ago(&self, minutes: i64) -> i64 {
        Local::now().timestamp() - minutes * 60
    }

    fn day(&self, days_ago: i64, hour: u32, minute: u32) -> i64 {
        let date = Local::now().date_naive() - chrono::Days::new(days_ago as u64);
        let time = date.and_hms_opt(hour, minute, 0).unwrap_or_default();
        Local
            .from_local_datetime(&time)
            .earliest()
            .map_or_else(|| self.ago(days_ago * 24 * 60), |t| t.timestamp())
    }

    fn post(&mut self, channel: &'static str, at: i64, user: &str, text: &str) -> Posted {
        self.serial += 1;
        let ts = format!("{at}.{:06}", self.serial);
        self.messages
            .entry(channel.to_string())
            .or_default()
            .push(message(&ts, user, text));
        Posted { channel, ts }
    }

    fn bot(&mut self, channel: &'static str, at: i64, text: &str) -> Posted {
        let posted = self.post(channel, at, "", text);
        let message = self.message(&posted);
        message.user = None;
        message.username = Some(DEPLOY_BOT.into());
        posted
    }

    fn reply(&mut self, parent: &Posted, at: i64, user: &str, text: &str) {
        let reply = self.post(parent.channel, at, user, text);
        self.message(&reply).thread_ts = Some(parent.ts.clone());
        let parent = self.message(parent);
        parent.thread_ts = Some(parent.ts.clone());
        parent.reply_count += 1;
        if !parent.reply_users.iter().any(|u| u == user) {
            parent.reply_users.push(user.to_string());
        }
    }

    fn react(&mut self, target: &Posted, name: &str, users: &[&str]) {
        let message = self.message(target);
        for user in users {
            message.add_reaction(name, user);
        }
    }

    fn attach(&mut self, target: &Posted, names: &[&str]) {
        self.message(target).files = names
            .iter()
            .map(|name| File {
                name: name.to_string(),
                ..File::default()
            })
            .collect();
    }

    fn message(&mut self, target: &Posted) -> &mut Message {
        self.messages
            .get_mut(target.channel)
            .and_then(|list| list.iter_mut().find(|m| m.ts == target.ts))
            .expect("a message posted by the builder")
    }
}

const MAYA: &str = "U11MAY";
const TOM: &str = "U12TOM";
const SIDE_GENERAL: &str = "C11GEN";
const IDEAS: &str = "C12IDE";
const DM_MAYA: &str = "D11MAY";

/// A second, smaller workspace: a weekend project with two friends, to
/// show switching between workspaces. Maya waits for the user in #ideas.
pub fn build_side() -> Content {
    let mut b = Builder::new();
    let kickoff = b.post(
        SIDE_GENERAL,
        b.day(3, 20, 10),
        MAYA,
        "Weekend plan: a tracker for our board game nights :game_die:",
    );
    b.react(&kickoff, "fire", &[TOM, ME]);
    b.post(
        SIDE_GENERAL,
        b.day(3, 20, 25),
        TOM,
        "I'll set up the repo and the CI tonight",
    );
    b.post(
        SIDE_GENERAL,
        b.ago(95),
        TOM,
        "Repo is up, PRs welcome :rocket:",
    );
    b.post(
        IDEAS,
        b.ago(180),
        TOM,
        "What if the app suggested a game from who is coming?",
    );
    b.post(
        IDEAS,
        b.ago(40),
        MAYA,
        "<@U0ME> could you take the scoring screen? You know the rules best :trophy:",
    );
    b.post(DM_MAYA, b.day(1, 18, 2), MAYA, "Saturday at mine, 7pm?");
    b.post(DM_MAYA, b.day(1, 18, 9), ME, "Perfect, I'll bring snacks");

    let mut messages = b.messages;
    for list in messages.values_mut() {
        list.sort_by(|a, b| a.ts.cmp(&b.ts));
    }
    let conversations: Vec<Conversation> = [
        json!({"id": SIDE_GENERAL, "name": "general", "topic": {"value": "Board game tracker"}}),
        json!({"id": IDEAS, "name": "ideas", "topic": {"value": "Anything goes"}}),
        json!({"id": DM_MAYA, "is_im": true, "user": MAYA}),
    ]
    .into_iter()
    .map(from_json)
    .collect();
    let counts = counts(&conversations, &messages, &[(IDEAS, 1)]);
    let users = [
        (ME, "alex", "Alex Morel"),
        (MAYA, "maya", "Maya Chen"),
        (TOM, "tom", "Tom Becker"),
    ]
    .into_iter()
    .map(|(id, name, real_name)| {
        from_json(json!({
            "id": id,
            "name": name,
            "real_name": real_name,
            "profile": {"display_name": real_name.split(' ').next(), "real_name": real_name},
        }))
    })
    .collect();
    Content {
        users,
        public_channels: conversations[..2].to_vec(),
        conversations,
        sections: Vec::new(),
        counts,
        messages,
        live: Vec::new(),
    }
}

pub fn message(ts: &str, user: &str, text: &str) -> Message {
    Message {
        ts: ts.to_string(),
        user: Some(user.to_string()),
        username: None,
        bot_profile: None,
        text: text.to_string(),
        subtype: None,
        thread_ts: None,
        reply_count: 0,
        reply_users: Vec::new(),
        subscribed: false,
        parent_user_id: None,
        edited: None,
        reactions: Vec::new(),
        files: Vec::new(),
        attachments: Vec::new(),
    }
}

fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).expect("demo data matches Slack's shapes")
}

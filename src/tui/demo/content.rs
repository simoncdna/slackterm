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

const DEPLOY_BOT: &str = "deploy-bot";

pub struct Content {
    pub users: Vec<User>,
    pub conversations: Vec<Conversation>,
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
        CAMILLE => "Top, merci ! Dis-moi s'il manque quelque chose :raised_hands:",
        HUGO => "Parfait, je t'appelle dans 5 min :telephone_receiver:",
        INES => "Super, je t'envoie les fichiers :art:",
        _ => "Bien reçu :+1:",
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
    let counts = counts(&messages);
    Content {
        users: users(),
        conversations: conversations(),
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
            "profile": {"display_name": first, "real_name": real_name},
        }))
    })
    .collect()
}

fn conversations() -> Vec<Conversation> {
    let channel = |id: &str, name: &str, topic: &str| json!({"id": id, "name": name, "topic": {"value": topic}});
    let direct = |id: &str, user: &str| json!({"id": id, "is_im": true, "user": user});
    [
        channel(GENERAL, "general", "Vie de l'équipe et annonces 📣"),
        channel(
            RELEASE,
            "release-2-0",
            "Lancement de la v2.0 · démo jeudi 14h · checklist épinglée",
        ),
        channel(
            DEPLOYS,
            "deploys",
            "Déploiements automatiques, staging et production",
        ),
        channel(DEV, "dev", "Questions techniques et revues de code"),
        channel(DESIGN, "design", "Maquettes, retours et inspiration"),
        channel(RANDOM, "random", "Tout le reste 🦆"),
        json!({
            "id": PRODUCT, "name": "produit", "is_private": true,
            "topic": {"value": "Roadmap et priorités"},
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
            "Lancement v2",
            "rocket",
            &[RELEASE, DEPLOYS, PRODUCT],
        ),
        section("S2", "stars", "Favoris", "star", &[DEV]),
        section("S3", "channels", "", "", &[]),
        section("S4", "standard", "Détente", "palm_tree", &[RANDOM]),
        section("S5", "direct_messages", "", "", &[]),
    ]
}

/// The read state at launch. Every conversation is listed, as the web
/// client would, so direct messages show in the sidebar.
fn counts(messages: &HashMap<String, Vec<Message>>) -> Vec<ConversationCount> {
    let unread: HashMap<&str, u32> = [(RELEASE, 1), (DESIGN, 0), (DM_CAMILLE, 2)].into();
    conversations()
        .into_iter()
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
        "Lundi, <@U04NAT> rejoint l'équipe mobile : réservez-lui un bel accueil :wave:",
    );
    b.react(&welcome, "wave", &[HUGO, INES, SARAH]);
    let figures = b.post(
        GENERAL,
        b.day(2, 14, 30),
        SARAH,
        "Les chiffres de septembre sont dans le drive : +18 % d'utilisateurs actifs \
         :chart_with_upwards_trend:",
    );
    b.react(&figures, "tada", &[CAMILLE, HUGO, ME]);
    let site = b.post(
        GENERAL,
        b.day(1, 9, 2),
        SARAH,
        "Bonjour à tous :wave: le nouveau site est en ligne : \
         <https://atelier-nova.fr|atelier-nova.fr>",
    );
    b.react(&site, "tada", &[CAMILLE, HUGO, NATHAN]);
    b.react(&site, "heart", &[INES]);
    let joined = b.post(
        GENERAL,
        b.day(1, 9, 30),
        NATHAN,
        "<@U04NAT> a rejoint le canal",
    );
    b.message(&joined).subtype = Some("channel_join".into());
    let lunch = b.post(
        GENERAL,
        b.day(1, 11, 48),
        HUGO,
        "Qui est chaud pour l'italien ce midi ? :pizza:",
    );
    b.react(&lunch, "raising_hand", &[CAMILLE, ME, NATHAN]);

    let demo = b.post(
        GENERAL,
        b.ago(190),
        CAMILLE,
        "Rappel : démo de la v2 jeudi à 14h devant toute l'équipe :rocket: \
         le déroulé est dans <#C02REL|release-2-0>",
    );
    b.react(&demo, "rocket", &[HUGO, INES, NATHAN, SARAH]);
    b.react(&demo, "+1", &[ME]);
    b.post(
        GENERAL,
        b.ago(187),
        NATHAN,
        "Hâte ! La build iOS part en TestFlight ce soir :sparkles:",
    );
    let stickers = b.post(
        GENERAL,
        b.ago(120),
        INES,
        "Petite nouveauté : on a enfin des stickers de l'atelier :art: passez me voir",
    );
    b.attach(&stickers, &["stickers-atelier.png"]);
    let blog = b.post(
        GENERAL,
        b.ago(42),
        SARAH,
        "<!here> l'article de blog sur la v2 est prêt à relire : \
         <https://docs.atelier-nova.fr/blog/v2|brouillon v2>",
    );
    b.reply(&blog, b.ago(39), CAMILLE, "Je relis cet après-midi :eyes:");
    b.reply(
        &blog,
        b.ago(20),
        ME,
        "Super boulot ! Deux petites coquilles signalées en commentaire",
    );
    let coffee = b.post(
        GENERAL,
        b.ago(8),
        HUGO,
        "La machine à café du 3e est enfin réparée :tada:",
    );
    b.react(&coffee, "100", &[INES, SARAH, ME]);
}

fn release(b: &mut Builder) {
    let checklist = b.post(
        RELEASE,
        b.day(1, 16, 10),
        CAMILLE,
        "Checklist de lancement v2.0 :clipboard:\n\
         • Notes de version — <@U0ME>\n\
         • Build Android signée — <@U04NAT>\n\
         • Migration de la base — <@U02HUG>\n\
         • Visuels des stores — <@U03INE>",
    );
    b.react(&checklist, "white_check_mark", &[HUGO, NATHAN, INES]);
    let migration = b.post(
        RELEASE,
        b.day(1, 16, 45),
        HUGO,
        "Pour la migration, je propose de la lancer mercredi soir :\n\
         ```cargo run --bin migrate -- --env prod --dry-run```\n\
         Le dry-run passe sur la copie de prod :white_check_mark:",
    );
    b.react(&migration, "+1", &[CAMILLE, ME]);
    b.reply(
        &migration,
        b.day(1, 16, 52),
        CAMILLE,
        "Mercredi soir ça me va. On prévient le support ?",
    );
    b.reply(
        &migration,
        b.day(1, 16, 55),
        HUGO,
        "Oui, je leur écris demain matin",
    );
    b.reply(
        &migration,
        b.day(1, 17, 20),
        NATHAN,
        "Pensez à couper les notifications push pendant la migration :pray:",
    );

    let visuals = b.post(
        RELEASE,
        b.ago(150),
        INES,
        "Les visuels pour les stores sont prêts :sparkles:",
    );
    b.attach(&visuals, &["store-ios.png", "store-android.png"]);
    b.react(&visuals, "heart_eyes", &[CAMILLE, SARAH]);
    b.react(&visuals, "fire", &[NATHAN]);
    let android = b.post(
        RELEASE,
        b.ago(60),
        NATHAN,
        "La build Android est signée et partie en revue sur le Play Store :robot_face:",
    );
    b.react(&android, "raised_hands", &[CAMILLE, HUGO]);
    b.post(
        RELEASE,
        b.ago(25),
        CAMILLE,
        "<@U0ME> il ne manque plus que les notes de version, \
         tu penses les avoir pour demain ? :pray:",
    );
}

fn deploys(b: &mut Builder) {
    b.bot(
        DEPLOYS,
        b.day(1, 11, 2),
        ":white_check_mark: `v1.9.4` déployé en production (2 min 10 s)",
    );
    let failed = b.bot(
        DEPLOYS,
        b.day(1, 18, 31),
        ":x: `v2.0.0-rc.2` : échec des tests d'intégration sur staging · \
         <https://ci.atelier-nova.fr/runs/4182|run #4182>",
    );
    b.react(&failed, "eyes", &[HUGO]);
    b.reply(
        &failed,
        b.day(1, 18, 40),
        HUGO,
        "Un test instable sur le paiement, je corrige",
    );
    b.reply(
        &failed,
        b.day(1, 19, 5),
        HUGO,
        "Corrigé dans <https://github.com/atelier-nova/app/pull/482|#482> :wrench:",
    );
    b.bot(
        DEPLOYS,
        b.ago(95),
        ":white_check_mark: `v2.0.0-rc.3` déployé sur staging (3 min 42 s)",
    );
}

fn dev(b: &mut Builder) {
    let pods = b.post(
        DEV,
        b.day(1, 10, 15),
        NATHAN,
        "Quelqu'un sait pourquoi `pod install` prend 10 minutes sur la CI ? :thinking_face:",
    );
    b.reply(
        &pods,
        b.day(1, 10, 31),
        HUGO,
        "Le cache CocoaPods n'est pas restauré : la clé dépend du `Podfile.lock`",
    );
    b.reply(
        &pods,
        b.day(1, 10, 58),
        NATHAN,
        "Bien vu, ça tombe à 40 s :zap:",
    );
    let pr = b.post(
        DEV,
        b.day(1, 15, 40),
        HUGO,
        "PR de la migration prête pour relecture : \
         <https://github.com/atelier-nova/app/pull/478|#478 Migration des comptes vers le nouveau schéma>",
    );
    b.react(&pr, "eyes", &[ME]);
    let review = b.post(
        DEV,
        b.ago(70),
        ME,
        "Relue et approuvée :white_check_mark: juste une remarque sur l'index de `accounts.email`",
    );
    b.message(&review).edited = Some(json!({"user": ME}));
    let fixed = b.post(DEV, b.ago(66), HUGO, "Merci ! Corrigé :+1:");
    b.react(&fixed, "raised_hands", &[ME]);
}

fn design(b: &mut Builder) {
    let palette = b.post(
        DESIGN,
        b.day(1, 14, 0),
        INES,
        "Nouvelle palette pour l'écran d'accueil :art:",
    );
    b.attach(&palette, &["accueil-v2.fig"]);
    b.react(&palette, "heart", &[CAMILLE, SARAH]);
    let onboarding = b.post(
        DESIGN,
        b.ago(35),
        INES,
        "J'ai mis à jour les maquettes de l'onboarding après les tests utilisateurs",
    );
    b.attach(&onboarding, &["onboarding-v3.fig"]);
    b.post(
        DESIGN,
        b.ago(33),
        INES,
        "Les principaux changements :\n\
         1. un écran de moins\n\
         2. le bouton « Passer » est plus visible\n\
         3. des illustrations plus légères",
    );
}

fn random(b: &mut Builder) {
    let games = b.post(
        RANDOM,
        b.day(2, 17, 0),
        NATHAN,
        "Jeux de société vendredi après le boulot ? :game_die:",
    );
    b.react(&games, "+1", &[HUGO, SARAH, INES]);
    let photo = b.post(
        RANDOM,
        b.day(1, 13, 10),
        SARAH,
        "La photo de l'équipe au resto :spaghetti:",
    );
    b.attach(&photo, &["resto.jpg"]);
    b.react(&photo, "heart", &[CAMILLE, INES, NATHAN]);
    let duck = b.post(
        RANDOM,
        b.ago(15),
        HUGO,
        "Le canard en plastique de mon bureau vient de résoudre un bug :duck:",
    );
    b.react(&duck, "joy", &[NATHAN, INES]);
}

fn product(b: &mut Builder) {
    let roadmap = b.post(
        PRODUCT,
        b.day(1, 11, 0),
        CAMILLE,
        "Roadmap T4 : on priorise le mode hors-ligne, puis les widgets :dart:",
    );
    b.reply(
        &roadmap,
        b.day(1, 11, 20),
        SARAH,
        "Ça colle avec les retours clients :+1:",
    );
    b.post(
        PRODUCT,
        b.ago(80),
        CAMILLE,
        "Le point produit est déplacé à 16h aujourd'hui",
    );
}

fn direct(b: &mut Builder) {
    b.post(
        DM_CAMILLE,
        b.day(1, 18, 2),
        CAMILLE,
        "Merci pour ton aide sur la checklist :pray:",
    );
    b.post(DM_CAMILLE, b.day(1, 18, 5), ME, "Avec plaisir !");
    b.post(
        DM_CAMILLE,
        b.ago(12),
        CAMILLE,
        "Tu as 5 minutes pour regarder le déroulé de la démo ?",
    );
    b.post(
        DM_CAMILLE,
        b.ago(11),
        CAMILLE,
        "C'est dans <#C02REL|release-2-0>, message épinglé :pushpin:",
    );

    b.post(
        DM_HUGO,
        b.ago(240),
        HUGO,
        "Tu peux relire ma PR quand tu as un moment ?",
    );
    b.post(DM_HUGO, b.ago(72), ME, "C'est fait, approuvée :+1:");

    let icons = b.post(
        DM_INES,
        b.day(1, 10, 0),
        INES,
        "Tu préfères quelle version de l'icône ?",
    );
    b.attach(&icons, &["icone-a.png", "icone-b.png"]);
    b.post(
        DM_INES,
        b.day(1, 10, 20),
        ME,
        "La B, sans hésiter :heart_eyes:",
    );

    b.post(
        GROUP,
        b.day(2, 16, 0),
        NATHAN,
        "On se cale une session de tests Android demain ?",
    );
    b.post(GROUP, b.day(2, 16, 4), INES, "Go pour 10h :+1:");
}

fn live(b: &mut Builder) -> Vec<Scheduled> {
    let migration = b
        .messages
        .get(RELEASE)
        .and_then(|list| {
            list.iter()
                .find(|m| m.text.starts_with("Pour la migration"))
        })
        .map(|m| m.ts.clone());
    let mut beta = message("", INES, "Je préviens aussi les testeurs de la bêta :bell:");
    beta.thread_ts = migration;

    let scheduled = |secs: u64, channel: &str, message: Message| Scheduled {
        after: Duration::from_secs(secs),
        channel: channel.to_string(),
        message,
    };
    let mut deployed = message(
        "",
        "",
        ":white_check_mark: `v2.0.0-rc.4` déployé sur staging (3 min 18 s)",
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
                "Dispo pour un call de 5 min après le stand-up ? :telephone_receiver:",
            ),
        ),
        scheduled(
            30,
            RANDOM,
            message("", SARAH, "Qui vient au resto jeudi midi ? :ramen:"),
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
        edited: None,
        reactions: Vec::new(),
        files: Vec::new(),
    }
}

fn from_json<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).expect("demo data matches Slack's shapes")
}

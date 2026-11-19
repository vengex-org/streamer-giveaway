//! Simulated Twitch chat for trying the app, recording screenshots and testing big giveaways
//! without a live stream. Start the app with `--demo`.

use std::thread;
use std::time::{Duration, Instant};

use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use rand::{RngExt, SeedableRng};

use crate::chat::{ChatBridge, ChatCommand, ChatEvent, ConnectionStatus};
use crate::domain::chat::{Badge, ChatMessage};

pub const DEMO_CHANNEL: &str = "pixelpaladin";
pub const DEMO_KEYWORD: &str = "GIVEME";

/// Messages the feed sends, spread over [`FEED_DURATION`].
pub const TOTAL_MESSAGES: usize = 6000;
pub const FEED_DURATION: Duration = Duration::from_secs(180);
/// Distinct chatters; most of them write more than once.
const CHATTERS: usize = 2400;
/// Share of messages that are giveaway entries (with variants that only some match modes accept).
const ENTRY_SHARE: f64 = 0.62;
/// After the feed ends, chat keeps going at this pace so the window does not look dead.
const IDLE_MESSAGE_EVERY: Duration = Duration::from_millis(400);
/// How long the drawn winner takes to answer.
const WINNER_REPLY_AFTER: Duration = Duration::from_secs(6);
/// The broadcaster's user id; their own "GIVEME" shows that the app never lets them enter.
const BROADCASTER_ID: &str = "0";

const FIRST: [&str; 16] = [
    "Pixel", "Turbo", "Sneaky", "Cozy", "Mega", "Lucky", "Shadow", "Frosty", "Salty", "Hyper",
    "Chill", "Retro", "Cosmic", "Spicy", "Sleepy", "Bouncy",
];
const SECOND: [&str; 16] = [
    "Panda", "Ninja", "Taco", "Wizard", "Goblin", "Otter", "Falcon", "Noodle", "Pirate", "Cactus",
    "Raccoon", "Dragon", "Pickle", "Comet", "Badger", "Muffin",
];
const ENTRIES: [&str; 6] = [
    "GIVEME",
    "GIVEME",
    "GIVEME",
    "giveme pls",
    "GIVEME LETS GO",
    "GIVEMEEE",
];
const CHATTER: [&str; 16] = [
    "LUL",
    "KEKW",
    "PogChamp",
    "that was clean",
    "gg",
    "monkaS",
    "lets gooo",
    "W streamer",
    "first time here, hi chat",
    "HYPE",
    "o7",
    "Clap",
    "what are we giving away?",
    "good luck everyone",
    "I never win these",
    "hi from Bratislava",
];
const WINNER_REPLIES: [&str; 2] = [
    "omg I actually won!!",
    "thank you so much, how do I claim it?",
];

/// One simulated chatter. Badges stay the same for all their messages.
#[derive(Debug, Clone)]
struct Chatter {
    user_id: String,
    name: String,
    badges: Vec<Badge>,
}

fn chatters(rng: &mut StdRng) -> Vec<Chatter> {
    (1..=CHATTERS)
        .map(|i| {
            let first = FIRST[i % FIRST.len()];
            let second = SECOND[(i / FIRST.len()) % SECOND.len()];
            let name = match i % 3 {
                0 => format!("{first}{second}"),
                1 => format!("{first}_{second}{}", i % 97),
                _ => format!("{}{}{}", first.to_lowercase(), second, i % 1000),
            };
            Chatter {
                user_id: i.to_string(),
                name,
                badges: badges(rng),
            }
        })
        .collect()
}

/// Roughly what a mid-size channel looks like: most viewers, a fifth subs, a few VIPs and mods.
fn badges(rng: &mut StdRng) -> Vec<Badge> {
    let roll: f64 = rng.random();
    let mut badges = Vec::new();
    match roll {
        r if r < 0.03 => badges.push(Badge::new("moderator", "1")),
        r if r < 0.05 => badges.push(Badge::new("vip", "1")),
        _ => {}
    }
    let sub: f64 = rng.random();
    match sub {
        s if s < 0.02 => badges.push(Badge::new("subscriber", "3012")),
        s if s < 0.06 => badges.push(Badge::new("subscriber", "2006")),
        s if s < 0.22 => badges.push(Badge::new("subscriber", "12")),
        _ => {}
    }
    badges
}

/// The scripted feed: [`TOTAL_MESSAGES`] messages with their time offset from the start, in
/// order. The same seed gives the same feed.
pub fn feed(seed: u64) -> Vec<(Duration, ChatMessage)> {
    let mut rng = StdRng::seed_from_u64(seed);
    let people = chatters(&mut rng);
    let span = FEED_DURATION.as_millis() as u64;
    let mut offsets: Vec<u64> = (0..TOTAL_MESSAGES)
        .map(|_| rng.random_range(0..span))
        .collect();
    offsets.sort_unstable();
    offsets
        .into_iter()
        .enumerate()
        .map(|(n, offset)| {
            let msg = if n % 997 == 500 {
                ChatMessage {
                    user_id: BROADCASTER_ID.into(),
                    display_name: DEMO_CHANNEL.into(),
                    badges: vec![Badge::new("broadcaster", "1")],
                    text: format!("type {DEMO_KEYWORD} to enter!"),
                }
            } else {
                let who = people.choose(&mut rng).expect("chatters exist");
                let pool: &[&str] = if rng.random_bool(ENTRY_SHARE) {
                    &ENTRIES
                } else {
                    &CHATTER
                };
                message(who, pool.choose(&mut rng).expect("lines exist"))
            };
            (Duration::from_millis(offset), msg)
        })
        .collect()
}

fn message(who: &Chatter, text: &str) -> ChatMessage {
    ChatMessage {
        user_id: who.user_id.clone(),
        display_name: who.name.clone(),
        badges: who.badges.clone(),
        text: text.to_owned(),
    }
}

/// Starts the simulated chat; it behaves like [`ChatBridge::spawn`] for the UI.
pub fn spawn() -> std::io::Result<ChatBridge> {
    let (command_tx, command_rx) = async_channel::unbounded();
    let (event_tx, event_rx) = async_channel::unbounded();
    thread::Builder::new()
        .name("demo-chat".into())
        .spawn(move || run(command_rx, event_tx))?;
    Ok(ChatBridge {
        commands: command_tx,
        events: event_rx,
    })
}

fn run(commands: async_channel::Receiver<ChatCommand>, events: async_channel::Sender<ChatEvent>) {
    let mut rng = StdRng::seed_from_u64(7);
    let people = chatters(&mut rng);
    let script = feed(42);
    let mut started: Option<Instant> = None;
    let mut next = 0;
    let mut idle_at = Instant::now();
    let mut replies: Vec<(Instant, ChatMessage)> = Vec::new();
    let send = |event| events.send_blocking(event).is_ok();

    loop {
        while let Ok(command) = commands.try_recv() {
            match command {
                ChatCommand::Join(channel) => {
                    let connected = send(ChatEvent::Status(ConnectionStatus::Connecting {
                        channel: channel.clone(),
                        cause: None,
                    }));
                    thread::sleep(Duration::from_millis(600));
                    if !connected || !send(ChatEvent::Status(ConnectionStatus::Connected(channel)))
                    {
                        return;
                    }
                    started = Some(Instant::now());
                    next = 0;
                }
                ChatCommand::Leave => {
                    started = None;
                    if !send(ChatEvent::Status(ConnectionStatus::Idle)) {
                        return;
                    }
                }
                ChatCommand::WinnerDrawn(user_id) => {
                    let winner = people.iter().find(|c| c.user_id == user_id);
                    if let Some(winner) = winner {
                        let now = Instant::now();
                        for (i, text) in WINNER_REPLIES.iter().enumerate() {
                            let at = now + WINNER_REPLY_AFTER + Duration::from_secs(4 * i as u64);
                            replies.push((at, message(winner, text)));
                        }
                    }
                }
            }
        }
        if commands.is_closed() {
            return;
        }

        let now = Instant::now();
        if let Some(start) = started {
            while next < script.len() && start + script[next].0 <= now {
                if !send(ChatEvent::Message(script[next].1.clone())) {
                    return;
                }
                next += 1;
            }
            if next == script.len() && now >= idle_at {
                let who = people.choose(&mut rng).expect("chatters exist");
                let line = CHATTER.choose(&mut rng).expect("lines exist");
                if !send(ChatEvent::Message(message(who, line))) {
                    return;
                }
                idle_at = now + IDLE_MESSAGE_EVERY;
            }
        }
        let (due, later): (Vec<_>, Vec<_>) = replies.into_iter().partition(|(at, _)| *at <= now);
        replies = later;
        for (_, msg) in due {
            if !send(ChatEvent::Message(msg)) {
                return;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::domain::chat::Keyword;
    use crate::domain::matcher::{MatchMode, matches};

    #[test]
    fn feed_has_6000_messages_in_order_within_three_minutes() {
        let feed = feed(42);
        assert_eq!(feed.len(), TOTAL_MESSAGES);
        assert!(feed.windows(2).all(|w| w[0].0 <= w[1].0));
        assert!(feed.last().unwrap().0 < FEED_DURATION);
    }

    #[test]
    fn most_messages_are_entries_from_many_people() {
        let feed = feed(42);
        let keyword = Keyword::parse(DEMO_KEYWORD).unwrap();
        let entries = feed
            .iter()
            .filter(|(_, m)| matches(MatchMode::Word, &keyword, &m.text))
            .count();
        assert!((3000..4500).contains(&entries), "{entries} entries");
        let people: HashSet<_> = feed.iter().map(|(_, m)| &m.user_id).collect();
        assert!(people.len() > 1500, "{} people", people.len());
    }

    #[test]
    fn same_seed_gives_the_same_feed() {
        let a: Vec<_> = feed(1)
            .into_iter()
            .map(|(t, m)| (t, m.text, m.user_id))
            .collect();
        let b: Vec<_> = feed(1)
            .into_iter()
            .map(|(t, m)| (t, m.text, m.user_id))
            .collect();
        assert_eq!(a, b);
    }

    #[test]
    fn a_chatter_keeps_the_same_badges() {
        let feed = feed(42);
        let mut seen = std::collections::HashMap::new();
        for (_, m) in &feed {
            let first = seen
                .entry(m.user_id.clone())
                .or_insert_with(|| m.badges.clone());
            assert_eq!(first, &m.badges, "user {}", m.user_id);
        }
    }
}

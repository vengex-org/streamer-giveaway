//! Measures a live chat: messages per second (mean and peak) and unique chatters.
//! The channel is a Twitch login, `kick:<name>` or `youtube:<link or @handle>`.
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use streamer_giveaway::chat::{
    ChatBridge, ChatCommand, ChatEvent, ChatTarget, ConnectionStatus, connection_problem_layer,
};
use streamer_giveaway::domain::chat::ChannelLogin;
use streamer_giveaway::domain::platform::{KickChannel, YouTubeSource};

fn main() {
    let channel = std::env::args().nth(1).unwrap();
    let secs: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let (_layer, problems) = connection_problem_layer();
    let bridge = ChatBridge::spawn(problems).unwrap();
    bridge
        .commands
        .send_blocking(ChatCommand::Join(target(&channel)))
        .unwrap();
    let mut start = None;
    let mut per_sec: HashMap<u64, u32> = HashMap::new();
    let mut users = HashSet::new();
    let mut total = 0u32;
    let deadline = Instant::now() + Duration::from_secs(secs + 15);
    while Instant::now() < deadline {
        let Ok(event) = bridge.events.recv_blocking() else {
            break;
        };
        if let ChatEvent::Status(_, status) = &event {
            eprintln!("{status:?}");
            if matches!(status, ConnectionStatus::Failed { .. }) {
                return;
            }
        }
        if let ChatEvent::Message(m) = event {
            let s = *start.get_or_insert_with(Instant::now);
            let t = s.elapsed().as_secs();
            if t >= secs {
                break;
            }
            *per_sec.entry(t).or_default() += 1;
            users.insert(m.user().clone());
            total += 1;
        }
    }
    let peak = per_sec.values().max().copied().unwrap_or(0);
    println!(
        "{channel}: {total} msgs in {secs}s, mean {:.1}/s, peak {peak}/s, unique chatters {}",
        total as f64 / secs as f64,
        users.len()
    );
}

fn target(raw: &str) -> ChatTarget {
    if let Some(name) = raw.strip_prefix("kick:") {
        ChatTarget::Kick(KickChannel::parse(name).unwrap())
    } else if let Some(source) = raw.strip_prefix("youtube:") {
        ChatTarget::YouTube(YouTubeSource::parse(source).unwrap())
    } else {
        ChatTarget::Twitch(ChannelLogin::parse(raw).unwrap())
    }
}

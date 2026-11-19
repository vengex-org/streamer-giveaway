//! Measures a live channel's chat: messages per second (mean and peak) and unique chatters.
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use streamer_giveaway::chat::{ChatBridge, ChatCommand, ChatEvent, connection_problem_layer};
use streamer_giveaway::domain::chat::ChannelLogin;

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
        .send_blocking(ChatCommand::Join(ChannelLogin::parse(&channel).unwrap()))
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

//! Prints how long a giveaway takes for a few audience sizes and Twitch delivery caps.
use std::time::Duration;

use streamer_giveaway::demo::{Crowd, crowd_feed, raffle_times};

fn main() {
    let secs = |d: Duration| format!("{:>5.0} s", d.as_secs_f64());
    println!("viewers  share  half-life  cap/s   entrants  50%     90%     99%     all");
    for (viewers, share, half_life) in [
        (8000, 0.4, 20),
        (8000, 0.4, 45),
        (8000, 0.6, 20),
        (100_000, 0.2, 20),
        (3_800_000, 0.05, 20),
    ] {
        for cap in [None, Some(300), Some(100)] {
            let crowd = Crowd {
                entry_share: share,
                half_life: Duration::from_secs(half_life),
                cap,
                ..Crowd::with_viewers(viewers)
            };
            let t = raffle_times(&crowd_feed(crowd, 42));
            let cap = cap.map_or("none".to_owned(), |c| c.to_string());
            println!(
                "{viewers:>9} {share:>5} {half_life:>7} s {cap:>7} {:>9} {} {} {} {}",
                t.entrants,
                secs(t.half),
                secs(t.ninety),
                secs(t.ninety_nine),
                secs(t.all)
            );
        }
    }
}

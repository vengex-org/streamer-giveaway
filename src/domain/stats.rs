use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Messages per second are averaged over this window.
const RATE_WINDOW: Duration = Duration::from_secs(10);
/// "New entrants in the last minute".
const ENTRY_WINDOW: Duration = Duration::from_secs(60);

/// Live chat throughput while entries are open. Times come in as `now`, never read inside.
#[derive(Debug, Clone, Default)]
pub struct ChatStats {
    messages: VecDeque<Instant>,
    entries: VecDeque<Instant>,
    last_entry: Option<Instant>,
}

impl ChatStats {
    pub fn record_message(&mut self, now: Instant) {
        self.messages.push_back(now);
        trim(&mut self.messages, now, RATE_WINDOW);
    }

    pub fn record_entry(&mut self, now: Instant) {
        self.entries.push_back(now);
        trim(&mut self.entries, now, ENTRY_WINDOW);
        self.last_entry = Some(now);
    }

    /// Chat messages per second over the last 10 seconds.
    pub fn messages_per_second(&self, now: Instant) -> f64 {
        let recent = self
            .messages
            .iter()
            .filter(|t| now.saturating_duration_since(**t) <= RATE_WINDOW)
            .count();
        recent as f64 / RATE_WINDOW.as_secs_f64()
    }

    /// New entrants in the last minute.
    pub fn entries_last_minute(&self, now: Instant) -> usize {
        self.entries
            .iter()
            .filter(|t| now.saturating_duration_since(**t) <= ENTRY_WINDOW)
            .count()
    }

    /// Time since the last new entrant; `None` before the first one.
    pub fn since_last_entry(&self, now: Instant) -> Option<Duration> {
        self.last_entry.map(|t| now.saturating_duration_since(t))
    }
}

fn trim(times: &mut VecDeque<Instant>, now: Instant, window: Duration) {
    while times
        .front()
        .is_some_and(|t| now.saturating_duration_since(*t) > window)
    {
        times.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: Duration = Duration::from_secs(1);

    #[test]
    fn rate_counts_the_last_ten_seconds() {
        let start = Instant::now();
        let mut stats = ChatStats::default();
        for i in 0..50 {
            stats.record_message(start + SEC * (i / 5));
        }
        // 50 messages over 10 s.
        assert_eq!(stats.messages_per_second(start + 9 * SEC), 5.0);
        // 20 s later all of them are older than the window.
        assert_eq!(stats.messages_per_second(start + 30 * SEC), 0.0);
    }

    #[test]
    fn entries_in_the_last_minute_drop_old_ones() {
        let start = Instant::now();
        let mut stats = ChatStats::default();
        stats.record_entry(start);
        stats.record_entry(start + 30 * SEC);
        stats.record_entry(start + 50 * SEC);
        assert_eq!(stats.entries_last_minute(start + 55 * SEC), 3);
        assert_eq!(stats.entries_last_minute(start + 75 * SEC), 2);
    }

    #[test]
    fn time_since_last_entry() {
        let start = Instant::now();
        let mut stats = ChatStats::default();
        assert_eq!(stats.since_last_entry(start), None);
        stats.record_entry(start);
        assert_eq!(stats.since_last_entry(start + 7 * SEC), Some(7 * SEC));
    }
}

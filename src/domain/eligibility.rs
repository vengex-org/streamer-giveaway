use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::role::Role;

/// Unit of the "active in chat" window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeUnit {
    #[default]
    Minutes,
    Hours,
}

/// "Active in chat in the last `amount` `unit`". `amount` is at least 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActiveWindow {
    pub amount: u32,
    pub unit: TimeUnit,
}

impl Default for ActiveWindow {
    fn default() -> Self {
        Self {
            amount: 10,
            unit: TimeUnit::Minutes,
        }
    }
}

impl ActiveWindow {
    pub fn duration(self) -> Duration {
        let secs = match self.unit {
            TimeUnit::Minutes => 60,
            TimeUnit::Hours => 3600,
        };
        Duration::from_secs(u64::from(self.amount) * secs)
    }

    /// Parses the window length typed by the user: a whole number of at least 1.
    pub fn parse_amount(raw: &str) -> Option<u32> {
        raw.trim().parse().ok().filter(|&n| n >= 1)
    }
}

/// Conditions a chatter must meet to enter. All off by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Eligibility {
    pub subs_only: bool,
    pub require_activity: bool,
    pub active_window: ActiveWindow,
}

impl Eligibility {
    /// Whether a chatter with `roles`, last seen chatting at `last_active`, may enter at `now`.
    pub fn allows(&self, roles: &[Role], last_active: Option<Instant>, now: Instant) -> bool {
        let sub_ok = !self.subs_only
            || roles
                .iter()
                .any(|r| matches!(r, Role::SubT1 | Role::SubT2 | Role::SubT3));
        let active_ok = !self.require_activity
            || last_active.is_some_and(|seen| {
                now.saturating_duration_since(seen) <= self.active_window.duration()
            });
        sub_ok && active_ok
    }
}

/// When each chatter last wrote something other than the keyword, since the app connected.
#[derive(Debug, Clone, Default)]
pub struct ActivityLog {
    last_chat: HashMap<String, Instant>,
}

/// Above this many chatters the log drops everyone quiet for longer than [`FORGET_AFTER`].
const PRUNE_ABOVE: usize = 20_000;
/// Activity older than this is never needed: the longest window anyone sets in practice.
const FORGET_AFTER: Duration = Duration::from_secs(24 * 3600);

impl ActivityLog {
    pub fn record(&mut self, user_id: &str, now: Instant) {
        if self.last_chat.len() >= PRUNE_ABOVE {
            self.last_chat
                .retain(|_, seen| now.saturating_duration_since(*seen) <= FORGET_AFTER);
        }
        self.last_chat.insert(user_id.to_owned(), now);
    }

    pub fn last(&self, user_id: &str) -> Option<Instant> {
        self.last_chat.get(user_id).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    fn active_10_min() -> Eligibility {
        Eligibility {
            require_activity: true,
            ..Eligibility::default()
        }
    }

    #[test]
    fn no_conditions_let_everyone_in() {
        let now = Instant::now();
        assert!(Eligibility::default().allows(&[Role::Pleb], None, now));
    }

    #[test]
    fn subs_only_accepts_any_tier_and_rejects_the_rest() {
        let subs = Eligibility {
            subs_only: true,
            ..Eligibility::default()
        };
        let now = Instant::now();
        for role in [Role::SubT1, Role::SubT2, Role::SubT3] {
            assert!(subs.allows(&[role, Role::Mod], None, now), "{role:?}");
        }
        for role in [Role::Pleb, Role::Vip, Role::Mod] {
            assert!(!subs.allows(&[role], None, now), "{role:?}");
        }
    }

    #[test]
    fn activity_inside_the_window_is_enough() {
        let start = Instant::now();
        let at = start + 5 * MIN;
        assert!(active_10_min().allows(&[Role::Pleb], Some(start), at));
        assert!(active_10_min().allows(&[Role::Pleb], Some(start), start + 10 * MIN));
    }

    #[test]
    fn activity_outside_the_window_or_none_is_rejected() {
        let start = Instant::now();
        assert!(!active_10_min().allows(&[Role::Pleb], Some(start), start + 11 * MIN));
        assert!(!active_10_min().allows(&[Role::Pleb], None, start));
    }

    #[test]
    fn both_conditions_must_hold() {
        let both = Eligibility {
            subs_only: true,
            ..active_10_min()
        };
        let now = Instant::now();
        assert!(!both.allows(&[Role::SubT1], None, now));
        assert!(!both.allows(&[Role::Pleb], Some(now), now));
        assert!(both.allows(&[Role::SubT1], Some(now), now));
    }

    #[test]
    fn window_length_in_hours() {
        let window = ActiveWindow {
            amount: 2,
            unit: TimeUnit::Hours,
        };
        assert_eq!(window.duration(), Duration::from_secs(7200));
    }

    #[test]
    fn window_amount_must_be_a_whole_number_from_one() {
        assert_eq!(ActiveWindow::parse_amount(" 15 "), Some(15));
        for raw in ["0", "", " ", "-1", "1.5", "x"] {
            assert_eq!(ActiveWindow::parse_amount(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn log_keeps_the_latest_time_per_user() {
        let start = Instant::now();
        let mut log = ActivityLog::default();
        log.record("1", start);
        log.record("1", start + MIN);
        assert_eq!(log.last("1"), Some(start + MIN));
        assert_eq!(log.last("2"), None);
    }
}

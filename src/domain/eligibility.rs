use std::collections::HashMap;
use std::num::NonZeroU32;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::platform::UserKey;
use super::role::Role;

/// Unit of the "active in chat" window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TimeUnit {
    #[default]
    Minutes,
    Hours,
}

/// "Active in chat in the last `amount` `unit`".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActiveWindow {
    pub amount: NonZeroU32,
    pub unit: TimeUnit,
}

impl Default for ActiveWindow {
    fn default() -> Self {
        Self {
            amount: NonZeroU32::MIN.saturating_add(9),
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
        Duration::from_secs(u64::from(self.amount.get()) * secs)
    }

    /// Parses the window length typed by the user: a whole number of at least 1.
    pub fn parse_amount(raw: &str) -> Option<NonZeroU32> {
        raw.trim().parse().ok()
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
#[derive(Debug, Clone)]
pub struct ActivityLog {
    last_chat: HashMap<UserKey, Instant>,
    /// Size at which the next pruning runs.
    prune_at: usize,
    /// Smallest `prune_at`; pruning never runs below this size.
    prune_floor: usize,
}

/// Default [`ActivityLog`] size before it starts dropping quiet chatters.
const PRUNE_ABOVE: usize = 20_000;
/// Activity is kept at least this long, so switching the condition on later still sees the past
/// day of chat.
const MIN_KEEP: Duration = Duration::from_secs(24 * 3600);

impl Default for ActivityLog {
    fn default() -> Self {
        Self::with_prune_above(PRUNE_ABOVE)
    }
}

impl ActivityLog {
    pub fn with_prune_above(prune_floor: usize) -> Self {
        Self {
            last_chat: HashMap::new(),
            prune_at: prune_floor,
            prune_floor,
        }
    }

    /// Records chat activity at `now`. When the log is large it drops chatters quiet for longer
    /// than `window` (and at least a day); afterwards it waits until it has doubled before pruning
    /// again, so a big, busy chat does not prune on every message.
    pub fn record(&mut self, user: &UserKey, now: Instant, window: Duration) {
        if self.last_chat.len() >= self.prune_at {
            let keep = window.max(MIN_KEEP);
            self.last_chat
                .retain(|_, seen| now.saturating_duration_since(*seen) <= keep);
            self.prune_at = (self.last_chat.len() * 2).max(self.prune_floor);
        }
        self.last_chat.insert(user.clone(), now);
    }

    pub fn last(&self, user: &UserKey) -> Option<Instant> {
        self.last_chat.get(user).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::platform::Platform;

    fn user(id: &str) -> UserKey {
        UserKey::new(Platform::Twitch, id)
    }

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
            amount: NonZeroU32::new(2).unwrap(),
            unit: TimeUnit::Hours,
        };
        assert_eq!(window.duration(), Duration::from_secs(7200));
    }

    #[test]
    fn window_amount_must_be_a_whole_number_from_one() {
        assert_eq!(
            ActiveWindow::parse_amount(" 15 ").map(NonZeroU32::get),
            Some(15)
        );
        for raw in ["0", "", " ", "-1", "1.5", "x"] {
            assert_eq!(ActiveWindow::parse_amount(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn log_keeps_the_latest_time_per_user() {
        let start = Instant::now();
        let mut log = ActivityLog::default();
        log.record(&user("1"), start, MIN);
        log.record(&user("1"), start + MIN, MIN);
        assert_eq!(log.last(&user("1")), Some(start + MIN));
        assert_eq!(log.last(&user("2")), None);
    }

    const HOUR: Duration = Duration::from_secs(3600);

    #[test]
    fn pruning_keeps_everyone_inside_a_long_window() {
        let start = Instant::now();
        let mut log = ActivityLog::with_prune_above(2);
        log.record(&user("old"), start, 48 * HOUR);
        log.record(&user("mid"), start + 18 * HOUR, 48 * HOUR);
        log.record(&user("new"), start + 30 * HOUR, 48 * HOUR);
        assert_eq!(log.last(&user("old")), Some(start));
    }

    #[test]
    fn pruning_drops_chatters_quiet_for_longer_than_a_day() {
        let start = Instant::now();
        let mut log = ActivityLog::with_prune_above(2);
        log.record(&user("old"), start, MIN);
        log.record(&user("mid"), start + 2 * HOUR, MIN);
        log.record(&user("new"), start + 25 * HOUR, MIN);
        assert_eq!(log.last(&user("old")), None);
        assert_eq!(log.last(&user("mid")), Some(start + 2 * HOUR));
    }

    #[test]
    fn a_busy_log_does_not_prune_on_every_message() {
        let start = Instant::now();
        let mut log = ActivityLog::with_prune_above(2);
        for i in 0..10 {
            log.record(&user(&i.to_string()), start, MIN);
        }
        // Nobody was old enough to drop, so each pruning doubled the threshold: 2, 4, 8, 16.
        assert_eq!(log.prune_at, 16);
        assert_eq!(log.last_chat.len(), 10);
    }
}

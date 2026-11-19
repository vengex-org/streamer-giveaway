//! Streaming platforms whose chats can feed one giveaway, and who a chatter is across them.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Twitch,
    Kick,
    YouTube,
}

impl Platform {
    pub const ALL: [Platform; 3] = [Platform::Twitch, Platform::Kick, Platform::YouTube];

    /// Lowercase name, as written to the CSV log and the config.
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Twitch => "twitch",
            Platform::Kick => "kick",
            Platform::YouTube => "youtube",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == raw)
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One chat account: the same id on two platforms belongs to two different people.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UserKey {
    pub platform: Platform,
    pub id: String,
}

impl UserKey {
    pub fn new(platform: Platform, id: impl Into<String>) -> Self {
        Self {
            platform,
            id: id.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for platform in Platform::ALL {
            assert_eq!(Platform::parse(platform.as_str()), Some(platform));
        }
        assert_eq!(Platform::parse("Twitch"), None);
        assert_eq!(Platform::parse(""), None);
    }

    #[test]
    fn same_id_on_two_platforms_is_two_users() {
        assert_ne!(
            UserKey::new(Platform::Twitch, "42"),
            UserKey::new(Platform::Kick, "42")
        );
    }
}

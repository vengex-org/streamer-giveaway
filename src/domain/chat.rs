use serde::{Deserialize, Serialize};

use super::badges::BadgeUrl;
use super::platform::{Platform, UserKey};
use super::role::{Role, kick_roles, roles_from_badges, youtube_roles};

/// A chat badge as Twitch sends it in the `badges` tag, e.g. `subscriber/3012`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Badge {
    pub name: String,
    pub version: String,
}

impl Badge {
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
        }
    }
}

/// One chat message, reduced to what the giveaway needs. Built only through the per-platform
/// constructors, which map that platform's badges to roles, so a message cannot carry another
/// platform's badge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    user: UserKey,
    display_name: String,
    text: String,
    roles: Vec<Role>,
    is_owner: bool,
    sub_badge: Option<SubBadge>,
}

/// What picks a subscriber's badge picture. Twitch and Kick name a badge of the channel's own
/// list (loaded on connect); YouTube sends the picture with the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubBadge {
    /// Twitch subscriber badge version, e.g. `3012`.
    Twitch(String),
    /// Kick subscription length in months.
    Kick(u32),
    /// YouTube member badge picture.
    YouTube(BadgeUrl),
}

/// A Kick chat badge as Kick sends it: `type` such as `subscriber`, and for subs the months.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KickBadge {
    pub kind: String,
    pub months: Option<u32>,
}

/// The YouTube author badges the giveaway cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YouTubeBadge {
    Owner,
    Moderator,
    /// Channel member; the picture when YouTube sent an allowed one.
    Member(Option<BadgeUrl>),
}

impl ChatMessage {
    pub fn twitch(
        id: impl Into<String>,
        display_name: impl Into<String>,
        text: impl Into<String>,
        badges: &[Badge],
    ) -> Self {
        Self {
            user: UserKey::new(Platform::Twitch, id),
            display_name: display_name.into(),
            text: text.into(),
            roles: roles_from_badges(badges),
            is_owner: badges.iter().any(|b| b.name == "broadcaster"),
            sub_badge: badges
                .iter()
                .find(|b| b.name == "subscriber")
                .map(|b| SubBadge::Twitch(b.version.clone())),
        }
    }

    pub fn kick(
        id: impl Into<String>,
        display_name: impl Into<String>,
        text: impl Into<String>,
        badges: &[KickBadge],
    ) -> Self {
        Self {
            user: UserKey::new(Platform::Kick, id),
            display_name: display_name.into(),
            text: text.into(),
            roles: kick_roles(badges.iter().map(|b| b.kind.as_str())),
            is_owner: badges.iter().any(|b| b.kind == "broadcaster"),
            sub_badge: badges
                .iter()
                .find(|b| b.kind == "subscriber")
                .and_then(|b| b.months)
                .map(SubBadge::Kick),
        }
    }

    pub fn youtube(
        channel_id: impl Into<String>,
        display_name: impl Into<String>,
        text: impl Into<String>,
        badges: &[YouTubeBadge],
    ) -> Self {
        Self {
            user: UserKey::new(Platform::YouTube, channel_id),
            display_name: display_name.into(),
            text: text.into(),
            roles: youtube_roles(badges),
            is_owner: badges.contains(&YouTubeBadge::Owner),
            sub_badge: badges.iter().find_map(|b| match b {
                YouTubeBadge::Member(Some(url)) => Some(SubBadge::YouTube(url.clone())),
                _ => None,
            }),
        }
    }

    pub fn user(&self) -> &UserKey {
        &self.user
    }

    pub fn platform(&self) -> Platform {
        self.user.platform
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn roles(&self) -> &[Role] {
        &self.roles
    }

    /// The channel owner; never enters their own giveaway.
    pub fn is_owner(&self) -> bool {
        self.is_owner
    }

    pub fn sub_badge(&self) -> Option<&SubBadge> {
        self.sub_badge.as_ref()
    }

    /// The same author saying something else.
    pub fn with_text(&self, text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..self.clone()
        }
    }
}

/// Giveaway keyword: trimmed, never empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Keyword {
    text: String,
    folded: String,
}

/// The user entered a keyword that is empty after trimming.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("keyword must not be empty")]
pub struct EmptyKeyword;

impl Keyword {
    pub fn parse(raw: &str) -> Result<Self, EmptyKeyword> {
        let text = raw.trim();
        if text.is_empty() {
            return Err(EmptyKeyword);
        }
        Ok(Self {
            text: text.to_owned(),
            folded: text.to_lowercase(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Lowercased form used for case-insensitive matching.
    pub fn folded(&self) -> &str {
        &self.folded
    }
}

/// Twitch channel login: lowercase `[a-z0-9_]{1,25}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ChannelLogin(String);

/// The user entered a channel name Twitch would not accept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid channel name '{0}': use 1-25 letters, digits or _")]
pub struct InvalidChannel(pub String);

impl ChannelLogin {
    pub fn parse(raw: &str) -> Result<Self, InvalidChannel> {
        let login = raw.trim().to_ascii_lowercase();
        let valid = (1..=25).contains(&login.len())
            && login
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if valid {
            Ok(Self(login))
        } else {
            Err(InvalidChannel(raw.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Keyword {
    type Error = EmptyKeyword;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(&raw)
    }
}

impl From<Keyword> for String {
    fn from(keyword: Keyword) -> Self {
        keyword.text
    }
}

impl TryFrom<String> for ChannelLogin {
    type Error = InvalidChannel;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(&raw)
    }
}

impl From<ChannelLogin> for String {
    fn from(channel: ChannelLogin) -> Self {
        channel.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_is_trimmed() {
        assert_eq!(Keyword::parse("  YEP ").unwrap().as_str(), "YEP");
    }

    #[test]
    fn keyword_rejects_empty_and_whitespace_only() {
        assert_eq!(Keyword::parse(""), Err(EmptyKeyword));
        assert_eq!(Keyword::parse(" \t "), Err(EmptyKeyword));
    }

    #[test]
    fn channel_is_trimmed_and_lowercased() {
        assert_eq!(
            ChannelLogin::parse(" Some_Streamer1 ").unwrap().as_str(),
            "some_streamer1"
        );
    }

    #[test]
    fn channel_rejects_hash_space_empty_and_26_chars() {
        let too_long = "a".repeat(26);
        for raw in ["#streamer", "two words", "", too_long.as_str()] {
            assert!(
                ChannelLogin::parse(raw).is_err(),
                "{raw:?} must be rejected"
            );
        }
    }

    #[test]
    fn channel_accepts_25_chars() {
        assert!(ChannelLogin::parse(&"a".repeat(25)).is_ok());
    }

    #[test]
    fn twitch_broadcaster_is_the_owner() {
        let viewer = ChatMessage::twitch("1", "S", "YEP", &[Badge::new("subscriber", "0")]);
        assert!(!viewer.is_owner());
        assert_eq!(viewer.sub_badge(), Some(&SubBadge::Twitch("0".into())));
        let owner = ChatMessage::twitch("1", "S", "YEP", &[Badge::new("broadcaster", "1")]);
        assert!(owner.is_owner());
    }

    #[test]
    fn kick_sub_months_pick_the_badge_and_broadcaster_is_the_owner() {
        let badge = |kind: &str, months| KickBadge {
            kind: kind.into(),
            months,
        };
        let sub = ChatMessage::kick("9", "K", "hi", &[badge("subscriber", Some(7))]);
        assert_eq!(sub.platform(), Platform::Kick);
        assert_eq!(sub.roles(), [Role::SubT1]);
        assert_eq!(sub.sub_badge(), Some(&SubBadge::Kick(7)));
        assert!(!sub.is_owner());
        let owner = ChatMessage::kick("9", "K", "hi", &[badge("broadcaster", None)]);
        assert!(owner.is_owner());
    }

    #[test]
    fn youtube_member_carries_its_picture_and_owner_is_detected() {
        let url = BadgeUrl::parse("https://yt3.ggpht.com/badge=s32").unwrap();
        let member =
            ChatMessage::youtube("UC1", "Y", "hi", &[YouTubeBadge::Member(Some(url.clone()))]);
        assert_eq!(member.roles(), [Role::SubT1]);
        assert_eq!(member.sub_badge(), Some(&SubBadge::YouTube(url)));
        let owner = ChatMessage::youtube("UC1", "Y", "hi", &[YouTubeBadge::Owner]);
        assert!(owner.is_owner());
    }
}

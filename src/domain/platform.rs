//! Streaming platforms whose chats can feed one giveaway, and who a chatter is across them.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    #[default]
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

/// Kick channel name as in `kick.com/<name>`: lowercase letters, digits, `_` and `-`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct KickChannel(String);

/// The user typed something that is not a Kick channel name or link.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid Kick channel '{0}': use the name from kick.com/<name>")]
pub struct InvalidKickChannel(pub String);

impl KickChannel {
    /// Accepts the bare name or a `kick.com/<name>` link.
    pub fn parse(raw: &str) -> Result<Self, InvalidKickChannel> {
        let invalid = || InvalidKickChannel(raw.to_owned());
        let lower = raw.trim().to_ascii_lowercase();
        let rest = strip_scheme(&lower);
        let name = match rest.strip_prefix("kick.com/") {
            Some(path) => path.split(['/', '?', '#']).next().unwrap_or_default(),
            None if rest.contains('/') || rest.contains('.') => return Err(invalid()),
            None => rest,
        };
        let valid = (1..=25).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
        valid.then(|| Self(name.to_owned())).ok_or_else(invalid)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for KickChannel {
    type Error = InvalidKickChannel;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(&raw)
    }
}

impl From<KickChannel> for String {
    fn from(channel: KickChannel) -> Self {
        channel.0
    }
}

/// Where to find a YouTube live chat: one stream, or a channel whose current live stream is used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum YouTubeSource {
    /// 11-character video id.
    Video(String),
    /// Channel handle without the `@`.
    Handle(String),
    /// Channel id, `UC` and 22 more characters.
    Channel(String),
}

/// The user typed something that is not a YouTube stream link, channel link or handle.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid YouTube source '{0}': use a stream link or the channel's @handle")]
pub struct InvalidYouTubeSource(pub String);

impl YouTubeSource {
    /// Accepts a stream link (`watch?v=`, `youtu.be/`, `/live/`), a channel link (`/@handle`,
    /// `/channel/UC...`), `@handle`, a bare handle or a channel id. A bare 11-character word is a
    /// handle, not a video id: handles that long exist, and streams come as links.
    pub fn parse(raw: &str) -> Result<Self, InvalidYouTubeSource> {
        let invalid = || InvalidYouTubeSource(raw.to_owned());
        let text = raw.trim();
        let rest = strip_scheme(text);
        let parsed = if let Some(path) = rest.strip_prefix("youtu.be/") {
            Some(Self::Video(first_segment(path).to_owned()))
        } else if let Some(path) = rest
            .strip_prefix("youtube.com/")
            .or_else(|| rest.strip_prefix("m.youtube.com/"))
        {
            Self::from_path(path)
        } else if rest.contains('/') || rest.contains('.') && !rest.starts_with('@') {
            None
        } else {
            let name = rest.strip_prefix('@').unwrap_or(rest);
            Some(Self::from_name(name))
        };
        parsed.filter(Self::is_valid).ok_or_else(invalid)
    }

    fn from_path(path: &str) -> Option<Self> {
        if let Some(query) = path.strip_prefix("watch?") {
            let id = query.split('&').find_map(|pair| pair.strip_prefix("v="))?;
            return Some(Self::Video(id.to_owned()));
        }
        if let Some(id) = path.strip_prefix("live/") {
            return Some(Self::Video(first_segment(id).to_owned()));
        }
        if let Some(id) = path.strip_prefix("channel/") {
            return Some(Self::Channel(first_segment(id).to_owned()));
        }
        path.strip_prefix('@')
            .map(|handle| Self::Handle(first_segment(handle).to_owned()))
    }

    fn from_name(name: &str) -> Self {
        if name.len() == 24 && name.starts_with("UC") {
            Self::Channel(name.to_owned())
        } else {
            Self::Handle(name.to_owned())
        }
    }

    fn is_valid(&self) -> bool {
        let id_chars = |s: &str| {
            s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        };
        match self {
            Self::Video(id) => id.len() == 11 && id_chars(id),
            Self::Channel(id) => id.len() == 24 && id.starts_with("UC") && id_chars(id),
            Self::Handle(handle) => {
                (3..=30).contains(&handle.len())
                    && handle
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
            }
        }
    }
}

impl std::fmt::Display for YouTubeSource {
    /// The form shown in the input and saved in the config; `parse` reads it back.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Video(id) => write!(f, "https://www.youtube.com/watch?v={id}"),
            Self::Handle(handle) => write!(f, "@{handle}"),
            Self::Channel(id) => write!(f, "https://www.youtube.com/channel/{id}"),
        }
    }
}

impl TryFrom<String> for YouTubeSource {
    type Error = InvalidYouTubeSource;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        Self::parse(&raw)
    }
}

impl From<YouTubeSource> for String {
    fn from(source: YouTubeSource) -> Self {
        source.to_string()
    }
}

fn strip_scheme(text: &str) -> &str {
    let text = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
        .unwrap_or(text);
    text.strip_prefix("www.").unwrap_or(text)
}

fn first_segment(path: &str) -> &str {
    path.split(['/', '?', '#', '&']).next().unwrap_or_default()
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

    #[test]
    fn kick_channel_takes_a_name_or_a_link() {
        for raw in [
            "xqc",
            " XQC ",
            "kick.com/xqc",
            "https://kick.com/xqc",
            "https://www.kick.com/xqc/videos",
        ] {
            assert_eq!(KickChannel::parse(raw).unwrap().as_str(), "xqc", "{raw:?}");
        }
        assert_eq!(
            KickChannel::parse("some-one_2").unwrap().as_str(),
            "some-one_2"
        );
    }

    #[test]
    fn kick_channel_rejects_empty_spaces_other_sites_and_long_names() {
        let long = "a".repeat(26);
        for raw in [
            "",
            " ",
            "two words",
            "twitch.tv/xqc",
            "https://evil.com/xqc",
            "kick.com/",
            long.as_str(),
        ] {
            assert!(KickChannel::parse(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn youtube_source_reads_stream_and_channel_links() {
        let video = YouTubeSource::Video("qSyo5d-dVzA".into());
        for raw in [
            "https://www.youtube.com/watch?v=qSyo5d-dVzA",
            "https://www.youtube.com/watch?feature=share&v=qSyo5d-dVzA&t=10",
            "youtu.be/qSyo5d-dVzA?si=x",
            "https://www.youtube.com/live/qSyo5d-dVzA?si=x",
            "https://m.youtube.com/watch?v=qSyo5d-dVzA",
        ] {
            assert_eq!(YouTubeSource::parse(raw).unwrap(), video, "{raw:?}");
        }
        let handle = YouTubeSource::Handle("IShowSpeed".into());
        for raw in [
            "@IShowSpeed",
            "IShowSpeed",
            " https://www.youtube.com/@IShowSpeed/live ",
        ] {
            assert_eq!(YouTubeSource::parse(raw).unwrap(), handle, "{raw:?}");
        }
        let channel = YouTubeSource::Channel("UCWsDFcIhY2DBi3GB5uykGXA".into());
        for raw in [
            "https://www.youtube.com/channel/UCWsDFcIhY2DBi3GB5uykGXA",
            "UCWsDFcIhY2DBi3GB5uykGXA",
        ] {
            assert_eq!(YouTubeSource::parse(raw).unwrap(), channel, "{raw:?}");
        }
    }

    #[test]
    fn a_bare_eleven_character_word_is_a_handle() {
        assert_eq!(
            YouTubeSource::parse("MrBeast6000").unwrap(),
            YouTubeSource::Handle("MrBeast6000".into())
        );
    }

    #[test]
    fn youtube_source_rejects_empty_spaces_other_sites_and_broken_ids() {
        for raw in [
            "",
            " ",
            "@",
            "two words",
            "https://twitch.tv/xqc",
            "https://vimeo.com/123",
            "https://www.youtube.com/watch?v=short",
            "https://www.youtube.com/feed/subscriptions",
        ] {
            assert!(YouTubeSource::parse(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn youtube_source_round_trips_through_its_saved_form() {
        for raw in [
            "youtu.be/qSyo5d-dVzA",
            "@IShowSpeed",
            "UCWsDFcIhY2DBi3GB5uykGXA",
        ] {
            let source = YouTubeSource::parse(raw).unwrap();
            assert_eq!(YouTubeSource::parse(&source.to_string()).unwrap(), source);
        }
    }
}

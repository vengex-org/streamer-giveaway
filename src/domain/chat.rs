use std::fmt;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatMessage {
    pub user_id: String,
    pub login: String,
    pub display_name: String,
    pub badges: Vec<Badge>,
    pub text: String,
}

impl ChatMessage {
    pub fn is_broadcaster(&self) -> bool {
        self.badges.iter().any(|b| b.name == "broadcaster")
    }
}

/// Giveaway keyword: trimmed, never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyword {
    text: String,
    folded: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyKeyword;

impl fmt::Display for EmptyKeyword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("keyword must not be empty")
    }
}

impl std::error::Error for EmptyKeyword {}

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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelLogin(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidChannel(pub String);

impl fmt::Display for InvalidChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid channel name '{}': use 1-25 letters, digits or _",
            self.0
        )
    }
}

impl std::error::Error for InvalidChannel {}

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
    fn broadcaster_is_detected_from_badge() {
        let mut msg = ChatMessage {
            user_id: "1".into(),
            login: "s".into(),
            display_name: "S".into(),
            badges: vec![Badge::new("subscriber", "0")],
            text: "YEP".into(),
        };
        assert!(!msg.is_broadcaster());
        msg.badges.push(Badge::new("broadcaster", "1"));
        assert!(msg.is_broadcaster());
    }
}

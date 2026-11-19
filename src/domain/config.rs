use serde::{Deserialize, Serialize};

use super::chat::{ChannelLogin, Keyword};
use super::eligibility::Eligibility;
use super::matcher::MatchMode;
use super::platform::{KickChannel, Platform, YouTubeSource};
use super::prefs::{Language, ThemeChoice, lenient, lenient_or_default};
use super::role::Weights;

/// User settings kept across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// One platform, or several chats at once.
    #[serde(deserialize_with = "lenient_or_default")]
    pub chat_mode: ChatMode,
    /// Twitch channel.
    pub channel: Option<ChannelLogin>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub kick_channel: Option<KickChannel>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub youtube_source: Option<YouTubeSource>,
    pub keyword: Keyword,
    pub mode: MatchMode,
    pub weights: Weights,
    /// A value this version cannot read falls back to "no conditions", keeping other settings.
    #[serde(deserialize_with = "lenient_or_default")]
    pub eligibility: Eligibility,
    /// None = follow the OS language.
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    /// None = follow the OS appearance.
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeChoice>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            chat_mode: ChatMode::default(),
            channel: None,
            kick_channel: None,
            youtube_source: None,
            keyword: Keyword::parse("YEP").expect("default keyword is not empty"),
            mode: MatchMode::default(),
            weights: Weights::default(),
            eligibility: Eligibility::default(),
            language: None,
            theme: None,
        }
    }
}

/// Whether the giveaway reads one platform's chat or several at once (multistream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatMode {
    Single(Platform),
    Multi,
}

impl ChatMode {
    /// The platforms this mode reads.
    pub fn platforms(self) -> Vec<Platform> {
        match self {
            Self::Single(platform) => vec![platform],
            Self::Multi => Platform::ALL.to_vec(),
        }
    }

    /// Chats to leave when switching to this mode: one platform keeps only its own chat.
    pub fn chats_to_leave(self, connected: &[Platform]) -> Vec<Platform> {
        connected
            .iter()
            .copied()
            .filter(|platform| !self.platforms().contains(platform))
            .collect()
    }
}

impl Default for ChatMode {
    fn default() -> Self {
        Self::Single(Platform::Twitch)
    }
}

impl Config {
    pub fn from_toml(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }

    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::role::Role;

    #[test]
    fn round_trip_keeps_every_setting() {
        let mut config = Config {
            channel: Some(ChannelLogin::parse("streamer").unwrap()),
            keyword: Keyword::parse("!join").unwrap(),
            mode: MatchMode::Exact,
            ..Config::default()
        };
        config.weights.set(Role::SubT3, 10);
        let text = config.to_toml().unwrap();
        assert_eq!(Config::from_toml(&text).unwrap(), config);
    }

    #[test]
    fn language_and_theme_round_trip() {
        let config = Config {
            language: Some(Language::Cs),
            theme: Some(ThemeChoice::Light),
            ..Config::default()
        };
        let text = config.to_toml().unwrap();
        assert_eq!(Config::from_toml(&text).unwrap(), config);
    }

    #[test]
    fn unknown_language_or_theme_is_unset_and_keeps_other_settings() {
        let config = Config::from_toml(
            "keyword = \"!join\"
language = \"de\"
theme = \"neon\"
",
        )
        .unwrap();
        assert_eq!(config.language, None);
        assert_eq!(config.theme, None);
        assert_eq!(config.keyword.as_str(), "!join");
    }

    #[test]
    fn eligibility_round_trips_and_defaults_to_off() {
        use crate::domain::eligibility::{ActiveWindow, TimeUnit};
        let config = Config {
            eligibility: Eligibility {
                subs_only: true,
                require_activity: true,
                active_window: ActiveWindow {
                    amount: std::num::NonZeroU32::new(2).unwrap(),
                    unit: TimeUnit::Hours,
                },
            },
            ..Config::default()
        };
        let text = config.to_toml().unwrap();
        assert_eq!(Config::from_toml(&text).unwrap(), config);
        assert_eq!(
            Config::from_toml("").unwrap().eligibility,
            Eligibility::default()
        );
    }

    #[test]
    fn unreadable_conditions_fall_back_to_none_and_keep_other_settings() {
        for text in [
            "keyword = \"!join\"\n[eligibility]\nsubs_only = true\nactive_window = { amount = 0 }\n",
            "keyword = \"!join\"\n[eligibility]\nsubs_only = true\nactive_window = { unit = \"days\" }\n",
        ] {
            let config = Config::from_toml(text).unwrap();
            assert_eq!(config.eligibility, Eligibility::default(), "{text}");
            assert_eq!(config.keyword.as_str(), "!join");
        }
    }

    #[test]
    fn empty_text_gives_defaults() {
        assert_eq!(Config::from_toml("").unwrap(), Config::default());
    }

    #[test]
    fn missing_weights_keep_their_defaults_and_unknown_keys_are_ignored() {
        let config = Config::from_toml("[weights]\npleb = 5\nfollower = 9\n").unwrap();
        assert_eq!(config.weights.pleb, 5);
        assert_eq!(config.weights.sub_t1, Weights::default().sub_t1);
    }

    #[test]
    fn empty_keyword_is_rejected() {
        assert!(Config::from_toml("keyword = \"  \"\n").is_err());
    }

    #[test]
    fn invalid_channel_is_rejected() {
        assert!(Config::from_toml("channel = \"#bad name\"\n").is_err());
    }

    #[test]
    fn platform_choice_and_inputs_round_trip() {
        for chat_mode in [ChatMode::Single(Platform::Kick), ChatMode::Multi] {
            let config = Config {
                chat_mode,
                kick_channel: Some(KickChannel::parse("xqc").unwrap()),
                youtube_source: Some(YouTubeSource::parse("@IShowSpeed").unwrap()),
                ..Config::default()
            };
            let text = config.to_toml().unwrap();
            assert_eq!(Config::from_toml(&text).unwrap(), config, "{text}");
        }
    }

    #[test]
    fn a_settings_file_from_before_kick_and_youtube_loads_as_twitch_only() {
        let text = "channel = \"streamer\"\nkeyword = \"YEP\"\nmode = \"word\"\n";
        let config = Config::from_toml(text).unwrap();
        assert_eq!(config.chat_mode, ChatMode::Single(Platform::Twitch));
        assert_eq!(config.kick_channel, None);
        assert_eq!(config.youtube_source, None);
        assert_eq!(config.channel.unwrap().as_str(), "streamer");
    }

    #[test]
    fn an_unreadable_platform_setting_is_dropped_and_the_rest_kept() {
        let text = "chat_mode = \"teams\"\nkick_channel = \"a b\"\nyoutube_source = \"https://vimeo.com/1\"\nkeyword = \"GO\"\n";
        let config = Config::from_toml(text).unwrap();
        assert_eq!(config.chat_mode, ChatMode::default());
        assert_eq!(config.kick_channel, None);
        assert_eq!(config.youtube_source, None);
        assert_eq!(config.keyword.as_str(), "GO");
    }

    #[test]
    fn one_platform_mode_leaves_the_other_chats_and_multistream_keeps_all() {
        let connected = [Platform::Twitch, Platform::Kick];
        assert_eq!(
            ChatMode::Single(Platform::Kick).chats_to_leave(&connected),
            [Platform::Twitch]
        );
        assert_eq!(
            ChatMode::Single(Platform::YouTube).chats_to_leave(&connected),
            [Platform::Twitch, Platform::Kick]
        );
        assert!(ChatMode::Multi.chats_to_leave(&connected).is_empty());
    }
}

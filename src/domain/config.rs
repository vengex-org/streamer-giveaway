use serde::{Deserialize, Serialize};

use super::chat::{ChannelLogin, Keyword};
use super::eligibility::Eligibility;
use super::matcher::MatchMode;
use super::prefs::{Language, ThemeChoice, lenient};
use super::role::Weights;

/// User settings kept across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub channel: Option<ChannelLogin>,
    pub keyword: Keyword,
    pub mode: MatchMode,
    pub weights: Weights,
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
            channel: None,
            keyword: Keyword::parse("YEP").expect("default keyword is not empty"),
            mode: MatchMode::default(),
            weights: Weights::default(),
            eligibility: Eligibility::default(),
            language: None,
            theme: None,
        }
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
                    amount: 2,
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
}

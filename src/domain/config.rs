use serde::{Deserialize, Serialize};

use super::chat::{ChannelLogin, Keyword};
use super::matcher::MatchMode;
use super::role::Weights;

/// User settings kept across restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub channel: Option<ChannelLogin>,
    pub keyword: Keyword,
    pub mode: MatchMode,
    pub weights: Weights,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            channel: None,
            keyword: Keyword::parse("YEP").expect("default keyword is not empty"),
            mode: MatchMode::default(),
            weights: Weights::default(),
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
            weights: Weights::default(),
        };
        config.weights.set(Role::SubT3, 10);
        let text = config.to_toml().unwrap();
        assert_eq!(Config::from_toml(&text).unwrap(), config);
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

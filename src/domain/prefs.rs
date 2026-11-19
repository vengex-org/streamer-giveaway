use serde::{Deserialize, Deserializer, Serialize};

/// UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Sk,
    En,
    Cs,
    Es,
}

impl Language {
    pub const ALL: [Language; 4] = [Language::Sk, Language::En, Language::Cs, Language::Es];

    /// The locale code the translation files use.
    pub fn code(self) -> &'static str {
        match self {
            Language::Sk => "sk",
            Language::En => "en",
            Language::Cs => "cs",
            Language::Es => "es",
        }
    }
}

/// UI color theme chosen by the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    Dark,
    Light,
}

/// The theme to show: the user's choice, or the OS appearance when they have not chosen.
pub fn effective_theme(choice: Option<ThemeChoice>, os_dark: bool) -> ThemeChoice {
    choice.unwrap_or(if os_dark {
        ThemeChoice::Dark
    } else {
        ThemeChoice::Light
    })
}

/// Reads an optional setting, treating a value this version does not know (e.g. a language added
/// later) as unset instead of failing the whole settings file.
pub fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = toml::Value::deserialize(deserializer)?;
    Ok(value.try_into().ok())
}

/// Reads a setting, falling back to its default when this version cannot read the value (e.g. a
/// time unit added later), instead of failing the whole settings file.
pub fn lenient_or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value = toml::Value::deserialize(deserializer)?;
    Ok(value.try_into().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_theme_follows_the_os() {
        assert_eq!(effective_theme(None, true), ThemeChoice::Dark);
        assert_eq!(effective_theme(None, false), ThemeChoice::Light);
    }

    #[test]
    fn chosen_theme_wins_over_the_os() {
        assert_eq!(
            effective_theme(Some(ThemeChoice::Light), true),
            ThemeChoice::Light
        );
        assert_eq!(
            effective_theme(Some(ThemeChoice::Dark), false),
            ThemeChoice::Dark
        );
    }
}

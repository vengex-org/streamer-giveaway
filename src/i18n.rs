//! UI language: picking it from the OS and switching translations. Texts live in
//! `locales/app.toml`.

use rust_i18n::t;

use crate::domain::matcher::MatchMode;
use crate::domain::prefs::Language;
use crate::domain::role::Role;

/// Maps an OS locale such as `sk-SK`, `es_MX.UTF-8` or `C` to a supported language; anything
/// unknown is English.
pub fn language_from_locale(locale: Option<&str>) -> Language {
    let primary = locale
        .and_then(|l| l.split(['-', '_', '.']).next())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match primary.as_str() {
        "sk" => Language::Sk,
        "cs" => Language::Cs,
        "es" => Language::Es,
        _ => Language::En,
    }
}

pub fn os_language() -> Language {
    language_from_locale(sys_locale::get_locale().as_deref())
}

/// Switches every translated text, ours and the widget library's, to `language`.
pub fn apply(language: Language) {
    rust_i18n::set_locale(language.code());
}

pub fn role_label(role: Role) -> String {
    match role {
        Role::Pleb => t!("role.pleb"),
        Role::SubT1 => t!("role.sub_t1"),
        Role::SubT2 => t!("role.sub_t2"),
        Role::SubT3 => t!("role.sub_t3"),
        Role::Vip => t!("role.vip"),
        Role::Mod => t!("role.mod"),
    }
    .into_owned()
}

pub fn mode_label(mode: MatchMode) -> String {
    match mode {
        MatchMode::Word => t!("mode.word"),
        MatchMode::Exact => t!("mode.exact"),
        MatchMode::Contains => t!("mode.contains"),
    }
    .into_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    const LOCALES: &str = include_str!("../locales/app.toml");

    /// Every translated text in the file, as (key, language -> text).
    fn leaves() -> Vec<(String, toml::Table)> {
        fn walk(prefix: &str, table: &toml::Table, out: &mut Vec<(String, toml::Table)>) {
            for (key, value) in table {
                let Some(inner) = value.as_table() else {
                    continue;
                };
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                if inner.values().all(toml::Value::is_str) {
                    out.push((path, inner.clone()));
                } else {
                    walk(&path, inner, out);
                }
            }
        }
        let root: toml::Table = LOCALES.parse().expect("locales/app.toml is valid TOML");
        let mut out = Vec::new();
        walk("", &root, &mut out);
        out
    }

    fn placeholders(text: &str) -> BTreeSet<&str> {
        text.match_indices("%{")
            .filter_map(|(start, _)| {
                let rest = &text[start + 2..];
                rest.find('}').map(|end| &rest[..end])
            })
            .collect()
    }

    #[test]
    fn every_text_exists_in_all_four_languages() {
        let expected: BTreeSet<&str> = Language::ALL.iter().map(|l| l.code()).collect();
        let leaves = leaves();
        assert!(leaves.len() > 50, "only {} texts found", leaves.len());
        for (key, texts) in leaves {
            let found: BTreeSet<&str> = texts.keys().map(String::as_str).collect();
            assert_eq!(found, expected, "{key}");
        }
    }

    #[test]
    fn every_language_uses_the_same_placeholders() {
        for (key, texts) in leaves() {
            let english = placeholders(texts["en"].as_str().unwrap());
            for (lang, text) in &texts {
                assert_eq!(
                    placeholders(text.as_str().unwrap()),
                    english,
                    "{key} in {lang}"
                );
            }
        }
    }

    #[test]
    fn translations_are_loaded_per_language() {
        assert_eq!(t!("mode.word", locale = "sk"), "Slovo");
        assert_eq!(t!("mode.word", locale = "es"), "Palabra");
        assert_eq!(
            t!("winner.since", time = "0:42", locale = "en"),
            "0:42 since the draw"
        );
    }

    #[test]
    fn os_locales_map_to_supported_languages() {
        let cases = [
            (None, Language::En),
            (Some(""), Language::En),
            (Some("C"), Language::En),
            (Some("en_US.UTF-8"), Language::En),
            (Some("de-DE"), Language::En),
            (Some("SK"), Language::Sk),
            (Some("sk_SK"), Language::Sk),
            (Some("sk-SK"), Language::Sk),
            (Some("cs"), Language::Cs),
            (Some("es-MX"), Language::Es),
        ];
        for (locale, expected) in cases {
            assert_eq!(language_from_locale(locale), expected, "{locale:?}");
        }
    }
}

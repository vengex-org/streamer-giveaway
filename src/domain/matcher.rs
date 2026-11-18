use serde::{Deserialize, Serialize};

use super::chat::Keyword;

/// How a chat message has to contain the keyword to count as an entry. All modes ignore case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MatchMode {
    /// The keyword as a whole word anywhere in the message.
    #[default]
    Word,
    /// The trimmed message equals the keyword.
    Exact,
    /// The keyword anywhere, also inside a longer word.
    Contains,
}

impl MatchMode {
    pub const ALL: [MatchMode; 3] = [MatchMode::Word, MatchMode::Exact, MatchMode::Contains];

    pub fn label(self) -> &'static str {
        match self {
            MatchMode::Word => "Word",
            MatchMode::Exact => "Whole message",
            MatchMode::Contains => "Contains",
        }
    }
}

/// Chatterino and 7TV append this invisible tag character to a repeated message so Twitch does
/// not drop it as a duplicate.
const DUPLICATE_BYPASS: char = '\u{E0000}';

pub fn matches(mode: MatchMode, keyword: &Keyword, text: &str) -> bool {
    let needle = keyword.folded();
    let text = text.replace(DUPLICATE_BYPASS, "");
    match mode {
        MatchMode::Exact => text.trim().to_lowercase() == needle,
        MatchMode::Contains => text.to_lowercase().contains(needle),
        MatchMode::Word => {
            let haystack = text.to_lowercase();
            haystack.match_indices(needle).any(|(start, _)| {
                let before = haystack[..start].chars().next_back();
                let after = haystack[start + needle.len()..].chars().next();
                !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
            })
        }
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kw(s: &str) -> Keyword {
        Keyword::parse(s).unwrap()
    }

    #[test]
    fn word_mode_finds_keyword_among_other_words_ignoring_case() {
        assert!(matches(MatchMode::Word, &kw("YEP"), "yep lets go"));
        assert!(matches(MatchMode::Word, &kw("YEP"), "ok, YEP!"));
    }

    #[test]
    fn word_mode_rejects_keyword_inside_longer_word() {
        assert!(!matches(MatchMode::Word, &kw("YEP"), "YEPPERS"));
        assert!(!matches(MatchMode::Word, &kw("YEP"), "noYEP"));
        assert!(!matches(MatchMode::Word, &kw("YEP"), "YEP_clap"));
    }

    #[test]
    fn word_mode_skips_embedded_hit_and_finds_later_standalone_one() {
        assert!(matches(MatchMode::Word, &kw("YEP"), "YEPPERS YEP"));
    }

    #[test]
    fn word_mode_handles_symbol_keyword() {
        assert!(matches(MatchMode::Word, &kw("!yep"), "!YEP please"));
        assert!(!matches(MatchMode::Word, &kw("!yep"), "!yepz"));
    }

    #[test]
    fn matching_folds_unicode_case() {
        assert!(matches(MatchMode::Word, &kw("straße"), "STRAẞE"));
        assert!(matches(MatchMode::Exact, &kw("ŽLUŤOUČKÝ"), "žluťoučký"));
    }

    #[test]
    fn exact_mode_trims_message_but_rejects_extra_words() {
        assert!(matches(MatchMode::Exact, &kw("YEP"), "  yep \n"));
        assert!(!matches(MatchMode::Exact, &kw("YEP"), "YEP YEP"));
    }

    #[test]
    fn duplicate_bypass_character_is_ignored() {
        assert!(matches(MatchMode::Exact, &kw("YEP"), "YEP \u{E0000}"));
        assert!(matches(MatchMode::Word, &kw("YEP"), "YEP\u{E0000}"));
    }

    #[test]
    fn contains_mode_matches_inside_words() {
        assert!(matches(MatchMode::Contains, &kw("YEP"), "YEPPERS"));
        assert!(!matches(MatchMode::Contains, &kw("YEP"), "nope"));
    }
}

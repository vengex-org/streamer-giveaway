//! Whether a release newer than this build is out, read from GitHub's latest release.

use serde::Deserialize;

/// GitHub API address of the newest published release (drafts and pre-releases are left out).
pub fn latest_release_url() -> String {
    let repo = env!("CARGO_PKG_REPOSITORY").trim_start_matches("https://github.com/");
    format!("https://api.github.com/repos/{repo}/releases/latest")
}

/// A newer release: its version without the `v` and the page to download it from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub url: String,
}

#[derive(Deserialize)]
struct LatestJson {
    tag_name: String,
    html_url: String,
}

/// The latest release when it is newer than `current`. A tag that is not `vMAJOR.MINOR.PATCH`
/// is never offered, since it cannot be compared.
pub fn newer_release(json: &str, current: &str) -> Result<Option<Release>, serde_json::Error> {
    let latest: LatestJson = serde_json::from_str(json)?;
    let version = latest.tag_name.trim_start_matches('v');
    let newer = matches!(
        (parse(version), parse(current)),
        (Some(latest), Some(current)) if latest > current
    );
    Ok(newer.then(|| Release {
        version: version.to_owned(),
        url: latest.html_url,
    }))
}

fn parse(version: &str) -> Option<[u64; 3]> {
    let mut parts = version.split('.').map(|part| part.parse::<u64>().ok());
    let parsed = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latest(tag: &str) -> String {
        format!(r#"{{"tag_name":"{tag}","html_url":"https://example.test/{tag}","draft":false}}"#)
    }

    #[test]
    fn a_higher_version_is_offered_with_its_page() {
        assert_eq!(
            newer_release(&latest("v0.5.0"), "0.4.2").unwrap(),
            Some(Release {
                version: "0.5.0".into(),
                url: "https://example.test/v0.5.0".into()
            })
        );
    }

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(
            newer_release(&latest("v0.4.10"), "0.4.9")
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn the_same_or_an_older_version_is_not_offered() {
        assert_eq!(newer_release(&latest("v0.4.2"), "0.4.2").unwrap(), None);
        assert_eq!(newer_release(&latest("v0.4.1"), "0.4.2").unwrap(), None);
    }

    #[test]
    fn a_tag_that_is_not_a_plain_version_is_not_offered() {
        assert_eq!(newer_release(&latest("v1.0.0-rc1"), "0.4.2").unwrap(), None);
        assert_eq!(newer_release(&latest("nightly"), "0.4.2").unwrap(), None);
    }

    #[test]
    fn the_url_names_this_repository() {
        assert_eq!(
            latest_release_url(),
            "https://api.github.com/repos/vengex-org/streamer-giveaway/releases/latest"
        );
    }
}

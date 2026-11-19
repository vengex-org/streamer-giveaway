//! A channel's own subscriber badge images. Chat only sends the badge version, such as
//! `subscriber/3012`; the pictures come from the community badge list at api.ivr.fi, which
//! needs no login. Without it the app shows the role name alone.

use std::collections::HashMap;

use serde::Deserialize;

use super::chat::ChannelLogin;

/// Badge list of one channel.
pub fn channel_badges_url(channel: &ChannelLogin) -> String {
    format!(
        "https://api.ivr.fi/v2/twitch/badges/channel?login={}",
        channel.as_str()
    )
}

#[derive(Deserialize)]
struct BadgeSet {
    set_id: String,
    versions: Vec<BadgeVersion>,
}

#[derive(Deserialize)]
struct BadgeVersion {
    id: String,
    image_url_2x: String,
}

/// Twitch's badge image host. The list comes from a third party, so any other address is dropped
/// and the app never loads a URL it was merely told to load.
const BADGE_CDN: &str = "https://static-cdn.jtvnw.net/";

/// Subscriber badge version (as chat sends it) to image URL, keeping only Twitch CDN images.
pub fn subscriber_badge_images(json: &str) -> Result<HashMap<String, String>, serde_json::Error> {
    let sets: Vec<BadgeSet> = serde_json::from_str(json)?;
    Ok(sets
        .into_iter()
        .filter(|set| set.set_id == "subscriber")
        .flat_map(|set| set.versions)
        .filter(|version| version.image_url_2x.starts_with(BADGE_CDN))
        .map(|version| (version.id, version.image_url_2x))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_subscriber_badges_are_kept() {
        let json = r#"[
            {"set_id":"bits","versions":[{"id":"100","image_url_2x":"https://b/100"}]},
            {"set_id":"subscriber","versions":[
                {"id":"0","image_url_2x":"https://static-cdn.jtvnw.net/badges/v1/a/2","title":"Subscriber"},
                {"id":"3012","image_url_2x":"https://static-cdn.jtvnw.net/badges/v1/b/2"}
            ]}
        ]"#;
        let images = subscriber_badge_images(json).unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images["3012"], "https://static-cdn.jtvnw.net/badges/v1/b/2");
    }

    #[test]
    fn images_outside_the_twitch_cdn_are_dropped() {
        let json = r#"[{"set_id":"subscriber","versions":[
            {"id":"0","image_url_2x":"http://static-cdn.jtvnw.net/badges/v1/a/2"},
            {"id":"3","image_url_2x":"https://static-cdn.jtvnw.net.evil.example/x"},
            {"id":"6","image_url_2x":"file:///C:/Windows/win.ini"},
            {"id":"9","image_url_2x":"http://127.0.0.1:8080/admin"}
        ]}]"#;
        assert!(subscriber_badge_images(json).unwrap().is_empty());
    }

    #[test]
    fn an_unexpected_answer_is_an_error() {
        assert!(subscriber_badge_images(r#"{"error":"not found"}"#).is_err());
    }

    #[test]
    fn the_url_names_the_channel() {
        let channel = ChannelLogin::parse("xqc").unwrap();
        assert!(channel_badges_url(&channel).ends_with("?login=xqc"));
    }
}

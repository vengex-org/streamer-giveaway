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

/// Subscriber badge version (as chat sends it) to image URL.
pub fn subscriber_badge_images(json: &str) -> Result<HashMap<String, String>, serde_json::Error> {
    let sets: Vec<BadgeSet> = serde_json::from_str(json)?;
    Ok(sets
        .into_iter()
        .filter(|set| set.set_id == "subscriber")
        .flat_map(|set| set.versions)
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
                {"id":"0","image_url_2x":"https://s/0","title":"Subscriber"},
                {"id":"3012","image_url_2x":"https://s/3012"}
            ]}
        ]"#;
        let images = subscriber_badge_images(json).unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images["3012"], "https://s/3012");
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

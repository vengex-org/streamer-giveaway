//! A channel's own subscriber badge images. Twitch chat only sends the badge version, such as
//! `subscriber/3012`, and the pictures come from the community badge list at api.ivr.fi, which
//! needs no login. Kick chat sends the months subscribed, and the channel API lists a picture per
//! month tier. YouTube sends the member picture with each message. Without a picture the app
//! shows the role name alone.

use std::collections::HashMap;

use serde::Deserialize;

use super::chat::{ChannelLogin, SubBadge};
use super::platform::Platform;

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

/// Image hosts badge pictures may come from: Twitch's CDN, Kick's file host and YouTube's image
/// host. The addresses come from third parties, so any other address is dropped and the app never
/// loads a URL it was merely told to load.
const BADGE_HOSTS: [&str; 3] = [
    "https://static-cdn.jtvnw.net/",
    "https://files.kick.com/",
    "https://yt3.ggpht.com/",
];

/// A badge picture address on one of the allowed image hosts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BadgeUrl(String);

impl BadgeUrl {
    /// `None` for anything outside `BADGE_HOSTS`.
    pub fn parse(raw: &str) -> Option<Self> {
        BADGE_HOSTS
            .iter()
            .any(|host| raw.starts_with(host))
            .then(|| Self(raw.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Subscriber badge version (as chat sends it) to picture, keeping only allowed image hosts.
pub fn subscriber_badge_images(json: &str) -> Result<HashMap<String, BadgeUrl>, serde_json::Error> {
    let sets: Vec<BadgeSet> = serde_json::from_str(json)?;
    Ok(sets
        .into_iter()
        .filter(|set| set.set_id == "subscriber")
        .flat_map(|set| set.versions)
        .filter_map(|version| Some((version.id, BadgeUrl::parse(&version.image_url_2x)?)))
        .collect())
}

#[derive(Deserialize)]
struct KickChannelBadges {
    #[serde(default)]
    subscriber_badges: Vec<KickTier>,
}

#[derive(Deserialize)]
struct KickTier {
    months: u32,
    badge_image: KickImage,
}

#[derive(Deserialize)]
struct KickImage {
    src: String,
}

/// Kick sub badge tiers from the channel API's answer, by months, keeping only allowed hosts.
pub fn kick_subscriber_badges(json: &str) -> Result<Vec<(u32, BadgeUrl)>, serde_json::Error> {
    let channel: KickChannelBadges = serde_json::from_str(json)?;
    let mut tiers: Vec<(u32, BadgeUrl)> = channel
        .subscriber_badges
        .into_iter()
        .filter_map(|tier| Some((tier.months, BadgeUrl::parse(&tier.badge_image.src)?)))
        .collect();
    tiers.sort_by_key(|(months, _)| *months);
    Ok(tiers)
}

/// The tier a sub of `months` has reached: the one with the most months not above it.
fn kick_tier(months: u32, tiers: &[(u32, BadgeUrl)]) -> Option<&BadgeUrl> {
    tiers
        .iter()
        .rev()
        .find(|(tier_months, _)| *tier_months <= months)
        .map(|(_, url)| url)
}

/// Badge pictures of the connected channels, per platform.
#[derive(Debug, Clone, Default)]
pub struct BadgeImages {
    twitch: HashMap<String, BadgeUrl>,
    kick: Vec<(u32, BadgeUrl)>,
}

impl BadgeImages {
    pub fn set_twitch(&mut self, images: HashMap<String, BadgeUrl>) {
        self.twitch = images;
    }

    pub fn set_kick(&mut self, tiers: Vec<(u32, BadgeUrl)>) {
        self.kick = tiers;
    }

    /// Forgets a platform's pictures when it connects to another channel.
    pub fn clear(&mut self, platform: Platform) {
        match platform {
            Platform::Twitch => self.twitch.clear(),
            Platform::Kick => self.kick.clear(),
            Platform::YouTube => {}
        }
    }

    pub fn picture<'a>(&'a self, badge: &'a SubBadge) -> Option<&'a BadgeUrl> {
        match badge {
            SubBadge::Twitch(version) => self.twitch.get(version),
            SubBadge::Kick(months) => kick_tier(*months, &self.kick),
            SubBadge::YouTube(url) => Some(url),
        }
    }
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
        assert_eq!(
            images["3012"].as_str(),
            "https://static-cdn.jtvnw.net/badges/v1/b/2"
        );
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
    fn only_allowed_image_hosts_make_a_badge_url() {
        for ok in [
            "https://static-cdn.jtvnw.net/badges/v1/a/2",
            "https://files.kick.com/channel_subscriber_badges/1",
            "https://yt3.ggpht.com/abc=s32-c-k",
        ] {
            assert!(BadgeUrl::parse(ok).is_some(), "{ok}");
        }
        for bad in [
            "",
            "http://files.kick.com/x",
            "https://static-cdn.jtvnw.net.evil.com/x",
            "https://evil-files.kick.com/x",
            "https://files.kick.com@evil.com/x",
            "https://yt3.ggpht.com.evil.com/x",
            "file:///C:/Windows/win.ini",
        ] {
            assert!(BadgeUrl::parse(bad).is_none(), "{bad}");
        }
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

    const KICK_CHANNEL: &str = include_str!("../../tests/fixtures/kick/channel.json");

    fn url(raw: &str) -> BadgeUrl {
        BadgeUrl::parse(raw).unwrap()
    }

    #[test]
    fn kick_tiers_come_sorted_from_the_channel_answer() {
        let tiers = kick_subscriber_badges(KICK_CHANNEL).unwrap();
        let months: Vec<u32> = tiers.iter().map(|(m, _)| *m).collect();
        assert!(!months.is_empty() && months.is_sorted(), "{months:?}");
        assert!(
            tiers
                .iter()
                .all(|(_, u)| u.as_str().starts_with("https://files.kick.com/"))
        );
        assert!(kick_subscriber_badges("{}").unwrap().is_empty());
    }

    #[test]
    fn a_kick_sub_gets_the_highest_tier_reached() {
        let tiers = vec![
            (1, url("https://files.kick.com/b/1")),
            (3, url("https://files.kick.com/b/3")),
            (6, url("https://files.kick.com/b/6")),
        ];
        let tier_of = |months| kick_tier(months, &tiers).map(BadgeUrl::as_str);
        assert_eq!(tier_of(1), Some("https://files.kick.com/b/1"));
        assert_eq!(tier_of(5), Some("https://files.kick.com/b/3"));
        assert_eq!(tier_of(40), Some("https://files.kick.com/b/6"));
        assert_eq!(tier_of(0), None);
        assert_eq!(kick_tier(4, &[]), None);
    }

    #[test]
    fn pictures_are_looked_up_per_platform() {
        let mut images = BadgeImages::default();
        images.set_twitch(HashMap::from([(
            "3012".to_owned(),
            url("https://static-cdn.jtvnw.net/b/2"),
        )]));
        images.set_kick(vec![(2, url("https://files.kick.com/b/2"))]);
        let member = url("https://yt3.ggpht.com/m=s32");
        assert!(images.picture(&SubBadge::Twitch("3012".into())).is_some());
        assert!(images.picture(&SubBadge::Twitch("0".into())).is_none());
        assert!(images.picture(&SubBadge::Kick(2)).is_some());
        assert_eq!(
            images.picture(&SubBadge::YouTube(member.clone())),
            Some(&member)
        );
        images.clear(Platform::Kick);
        assert!(images.picture(&SubBadge::Kick(2)).is_none());
    }
}

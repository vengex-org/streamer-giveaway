//! Icons the app uses on top of the widget library's default set.

use std::borrow::Cow;

use gpui_kit::{AssetSource, SharedString};

gpui_kit::assets::icon_assets!(
    ExtraIcons,
    [
        Trophy,
        MessagesSquare,
        MessageCircleMore,
        SunMoon,
        Search,
        FlaskConical
    ]
);

/// Platform logos from simple-icons (CC0), drawn in each platform's color.
const PLATFORM_LOGOS: [(&str, &[u8]); 3] = [
    (
        "platforms/twitch.svg",
        include_bytes!("../assets/platforms/twitch.svg"),
    ),
    (
        "platforms/kick.svg",
        include_bytes!("../assets/platforms/kick.svg"),
    ),
    (
        "platforms/youtube.svg",
        include_bytes!("../assets/platforms/youtube.svg"),
    ),
];

/// Our extra icons first, then the widget library's own assets.
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some((_, bytes)) = PLATFORM_LOGOS.iter().find(|(name, _)| *name == path) {
            return Ok(Some(Cow::Borrowed(bytes)));
        }
        if let Some(bytes) = ExtraIcons.load(path)? {
            return Ok(Some(bytes));
        }
        gpui_kit::assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut paths = gpui_kit::assets::Assets.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        paths.sort();
        paths.dedup();
        Ok(paths)
    }
}

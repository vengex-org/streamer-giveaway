use gpui_kit::*;
use streamer_giveaway::chat::{ChatBridge, connection_problem_layer};
use streamer_giveaway::demo;
use streamer_giveaway::domain::chat::{ChannelLogin, Keyword};
use streamer_giveaway::storage::{ConfigStore, Loaded};
use streamer_giveaway::ui::AppView;
use tracing_subscriber::prelude::*;

fn main() {
    let (problem_layer, problems) = connection_problem_layer();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_filter(tracing_subscriber::filter::LevelFilter::WARN),
        )
        .with(
            problem_layer.with_filter(
                tracing_subscriber::filter::Targets::new()
                    .with_target("twitch_irc", tracing::Level::INFO),
            ),
        )
        .init();

    // `--demo` swaps Twitch for a simulated chat and keeps its own settings file;
    // `--demo-viewers=N` (with optional `--demo-share`, `--demo-half-life`, `--demo-chat`,
    // `--demo-cap`) plays a giveaway announced to N viewers instead.
    let args: Vec<String> = std::env::args().collect();
    let crowd = demo_crowd(&args);
    let demo_mode = crowd.is_some() || args.iter().any(|arg| arg == "--demo");
    let store = if demo_mode {
        ConfigStore::demo_in_user_config_dir()
    } else {
        ConfigStore::in_user_config_dir()
    };
    let legacy_warning = store
        .as_ref()
        .filter(|_| !demo_mode)
        .zip(ConfigStore::legacy_user_config_dir())
        .and_then(|(store, legacy)| store.adopt_legacy(&legacy));
    let Loaded {
        mut config,
        warning,
        may_save,
    } = match &store {
        Some(store) => store.load(),
        None => Loaded {
            config: Default::default(),
            warning: Some("no user config directory; settings are not saved".into()),
            may_save: false,
        },
    };
    let store = store.filter(|_| may_save);
    let warning = [legacy_warning, warning]
        .into_iter()
        .flatten()
        .reduce(|first, second| format!("{first}\n{second}"));
    let bridge = if demo_mode {
        if config.channel.is_none() {
            config.channel = ChannelLogin::parse(demo::DEMO_CHANNEL).ok();
            config.keyword = Keyword::parse(demo::DEMO_KEYWORD).expect("demo keyword is not empty");
        }
        demo::spawn(crowd)
    } else {
        ChatBridge::spawn(problems)
    }
    .expect("failed to start the chat thread");

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("Streamer Giveaway".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1180.), px(760.)),
                cx,
            ))),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| AppView::new(bridge, store, config, warning, window, cx))
        })
        .expect("failed to open the main window");
    });
}

/// Reads `--demo-viewers=N` and its companions; `None` when no viewer count is given.
fn demo_crowd(args: &[String]) -> Option<demo::Crowd> {
    let value = |name: &str| {
        args.iter()
            .find_map(|arg| arg.strip_prefix(name)?.strip_prefix('='))
            .map(str::to_owned)
    };
    let viewers = value("--demo-viewers")?.parse().ok()?;
    let mut crowd = demo::Crowd::with_viewers(viewers);
    if let Some(share) = value("--demo-share").and_then(|v| v.parse().ok()) {
        crowd.entry_share = share;
    }
    if let Some(secs) = value("--demo-half-life").and_then(|v| v.parse().ok()) {
        crowd.half_life = std::time::Duration::from_secs(secs);
    }
    if let Some(rate) = value("--demo-chat").and_then(|v| v.parse().ok()) {
        crowd.chat_per_second = rate;
    }
    crowd.cap = value("--demo-cap").and_then(|v| v.parse().ok());
    Some(crowd)
}

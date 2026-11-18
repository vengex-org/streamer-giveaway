use gpui_kit::*;
use streamer_giveaway::chat::{ChatBridge, connection_problem_layer};
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

    let store = ConfigStore::in_user_config_dir();
    let legacy_warning = store
        .as_ref()
        .zip(ConfigStore::legacy_user_config_dir())
        .and_then(|(store, legacy)| store.adopt_legacy(&legacy));
    let Loaded {
        config,
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
    let bridge = ChatBridge::spawn(problems).expect("failed to start the chat thread");

    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some("Streamer Giveaway".into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(820.), px(640.)),
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

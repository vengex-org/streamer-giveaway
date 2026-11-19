mod chat_window;
mod header;
mod history;
mod palette;
mod stage;
mod steps;

use std::collections::{BTreeMap, HashMap};
use std::time::Instant;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::*;
use rust_i18n::t;

use crate::chat::{ChatBridge, ChatCommand, ChatEvent, ChatTarget, ConnectionStatus};
use crate::domain::badges::{self, BadgeImages};
use crate::domain::chat::{ChannelLogin, Keyword};
use crate::domain::config::{ChatMode, Config};
use crate::domain::eligibility::{ActiveWindow, Eligibility, TimeUnit};
use crate::domain::giveaway::{DrawOutcome, Giveaway, GiveawayState, MessageOutcome};
use crate::domain::matcher::MatchMode;
use crate::domain::platform::{KickChannel, Platform, UserKey, YouTubeSource};
use crate::domain::prefs::{Language, ThemeChoice, effective_theme};
use crate::domain::role::{Role, Weights};
use crate::domain::session_log::{self, LogKind, SessionLog};
use crate::domain::stats::ChatStats;
use crate::domain::winner::{WaitState, WinnerWatch};
use crate::i18n;
use crate::storage::{self, ConfigStore};
use header::{LanguageOption, language_options};
use history::History;
use palette::Palette;
use stage::{Stage, TICK};

/// The two screens: running a giveaway, and the list of past ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Giveaway,
    History,
}

/// A giveaway log on disk and when its entries opened, so later rows get the right time.
struct SavedLog {
    path: std::path::PathBuf,
    opened_at: Instant,
}

/// What a notice is about. Each source replaces or clears only its own notice, so fixing one
/// problem never hides another that still exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NoticeSource {
    Startup,
    Demo,
    Save,
    Chat,
    Channel,
    Keyword,
    Weight(Role),
    Window,
    Log,
}

/// A problem to show, kept untranslated so a language switch re-renders it.
#[derive(Debug, Clone)]
enum Notice {
    Startup(String),
    DemoFailed(String),
    Save(String),
    ChatStopped,
    ChatRejected(String),
    ChannelInvalid(String),
    KickInvalid(String),
    YouTubeInvalid(String),
    KeywordEmpty,
    WeightInvalid(Role),
    WindowInvalid,
    LogFailed(String),
}

impl Notice {
    fn text(&self) -> String {
        match self {
            Notice::Startup(cause) => t!("error.startup", cause = cause),
            Notice::DemoFailed(cause) => t!("error.demo", cause = cause),
            Notice::Save(cause) => t!("error.save", cause = cause),
            Notice::ChatStopped => t!("error.chat_stopped"),
            Notice::ChatRejected(cause) => t!("error.chat_rejected", cause = cause),
            Notice::ChannelInvalid(raw) => t!("error.channel_invalid", raw = raw),
            Notice::KickInvalid(raw) => t!("error.kick_invalid", raw = raw),
            Notice::YouTubeInvalid(raw) => t!("error.youtube_invalid", raw = raw),
            Notice::KeywordEmpty => t!("error.keyword_empty"),
            Notice::WindowInvalid => t!("error.window_invalid"),
            Notice::LogFailed(cause) => t!("error.log", cause = cause),
            Notice::WeightInvalid(role) => {
                t!("error.weight_invalid", role = i18n::role_label(*role))
            }
        }
        .into_owned()
    }
}

/// Whether this window reads real chats or the simulated one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Live,
    Demo,
}

pub struct AppView {
    mode: RunMode,
    config: Config,
    store: Option<ConfigStore>,
    giveaway: Giveaway,
    commands: async_channel::Sender<ChatCommand>,
    /// Each platform's chat connection; a missing platform is idle.
    statuses: BTreeMap<Platform, ConnectionStatus>,
    /// Joins sent whose first status has not come back yet.
    joining: std::collections::BTreeSet<Platform>,
    /// The channel each platform's badge pictures belong to, so a reconnect keeps them.
    badge_channels: HashMap<Platform, ChatTarget>,
    notices: BTreeMap<NoticeSource, Notice>,
    stage: Stage,
    /// Whether the rules step shows its editor (weights and entry conditions).
    rules_open: bool,
    /// Finished steps the streamer unfolded to change them; the others fold into one line.
    unfolded_steps: std::collections::HashSet<steps::Step>,
    os_dark: bool,
    /// The OS language, read once at startup.
    os_language: Language,
    palette: Palette,
    channel_input: Entity<InputState>,
    kick_input: Entity<InputState>,
    youtube_input: Entity<InputState>,
    keyword_input: Entity<InputState>,
    weight_inputs: Vec<(Role, Entity<InputState>)>,
    window_input: Entity<InputState>,
    language_select: Entity<SelectState<Vec<LanguageOption>>>,
    entrants_scroll: UniformListScrollHandle,
    /// Demo only: the window that shows the simulated chat, while it is open.
    chat_window: Option<(AnyWindowHandle, WeakEntity<chat_window::ChatWindow>)>,
    ticker: Option<Task<()>>,
    /// Throughput while entries are open.
    stats: ChatStats,
    /// Every message while entries are open; written to CSV when they close.
    session: Option<SessionLog>,
    /// The last giveaway log written; winners drawn later are added to it.
    last_log: Option<SavedLog>,
    page: Page,
    /// Read from the logs folder each time the history page opens.
    history: History,
    history_scroll: ScrollHandle,
    search_input: Entity<InputState>,
    /// Entrant indices that match the search, best first; `None` while the search is empty.
    search_hits: Option<Vec<usize>>,
    /// The query `search_hits` were computed for.
    search_hits_query: Option<String>,
    /// Subscriber badge version to picture URL for the connected channel; empty until loaded.
    badge_images: BadgeImages,
    /// Badge list downloads in flight, one per platform.
    badge_fetch: HashMap<Platform, Task<()>>,
    /// "Clear history" was pressed once and waits for the confirming second press.
    confirm_clear_history: bool,
    stats_ticker: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    pub fn new(
        mode: RunMode,
        bridge: ChatBridge,
        store: Option<ConfigStore>,
        config: Config,
        startup_notice: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let os_language = i18n::os_language();
        i18n::apply(config.language.unwrap_or(os_language));
        let os_dark = is_dark(window.appearance());
        let theme = effective_theme(config.theme, os_dark);
        let palette = Palette::for_theme(theme);
        Palette::apply(theme, window, cx);

        let channel = config.channel.as_ref().map(|c| c.as_str().to_owned());
        let channel_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("step.connect.placeholder"))
                .default_value(channel.unwrap_or_default())
        });
        let kick_channel = config.kick_channel.as_ref().map(|c| c.as_str().to_owned());
        let kick_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("step.connect.placeholder_kick"))
                .default_value(kick_channel.unwrap_or_default())
        });
        let youtube_source = config.youtube_source.as_ref().map(ToString::to_string);
        let youtube_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("step.connect.placeholder_youtube"))
                .default_value(youtube_source.unwrap_or_default())
        });
        let keyword_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("step.keyword.placeholder"))
                .default_value(config.keyword.as_str().to_owned())
        });
        let weight_inputs: Vec<_> = Role::ALL
            .iter()
            .map(|&role| {
                let weight = config.weights.get(role).to_string();
                (
                    role,
                    cx.new(|cx| InputState::new(window, cx).default_value(weight)),
                )
            })
            .collect();

        let chosen_language = config.language;
        let language_select = cx.new(|cx| {
            let mut state = SelectState::new(language_options(), None, window, cx);
            state.set_selected_value(&chosen_language, window, cx);
            state
        });

        let search_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(t!("stage.search.placeholder")));

        let window_amount = config.eligibility.active_window.amount.to_string();
        let window_input = cx.new(|cx| InputState::new(window, cx).default_value(window_amount));

        let mut subscriptions = vec![
            cx.subscribe_in(&window_input, window, |this, input, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value();
                    this.set_window_amount(&text, cx);
                }
            }),
            cx.subscribe_in(&language_select, window, |this, _, event, window, cx| {
                let SelectEvent::Confirm(Some(choice)) = event else {
                    return;
                };
                this.set_language(*choice, window, cx);
            }),
            cx.subscribe_in(&channel_input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.connect(Platform::Twitch, window, cx);
                }
            }),
            cx.subscribe_in(&kick_input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.connect(Platform::Kick, window, cx);
                }
            }),
            cx.subscribe_in(&youtube_input, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.connect(Platform::YouTube, window, cx);
                }
            }),
            cx.subscribe_in(&search_input, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.refresh_search(cx);
                    cx.notify();
                }
            }),
            cx.subscribe_in(&keyword_input, window, Self::on_keyword_input),
            cx.observe_window_appearance(window, |this, window, cx| {
                this.os_dark = is_dark(window.appearance());
                this.apply_theme(window, cx);
            }),
        ];
        for (role, input) in &weight_inputs {
            let role = *role;
            subscriptions.push(
                cx.subscribe_in(input, window, move |this, input, event, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let text = input.read(cx).value();
                        this.set_weight(role, &text, cx);
                    }
                }),
            );
        }

        // The demo opens its chat window next to the main one, once the main one exists.
        if mode == RunMode::Demo {
            cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                this.update(cx, |view, cx| view.show_chat_window(cx)).ok();
            })
            .detach();
        }

        let events = bridge.events.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            while let Ok(event) = events.recv().await {
                let applied = this.update(cx, |view, cx| {
                    if view.on_chat_event(event, cx) {
                        cx.notify();
                    }
                });
                if applied.is_err() {
                    break;
                }
            }
        })
        .detach();

        let mut giveaway = Giveaway::new(config.keyword.clone(), config.mode);
        giveaway.set_eligibility(config.eligibility);

        Self {
            giveaway,
            mode,
            config,
            store,
            commands: bridge.commands,
            statuses: BTreeMap::new(),
            joining: std::collections::BTreeSet::new(),
            badge_channels: HashMap::new(),
            notices: startup_notice
                .map(|cause| (NoticeSource::Startup, Notice::Startup(cause)))
                .into_iter()
                .collect(),
            stage: Stage::List,
            rules_open: false,
            unfolded_steps: std::collections::HashSet::new(),
            os_dark,
            os_language,
            palette,
            channel_input,
            kick_input,
            youtube_input,
            keyword_input,
            weight_inputs,
            window_input,
            language_select,
            entrants_scroll: UniformListScrollHandle::new(),
            chat_window: None,
            ticker: None,
            stats: ChatStats::default(),
            session: None,
            last_log: None,
            page: Page::Giveaway,
            history: History::Loaded(Vec::new()),
            history_scroll: ScrollHandle::new(),
            search_input,
            search_hits: None,
            search_hits_query: None,
            badge_images: BadgeImages::default(),
            badge_fetch: HashMap::new(),
            confirm_clear_history: false,
            stats_ticker: None,
            _subscriptions: subscriptions,
        }
    }

    fn theme(&self) -> ThemeChoice {
        effective_theme(self.config.theme, self.os_dark)
    }

    /// Entrants who can still win (weight above 0).
    fn can_win(&self) -> usize {
        self.giveaway
            .counts_by_role(&self.config.weights)
            .iter()
            .map(|(_, n)| n)
            .sum()
    }

    /// Applies a chat event; returns whether anything visible changed.
    fn on_chat_event(&mut self, event: ChatEvent, cx: &mut Context<Self>) -> bool {
        match event {
            ChatEvent::Message(msg) => {
                if let Some(chat) = self.open_chat_window() {
                    chat.update(cx, |chat, cx| chat.push(&msg, cx));
                }
                let now = Instant::now();
                let matched = self.giveaway.matches_keyword(msg.text());
                let follow = self.entrants_at_bottom();
                let outcome = self.giveaway.on_message(&msg, &self.config.weights, now);
                if outcome == MessageOutcome::Entered {
                    if self.search_hits.is_some() {
                        self.search_hits = Some(self.search_matches());
                    } else if follow {
                        self.entrants_scroll.scroll_to_bottom();
                    }
                }
                if let Some(session) = &mut self.session {
                    let kind = LogKind::from_outcome(outcome, matched);
                    session.record(now, kind, msg.user(), msg.display_name());
                    self.stats.record_message(now);
                    if outcome == MessageOutcome::Entered {
                        self.stats.record_entry(now);
                    }
                }
                let replied = self.stage.on_message(&msg, now);
                outcome != MessageOutcome::Ignored || replied
            }
            ChatEvent::Status(platform, status) => {
                self.joining.remove(&platform);
                // A channel's badge list loads once per channel, not on every reconnect.
                if let ConnectionStatus::Connected(target) = &status
                    && self.badge_channels.get(&platform) != Some(target)
                {
                    self.badge_channels.insert(platform, target.clone());
                    match target {
                        ChatTarget::Twitch(channel) => self.load_badges(channel.clone(), cx),
                        ChatTarget::Kick(channel) => self.load_kick_badges(channel, cx),
                        ChatTarget::YouTube(_) => {}
                    }
                }
                self.statuses.insert(platform, status);
                true
            }
            ChatEvent::Rejected(reason) => {
                self.set_notice(NoticeSource::Chat, Some(Notice::ChatRejected(reason)));
                true
            }
        }
    }

    fn on_keyword_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { .. } | InputEvent::Blur => {
                self.commit_rule(self.config.mode, cx);
            }
            // Only the step highlight depends on the pending text; the rule waits for Enter.
            InputEvent::Change => {}
            _ => return,
        }
        cx.notify();
    }

    /// Takes the keyword field's text and `mode` into the giveaway rule. Called on Enter, on
    /// leaving the field, and before opening entries or switching mode (buttons do not take
    /// focus, so the field never blurs on a click). Never per keystroke: a half-typed keyword
    /// would let unintended messages into an open giveaway. Returns false when the keyword is
    /// invalid; then nothing changes.
    fn commit_rule(&mut self, mode: MatchMode, cx: &mut Context<Self>) -> bool {
        let Ok(keyword) = Keyword::parse(&self.keyword_input.read(cx).value()) else {
            self.set_notice(NoticeSource::Keyword, Some(Notice::KeywordEmpty));
            return false;
        };
        self.set_notice(NoticeSource::Keyword, None);
        if keyword != self.config.keyword || mode != self.config.mode {
            self.config.keyword = keyword;
            self.config.mode = mode;
            self.giveaway
                .set_rule(self.config.keyword.clone(), self.config.mode);
            self.save();
        }
        true
    }

    fn keyword_text_valid(&self, cx: &App) -> bool {
        Keyword::parse(&self.keyword_input.read(cx).value()).is_ok()
    }

    fn set_notice(&mut self, source: NoticeSource, notice: Option<Notice>) {
        match notice {
            Some(notice) => self.notices.insert(source, notice),
            None => self.notices.remove(&source),
        };
    }

    fn status(&self, platform: Platform) -> &ConnectionStatus {
        const IDLE: &ConnectionStatus = &ConnectionStatus::Idle;
        self.statuses.get(&platform).unwrap_or(IDLE)
    }

    /// Whether any chat feeds the giveaway right now.
    fn any_connected(&self) -> bool {
        self.statuses
            .values()
            .any(|status| matches!(status, ConnectionStatus::Connected(_)))
    }

    fn input(&self, platform: Platform) -> &Entity<InputState> {
        match platform {
            Platform::Twitch => &self.channel_input,
            Platform::Kick => &self.kick_input,
            Platform::YouTube => &self.youtube_input,
        }
    }

    /// One platform mode shows one chat field. A link to another platform entered there moves to
    /// that platform's field and chooses it; the field it was entered in gets its saved name back.
    /// Returns the platform to connect.
    fn follow_link(
        &mut self,
        field: Platform,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Platform {
        if self.config.chat_mode != ChatMode::Single {
            return field;
        }
        let text = self.input(field).read(cx).value().to_string();
        let Some(linked) = Platform::of_link(&text).filter(|&linked| linked != field) else {
            return field;
        };
        let saved = self.saved_chat_name(field);
        self.input(field)
            .update(cx, |input, cx| input.set_value(saved, window, cx));
        self.input(linked)
            .update(cx, |input, cx| input.set_value(text, window, cx));
        self.choose_chats(ChatMode::Single, linked, cx);
        linked
    }

    /// The channel or stream last connected on `platform`, as its field shows it.
    fn saved_chat_name(&self, platform: Platform) -> String {
        match platform {
            Platform::Twitch => self.config.channel.as_ref().map(|c| c.as_str().to_owned()),
            Platform::Kick => self
                .config
                .kick_channel
                .as_ref()
                .map(|c| c.as_str().to_owned()),
            Platform::YouTube => self.config.youtube_source.as_ref().map(ToString::to_string),
        }
        .unwrap_or_default()
    }

    /// Reads the platform's input and joins that chat; a typo shows a notice instead.
    fn connect(&mut self, field: Platform, window: &mut Window, cx: &mut Context<Self>) {
        let platform = self.follow_link(field, window, cx);
        let raw = self.input(platform).read(cx).value().to_string();
        let target = match platform {
            Platform::Twitch => ChannelLogin::parse(&raw)
                .map(ChatTarget::Twitch)
                .map_err(|_| Notice::ChannelInvalid(raw.clone())),
            Platform::Kick => KickChannel::parse(&raw)
                .map(ChatTarget::Kick)
                .map_err(|_| Notice::KickInvalid(raw.clone())),
            Platform::YouTube => YouTubeSource::parse(&raw)
                .map(ChatTarget::YouTube)
                .map_err(|_| Notice::YouTubeInvalid(raw.clone())),
        };
        match target {
            Ok(target) => {
                match &target {
                    ChatTarget::Twitch(channel) => self.config.channel = Some(channel.clone()),
                    ChatTarget::Kick(channel) => self.config.kick_channel = Some(channel.clone()),
                    ChatTarget::YouTube(source) => {
                        self.config.youtube_source = Some(source.clone());
                    }
                }
                self.joining.insert(platform);
                self.send(ChatCommand::Join(target));
                self.set_notice(NoticeSource::Channel, None);
                self.set_notice(NoticeSource::Chat, None);
                self.save();
            }
            Err(notice) => self.set_notice(NoticeSource::Channel, Some(notice)),
        }
        cx.notify();
    }

    fn disconnect(&mut self, platform: Platform, cx: &mut Context<Self>) {
        self.joining.remove(&platform);
        self.send(ChatCommand::Leave(platform));
        cx.notify();
    }

    /// One platform (and which) or multistream. Every chat the new choice does not read is left,
    /// also one still connecting, so the giveaway only takes entries from the chats on screen.
    fn choose_chats(&mut self, mode: ChatMode, platform: Platform, cx: &mut Context<Self>) {
        self.config.chat_mode = mode;
        self.config.platform = platform;
        let open: Vec<Platform> = Platform::ALL
            .into_iter()
            .filter(|p| {
                !matches!(self.status(*p), ConnectionStatus::Idle) || self.joining.contains(p)
            })
            .collect();
        for platform in self.config.chats_to_leave(&open) {
            self.joining.remove(&platform);
            self.send(ChatCommand::Leave(platform));
        }
        self.set_notice(NoticeSource::Channel, None);
        self.save();
        cx.notify();
    }

    fn send(&mut self, command: ChatCommand) {
        if self.commands.try_send(command).is_err() {
            self.set_notice(NoticeSource::Chat, Some(Notice::ChatStopped));
        }
    }

    fn set_mode(&mut self, mode: MatchMode, cx: &mut Context<Self>) {
        self.commit_rule(mode, cx);
        cx.notify();
    }

    fn set_weight(&mut self, role: Role, text: &str, cx: &mut Context<Self>) {
        match Weights::parse_weight(text) {
            Ok(weight) => {
                self.set_notice(NoticeSource::Weight(role), None);
                if self.config.weights.get(role) != weight {
                    self.config.weights.set(role, weight);
                    self.save();
                }
            }
            Err(_) => self.set_notice(
                NoticeSource::Weight(role),
                Some(Notice::WeightInvalid(role)),
            ),
        }
        cx.notify();
    }

    fn update_eligibility(
        &mut self,
        change: impl FnOnce(&mut Eligibility),
        cx: &mut Context<Self>,
    ) {
        change(&mut self.config.eligibility);
        self.giveaway.set_eligibility(self.config.eligibility);
        self.save();
        cx.notify();
    }

    fn set_window_amount(&mut self, text: &str, cx: &mut Context<Self>) {
        match ActiveWindow::parse_amount(text) {
            Some(amount) => {
                self.set_notice(NoticeSource::Window, None);
                if amount != self.config.eligibility.active_window.amount {
                    self.update_eligibility(|e| e.active_window.amount = amount, cx);
                }
            }
            None => self.set_notice(NoticeSource::Window, Some(Notice::WindowInvalid)),
        }
        cx.notify();
    }

    fn set_window_unit(&mut self, unit: TimeUnit, cx: &mut Context<Self>) {
        self.update_eligibility(|e| e.active_window.unit = unit, cx);
    }

    fn save(&mut self) {
        // Without a store the startup notice already says that settings are not saved.
        let Some(store) = &self.store else { return };
        match store.save(&self.config) {
            Ok(()) => self.set_notice(NoticeSource::Save, None),
            Err(err) => self.set_notice(NoticeSource::Save, Some(Notice::Save(err.to_string()))),
        }
    }

    fn toggle_entries(&mut self, cx: &mut Context<Self>) {
        match self.giveaway.state() {
            GiveawayState::Open => {
                self.giveaway.close();
                self.finish_session();
            }
            GiveawayState::Closed => {
                if self.commit_rule(self.config.mode, cx) {
                    self.giveaway.open();
                    self.start_session(cx);
                }
            }
        }
        cx.notify();
    }

    /// Starts measuring: fresh stats, a new log, and a once-per-second refresh of the stats line.
    fn start_session(&mut self, cx: &mut Context<Self>) {
        self.session = Some(SessionLog::new(Instant::now()));
        self.stats = ChatStats::default();
        self.stats_ticker = Some(cx.spawn(
            async move |this: WeakEntity<Self>, cx: &mut AsyncApp| loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(1))
                    .await;
                let open = this
                    .update(cx, |view, cx| {
                        cx.notify();
                        view.session.is_some()
                    })
                    .unwrap_or(false);
                if !open {
                    break;
                }
            },
        ));
    }

    /// Writes the log of the entries that just closed. A failure is shown, never blocks.
    fn finish_session(&mut self) {
        self.stats_ticker = None;
        let Some(session) = self.session.take() else {
            return;
        };
        let Some(dir) = ConfigStore::logs_dir() else {
            self.set_notice(
                NoticeSource::Log,
                Some(Notice::LogFailed("no user data folder".into())),
            );
            return;
        };
        let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%SZ").to_string();
        match storage::write_session_log(&dir, &stamp, &session.to_csv()) {
            Ok(path) => {
                self.set_notice(NoticeSource::Log, None);
                self.last_log = Some(SavedLog {
                    path,
                    opened_at: session.opened_at(),
                });
            }
            Err(err) => {
                self.set_notice(NoticeSource::Log, Some(Notice::LogFailed(err.to_string())))
            }
        }
    }

    fn draw(&mut self, cx: &mut Context<Self>) {
        // Drawing closes entries; the log of this giveaway ends here.
        self.finish_session();
        let weights = self.config.weights;
        let pool: Vec<String> = self
            .giveaway
            .entrants()
            .filter(|e| e.effective(&weights).1 > 0)
            .map(|e| e.display_name.clone())
            .collect();
        match self.giveaway.draw(&weights, &mut rand::rng()) {
            DrawOutcome::Winner(winner) => {
                self.log_winner(&winner.user, &winner.display_name);
                let reel = stage::reveal_reel(&pool, &winner.display_name, &mut rand::rng());
                self.send(ChatCommand::WinnerDrawn(winner.user.clone()));
                self.stage = Stage::Revealing {
                    reel,
                    frame: 0,
                    watch: WinnerWatch::new(winner, Instant::now()),
                };
                self.start_ticker(cx);
            }
            // The draw buttons are disabled with the reason shown whenever nobody can win.
            DrawOutcome::EmptyPool | DrawOutcome::NoDrawable => {}
        }
        cx.notify();
    }

    /// Whether the entrants list shows its last rows (or does not scroll yet). New entrants keep
    /// such a list pinned to the bottom; someone reading further up stays where they are.
    fn entrants_at_bottom(&self) -> bool {
        let state = self.entrants_scroll.0.borrow();
        let max = state.base_handle.max_offset().y;
        // Within about half a row of the end counts as the end.
        max <= px(0.) || -state.base_handle.offset().y >= max - px(20.)
    }

    /// The search query as typed, trimmed.
    fn search_query(&self, cx: &App) -> String {
        self.search_input.read(cx).value().trim().to_owned()
    }

    fn search_matches_for(&self, query: &str) -> Vec<usize> {
        crate::domain::search::search(
            query,
            self.giveaway.entrants().map(|e| e.display_name.as_str()),
        )
    }

    fn search_matches(&self) -> Vec<usize> {
        self.search_hits_query
            .as_deref()
            .map(|query| self.search_matches_for(query))
            .unwrap_or_default()
    }

    fn refresh_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search_query(cx);
        if query.is_empty() {
            self.search_hits = None;
            self.search_hits_query = None;
        } else {
            self.search_hits = Some(self.search_matches_for(&query));
            self.search_hits_query = Some(query);
        }
        self.entrants_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    /// Fetches the channel's subscriber badge pictures. A failure only means rows show the role
    /// name without a picture, so it is logged, not shown.
    fn load_badges(&mut self, channel: ChannelLogin, cx: &mut Context<Self>) {
        self.load_badge_list(
            Platform::Twitch,
            badges::channel_badges_url(&channel),
            |images, json| badges::subscriber_badge_images(json).map(|i| images.set_twitch(i)),
            cx,
        );
    }

    fn load_kick_badges(&mut self, channel: &KickChannel, cx: &mut Context<Self>) {
        self.load_badge_list(
            Platform::Kick,
            crate::chat::kick_channel_url(channel),
            |images, json| badges::kick_subscriber_badges(json).map(|t| images.set_kick(t)),
            cx,
        );
    }

    /// Downloads a channel's badge list and stores it with `apply`. A failure only means rows
    /// show the role name without a picture, so it is logged, not shown.
    fn load_badge_list(
        &mut self,
        platform: Platform,
        url: String,
        apply: fn(&mut BadgeImages, &str) -> Result<(), serde_json::Error>,
        cx: &mut Context<Self>,
    ) {
        self.badge_images.clear(platform);
        let client = cx.http_client();
        let task = cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let body = async {
                use futures::AsyncReadExt as _;
                let mut response = client
                    .get(&url, http_client::AsyncBody::empty(), true)
                    .await?;
                anyhow::ensure!(response.status().is_success(), "HTTP {}", response.status());
                let mut body = String::new();
                response.body_mut().read_to_string(&mut body).await?;
                Ok::<_, anyhow::Error>(body)
            }
            .await;
            let applied = match body {
                Ok(body) => this
                    .update(cx, |view, cx| {
                        let applied = apply(&mut view.badge_images, &body);
                        cx.notify();
                        applied
                    })
                    .and_then(|applied| applied.map_err(anyhow::Error::from)),
                Err(err) => Err(err),
            };
            if let Err(err) = applied {
                tracing::warn!(%platform, "channel badges not loaded: {err:#}");
            }
        });
        self.badge_fetch.insert(platform, task);
    }

    /// Starts this app again in demo mode, as a second window with its own settings, so the
    /// streamer can try a giveaway without touching their real channel or settings.
    /// The demo's chat window, when it is open.
    fn open_chat_window(&self) -> Option<Entity<chat_window::ChatWindow>> {
        self.chat_window.as_ref()?.1.upgrade()
    }

    /// Brings the demo's chat window to the front, opening it again if it was closed.
    fn show_chat_window(&mut self, cx: &mut Context<Self>) {
        if self.open_chat_window().is_some()
            && let Some((handle, _)) = &self.chat_window
            && handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        match chat_window::ChatWindow::open(self.palette, cx) {
            Ok((handle, chat)) => self.chat_window = Some((handle, chat.downgrade())),
            Err(err) => tracing::warn!("chat window not opened: {err}"),
        }
    }

    fn open_demo(&mut self, cx: &mut Context<Self>) {
        let started = std::env::current_exe()
            .and_then(|exe| std::process::Command::new(exe).arg("--demo").spawn());
        match started {
            Ok(_) => {
                self.notices.remove(&NoticeSource::Demo);
            }
            Err(err) => {
                tracing::warn!("demo window not started: {err}");
                self.notices
                    .insert(NoticeSource::Demo, Notice::DemoFailed(err.to_string()));
            }
        }
        cx.notify();
    }

    /// Adds a drawn winner to the log of the giveaway they were drawn from.
    fn log_winner(&mut self, user: &UserKey, display_name: &str) {
        let Some(log) = &self.last_log else {
            return;
        };
        let row =
            session_log::csv_row(log.opened_at.elapsed(), LogKind::Winner, user, display_name);
        if let Err(err) = storage::append_session_row(&log.path, &row) {
            self.set_notice(NoticeSource::Log, Some(Notice::LogFailed(err.to_string())));
        }
    }

    fn set_page(&mut self, page: Page, cx: &mut Context<Self>) {
        if page == Page::History {
            self.history = history::load_history();
        }
        self.confirm_clear_history = false;
        self.page = page;
        cx.notify();
    }

    fn start_ticker(&mut self, cx: &mut Context<Self>) {
        self.ticker = Some(
            cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                loop {
                    cx.background_executor().timer(TICK).await;
                    let running = this.update(cx, |view, cx| view.tick(cx)).unwrap_or(false);
                    if !running {
                        break;
                    }
                }
            }),
        );
    }

    /// Advances the reveal or the stopwatch; returns whether the ticker is still needed.
    fn tick(&mut self, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        match &mut self.stage {
            Stage::List => false,
            Stage::Revealing { reel, frame, .. } => {
                *frame += 1;
                if *frame >= reel.len()
                    && let Stage::Revealing { watch, .. } =
                        std::mem::replace(&mut self.stage, Stage::List)
                {
                    self.stage = Stage::Winner {
                        shown_second: watch.elapsed(now).as_secs(),
                        watch,
                    };
                }
                cx.notify();
                true
            }
            Stage::Winner {
                watch,
                shown_second,
            } => {
                let second = watch.elapsed(now).as_secs();
                if second != *shown_second {
                    *shown_second = second;
                    cx.notify();
                }
                watch.state() == WaitState::Waiting
            }
        }
    }

    fn finish_winner(&mut self, cx: &mut Context<Self>) {
        self.stage = Stage::List;
        self.ticker = None;
        cx.notify();
    }

    fn clear_list(&mut self, cx: &mut Context<Self>) {
        self.giveaway.reset();
        self.refresh_search(cx);
        self.finish_winner(cx);
    }

    /// `None` follows the OS language.
    fn set_language(
        &mut self,
        language: Option<Language>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        i18n::apply(language.unwrap_or(self.os_language));
        self.config.language = language;
        self.save();
        self.channel_input.update(cx, |input, cx| {
            input.set_placeholder(t!("step.connect.placeholder"), window, cx)
        });
        self.kick_input.update(cx, |input, cx| {
            input.set_placeholder(t!("step.connect.placeholder_kick"), window, cx)
        });
        self.youtube_input.update(cx, |input, cx| {
            input.set_placeholder(t!("step.connect.placeholder_youtube"), window, cx)
        });
        self.keyword_input.update(cx, |input, cx| {
            input.set_placeholder(t!("step.keyword.placeholder"), window, cx)
        });
        // "Auto" is labeled in the UI language, so the items are rebuilt with the new texts.
        self.language_select.update(cx, |select, cx| {
            select.set_items(language_options(), window, cx);
            select.set_selected_value(&language, window, cx);
        });
        cx.notify();
    }

    /// `None` follows the OS appearance.
    fn set_theme(
        &mut self,
        theme: Option<ThemeChoice>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.theme = theme;
        self.save();
        self.apply_theme(window, cx);
    }

    fn apply_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let theme = self.theme();
        self.palette = Palette::for_theme(theme);
        Palette::apply(theme, window, cx);
        if let Some(chat) = self.open_chat_window() {
            let palette = self.palette;
            chat.update(cx, |chat, cx| chat.set_palette(palette, cx));
        }
        cx.notify();
    }
}

fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let can_win = self.can_win();
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(p.bg)
            .text_color(p.text)
            .child(self.render_header(window, cx))
            .child(match self.page {
                Page::Giveaway => div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_steps(can_win, cx))
                    .child(self.render_stage(can_win, cx))
                    .into_any_element(),
                Page::History => div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_history(cx))
                    .into_any_element(),
            })
    }
}

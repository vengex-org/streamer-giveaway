use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::label::Label;
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::collections::BTreeMap;

use crate::chat::{ChatBridge, ChatCommand, ChatEvent, ConnectionStatus};
use crate::domain::chat::{ChannelLogin, Keyword};
use crate::domain::config::Config;
use crate::domain::giveaway::{DrawOutcome, Entrant, Giveaway, GiveawayState, MessageOutcome};
use crate::domain::matcher::MatchMode;
use crate::domain::role::{Role, Weights};
use crate::storage::ConfigStore;

/// What a notice is about. Each source replaces or clears only its own notice, so fixing one
/// problem never hides another that still exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum NoticeSource {
    Startup,
    Save,
    Chat,
    Channel,
    Keyword,
    Weight(Role),
    Draw,
}

pub struct AppView {
    config: Config,
    store: Option<ConfigStore>,
    giveaway: Giveaway,
    commands: async_channel::Sender<ChatCommand>,
    status: ConnectionStatus,
    /// Problems worth telling the user, at most one per source.
    notices: BTreeMap<NoticeSource, String>,
    entrants_scroll: UniformListScrollHandle,
    winner: Option<Entrant>,
    channel_input: Entity<InputState>,
    keyword_input: Entity<InputState>,
    weight_inputs: Vec<(Role, Entity<InputState>)>,
    _subscriptions: Vec<Subscription>,
}

impl AppView {
    pub fn new(
        bridge: ChatBridge,
        store: Option<ConfigStore>,
        config: Config,
        startup_notice: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let channel = config.channel.as_ref().map(|c| c.as_str().to_owned());
        let channel_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("channel name")
                .default_value(channel.unwrap_or_default())
        });
        let keyword_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("keyword, e.g. YEP")
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

        let mut subscriptions = vec![
            cx.subscribe_in(&channel_input, window, Self::on_channel_input),
            cx.subscribe_in(&keyword_input, window, Self::on_keyword_input),
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

        let events = bridge.events.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            while let Ok(event) = events.recv().await {
                let applied = this.update(cx, |view, cx| {
                    if view.on_chat_event(event) {
                        cx.notify();
                    }
                });
                if applied.is_err() {
                    break;
                }
            }
        })
        .detach();

        Self {
            giveaway: Giveaway::new(config.keyword.clone(), config.mode),
            config,
            store,
            commands: bridge.commands,
            status: ConnectionStatus::Idle,
            notices: startup_notice
                .map(|text| (NoticeSource::Startup, text))
                .into_iter()
                .collect(),
            entrants_scroll: UniformListScrollHandle::new(),
            winner: None,
            channel_input,
            keyword_input,
            weight_inputs,
            _subscriptions: subscriptions,
        }
    }

    /// Applies a chat event; returns whether anything visible changed.
    fn on_chat_event(&mut self, event: ChatEvent) -> bool {
        match event {
            ChatEvent::Message(msg) => {
                self.giveaway.on_message(&msg, &self.config.weights) != MessageOutcome::Ignored
            }
            ChatEvent::Status(status) => {
                self.status = status;
                true
            }
            ChatEvent::Rejected(reason) => {
                self.set_notice(NoticeSource::Chat, Some(reason));
                true
            }
        }
    }

    fn on_channel_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. }) {
            self.connect(cx);
        }
    }

    fn on_keyword_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            self.commit_rule(self.config.mode, cx);
            cx.notify();
        }
    }

    /// Takes the keyword field's text into the giveaway rule. Called on Enter, on leaving the
    /// field, and before opening entries or switching mode (buttons do not take focus, so the
    /// field never blurs on a click). Never per keystroke: a half-typed keyword would let
    /// unintended messages into an open giveaway. Returns false when the keyword is invalid.
    fn commit_rule(&mut self, mode: MatchMode, cx: &mut Context<Self>) -> bool {
        match Keyword::parse(&self.keyword_input.read(cx).value()) {
            Ok(keyword) => {
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
            Err(err) => {
                self.set_notice(NoticeSource::Keyword, Some(err.to_string()));
                false
            }
        }
    }

    fn set_notice(&mut self, source: NoticeSource, text: Option<String>) {
        match text {
            Some(text) => self.notices.insert(source, text),
            None => self.notices.remove(&source),
        };
    }

    fn connect(&mut self, cx: &mut Context<Self>) {
        match ChannelLogin::parse(&self.channel_input.read(cx).value()) {
            Ok(channel) => {
                self.send(ChatCommand::Join(channel.clone()));
                self.config.channel = Some(channel);
                self.set_notice(NoticeSource::Channel, None);
                self.set_notice(NoticeSource::Chat, None);
                self.save();
            }
            Err(err) => self.set_notice(NoticeSource::Channel, Some(err.to_string())),
        }
        cx.notify();
    }

    fn disconnect(&mut self, cx: &mut Context<Self>) {
        self.send(ChatCommand::Leave);
        cx.notify();
    }

    fn send(&mut self, command: ChatCommand) {
        if self.commands.try_send(command).is_err() {
            self.set_notice(
                NoticeSource::Chat,
                Some("chat connection thread has stopped; restart the app".into()),
            );
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
            Err(err) => self.set_notice(
                NoticeSource::Weight(role),
                Some(format!("{}: {err}", crate::i18n::role_label(role))),
            ),
        }
        cx.notify();
    }

    fn save(&mut self) {
        // Without a store the startup notice already says that settings are not saved.
        let Some(store) = &self.store else { return };
        match store.save(&self.config) {
            Ok(()) => self.set_notice(NoticeSource::Save, None),
            Err(err) => self.set_notice(NoticeSource::Save, Some(err.to_string())),
        }
    }

    fn draw(&mut self, cx: &mut Context<Self>) {
        match self.giveaway.draw(&self.config.weights, &mut rand::rng()) {
            DrawOutcome::Winner(winner) => {
                self.winner = Some(winner);
                self.set_notice(NoticeSource::Draw, None);
            }
            DrawOutcome::EmptyPool => {
                self.winner = None;
                self.set_notice(NoticeSource::Draw, Some("nobody can win yet".into()));
            }
            DrawOutcome::NoDrawable => {
                self.winner = None;
                self.set_notice(
                    NoticeSource::Draw,
                    Some("every entrant has weight 0; raise a weight to draw".into()),
                );
            }
        }
        cx.notify();
    }

    fn set_state(&mut self, state: GiveawayState, cx: &mut Context<Self>) {
        match state {
            GiveawayState::Open => {
                if self.commit_rule(self.config.mode, cx) {
                    self.giveaway.open();
                }
            }
            GiveawayState::Closed => self.giveaway.close(),
        }
        cx.notify();
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        self.giveaway.reset();
        self.winner = None;
        cx.notify();
    }

    fn render_connection(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (color, text) = match &self.status {
            ConnectionStatus::Idle => (theme.muted_foreground, "not connected".to_owned()),
            ConnectionStatus::Connecting { channel, cause } => (
                theme.warning,
                match cause {
                    Some(cause) => format!("connecting to #{}: {cause}", channel.as_str()),
                    None => format!("connecting to #{}", channel.as_str()),
                },
            ),
            ConnectionStatus::Connected(c) => (theme.success, format!("reading #{}", c.as_str())),
            ConnectionStatus::Reconnecting { channel, cause } => (
                theme.danger,
                match cause {
                    Some(cause) => {
                        format!("#{} disconnected, reconnecting: {cause}", channel.as_str())
                    }
                    None => format!("#{} disconnected, reconnecting", channel.as_str()),
                },
            ),
        };
        let has_channel = !matches!(self.status, ConnectionStatus::Idle);
        h_flex()
            .gap_2()
            .items_center()
            .child(Input::new(&self.channel_input).w(px(220.)).small())
            .child(
                Button::new("connect")
                    .primary()
                    .small()
                    .label("Connect")
                    .on_click(cx.listener(|this, _, _, cx| this.connect(cx))),
            )
            .child(
                Button::new("disconnect")
                    .ghost()
                    .small()
                    .label("Disconnect")
                    .disabled(!has_channel)
                    .on_click(cx.listener(|this, _, _, cx| this.disconnect(cx))),
            )
            .child(div().size(px(10.)).rounded_full().bg(color))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(text),
            )
    }

    fn render_rule(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let open = self.giveaway.state() == GiveawayState::Open;
        let modes = MatchMode::ALL.iter().map(|&mode| {
            Button::new(SharedString::from(format!("mode-{mode:?}")))
                .small()
                .outline()
                .label(crate::i18n::mode_label(mode))
                .selected(self.config.mode == mode)
                .toggled(self.config.mode == mode)
                .on_click(cx.listener(move |this, _, _, cx| this.set_mode(mode, cx)))
        });
        h_flex()
            .gap_2()
            .items_center()
            .child(Label::new("Keyword"))
            .child(Input::new(&self.keyword_input).w(px(160.)).small())
            .children(modes)
            .child(div().flex_1())
            .child(
                div()
                    .text_sm()
                    .text_color(if open {
                        cx.theme().success
                    } else {
                        cx.theme().muted_foreground
                    })
                    .child(if open {
                        "entries open"
                    } else {
                        "entries closed"
                    }),
            )
            .child(
                Button::new("open-close")
                    .small()
                    .when(open, |b| b.danger().label("Close entries"))
                    .when(!open, |b| b.success().label("Open entries"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let next = if open {
                            GiveawayState::Closed
                        } else {
                            GiveawayState::Open
                        };
                        this.set_state(next, cx)
                    })),
            )
            .child(
                Button::new("reset")
                    .small()
                    .ghost()
                    .label("Reset")
                    .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
            )
    }

    fn render_weights(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let counts = self.giveaway.counts_by_role(&self.config.weights);
        let muted = cx.theme().muted_foreground;
        h_flex()
            .gap_3()
            .children(self.weight_inputs.iter().map(|(role, input)| {
                let count = counts
                    .iter()
                    .find(|(r, _)| r == role)
                    .map_or(0, |(_, n)| *n);
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .child(crate::i18n::role_label(*role)),
                    )
                    .child(Input::new(input).w(px(64.)).small())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{count} can win")),
                    )
            }))
    }

    fn render_entrants(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        // Only the visible rows are built: a big chat can bring thousands of entrants.
        let rows = uniform_list(
            "entrants",
            self.giveaway.entrants().len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                let theme = cx.theme();
                this.giveaway
                    .entrants()
                    .enumerate()
                    .skip(range.start)
                    .take(range.len())
                    .map(|(i, entrant)| {
                        let (role, weight) = entrant.effective(&this.config.weights);
                        h_flex()
                            .w_full()
                            .px_2()
                            .py_1()
                            .gap_2()
                            .when(i % 2 == 1, |row| row.bg(theme.list_even))
                            .when(weight == 0, |row| row.text_color(theme.muted_foreground))
                            .child(div().flex_1().child(entrant.display_name.clone()))
                            .child(div().w(px(80.)).child(crate::i18n::role_label(role)))
                            .child(div().w(px(60.)).text_right().child(weight.to_string()))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.entrants_scroll)
        .size_full();
        let can_win: usize = self
            .giveaway
            .counts_by_role(&self.config.weights)
            .iter()
            .map(|(_, n)| n)
            .sum();
        v_flex()
            .flex_1()
            .min_h_0()
            .border_1()
            .border_color(theme.border)
            .rounded_md()
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .text_sm()
                    .font_semibold()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().flex_1().child(format!(
                        "Entrants ({}, {can_win} can win)",
                        self.giveaway.entrants().len()
                    )))
                    .child(div().w(px(80.)).child("Role"))
                    .child(div().w(px(60.)).text_right().child("Tickets")),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(rows)
                    .child(Scrollbar::vertical(&self.entrants_scroll).viewport_from_layout()),
            )
    }

    fn render_draw(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .gap_3()
            .items_center()
            .child(
                Button::new("draw")
                    .primary()
                    .label("Draw winner")
                    .on_click(cx.listener(|this, _, _, cx| this.draw(cx))),
            )
            .children(self.winner.as_ref().map(|w| {
                div()
                    .text_xl()
                    .font_semibold()
                    .text_color(theme.success)
                    .child(format!("Winner: {}", w.display_name))
            }))
            .child(
                v_flex().children(
                    self.notices
                        .values()
                        .map(|n| div().text_sm().text_color(theme.danger).child(n.clone())),
                ),
            )
    }
}

impl Render for AppView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_4()
            .gap_4()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_connection(cx))
            .child(self.render_rule(cx))
            .child(self.render_weights(cx))
            .child(self.render_entrants(cx))
            .child(self.render_draw(cx))
    }
}

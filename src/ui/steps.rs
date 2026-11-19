use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use super::{AppView, Stage};
use crate::chat::ConnectionStatus;
use crate::domain::giveaway::GiveawayState;
use crate::domain::matcher::MatchMode;
use crate::i18n;

const CHECK: &str = "\u{2713}";

/// The four steps of running a giveaway, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Connect,
    Keyword,
    Entries,
    Draw,
}

/// What the step guide needs to know about the current state.
#[derive(Debug, Clone, Copy)]
pub(super) struct StepInputs {
    pub connected: bool,
    pub keyword_text_valid: bool,
    pub can_win: usize,
    pub winner_shown: bool,
}

/// The step the streamer should do next; none while a draw is on screen (the winner panel has
/// its own actions).
pub(super) fn next_step(inputs: StepInputs) -> Option<Step> {
    if inputs.winner_shown {
        None
    } else if !inputs.connected {
        Some(Step::Connect)
    } else if !inputs.keyword_text_valid {
        Some(Step::Keyword)
    } else if inputs.can_win == 0 {
        Some(Step::Entries)
    } else {
        Some(Step::Draw)
    }
}

impl AppView {
    pub(super) fn render_steps(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let connected = matches!(self.status, ConnectionStatus::Connected(_));
        let keyword_text_valid = self.keyword_text_valid(cx);
        let can_win = self.can_win();
        let open = self.giveaway.state() == GiveawayState::Open;
        let next = next_step(StepInputs {
            connected,
            keyword_text_valid,
            can_win,
            winner_shown: !matches!(self.stage, Stage::List),
        });

        let notices = self.notices.values().map(|notice| {
            div()
                .px_3()
                .py_2()
                .rounded_md()
                .bg(p.danger_soft)
                .text_sm()
                .text_color(p.danger)
                .child(notice.text())
        });

        v_flex()
            .id("steps")
            .w(px(390.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .p_4()
            .gap_3()
            .bg(p.bg)
            .border_r_1()
            .border_color(p.border)
            .children(notices)
            .child(self.step_card(
                1,
                next == Some(Step::Connect),
                connected,
                t!("step.connect.title").into_owned(),
                t!("step.connect.hint").into_owned(),
                self.render_connect(next == Some(Step::Connect), cx),
            ))
            .child(self.step_card(
                2,
                next == Some(Step::Keyword),
                connected && keyword_text_valid,
                t!("step.keyword.title").into_owned(),
                t!("step.keyword.hint").into_owned(),
                self.render_keyword(cx),
            ))
            .child(self.step_card(
                3,
                next == Some(Step::Entries),
                open || can_win > 0,
                t!("step.entries.title").into_owned(),
                if open {
                    t!("step.entries.hint_open").into_owned()
                } else {
                    t!("step.entries.hint_closed").into_owned()
                },
                self.render_entries(next == Some(Step::Entries), connected, open, cx),
            ))
            .child(self.step_card(
                4,
                next == Some(Step::Draw),
                false,
                t!("step.draw.title").into_owned(),
                t!("step.draw.hint").into_owned(),
                self.render_draw(next == Some(Step::Draw), can_win, cx),
            ))
    }

    fn step_card(
        &self,
        number: usize,
        active: bool,
        done: bool,
        title: String,
        hint: String,
        body: impl IntoElement,
    ) -> impl IntoElement {
        let p = self.palette;
        let (badge_bg, badge_fg, badge) = if done && !active {
            (p.success, p.on_accent, CHECK.to_owned())
        } else if active {
            (p.accent, p.on_accent, number.to_string())
        } else {
            (p.surface_alt, p.muted, number.to_string())
        };
        v_flex()
            .px_4()
            .py_3()
            .gap_2()
            .rounded_lg()
            .bg(p.surface)
            .border_1()
            .border_color(if active { p.accent } else { p.border })
            .when(active, |card| card.shadow_md())
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .size(px(28.))
                            .flex_none()
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(badge_bg)
                            .text_color(badge_fg)
                            .text_sm()
                            .font_bold()
                            .child(badge),
                    )
                    .child(div().flex_1().font_semibold().child(title))
                    .when(active, |row| {
                        row.child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_full()
                                .bg(p.accent_soft)
                                .text_color(p.accent)
                                .text_xs()
                                .font_semibold()
                                .child(t!("step.next").into_owned()),
                        )
                    }),
            )
            // Only the step to do now explains itself; the rest stay short so all four fit.
            .when(active, |card| {
                card.child(div().text_sm().text_color(p.muted).child(hint))
            })
            .child(body)
    }

    fn render_connect(&self, active: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let has_channel = !matches!(self.status, ConnectionStatus::Idle);
        h_flex()
            .gap_2()
            .child(Input::new(&self.channel_input).flex_1())
            .child(if has_channel {
                Button::new("disconnect")
                    .outline()
                    .label(t!("step.connect.disconnect").into_owned())
                    .on_click(cx.listener(|this, _, _, cx| this.disconnect(cx)))
            } else {
                Button::new("connect")
                    .when(active, |b| b.primary())
                    .when(!active, |b| b.outline())
                    .label(t!("step.connect.connect").into_owned())
                    .on_click(cx.listener(|this, _, _, cx| this.connect(cx)))
            })
    }

    fn render_keyword(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let keyword = self.config.keyword.as_str();
        let example = match self.config.mode {
            MatchMode::Word => t!("step.keyword.example.word", kw = keyword),
            MatchMode::Exact => t!("step.keyword.example.exact", kw = keyword),
            MatchMode::Contains => t!("step.keyword.example.contains", kw = keyword),
        };
        let modes = MatchMode::ALL.iter().map(|&mode| {
            let selected = self.config.mode == mode;
            Button::new(SharedString::from(format!("mode-{mode:?}")))
                .small()
                .flex_1()
                .when(selected, |b| b.primary())
                .when(!selected, |b| b.outline())
                .label(i18n::mode_label(mode))
                .toggled(selected)
                .on_click(cx.listener(move |this, _, _, cx| this.set_mode(mode, cx)))
        });
        let bonus_label = if self.bonus_open {
            t!("step.keyword.bonus_hide")
        } else {
            t!("step.keyword.bonus_show")
        };

        v_flex()
            .gap_2()
            .child(Input::new(&self.keyword_input))
            .child(h_flex().gap_1().children(modes))
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child(example.into_owned()),
            )
            .child(
                h_flex().child(
                    Button::new("bonus")
                        .xsmall()
                        .ghost()
                        .label(bonus_label.into_owned())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.bonus_open = !this.bonus_open;
                            cx.notify();
                        })),
                ),
            )
            .when(self.bonus_open, |col| col.child(self.render_bonus()))
    }

    fn render_bonus(&self) -> impl IntoElement {
        let p = self.palette;
        let fields = self.weight_inputs.iter().map(|(role, input)| {
            v_flex()
                .gap_1()
                .w(px(98.))
                .child(
                    div()
                        .text_xs()
                        .text_color(p.muted)
                        .truncate()
                        .child(i18n::role_label(*role)),
                )
                .child(Input::new(input).small())
        });
        v_flex()
            .gap_2()
            .p_3()
            .rounded_md()
            .bg(p.surface_alt)
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child(t!("step.keyword.bonus_hint").into_owned()),
            )
            .child(h_flex().flex_wrap().gap_2().children(fields))
    }

    fn render_entries(
        &self,
        active: bool,
        connected: bool,
        open: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = self.palette;
        let blocked = !open && !connected;
        let button = Button::new("entries")
            .w_full()
            .when(open, |b| {
                b.danger().label(t!("step.entries.close").into_owned())
            })
            .when(!open && active, |b| {
                b.primary().label(t!("step.entries.open").into_owned())
            })
            .when(!open && !active, |b| {
                b.outline().label(t!("step.entries.open").into_owned())
            })
            .disabled(blocked)
            .on_click(cx.listener(|this, _, _, cx| this.toggle_entries(cx)));
        v_flex().gap_2().child(button).when(blocked, |col| {
            col.child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child(t!("step.entries.connect_first").into_owned()),
            )
        })
    }

    fn render_draw(
        &self,
        active: bool,
        can_win: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = self.palette;
        let entrants = self.giveaway.entrants().len();
        let drawing = !matches!(self.stage, Stage::List);
        let reason = if can_win > 0 {
            None
        } else if entrants > 0 {
            Some(t!("step.draw.all_zero"))
        } else {
            Some(t!("step.draw.nobody"))
        };
        v_flex()
            .gap_2()
            .child(
                Button::new("draw")
                    .large()
                    .w_full()
                    .when(active, |b| b.primary())
                    .when(!active, |b| b.outline())
                    .label(t!("step.draw.draw").into_owned())
                    .disabled(can_win == 0 || drawing)
                    .on_click(cx.listener(|this, _, _, cx| this.draw(cx))),
            )
            .children(reason.map(|r| div().text_xs().text_color(p.muted).child(r.into_owned())))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Button::new("clear")
                            .xsmall()
                            .ghost()
                            .label(t!("step.draw.clear").into_owned())
                            .disabled(entrants == 0 && !drawing)
                            .on_click(cx.listener(|this, _, _, cx| this.clear_list(cx))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(p.muted)
                            .child(t!("step.draw.clear_hint").into_owned()),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that brings in gpui's own `test` attribute through `gpui_kit::*`.
    use super::{Step, StepInputs, next_step};

    fn inputs() -> StepInputs {
        StepInputs {
            connected: true,
            keyword_text_valid: true,
            can_win: 0,
            winner_shown: false,
        }
    }

    #[test]
    fn not_connected_points_at_connect() {
        let state = StepInputs {
            connected: false,
            keyword_text_valid: false,
            ..inputs()
        };
        assert_eq!(next_step(state), Some(Step::Connect));
    }

    #[test]
    fn empty_keyword_text_points_at_keyword() {
        let state = StepInputs {
            keyword_text_valid: false,
            ..inputs()
        };
        assert_eq!(next_step(state), Some(Step::Keyword));
    }

    #[test]
    fn nobody_who_can_win_points_at_entries() {
        assert_eq!(next_step(inputs()), Some(Step::Entries));
    }

    #[test]
    fn someone_who_can_win_points_at_draw() {
        let state = StepInputs {
            can_win: 3,
            ..inputs()
        };
        assert_eq!(next_step(state), Some(Step::Draw));
    }

    #[test]
    fn no_step_while_a_winner_is_shown() {
        let state = StepInputs {
            winner_shown: true,
            connected: false,
            ..inputs()
        };
        assert_eq!(next_step(state), None);
    }
}

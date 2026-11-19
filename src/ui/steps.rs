use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use super::palette;
use super::{AppView, Stage};
use crate::chat::ConnectionStatus;
use crate::domain::config::ChatMode;
use crate::domain::eligibility::TimeUnit;
use crate::domain::giveaway::GiveawayState;
use crate::domain::matcher::MatchMode;
use crate::domain::platform::Platform;
use crate::domain::role::Role;
use crate::i18n;

const CHECK: &str = "\u{2713}";

/// The steps the guide points at, in order. Setting the rules is optional, so it is never
/// the step to do next.
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
    pub draw_in_progress: bool,
}

/// The step the streamer should do next; none while a draw is on screen (the winner panel has
/// its own actions).
pub(super) fn next_step(inputs: StepInputs) -> Option<Step> {
    if inputs.draw_in_progress {
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
    pub(super) fn render_steps(&self, can_win: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let connected = self.any_connected();
        let keyword_text_valid = self.keyword_text_valid(cx);
        let open = self.giveaway.state() == GiveawayState::Open;
        let next = next_step(StepInputs {
            connected,
            keyword_text_valid,
            can_win,
            draw_in_progress: !matches!(self.stage, Stage::List),
        });

        let notices = self.notices.values().map(|notice| {
            div()
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(p.danger_soft)
                .text_sm()
                .text_color(p.danger)
                .child(notice.text())
        });

        v_flex()
            .id("steps")
            .w(px(400.))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .px_4()
            .py_4()
            .gap_3()
            .bg(p.bg)
            .border_r_1()
            .border_color(p.border)
            .children(notices)
            .child(self.step_section(
                1,
                next == Some(Step::Connect),
                connected,
                t!("step.connect.title").into_owned(),
                self.connect_hint(),
                self.render_connect(next == Some(Step::Connect), cx),
            ))
            .child(self.step_section(
                2,
                next == Some(Step::Keyword),
                connected && keyword_text_valid,
                t!("step.keyword.title").into_owned(),
                t!("step.keyword.hint").into_owned(),
                self.render_keyword(cx),
            ))
            .child(self.step_section(
                3,
                self.rules_open,
                false,
                t!("step.rules.title").into_owned(),
                t!("step.rules.hint").into_owned(),
                self.render_rules(cx),
            ))
            .child(self.step_section(
                4,
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
            .child(self.step_section(
                5,
                next == Some(Step::Draw),
                false,
                t!("step.draw.title").into_owned(),
                t!("step.draw.hint").into_owned(),
                self.render_draw(next == Some(Step::Draw), can_win, cx),
            ))
    }

    /// One step as a grouped section: a caption with its number above a rounded container. The
    /// step to do now gets the accent border and its hint; a finished one shows a check mark.
    fn step_section(
        &self,
        number: usize,
        active: bool,
        done: bool,
        title: String,
        hint: String,
        body: impl IntoElement,
    ) -> impl IntoElement {
        let p = self.palette;
        let mark = if done && !active {
            CHECK.to_owned()
        } else {
            number.to_string()
        };
        v_flex()
            .p_3()
            .gap_2()
            .rounded_xl()
            .bg(p.surface)
            .border_1()
            .border_color(if active { p.accent } else { p.border })
            .child(
                h_flex()
                    .gap_2()
                    .text_sm()
                    .font_semibold()
                    .child(
                        div()
                            .w(px(14.))
                            .text_color(if active { p.accent_text } else { p.muted })
                            .child(mark),
                    )
                    .child(div().flex_1().child(title)),
            )
            // Only the step to do now explains itself, so the column stays short.
            .when(active, |card| {
                card.child(div().text_sm().text_color(p.muted).child(hint))
            })
            .child(body)
    }

    fn connect_hint(&self) -> String {
        match self.config.chat_mode {
            ChatMode::Single(Platform::Twitch) => t!("step.connect.hint"),
            ChatMode::Single(Platform::Kick) => t!("step.connect.hint_kick"),
            ChatMode::Single(Platform::YouTube) => t!("step.connect.hint_youtube"),
            ChatMode::Multi => t!("step.connect.hint_multi"),
        }
        .into_owned()
    }

    /// First the choice of one platform or multistream, then the chat inputs for it.
    fn render_connect(&self, active: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let mode = self.config.chat_mode;
        let single_platform = match mode {
            ChatMode::Single(platform) => Some(platform),
            ChatMode::Multi => None,
        };
        let remembered = single_platform.unwrap_or(Platform::Twitch);
        let modes = [
            (
                ChatMode::Single(remembered),
                t!("step.connect.mode.single"),
                single_platform.is_some(),
            ),
            (
                ChatMode::Multi,
                t!("step.connect.mode.multi"),
                single_platform.is_none(),
            ),
        ]
        .map(|(choice, label, selected)| {
            Button::new(match choice {
                ChatMode::Single(_) => "chat-mode-single",
                ChatMode::Multi => "chat-mode-multi",
            })
            .small()
            .flex_1()
            .when(selected, |b| b.primary())
            .when(!selected, |b| b.ghost())
            .label(label.into_owned())
            .on_click(cx.listener(move |this, _, _, cx| this.set_chat_mode(choice, cx)))
        });
        let mut body = v_flex().gap_2().child(
            h_flex()
                .p_0p5()
                .gap_0p5()
                .rounded_lg()
                .bg(p.surface_alt)
                .children(modes),
        );
        body = match single_platform {
            Some(chosen) => {
                let platforms = Platform::ALL.map(|platform| {
                    let selected = platform == chosen;
                    Button::new(SharedString::from(format!("platform-{platform}")))
                        .small()
                        .flex_1()
                        .when(selected, |b| b.outline())
                        .when(!selected, |b| b.ghost())
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(palette::platform_logo(platform, 14.))
                                .child(i18n::platform_name(platform)),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_chat_mode(ChatMode::Single(platform), cx)
                        }))
                });
                body.child(h_flex().gap_1().children(platforms))
                    .child(self.render_chat_row(chosen, active, false, cx))
            }
            None => {
                let mut rows = Vec::new();
                for platform in Platform::ALL {
                    rows.push(self.render_chat_row(platform, active, true, cx));
                }
                body.children(rows)
            }
        };
        body
    }

    /// One chat's input and connect button; in multistream with the logo and its own status.
    fn render_chat_row(
        &self,
        platform: Platform,
        active: bool,
        multi: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = self.palette;
        let status = self.status(platform);
        // A failed chat offers Connect again; its status line still says why it stopped.
        let running = !matches!(
            status,
            ConnectionStatus::Idle | ConnectionStatus::Failed { .. }
        );
        let started = !matches!(status, ConnectionStatus::Idle);
        let button = if running {
            Button::new(SharedString::from(format!("disconnect-{platform}")))
                .outline()
                .label(t!("step.connect.disconnect").into_owned())
                .on_click(cx.listener(move |this, _, _, cx| this.disconnect(platform, cx)))
        } else {
            Button::new(SharedString::from(format!("connect-{platform}")))
                .when(active, |b| b.primary())
                .when(!active, |b| b.outline())
                .label(t!("step.connect.connect").into_owned())
                .on_click(cx.listener(move |this, _, _, cx| this.connect(platform, cx)))
        };
        let prefix = if multi {
            palette::platform_logo(platform, 14.).into_any_element()
        } else {
            Icon::new(IconName::MessagesSquare)
                .text_color(p.muted)
                .into_any_element()
        };
        let row = h_flex()
            .gap_2()
            .child(Input::new(self.input(platform)).prefix(prefix).flex_1())
            .child(button);
        let (dot, text) = self.status_line(status);
        v_flex()
            .gap_1()
            .child(row)
            .when(multi && started, |column| {
                column.child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .min_w_0()
                        .child(div().size(px(6.)).flex_none().rounded_full().bg(dot))
                        .child(div().text_xs().text_color(p.muted).truncate().child(text)),
                )
            })
            .into_any_element()
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
                .when(!selected, |b| b.ghost())
                .label(i18n::mode_label(mode))
                .toggled(selected)
                .on_click(cx.listener(move |this, _, _, cx| this.set_mode(mode, cx)))
        });

        v_flex()
            .gap_2()
            .child(Input::new(&self.keyword_input))
            // A segmented control: one track, the chosen segment filled.
            .child(
                h_flex()
                    .p_0p5()
                    .gap_0p5()
                    .rounded_lg()
                    .bg(p.surface_alt)
                    .children(modes),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child(example.into_owned()),
            )
    }

    /// Always shows who can enter and how many tickets each group gets, so nobody has to hunt
    /// for them; "Edit rules" opens the controls under the summary.
    fn render_rules(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let row = |label: String, value: AnyElement| {
            h_flex()
                .gap_3()
                .items_start()
                .child(
                    div()
                        .w(px(56.))
                        .flex_none()
                        .text_sm()
                        .text_color(p.muted)
                        .child(label),
                )
                .child(div().flex_1().min_w_0().text_sm().child(value))
        };
        // One item per group, wrapping as a whole, so a line never starts with a separator.
        let tickets = h_flex()
            .flex_wrap()
            .gap_x_4()
            .gap_y_1()
            .children(Role::ALL.iter().map(|&role| {
                h_flex().gap_1().child(i18n::role_label(role)).child(
                    div()
                        .font_semibold()
                        .text_color(p.accent_text)
                        .child(format!("×{}", self.config.weights.get(role))),
                )
            }))
            .into_any_element();
        let toggle_label = if self.rules_open {
            t!("step.rules.close")
        } else {
            t!("step.rules.edit")
        };
        v_flex()
            .gap_2()
            // The editor shows the same values, so the summary steps aside while it is open.
            .when(!self.rules_open, |col| {
                col.child(row(
                    t!("step.rules.who_short").into_owned(),
                    self.who_summary().into_any_element(),
                ))
                .child(row(t!("step.rules.tickets").into_owned(), tickets))
            })
            .when(self.rules_open, |col| {
                col.child(self.render_who(cx)).child(self.render_bonus())
            })
            .child(
                Button::new("rules")
                    .small()
                    .w_full()
                    .when(self.rules_open, |b| b.primary())
                    .when(!self.rules_open, |b| b.outline())
                    .label(toggle_label.into_owned())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.rules_open = !this.rules_open;
                        cx.notify();
                    })),
            )
    }

    fn render_bonus(&self) -> impl IntoElement {
        let p = self.palette;
        let fields = self.weight_inputs.iter().map(|(role, input)| {
            v_flex()
                .gap_1()
                .w(px(96.))
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
            .rounded_lg()
            .bg(p.surface_alt)
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .child(t!("step.rules.tickets").into_owned()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child(t!("step.keyword.bonus_hint").into_owned()),
            )
            .child(h_flex().flex_wrap().gap_2().children(fields))
    }

    fn who_summary(&self) -> String {
        let e = self.config.eligibility;
        let mut rules = Vec::new();
        if e.subs_only {
            rules.push(t!("who.subs").into_owned());
        }
        if e.require_activity {
            rules.push(
                t!(
                    "who.active_summary",
                    amount = e.active_window.amount,
                    unit = i18n::unit_label(e.active_window.unit)
                )
                .into_owned(),
            );
        }
        if rules.is_empty() {
            t!("who.everyone").into_owned()
        } else {
            rules.join(", ")
        }
    }

    fn render_who(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let e = self.config.eligibility;
        let units = [TimeUnit::Minutes, TimeUnit::Hours].map(|unit| {
            let selected = e.active_window.unit == unit;
            Button::new(SharedString::from(format!("unit-{unit:?}")))
                .xsmall()
                .when(selected, |b| b.primary())
                .when(!selected, |b| b.ghost())
                .label(i18n::unit_label(unit))
                .toggled(selected)
                .on_click(cx.listener(move |this, _, _, cx| this.set_window_unit(unit, cx)))
        });
        v_flex()
            .gap_2()
            .p_3()
            .rounded_lg()
            .bg(p.surface_alt)
            .child(
                div()
                    .text_sm()
                    .font_semibold()
                    .child(t!("step.rules.who").into_owned()),
            )
            .child(
                Checkbox::new("subs-only")
                    .checked(e.subs_only)
                    .label(t!("who.subs").into_owned())
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        let checked = *checked;
                        this.update_eligibility(|e| e.subs_only = checked, cx)
                    })),
            )
            .child(
                Checkbox::new("require-activity")
                    .checked(e.require_activity)
                    .label(t!("who.active").into_owned())
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        let checked = *checked;
                        this.update_eligibility(|e| e.require_activity = checked, cx)
                    })),
            )
            .child(
                h_flex()
                    .gap_1()
                    .pl_6()
                    .items_center()
                    .child(Input::new(&self.window_input).small().w(px(64.)))
                    .child(
                        h_flex()
                            .p_0p5()
                            .gap_0p5()
                            .rounded_md()
                            .bg(p.surface)
                            .children(units),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child(t!("who.hint").into_owned()),
            )
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
            .icon(if open {
                IconName::Square
            } else {
                IconName::Play
            })
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
        let reason = if drawing {
            Some(t!("step.draw.busy"))
        } else if can_win > 0 {
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
                    .icon(IconName::Trophy)
                    .when(active, |b| b.primary())
                    .when(!active, |b| b.outline())
                    .label(t!("step.draw.draw").into_owned())
                    .disabled(can_win == 0 || drawing)
                    .on_click(cx.listener(|this, _, _, cx| this.draw(cx))),
            )
            .children(reason.map(|r| div().text_xs().text_color(p.muted).child(r.into_owned())))
            // No file path here: the window is on stream, and the path shows the Windows user name.
            .children(self.last_log.as_ref().map(|_| {
                div()
                    .text_xs()
                    .text_color(p.muted)
                    .child(t!("step.draw.log_saved").into_owned())
            }))
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
            draw_in_progress: false,
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
    fn no_step_while_a_draw_is_on_screen() {
        let state = StepInputs {
            draw_in_progress: true,
            connected: false,
            ..inputs()
        };
        assert_eq!(next_step(state), None);
    }
}

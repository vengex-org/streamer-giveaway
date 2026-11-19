use std::time::{Duration, Instant};

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rand::Rng;
use rand::seq::IndexedRandom;
use rust_i18n::t;

use super::AppView;
use crate::domain::giveaway::{Entrant, GiveawayState};
use crate::domain::winner::{WaitState, WinnerWatch};
use crate::i18n;

/// Names shown during the reveal, one per tick; with a 100 ms tick the reveal takes about 2 s.
pub(super) const REVEAL_FRAMES: usize = 20;

/// What the stage shows.
pub(super) enum Stage {
    List,
    Revealing {
        reel: Vec<String>,
        frame: usize,
        winner: Entrant,
    },
    Winner {
        watch: WinnerWatch,
        /// Last whole second rendered, so the stopwatch redraws once per second.
        shown_second: u64,
    },
}

/// Names to flash before the winner: random names from the pool, ending on the winner.
pub(super) fn reveal_reel<R: Rng + ?Sized>(
    pool: &[String],
    winner: &str,
    rng: &mut R,
) -> Vec<String> {
    let mut reel: Vec<String> = (1..REVEAL_FRAMES)
        .filter_map(|_| pool.choose(rng).cloned())
        .collect();
    reel.push(winner.to_owned());
    reel
}

/// `m:ss`, e.g. `0:42` or `12:05`.
pub(super) fn format_clock(duration: Duration) -> String {
    let secs = duration.as_secs();
    format!("{}:{:02}", secs / 60, secs % 60)
}

impl AppView {
    pub(super) fn render_stage(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.stage {
            Stage::List => self.render_entrants(cx).into_any_element(),
            Stage::Revealing { reel, frame, .. } => self
                .render_reveal(&reel[(*frame).min(reel.len() - 1)])
                .into_any_element(),
            Stage::Winner { watch, .. } => self.render_winner(watch, cx).into_any_element(),
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .p_6()
            .gap_5()
            .child(self.render_stats())
            .child(body)
    }

    fn render_stats(&self) -> impl IntoElement {
        let p = self.palette;
        let open = self.giveaway.state() == GiveawayState::Open;
        let number_card = |value: usize, label: String| {
            v_flex()
                .flex_1()
                .p_5()
                .gap_1()
                .rounded_xl()
                .bg(p.surface)
                .border_1()
                .border_color(p.border)
                .child(
                    div()
                        .text_size(px(44.))
                        .font_black()
                        .line_height(px(48.))
                        .child(value.to_string()),
                )
                .child(div().text_color(p.muted).child(label))
        };
        h_flex()
            .gap_4()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .p_5()
                    .gap_1()
                    .rounded_xl()
                    .bg(p.accent_soft)
                    .border_1()
                    .border_color(p.accent)
                    .child(
                        h_flex()
                            .flex_wrap()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .text_color(p.muted)
                                    .child(t!("stage.keyword_card").into_owned()),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .rounded_full()
                                    .text_xs()
                                    .font_semibold()
                                    .bg(if open { p.success } else { p.surface_alt })
                                    .text_color(if open { p.on_accent } else { p.muted })
                                    .child(if open {
                                        t!("stage.entries_open").into_owned()
                                    } else {
                                        t!("stage.entries_closed").into_owned()
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(44.))
                            .line_height(px(48.))
                            .font_black()
                            .text_color(p.accent)
                            .truncate()
                            .child(self.config.keyword.as_str().to_owned()),
                    ),
            )
            .child(number_card(
                self.giveaway.entrants().len(),
                t!("stage.entered").into_owned(),
            ))
            .child(number_card(
                self.can_win(),
                t!("stage.can_win").into_owned(),
            ))
    }

    fn render_entrants(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let count = self.giveaway.entrants().len();
        let header = h_flex()
            .px_4()
            .py_2()
            .gap_2()
            .text_sm()
            .font_semibold()
            .text_color(p.muted)
            .border_b_1()
            .border_color(p.border)
            .child(div().flex_1().child(t!("stage.list.title").into_owned()))
            .child(div().w(px(120.)).child(t!("stage.list.role").into_owned()))
            .child(
                div()
                    .w(px(70.))
                    .text_right()
                    .child(t!("stage.list.tickets").into_owned()),
            );
        let body = if count == 0 {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .p_6()
                .text_color(p.muted)
                .child(t!("stage.list.empty", kw = self.config.keyword.as_str()).into_owned())
                .into_any_element()
        } else {
            // Only the visible rows are built: a big chat can bring thousands of entrants.
            let rows = uniform_list(
                "entrants",
                count,
                cx.processor(|this, range: std::ops::Range<usize>, _, _| {
                    let p = this.palette;
                    this.giveaway
                        .entrants()
                        .enumerate()
                        .skip(range.start)
                        .take(range.len())
                        .map(|(i, entrant)| {
                            let (role, weight) = entrant.effective(&this.config.weights);
                            h_flex()
                                .w_full()
                                .px_4()
                                .py_1p5()
                                .gap_2()
                                .when(i % 2 == 1, |row| row.bg(p.surface_alt))
                                .when(weight == 0, |row| row.text_color(p.muted))
                                .child(
                                    div()
                                        .flex_1()
                                        .truncate()
                                        .child(entrant.display_name.clone()),
                                )
                                .child(div().w(px(120.)).text_sm().child(i18n::role_label(role)))
                                .child(
                                    div()
                                        .w(px(70.))
                                        .text_right()
                                        .font_semibold()
                                        .child(weight.to_string()),
                                )
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.entrants_scroll)
            .size_full();
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .child(rows)
                .child(Scrollbar::vertical(&self.entrants_scroll).viewport_from_layout())
                .into_any_element()
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .rounded_xl()
            .bg(p.surface)
            .border_1()
            .border_color(p.border)
            .overflow_hidden()
            .child(header)
            .child(body)
    }

    fn render_reveal(&self, name: &str) -> impl IntoElement {
        let p = self.palette;
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_3()
            .rounded_xl()
            .bg(p.surface)
            .border_2()
            .border_color(p.gold)
            .child(
                div()
                    .text_lg()
                    .text_color(p.muted)
                    .child(t!("winner.drawing").into_owned()),
            )
            .child(
                div()
                    .text_size(px(60.))
                    .font_black()
                    .text_color(p.text)
                    .child(name.to_owned()),
            )
    }

    fn render_winner(&self, watch: &WinnerWatch, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let winner = watch.winner();
        let (role, _) = winner.effective(&self.config.weights);
        let elapsed = format_clock(watch.elapsed(Instant::now()));
        let status = match watch.state() {
            WaitState::Waiting => div()
                .text_color(p.muted)
                .child(t!("winner.waiting", name = winner.display_name.as_str()).into_owned()),
            WaitState::Answered { after } => div()
                .px_3()
                .py_1()
                .rounded_md()
                .bg(p.gold_soft)
                .text_color(p.gold)
                .font_semibold()
                .child(t!("winner.answered", time = format_clock(after)).into_owned()),
        };
        let replies = watch.replies().iter().map(|reply| {
            h_flex()
                .gap_3()
                .px_4()
                .py_2()
                .child(
                    div()
                        .w(px(48.))
                        .flex_none()
                        .text_sm()
                        .text_color(p.muted)
                        .child(format_clock(reply.after)),
                )
                .child(div().flex_1().min_w_0().child(reply.text.clone()))
        });
        let can_win = self.can_win();

        v_flex()
            .flex_1()
            .min_h_0()
            .p_6()
            .gap_4()
            .items_center()
            .rounded_xl()
            .bg(p.surface)
            .border_2()
            .border_color(p.gold)
            .child(
                div()
                    .text_lg()
                    .font_bold()
                    .text_color(p.gold)
                    .child(t!("winner.title").into_owned()),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(60.))
                            .line_height(px(64.))
                            .font_black()
                            .text_color(p.gold)
                            .child(winner.display_name.clone()),
                    )
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_full()
                            .bg(p.accent_soft)
                            .text_color(p.accent)
                            .text_sm()
                            .font_semibold()
                            .child(i18n::role_label(role)),
                    ),
            )
            .child(
                div()
                    .text_xl()
                    .font_semibold()
                    .child(t!("winner.since", time = elapsed).into_owned()),
            )
            .child(status)
            // The replies box appears with the first reply; until then empty space, not an empty box.
            .child(if watch.replies().is_empty() {
                div().flex_1().into_any_element()
            } else {
                v_flex()
                    .id("replies")
                    .w_full()
                    .max_w(px(640.))
                    .flex_1()
                    .min_h(px(80.))
                    .overflow_y_scroll()
                    .rounded_md()
                    .bg(p.surface_alt)
                    .children(replies)
                    .into_any_element()
            })
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        Button::new("draw-another")
                            .large()
                            .primary()
                            .label(t!("winner.again").into_owned())
                            .disabled(can_win == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.draw(cx))),
                    )
                    .child(
                        Button::new("done")
                            .large()
                            .outline()
                            .label(t!("winner.done").into_owned())
                            .on_click(cx.listener(|this, _, _, cx| this.finish_winner(cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    // Not `super::*`: that brings in gpui's own `test` attribute through `gpui_kit::*`.
    use super::{Duration, REVEAL_FRAMES, format_clock, reveal_reel};

    #[test]
    fn reel_ends_on_the_winner_and_uses_only_pool_names() {
        let pool: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        let reel = reveal_reel(&pool, "winner", &mut StdRng::seed_from_u64(1));
        assert_eq!(reel.len(), REVEAL_FRAMES);
        assert_eq!(reel.last().map(String::as_str), Some("winner"));
        assert!(reel[..REVEAL_FRAMES - 1].iter().all(|n| pool.contains(n)));
    }

    #[test]
    fn reel_with_empty_pool_shows_only_the_winner() {
        let reel = reveal_reel(&[], "solo", &mut StdRng::seed_from_u64(1));
        assert_eq!(reel, ["solo"]);
    }

    #[test]
    fn clock_shows_minutes_and_padded_seconds() {
        assert_eq!(format_clock(Duration::ZERO), "0:00");
        assert_eq!(format_clock(Duration::from_secs(42)), "0:42");
        assert_eq!(format_clock(Duration::from_millis(125_900)), "2:05");
        assert_eq!(format_clock(Duration::from_secs(3600)), "60:00");
    }
}

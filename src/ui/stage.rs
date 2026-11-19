use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::scroll::Scrollbar;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rand::Rng;
use rand::seq::IndexedRandom;
use rust_i18n::t;

use super::AppView;
use crate::domain::chat::ChatMessage;
use crate::domain::giveaway::GiveawayState;
use crate::domain::winner::{WaitState, WinnerWatch};
use crate::i18n;

/// How often the stage ticker runs while a draw is shown: one reveal frame, and often enough for
/// a stopwatch that changes once per second.
pub(super) const TICK: Duration = Duration::from_millis(100);
/// How long the names flash before the winner shows.
const REVEAL: Duration = Duration::from_secs(2);
/// Names shown during the reveal, one per tick.
pub(super) const REVEAL_FRAMES: usize = (REVEAL.as_millis() / TICK.as_millis()) as usize;

/// What the stage shows. The winner is followed from the moment of the draw, so a reply sent
/// during the reveal counts and the stopwatch starts at the draw.
pub(super) enum Stage {
    List,
    Revealing {
        reel: Vec<String>,
        frame: usize,
        watch: WinnerWatch,
    },
    Winner {
        watch: WinnerWatch,
        /// Last whole second rendered, so the stopwatch redraws once per second.
        shown_second: u64,
    },
}

impl Stage {
    /// Passes a chat message to the drawn winner's watch; returns whether it was the winner's.
    pub(super) fn on_message(&mut self, msg: &ChatMessage, now: Instant) -> bool {
        match self {
            Stage::List => false,
            Stage::Revealing { watch, .. } | Stage::Winner { watch, .. } => {
                watch.on_message(msg, now)
            }
        }
    }
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
    pub(super) fn render_stage(&self, can_win: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match &self.stage {
            Stage::List => self.render_entrants(cx).into_any_element(),
            Stage::Revealing { reel, frame, .. } => self
                .render_reveal(&reel[(*frame).min(reel.len() - 1)])
                .into_any_element(),
            Stage::Winner { watch, .. } => {
                self.render_winner(watch, can_win, cx).into_any_element()
            }
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .p_6()
            .gap_4()
            .child(self.render_scoreboard(can_win))
            .child(body)
    }

    /// The part viewers read from across the room: the keyword, whether entries are open, and how
    /// many people can win. Live throughput sits in one quiet line under it while entries are open.
    fn render_scoreboard(&self, can_win: usize) -> impl IntoElement {
        let p = self.palette;
        let open = self.giveaway.state() == GiveawayState::Open;
        let state = h_flex()
            .gap_2()
            .items_center()
            .text_sm()
            .font_semibold()
            .text_color(if open { p.live } else { p.muted })
            .child(
                div()
                    .size(px(10.))
                    .rounded_full()
                    .bg(if open { p.live } else { p.border }),
            )
            .child(if open {
                t!("stage.entries_open").into_owned()
            } else {
                t!("stage.entries_closed").into_owned()
            });
        v_flex()
            .px_6()
            .py_5()
            .gap_3()
            .rounded_2xl()
            .bg(p.surface)
            .child(
                h_flex()
                    .gap_6()
                    .items_end()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(p.muted)
                                    .child(t!("stage.keyword_card").into_owned()),
                            )
                            .child(
                                div()
                                    .text_size(px(56.))
                                    .line_height(px(60.))
                                    .font_black()
                                    .text_color(p.accent_text)
                                    .truncate()
                                    .child(self.config.keyword.as_str().to_owned()),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_none()
                            .items_end()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(p.muted)
                                    .child(t!("stage.can_win").into_owned()),
                            )
                            .child(
                                div()
                                    .text_size(px(56.))
                                    .line_height(px(60.))
                                    .font_black()
                                    .child(can_win.to_string()),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap_4()
                    .items_center()
                    .pt_3()
                    .border_t_1()
                    .border_color(p.border)
                    .child(state)
                    .when(self.session.is_some(), |row| {
                        row.child(self.render_live_stats())
                    }),
            )
    }

    fn render_live_stats(&self) -> impl IntoElement {
        let p = self.palette;
        let now = Instant::now();
        let rate = format!("{:.1}", self.stats.messages_per_second(now));
        let last = match self.stats.since_last_entry(now) {
            Some(ago) => t!("stage.stats.last", ago = format_clock(ago)),
            None => t!("stage.stats.none"),
        };
        let parts = [
            t!("stage.stats.rate", rate = rate).into_owned(),
            t!(
                "stage.stats.new",
                count = self.stats.entries_last_minute(now)
            )
            .into_owned(),
            last.into_owned(),
        ];
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_sm()
            .text_color(p.muted)
            .child(parts.join("  ·  "))
    }

    fn render_entrants(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let count = self.giveaway.entrants().len();
        let header = h_flex()
            .px_5()
            .py_2p5()
            .gap_2()
            .text_xs()
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
        let body =
            if count == 0 {
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .p_6()
                    .child(
                        Icon::new(IconName::MessageCircleMore)
                            .size(px(44.))
                            .text_color(p.muted),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_semibold()
                            .child(t!("stage.list.empty_title").into_owned()),
                    )
                    .child(div().text_color(p.muted).child(
                        t!("stage.list.empty", kw = self.config.keyword.as_str()).into_owned(),
                    ))
                    .into_any_element()
            } else {
                // Only the visible rows are built: a big chat can bring thousands of entrants.
                let rows = uniform_list(
                    "entrants",
                    count,
                    cx.processor(|this, range: std::ops::Range<usize>, _, _| {
                        let p = this.palette;
                        range
                            .filter_map(|i| this.giveaway.entrant_at(i))
                            .map(|entrant| {
                                let (role, weight) = entrant.effective(&this.config.weights);
                                h_flex()
                                    .w_full()
                                    .px_5()
                                    .py_2()
                                    .gap_2()
                                    .border_b_1()
                                    .border_color(p.surface_alt)
                                    .when(weight == 0, |row| row.text_color(p.muted))
                                    .child(
                                        div()
                                            .flex_1()
                                            .truncate()
                                            .child(entrant.display_name.clone()),
                                    )
                                    .child(
                                        div()
                                            .w(px(120.))
                                            .text_sm()
                                            .text_color(p.muted)
                                            .child(i18n::role_label(role)),
                                    )
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
            .rounded_2xl()
            .bg(p.surface)
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
            .rounded_2xl()
            .bg(p.surface)
            .child(
                div()
                    .text_lg()
                    .text_color(p.muted)
                    .child(t!("winner.drawing").into_owned()),
            )
            .child(
                div()
                    .text_size(px(64.))
                    .font_black()
                    .text_color(p.text)
                    .child(name.to_owned()),
            )
    }

    fn render_winner(
        &self,
        watch: &WinnerWatch,
        can_win: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let p = self.palette;
        let winner = watch.winner();
        let (role, _) = winner.effective(&self.config.weights);
        let elapsed = format_clock(watch.elapsed(Instant::now()));
        let status = match watch.state() {
            WaitState::Waiting => div()
                .text_color(p.muted)
                .child(t!("winner.waiting", name = winner.display_name.as_str()).into_owned()),
            WaitState::Answered { after } => div()
                .px_4()
                .py_1p5()
                .rounded_full()
                .bg(p.accent_soft)
                .text_color(p.accent_text)
                .font_semibold()
                .child(t!("winner.answered", time = format_clock(after)).into_owned()),
        };
        let replies = watch.replies().iter().map(|reply| {
            h_flex()
                .gap_3()
                .px_4()
                .py_2()
                .border_b_1()
                .border_color(p.border)
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

        v_flex()
            .flex_1()
            .min_h_0()
            .p_8()
            .gap_4()
            .items_center()
            .rounded_2xl()
            .bg(p.surface)
            .border_2()
            .border_color(p.accent)
            .child(
                div()
                    .text_base()
                    .font_semibold()
                    .text_color(p.accent_text)
                    .child(t!("winner.title").into_owned()),
            )
            .child(
                div()
                    .text_size(px(68.))
                    .line_height(px(72.))
                    .font_black()
                    .text_color(p.accent_text)
                    .child(winner.display_name.clone()),
            )
            .child(div().text_color(p.muted).child(i18n::role_label(role)))
            .child(
                div()
                    .text_size(px(28.))
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
                    .rounded_xl()
                    .bg(p.surface_alt)
                    .children(replies)
                    .into_any_element()
            })
            .when(can_win == 0, |panel| {
                panel.child(
                    div()
                        .text_sm()
                        .text_color(p.muted)
                        .child(t!("winner.nobody_left").into_owned()),
                )
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
    use std::time::Instant;

    use super::{Duration, REVEAL_FRAMES, Stage, format_clock, reveal_reel};
    use crate::domain::chat::ChatMessage;
    use crate::domain::giveaway::Entrant;
    use crate::domain::role::Role;
    use crate::domain::winner::{WaitState, WinnerWatch};

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

    #[test]
    fn reveal_lasts_about_two_seconds() {
        assert_eq!(REVEAL_FRAMES, 20);
    }

    #[test]
    fn a_reply_during_the_reveal_counts() {
        let drawn_at = Instant::now();
        let winner = Entrant {
            user_id: "7".into(),
            display_name: "Lucky".into(),
            roles: vec![Role::Pleb],
        };
        let mut stage = Stage::Revealing {
            reel: vec!["Lucky".into()],
            frame: 0,
            watch: WinnerWatch::new(winner, drawn_at),
        };
        let reply = ChatMessage {
            user_id: "7".into(),
            display_name: "Lucky".into(),
            badges: vec![],
            text: "me!".into(),
        };
        assert!(stage.on_message(&reply, drawn_at + Duration::from_secs(1)));
        let Stage::Revealing { watch, .. } = stage else {
            panic!("still revealing");
        };
        assert_eq!(
            watch.state(),
            WaitState::Answered {
                after: Duration::from_secs(1)
            }
        );
    }
}

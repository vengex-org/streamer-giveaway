use std::path::PathBuf;

use chrono::{Local, NaiveDateTime, TimeZone, Utc};
use gpui_kit::component::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::*;
use gpui_kit::*;
use rust_i18n::t;

use super::AppView;
use super::stage::format_clock;
use crate::domain::session_log::{self, SessionSummary};
use crate::storage::{self, ConfigStore};

/// One past giveaway as the history page lists it.
pub(super) struct HistoryEntry {
    /// Local start time, or the raw stamp when it does not parse.
    when: String,
    summary: Result<SessionSummary, String>,
}

/// What the history page found in the logs folder.
pub(super) enum History {
    Loaded(Vec<HistoryEntry>),
    /// The folder exists but cannot be read.
    Failed(String),
}

/// `20261119-085751Z` (UTC, from the file name) as local `2026-11-19 09:57`.
fn local_time(stamp: &str) -> Option<String> {
    let utc = NaiveDateTime::parse_from_str(stamp, "%Y%m%d-%H%M%SZ").ok()?;
    let local = Utc.from_utc_datetime(&utc).with_timezone(&Local);
    Some(local.format("%Y-%m-%d %H:%M").to_string())
}

/// Reads every log in the logs folder. A file that cannot be read or parsed keeps its row with
/// the reason, so nothing disappears without a word.
pub(super) fn load_history() -> History {
    let Some(dir) = ConfigStore::logs_dir() else {
        return History::Failed("no user data folder".into());
    };
    match storage::session_logs(&dir) {
        Ok(logs) => History::Loaded(
            logs.into_iter()
                .map(|(stamp, path)| HistoryEntry {
                    when: local_time(&stamp).unwrap_or(stamp),
                    summary: std::fs::read_to_string(&path)
                        .map_err(|err| err.to_string())
                        .and_then(|csv| {
                            session_log::summarize(&csv).map_err(|err| err.to_string())
                        }),
                })
                .collect(),
        ),
        Err(err) => History::Failed(err.to_string()),
    }
}

/// Where the logs live, for "Open folder"; created so the button always has somewhere to go.
fn logs_folder() -> Option<PathBuf> {
    let dir = ConfigStore::logs_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

const WHEN_W: f32 = 170.;
const NUMBER_W: f32 = 110.;

impl AppView {
    pub(super) fn render_history(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let header_cell = |text: String, width: Option<f32>| {
            let cell = div()
                .text_xs()
                .font_semibold()
                .text_color(p.muted)
                .child(text);
            match width {
                Some(w) => cell.w(px(w)).flex_none(),
                None => cell.flex_1().min_w_0(),
            }
        };
        let header = h_flex()
            .px_5()
            .py_2p5()
            .gap_3()
            .border_b_1()
            .border_color(p.border)
            .child(header_cell(
                t!("history.col.when").into_owned(),
                Some(WHEN_W),
            ))
            .child(header_cell(
                t!("history.col.open").into_owned(),
                Some(NUMBER_W),
            ))
            .child(header_cell(
                t!("history.col.entrants").into_owned(),
                Some(NUMBER_W),
            ))
            .child(header_cell(
                t!("history.col.messages").into_owned(),
                Some(NUMBER_W),
            ))
            .child(header_cell(
                t!("history.col.ninety").into_owned(),
                Some(140.),
            ))
            .child(header_cell(t!("history.col.winners").into_owned(), None));

        let body = match &self.history {
            History::Failed(cause) => self
                .history_message(t!("history.list_failed", cause = cause.as_str()).into_owned())
                .into_any_element(),
            History::Loaded(entries) if entries.is_empty() => self
                .history_message(t!("history.empty").into_owned())
                .into_any_element(),
            History::Loaded(entries) => v_flex()
                .id("history-rows")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(entries.iter().map(|entry| self.history_row(entry)))
                .into_any_element(),
        };

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .p_6()
            .gap_4()
            .child(
                h_flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_xl()
                            .font_bold()
                            .child(t!("nav.history").into_owned()),
                    )
                    .child(
                        Button::new("open-logs")
                            .outline()
                            .icon(IconName::FolderOpen)
                            .label(t!("history.open_folder").into_owned())
                            .on_click(cx.listener(|_, _, _, cx| {
                                if let Some(dir) = logs_folder() {
                                    cx.open_with_system(&dir);
                                }
                            })),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .rounded_2xl()
                    .bg(p.surface)
                    .overflow_hidden()
                    .child(header)
                    .child(body),
            )
    }

    fn history_message(&self, text: String) -> impl IntoElement {
        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .text_color(self.palette.muted)
            .child(text)
    }

    fn history_row(&self, entry: &HistoryEntry) -> impl IntoElement {
        let p = self.palette;
        let cell = |text: String, width: f32| div().w(px(width)).flex_none().child(text);
        let row = h_flex()
            .px_5()
            .py_2()
            .gap_3()
            .border_b_1()
            .border_color(p.surface_alt)
            .child(cell(entry.when.clone(), WHEN_W));
        match &entry.summary {
            Ok(summary) => row
                .child(cell(format_clock(summary.open_for), NUMBER_W))
                .child(cell(summary.entrants.to_string(), NUMBER_W))
                .child(cell(summary.messages.to_string(), NUMBER_W))
                .child(cell(
                    summary
                        .ninety_percent_in
                        .map(format_clock)
                        .unwrap_or_else(|| "–".to_owned()),
                    140.,
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(p.accent_text)
                        .child(summary.winners.join(", ")),
                ),
            Err(cause) => row.child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(p.danger)
                    .child(t!("history.unreadable", cause = cause.as_str()).into_owned()),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that brings in gpui's own `test` attribute through `gpui_kit::*`.
    use super::local_time;

    #[test]
    fn stamps_become_readable_times_and_bad_stamps_are_rejected() {
        let shown = local_time("20261119-085751Z").unwrap();
        assert!(shown.starts_with("2026-11-1"), "{shown}");
        assert_eq!(shown.len(), "2026-11-19 09:57".len());
        assert_eq!(local_time("yesterday"), None);
    }
}

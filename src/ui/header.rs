use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::select::{Select, SelectItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rust_i18n::t;

use super::{AppView, Page, RunMode};
use crate::chat::{ConnectionStatus, FailReason};
use crate::domain::prefs::{Language, ThemeChoice};
use crate::i18n;

/// One row of the language select. `None` follows the OS language.
#[derive(Debug, Clone)]
pub(super) struct LanguageOption {
    choice: Option<Language>,
    title: SharedString,
}

impl SelectItem for LanguageOption {
    type Value = Option<Language>;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.choice
    }
}

/// "Auto" first, labeled in the current UI language, then each language in its own name.
pub(super) fn language_options() -> Vec<LanguageOption> {
    let auto = LanguageOption {
        choice: None,
        title: t!("header.lang_auto").into_owned().into(),
    };
    let languages = Language::ALL.iter().map(|&language| LanguageOption {
        choice: Some(language),
        title: i18n::language_name(language).into(),
    });
    std::iter::once(auto).chain(languages).collect()
}

impl AppView {
    pub(super) fn render_header(&self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let (dot, status) = match &self.status {
            ConnectionStatus::Idle => (p.muted, t!("header.status.idle")),
            ConnectionStatus::Connecting { target, cause } => (
                p.accent,
                match cause {
                    Some(cause) => t!(
                        "header.status.connecting_cause",
                        channel = target.label(),
                        cause = cause
                    ),
                    None => t!("header.status.connecting", channel = target.label()),
                },
            ),
            ConnectionStatus::Connected(target) => (
                p.success,
                t!("header.status.connected", channel = target.label()),
            ),
            ConnectionStatus::Reconnecting { target, cause } => (
                p.danger,
                match cause {
                    Some(cause) => t!(
                        "header.status.reconnecting_cause",
                        channel = target.label(),
                        cause = cause
                    ),
                    None => t!("header.status.reconnecting", channel = target.label()),
                },
            ),
            ConnectionStatus::Failed { target, reason } => (
                p.danger,
                t!(
                    "header.status.failed",
                    channel = target.label(),
                    reason = fail_reason(reason)
                ),
            ),
        };

        // One button walks System, Light, Dark; its icon and tooltip say which is on.
        let (theme_icon, theme_label, next_theme) = match self.config.theme {
            None => (
                IconName::SunMoon,
                t!("header.theme.system"),
                Some(ThemeChoice::Light),
            ),
            Some(ThemeChoice::Light) => (
                IconName::Sun,
                t!("header.theme.light"),
                Some(ThemeChoice::Dark),
            ),
            Some(ThemeChoice::Dark) => (IconName::Moon, t!("header.theme.dark"), None),
        };
        let theme = Button::new("theme")
            .ghost()
            .icon(theme_icon)
            .tooltip(t!("header.theme.tooltip", theme = theme_label).into_owned())
            .on_click(
                cx.listener(move |this, _, window, cx| this.set_theme(next_theme, window, cx)),
            );
        // Live: a button that opens a demo window. Demo: a label, so the two windows are told apart.
        let demo = match self.mode {
            RunMode::Live => Button::new("demo")
                .small()
                .ghost()
                .icon(IconName::FlaskConical)
                .label(t!("header.demo.button").into_owned())
                .tooltip(t!("header.demo.tooltip").into_owned())
                .on_click(cx.listener(|this, _, _, cx| this.open_demo(cx)))
                .into_any_element(),
            RunMode::Demo => div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .bg(p.accent_soft)
                .text_sm()
                .font_semibold()
                .text_color(p.accent_text)
                .child(t!("header.demo.badge").into_owned())
                .into_any_element(),
        };
        let pages = [
            (Page::Giveaway, t!("nav.giveaway")),
            (Page::History, t!("nav.history")),
        ]
        .map(|(page, label)| {
            let selected = self.page == page;
            Button::new(SharedString::from(format!("page-{page:?}")))
                .small()
                .when(selected, |b| b.primary())
                .when(!selected, |b| b.ghost())
                .label(label.into_owned())
                .on_click(cx.listener(move |this, _, _, cx| this.set_page(page, cx)))
        });

        h_flex()
            .px_5()
            .py_3()
            .gap_4()
            .items_center()
            .bg(p.bg)
            .border_b_1()
            .border_color(p.border)
            .child(
                div()
                    .text_base()
                    .font_bold()
                    .text_color(p.text)
                    .child(t!("app.title").into_owned()),
            )
            .child(
                h_flex()
                    .p_0p5()
                    .gap_0p5()
                    .rounded_lg()
                    .bg(p.surface_alt)
                    .children(pages),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .min_w_0()
                    .child(div().size(px(8.)).rounded_full().bg(dot))
                    .child(
                        div()
                            .text_sm()
                            .text_color(p.muted)
                            .truncate()
                            .child(status.into_owned()),
                    ),
            )
            .child(div().flex_1())
            .child(demo)
            // The select fills its parent, so a fixed box keeps it from taking the header's room.
            .child(
                div().flex_none().w(px(190.)).child(
                    Select::new(&self.language_select)
                        .id("language")
                        .small()
                        .w_full(),
                ),
            )
            .child(theme)
    }
}

/// Why a chat cannot be read, in the UI language.
pub(super) fn fail_reason(reason: &FailReason) -> String {
    match reason {
        FailReason::NotFound => t!("status.fail.not_found"),
        FailReason::Offline => t!("status.fail.offline"),
        FailReason::Blocked => t!("status.fail.blocked"),
        FailReason::Protocol(detail) => t!("status.fail.protocol", detail = detail),
    }
    .into_owned()
}

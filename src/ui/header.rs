use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::*;
use gpui_kit::*;
use rust_i18n::t;

use super::AppView;
use crate::chat::ConnectionStatus;
use crate::domain::prefs::{Language, ThemeChoice};

impl AppView {
    pub(super) fn render_header(&self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let (dot, status) = match &self.status {
            ConnectionStatus::Idle => (p.muted, t!("header.status.idle")),
            ConnectionStatus::Connecting { channel, cause } => (
                p.gold,
                match cause {
                    Some(cause) => t!(
                        "header.status.connecting_cause",
                        channel = channel.as_str(),
                        cause = cause
                    ),
                    None => t!("header.status.connecting", channel = channel.as_str()),
                },
            ),
            ConnectionStatus::Connected(channel) => (
                p.success,
                t!("header.status.connected", channel = channel.as_str()),
            ),
            ConnectionStatus::Reconnecting { channel, cause } => (
                p.danger,
                match cause {
                    Some(cause) => t!(
                        "header.status.reconnecting_cause",
                        channel = channel.as_str(),
                        cause = cause
                    ),
                    None => t!("header.status.reconnecting", channel = channel.as_str()),
                },
            ),
        };

        let current = self.language();
        let languages = Language::ALL.iter().map(|&language| {
            Button::new(SharedString::from(format!("lang-{}", language.code())))
                .xsmall()
                .ghost()
                .label(language.code().to_uppercase())
                .selected(language == current)
                .on_click(
                    cx.listener(move |this, _, window, cx| this.set_language(language, window, cx)),
                )
        });
        let theme_label = match self.theme() {
            ThemeChoice::Dark => t!("header.theme.to_light"),
            ThemeChoice::Light => t!("header.theme.to_dark"),
        };

        h_flex()
            .px_5()
            .py_3()
            .gap_4()
            .items_center()
            .bg(p.surface)
            .border_b_1()
            .border_color(p.border)
            .child(
                div()
                    .text_lg()
                    .font_bold()
                    .text_color(p.accent)
                    .child(t!("app.title").into_owned()),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .bg(p.surface_alt)
                    .min_w_0()
                    .child(div().size(px(9.)).rounded_full().bg(dot))
                    .child(
                        div()
                            .text_sm()
                            .text_color(p.muted)
                            .truncate()
                            .child(status.into_owned()),
                    ),
            )
            .child(div().flex_1())
            .child(h_flex().gap_1().children(languages))
            .child(
                Button::new("theme")
                    .small()
                    .outline()
                    .label(theme_label.into_owned())
                    .on_click(cx.listener(|this, _, window, cx| this.toggle_theme(window, cx))),
            )
    }
}

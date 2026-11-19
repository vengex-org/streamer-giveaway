//! The demo's chat window: the simulated chat as it reaches the app, at the pace it arrives, so
//! the streamer sees what the giveaway reads.

use std::collections::VecDeque;

use gpui_kit::component::*;
use gpui_kit::*;
use rust_i18n::t;

use super::palette::{self, Palette};
use crate::domain::chat::ChatMessage;

/// Messages kept on screen; older ones scroll away. A fast chat passes this many in a second.
const KEPT: usize = 500;

pub(super) struct ChatWindow {
    messages: VecDeque<ChatMessage>,
    /// Every message since the window opened, kept or not.
    received: u64,
    palette: Palette,
    scroll: UniformListScrollHandle,
}

impl ChatWindow {
    /// Opens the window beside the main one.
    pub(super) fn open(palette: Palette, cx: &mut App) -> Result<(AnyWindowHandle, Entity<Self>)> {
        let options = WindowOptions {
            titlebar: Some(TitlebarOptions {
                title: Some(t!("chat_window.title").into_owned().into()),
                ..Default::default()
            }),
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(40.), px(40.)),
                size(px(460.), px(760.)),
            ))),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| {
            cx.new(|_| Self {
                messages: VecDeque::with_capacity(KEPT),
                received: 0,
                palette,
                scroll: UniformListScrollHandle::new(),
            })
        })
    }

    pub(super) fn push(&mut self, message: &ChatMessage, cx: &mut Context<Self>) {
        if self.messages.len() == KEPT {
            self.messages.pop_front();
        }
        self.messages.push_back(message.clone());
        self.received += 1;
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    pub(super) fn set_palette(&mut self, palette: Palette, cx: &mut Context<Self>) {
        self.palette = palette;
        cx.notify();
    }
}

impl Render for ChatWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = self.palette;
        let rows = uniform_list(
            "chat",
            self.messages.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, _| {
                let p = this.palette;
                range
                    .filter_map(|index| this.messages.get(index))
                    .map(|message| {
                        h_flex()
                            .w_full()
                            .px_3()
                            .py_1()
                            .gap_2()
                            .text_sm()
                            .child(palette::platform_logo(message.platform(), 14.))
                            .child(
                                div()
                                    .flex_none()
                                    .font_semibold()
                                    .text_color(palette::platform_color(message.platform()))
                                    .child(message.display_name().to_owned()),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .truncate()
                                    .text_color(p.text)
                                    .child(message.text().to_owned()),
                            )
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .flex_1();
        v_flex()
            .size_full()
            .bg(p.bg)
            .text_color(p.text)
            .child(
                div()
                    .px_3()
                    .py_2()
                    .border_b_1()
                    .border_color(p.border)
                    .text_sm()
                    .text_color(p.muted)
                    .child(
                        t!(
                            "chat_window.count",
                            count = self.received,
                            kept = self.messages.len()
                        )
                        .into_owned(),
                    ),
            )
            .child(rows)
    }
}

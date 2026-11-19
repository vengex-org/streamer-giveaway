use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig, ThemeMode};
use gpui_kit::*;
use serde_json::json;

use crate::domain::prefs::ThemeChoice;

/// The app's colors as `0xRRGGBB` (or `0xRRGGBBAA` for the soft tints). One violet accent for
/// actions, gold only for the winner.
#[derive(Debug, Clone, Copy)]
struct Swatches {
    bg: u32,
    surface: u32,
    surface_alt: u32,
    border: u32,
    text: u32,
    muted: u32,
    accent: u32,
    accent_hover: u32,
    accent_soft: u32,
    on_accent: u32,
    gold: u32,
    gold_soft: u32,
    success: u32,
    danger: u32,
    danger_soft: u32,
}

const DARK: Swatches = Swatches {
    bg: 0x0f0e17ff,
    surface: 0x1a1826ff,
    surface_alt: 0x232034ff,
    border: 0x312c4aff,
    text: 0xeceaf6ff,
    muted: 0x9a96b2ff,
    accent: 0x8b5cf6ff,
    accent_hover: 0x9d74f8ff,
    accent_soft: 0x8b5cf62e,
    on_accent: 0xffffffff,
    gold: 0xf5b83dff,
    gold_soft: 0xf5b83d24,
    success: 0x22c55eff,
    danger: 0xf87171ff,
    danger_soft: 0xef444424,
};

const LIGHT: Swatches = Swatches {
    bg: 0xf6f5fbff,
    surface: 0xffffffff,
    surface_alt: 0xf1eff9ff,
    border: 0xe2dff0ff,
    text: 0x1b1830ff,
    muted: 0x6b6785ff,
    accent: 0x7c3aedff,
    accent_hover: 0x6d28d9ff,
    accent_soft: 0x7c3aed1f,
    on_accent: 0xffffffff,
    gold: 0xc98a06ff,
    gold_soft: 0xf5b83d33,
    success: 0x16a34aff,
    danger: 0xdc2626ff,
    danger_soft: 0xdc26261a,
};

fn css(color: u32) -> String {
    format!("#{color:08x}")
}

/// The colors our own elements paint with; the widget library gets the same values through
/// [`Palette::apply`].
#[derive(Debug, Clone, Copy)]
pub(super) struct Palette {
    pub bg: Hsla,
    pub surface: Hsla,
    pub surface_alt: Hsla,
    pub border: Hsla,
    pub text: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub on_accent: Hsla,
    pub gold: Hsla,
    pub gold_soft: Hsla,
    pub success: Hsla,
    pub danger: Hsla,
    pub danger_soft: Hsla,
}

impl Palette {
    pub fn for_theme(theme: ThemeChoice) -> Self {
        let s = swatches(theme);
        let c = |v: u32| -> Hsla { rgba(v).into() };
        Self {
            bg: c(s.bg),
            surface: c(s.surface),
            surface_alt: c(s.surface_alt),
            border: c(s.border),
            text: c(s.text),
            muted: c(s.muted),
            accent: c(s.accent),
            accent_soft: c(s.accent_soft),
            on_accent: c(s.on_accent),
            gold: c(s.gold),
            gold_soft: c(s.gold_soft),
            success: c(s.success),
            danger: c(s.danger),
            danger_soft: c(s.danger_soft),
        }
    }

    /// Installs our colors as the widget library's theme for `theme` and switches to it, so
    /// inputs, buttons and scrollbars match the rest of the window.
    pub fn apply(theme: ThemeChoice, window: &mut Window, cx: &mut App) {
        let config = Rc::new(theme_config(theme));
        Theme::global_mut(cx).apply_config(&config);
        let mode = match theme {
            ThemeChoice::Dark => ThemeMode::Dark,
            ThemeChoice::Light => ThemeMode::Light,
        };
        Theme::change(mode, Some(window), cx);
    }
}

fn swatches(theme: ThemeChoice) -> Swatches {
    match theme {
        ThemeChoice::Dark => DARK,
        ThemeChoice::Light => LIGHT,
    }
}

fn theme_config(theme: ThemeChoice) -> ThemeConfig {
    let s = swatches(theme);
    let (name, mode) = match theme {
        ThemeChoice::Dark => ("Streamer Giveaway Dark", "dark"),
        ThemeChoice::Light => ("Streamer Giveaway Light", "light"),
    };
    let value = json!({
        "name": name,
        "mode": mode,
        "radius": 8,
        "colors": {
            "background": css(s.bg),
            "foreground": css(s.text),
            "border": css(s.border),
            "input.border": css(s.border),
            "ring": css(s.accent),
            "caret": css(s.accent),
            "selection.background": css(s.accent_soft),
            "muted.background": css(s.surface_alt),
            "muted.foreground": css(s.muted),
            "accent.background": css(s.surface_alt),
            "accent.foreground": css(s.text),
            "popover.background": css(s.surface),
            "popover.foreground": css(s.text),
            "list.background": css(s.surface),
            "list.even.background": css(s.surface_alt),
            "list.hover.background": css(s.surface_alt),
            "primary.background": css(s.accent),
            "primary.hover.background": css(s.accent_hover),
            "primary.active.background": css(s.accent_hover),
            "primary.foreground": css(s.on_accent),
            "button.primary.background": css(s.accent),
            "button.primary.hover.background": css(s.accent_hover),
            "button.primary.active.background": css(s.accent_hover),
            "button.primary.foreground": css(s.on_accent),
            "secondary.background": css(s.surface_alt),
            "secondary.hover.background": css(s.border),
            "secondary.active.background": css(s.border),
            "secondary.foreground": css(s.text),
            "danger.background": css(s.danger),
            "danger.hover.background": css(s.danger),
            "danger.active.background": css(s.danger),
            "danger.foreground": css(s.on_accent),
            "success.background": css(s.success),
            "success.foreground": css(s.on_accent),
            "scrollbar.thumb.background": css(s.border),
            "scrollbar.thumb.hover.background": css(s.muted)
        }
    });
    serde_json::from_value(value).expect("built-in theme matches the ThemeConfig schema")
}

#[cfg(test)]
mod tests {
    // Not `super::*`: that brings in gpui's own `test` attribute through `gpui_kit::*`.
    use super::{ThemeChoice, theme_config};

    #[test]
    fn built_in_themes_parse() {
        for theme in [ThemeChoice::Dark, ThemeChoice::Light] {
            let config = theme_config(theme);
            assert!(config.colors.primary.is_some());
        }
    }
}

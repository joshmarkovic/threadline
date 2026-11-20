use std::rc::Rc;

use gpui_kit::component::theme::{ActiveTheme, Theme, ThemeSet};
use gpui_kit::{App, Hsla, rgb};

#[derive(Clone, Copy)]
pub(crate) struct Palette {
    pub canvas: Hsla,
    pub panel: Hsla,
    pub surface: Hsla,
    pub border: Hsla,
    pub ink: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
    pub accent_bg: Hsla,
    pub good: Hsla,
    pub warn: Hsla,
    pub bad: Hsla,
    pub dark: bool,
}

impl Palette {
    pub(crate) fn current(cx: &App) -> Self {
        let dark = cx.theme().mode.is_dark();
        let colors = if dark {
            [
                0x181917, 0x22231f, 0x292b25, 0x41433a, 0xf2f0e7, 0xb6b6aa, 0xffa382, 0x492e26,
                0x99d3ac, 0xe2b35a, 0xef7d86,
            ]
        } else {
            [
                0xf0efe9, 0xfffefa, 0xf7f6f0, 0xdcdace, 0x252820, 0x606457, 0xa83e24, 0xf7e5da,
                0x2c6b4b, 0x8b6a1f, 0xb93442,
            ]
        };
        let [
            canvas,
            panel,
            surface,
            border,
            ink,
            muted,
            accent,
            accent_bg,
            good,
            warn,
            bad,
        ] = colors.map(|color| Hsla::from(rgb(color)));
        Self {
            canvas,
            panel,
            surface,
            border,
            ink,
            muted,
            accent,
            accent_bg,
            good,
            warn,
            bad,
            dark,
        }
    }
}

/// Gives every component the Threadline colors, in light and dark appearance.
pub(crate) fn install_theme(cx: &mut App) {
    let set: ThemeSet = serde_json::from_str(include_str!("../assets/theme.json"))
        .expect("the bundled theme is valid");
    Theme::update(cx, |theme| {
        let dark = theme.mode.is_dark();
        let mut active = None;
        for config in set.themes {
            let config = Rc::new(config);
            if config.mode.is_dark() == dark {
                active = Some(config.clone());
            }
            if config.mode.is_dark() {
                theme.dark_theme = config;
            } else {
                theme.light_theme = config;
            }
        }
        if let Some(active) = active {
            theme.apply_config(&active);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;

    #[gpui_kit::test]
    fn bundled_theme_applies_in_both_appearances(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            install_theme(cx);
            let light = Palette::current(cx);
            assert!(!light.dark);
            assert_eq!(cx.theme().primary, light.accent);
            Theme::change(gpui_kit::component::theme::ThemeMode::Dark, None, cx);
            let dark = Palette::current(cx);
            assert!(dark.dark);
            assert_eq!(cx.theme().primary, dark.accent);
        });
    }
}

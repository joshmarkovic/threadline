use gpui_kit::component::theme::ActiveTheme;
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
    pub dark: bool,
}

impl Palette {
    pub(crate) fn current(cx: &App) -> Self {
        let dark = cx.theme().mode.is_dark();
        let colors = if dark {
            [
                0x181917, 0x22231f, 0x292b25, 0x41433a, 0xf2f0e7, 0xb6b6aa, 0xffa382, 0x492e26,
                0x99d3ac,
            ]
        } else {
            [
                0xf0efe9, 0xfffefa, 0xf7f6f0, 0xdcdace, 0x252820, 0x606457, 0xa83e24, 0xf7e5da,
                0x2c6b4b,
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
            dark,
        }
    }
}

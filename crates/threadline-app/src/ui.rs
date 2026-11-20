//! Shared building blocks for navigation lists and settings pages.

use gpui_kit::base::TestTextExt;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::{
    Div, FontWeight, Hsla, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::FluentBuilder, px, rgb,
};

use crate::palette::Palette;

/// The widest a column of settings or setup content grows.
pub(crate) const CONTENT_WIDTH: f32 = 760.;

/// A full-width row in a navigation list, with its label at the leading edge.
pub(crate) fn nav(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Button {
    let id = id.into();
    let label = label.into();
    Button::new(id.clone())
        .ghost()
        .small()
        .w_full()
        .accessibility_id(id)
        .accessibility_label(label.clone())
        .child(
            div()
                .w_full()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(label.clone().test_text(label)),
        )
}

/// The small heading above a group of navigation rows.
pub(crate) fn nav_heading(title: &'static str, palette: Palette) -> Div {
    div()
        .px_2()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette.muted)
        .child(title)
}

/// A titled panel that groups related settings.
pub(crate) fn card(title: &'static str, description: &'static str, palette: Palette) -> Div {
    div()
        .w_full()
        .max_w(px(CONTENT_WIDTH))
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(palette.border)
        .bg(palette.panel)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .when(!description.is_empty(), |heading| {
                    heading.child(div().text_sm().text_color(palette.muted).child(description))
                }),
        )
}

/// One setting: what it does at the leading edge, its control at the trailing edge.
pub(crate) fn row(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    palette: Palette,
) -> Div {
    let description = description.into();
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .pt_3()
        .border_t_1()
        .border_color(palette.border)
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_sm().child(title.into()))
                .when(!description.is_empty(), |text| {
                    text.child(div().text_xs().text_color(palette.muted).child(description))
                }),
        )
        .child(div().flex_shrink_0().child(control))
}

/// A button drawn as an on/off switch. `name` is what assistive tools announce.
pub(crate) fn toggle(
    id: impl Into<SharedString>,
    name: impl Into<SharedString>,
    on: bool,
    palette: Palette,
) -> Button {
    let id = id.into();
    let track = if on {
        palette.accent
    } else {
        palette.muted.opacity(0.45)
    };
    Button::new(id.clone())
        .ghost()
        .small()
        .accessibility_id(id)
        .accessibility_label(name)
        .child(
            div()
                .w(px(34.))
                .h(px(20.))
                .p(px(2.))
                .rounded_full()
                .bg(track)
                .flex()
                .when(on, |track| track.justify_end())
                .child(div().size(px(16.)).rounded_full().bg(rgb(0xfffefa))),
        )
}

/// A short status marker, such as "Signed in".
pub(crate) fn tag(text: impl Into<SharedString>, color: Hsla) -> Div {
    div()
        .flex_shrink_0()
        .px_2()
        .rounded_full()
        .border_1()
        .border_color(color.opacity(0.45))
        .text_xs()
        .text_color(color)
        .child(text.into())
}

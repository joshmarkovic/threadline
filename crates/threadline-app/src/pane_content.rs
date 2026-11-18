use gpui_kit::base::text::{TextView, TextViewState, TextViewStyle};
use gpui_kit::component::diff::{Diff, DiffAnnotation, DiffLinePosition, DiffSide, DiffState};
use gpui_kit::component::theme::Theme;
use gpui_kit::{
    App, AppContext, Bounds, Context, Entity, FontWeight, InteractiveElement, IntoElement, Pixels,
    Render, Role, StyleRefinement, Styled, Subscription, TestSupportExt, Window, div, prelude::*,
};

use crate::fixtures::{DISCUSSION_LEAD_BLOCK, PATCH_PATH, issue_discussion};
use crate::palette::Palette;

pub(crate) struct DiscussionPane {
    text: Entity<TextViewState>,
    _subscriptions: [Subscription; 2],
}

impl DiscussionPane {
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let text = cx.new(|cx| TextViewState::markdown(&issue_discussion(), cx));
        let subscriptions = [
            cx.observe(&text, |_, _, cx| cx.notify()),
            cx.observe_global::<Theme>(|_, cx| cx.notify()),
        ];
        Self {
            text,
            _subscriptions: subscriptions,
        }
    }

    pub(crate) fn opening_bounds(&self, cx: &App) -> Option<Bounds<Pixels>> {
        let list = self.text.read(cx).list_state();
        list.bounds_for_item(DISCUSSION_LEAD_BLOCK)
            .filter(|bounds| list.viewport_bounds().contains(&bounds.center()))
    }
}

impl Render for DiscussionPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .px_5()
            .py_4()
            .child(markdown(&self.text, Palette::current(cx)).scrollable(true))
    }
}

pub(crate) struct ReviewDiff {
    diff: Entity<DiffState>,
    annotated: bool,
    _subscriptions: [Subscription; 2],
}

impl ReviewDiff {
    pub(crate) fn new(diff: Entity<DiffState>, annotated: bool, cx: &mut Context<Self>) -> Self {
        let subscriptions = [
            cx.observe(&diff, |_, _, cx| cx.notify()),
            cx.observe_global::<Theme>(|_, cx| cx.notify()),
        ];
        Self {
            diff,
            annotated,
            _subscriptions: subscriptions,
        }
    }
}

impl Render for ReviewDiff {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::current(cx);
        Diff::new(&self.diff)
            .size_full()
            .annotations(
                self.annotated
                    .then(|| {
                        DiffAnnotation::line(
                            "cache-review-note",
                            DiffLinePosition::new(PATCH_PATH, DiffSide::Modified, 3),
                        )
                    })
                    .into_iter()
                    .collect::<Vec<_>>(),
            )
            .render_annotation(move |_, _, _| {
                div()
                    .id("cache-review-note")
                    .role(Role::Comment)
                    .aria_label("Mira Chen: Keep the last confirmed revision on screen.")
                    .test_support()
                    .mx_2()
                    .my_1()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(palette.accent_bg)
                    .text_color(palette.ink)
                    .text_xs()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Mira Chen · Review note"),
                    )
                    .child("Keep the last confirmed revision on screen.")
            })
    }
}

pub(crate) fn pane_size() -> StyleRefinement {
    div().size_full().style().clone()
}

pub(crate) fn markdown(state: &Entity<TextViewState>, palette: Palette) -> TextView {
    TextView::new(state).selectable(true).style(
        TextViewStyle::default()
            .with_dark(palette.dark)
            .with_foreground(palette.ink)
            .with_muted_foreground(palette.muted)
            .with_link(palette.accent)
            .with_selection(palette.accent_bg)
            .with_code_background(palette.surface)
            .with_border(palette.border),
    )
}

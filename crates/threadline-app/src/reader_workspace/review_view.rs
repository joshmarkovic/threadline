use super::{pr::PullRequestView, view::control, *};
use crate::palette::Palette;
use gpui_kit::{
    AnyElement, InteractiveElement, IntoElement, ParentElement, Role, StatefulInteractiveElement,
    Styled,
    component::{Disableable, Selectable, input::Textarea},
    div,
};
use threadline_storage::{pr::SourceSide, reviews::ReviewEvent};

impl ReaderWorkspace {
    pub(super) fn review_view(
        &self,
        pane: usize,
        tab: &Tab,
        pr: &PullRequestView,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let item = tab.item;
        let editor = self.review_editors.get(&item);
        let owned = editor.filter(|editor| editor.tab_id == tab.id);
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_1()
            .px_2()
            .py_1()
            .flex_shrink_0();
        if owned.is_none() {
            let id = format!("pr-{pane}-review-open");
            let target = tab.clone();
            let button = control(
                id.clone(),
                if editor.is_some() {
                    "Go to review draft"
                } else {
                    "Start review"
                },
            )
            .disabled(self.review_controls_busy(item))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_review(target.clone(), None, false, window, cx);
            }));
            return panel
                .child(self.observed_control(id, button, false))
                .into_any_element();
        }

        let editor = owned.expect("the editing tab owns its draft");
        let revision = &pr.data.revision;
        let stale = editor.revision().id != revision.id;
        let mut event_controls = div().flex().flex_wrap().gap_1();
        for (key, label, event) in [
            ("comment", "Comment", ReviewEvent::Comment),
            ("approve", "Approve", ReviewEvent::Approve),
            (
                "request-changes",
                "Request changes",
                ReviewEvent::RequestChanges,
            ),
        ] {
            let id = format!("pr-{pane}-review-{key}");
            let button = control(id.clone(), label)
                .selected(editor.event == event)
                .disabled(editor.sending)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_review_event(item, event, cx);
                }));
            event_controls = event_controls.child(self.observed_control(id, button, false));
        }

        let summary_id = format!("pr-{pane}-review-summary");
        panel = panel.child(event_controls).child(
            self.observed_control(
                summary_id.clone(),
                Textarea::new(&editor.summary)
                    .accessibility_id(summary_id)
                    .aria_label("Review summary"),
                true,
            ),
        );

        let mut anchors = div().flex().flex_wrap().gap_1();
        for (index, inline) in editor.inline.iter().enumerate() {
            let id = format!("pr-{pane}-review-anchor-{index}");
            let anchor = &inline.anchor;
            let side = match anchor.side {
                SourceSide::Left => "original",
                SourceSide::Right => "modified",
            };
            let description = format!(
                "{} · {side} {}–{}{}",
                anchor.source_path,
                anchor.first_line,
                anchor.last_line,
                if stale
                    && editor.remap_revision == Some(revision.id)
                    && editor.remapped.contains_key(&inline.key)
                {
                    " · replacement selected"
                } else {
                    ""
                }
            );
            let button = control(id.clone(), description)
                .selected(editor.focused_inline == Some(index))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_review_inline(item, index, window, cx);
                }));
            anchors = anchors.child(self.observed_control(id, button, false));
        }
        panel = panel.child(anchors);
        if let Some((index, inline)) = editor
            .focused_inline
            .and_then(|index| editor.inline.get(index).map(|inline| (index, inline)))
        {
            if stale {
                panel = panel.child(div().text_xs().text_color(palette.muted).child(format!(
                    "Original context at {}\n{}",
                    inline.anchor.source_oid, inline.anchor.original_hunk,
                )));
            }
            let id = format!("pr-{pane}-review-inline");
            panel = panel.child(
                self.observed_control(
                    id.clone(),
                    Textarea::new(&inline.input)
                        .accessibility_id(id)
                        .aria_label("Inline review comment"),
                    true,
                ),
            );
            let id = format!("pr-{pane}-review-remove-inline");
            let button = control(id.clone(), "Remove inline draft")
                .disabled(editor.sending)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.remove_review_inline(item, index, cx);
                }));
            panel = panel.child(self.observed_control(id, button, false));
        }

        let mut actions = div().flex().flex_wrap().items_center().gap_2();
        if stale {
            let id = format!("pr-{pane}-review-reanchor");
            let revision = revision.clone();
            let missing_ranges = !editor.inline.is_empty()
                && (editor.remap_revision != Some(revision.id)
                    || editor
                        .inline
                        .iter()
                        .any(|inline| !editor.remapped.contains_key(&inline.key)));
            let button = control(id.clone(), "Use selected replacements")
                .disabled(missing_ranges || editor.sending || !editor.saved())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.confirm_review_reanchor(item, revision.clone(), cx);
                }));
            actions = actions
                .child(self.observed_control(id, button, false))
                .child(div().text_xs().child(format!(
                    "Draft uses {}. Current head is {}. Select replacement lines in the diff before confirming.",
                    editor.revision().head.as_str(),
                    pr.data.revision.head.as_str(),
                )));
        } else {
            let id = format!("pr-{pane}-review-submit");
            let current = revision.id;
            let button = control(id.clone(), "Queue review")
                .disabled(
                    editor.sending || !editor.saved() || editor.submitted == Some(editor.version),
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.submit_review_editor(item, current, window, cx);
                }));
            actions = actions.child(self.observed_control(id, button, false));
        }
        let progress = if editor.failed {
            "Review draft could not be saved. Edit to retry."
        } else if editor.submitted == Some(editor.version) {
            "Review queued"
        } else if editor.saved() {
            "Review draft saved on this device"
        } else {
            "Saving review draft…"
        };
        panel
            .child(
                actions.child(
                    div()
                        .id(format!("pr-{pane}-review-status"))
                        .role(Role::Status)
                        .text_xs()
                        .text_color(palette.muted)
                        .child(progress),
                ),
            )
            .into_any_element()
    }
}

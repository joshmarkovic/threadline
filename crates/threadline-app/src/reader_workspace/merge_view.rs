use super::{
    pr::PullRequestView,
    view::{control, time_label},
    *,
};
use crate::palette::Palette;
use gpui_kit::{
    AnyElement, InteractiveElement, IntoElement, ParentElement, Role, StatefulInteractiveElement,
    Styled,
    component::{Disableable, Selectable},
    div,
};
use threadline_storage::{
    merges::{MergeMethod, MergePolicy, MergeStatus, MergeSubmission, QueueRequirement},
    pr::PrRevision,
    queue::State,
    writes::Permission,
};

pub(super) struct MergeView {
    revision: PrRevision,
    method: Option<MergeMethod>,
    pub(super) busy: bool,
}

fn method_name(method: MergeMethod) -> &'static str {
    match method {
        MergeMethod::Merge => "Merge",
        MergeMethod::Squash => "Squash",
        MergeMethod::Rebase => "Rebase",
    }
}

fn available(policy: &MergePolicy) -> bool {
    policy.queue != QueueRequirement::Required
        && policy.viewer_permission != Permission::Denied
        && policy.open != Some(false)
        && policy.draft != Some(true)
        && policy.merged != Some(true)
}

fn merge_progress(status: &MergeStatus) -> &'static str {
    match status.state {
        State::Queued => "Queued locally",
        State::Validating => "Checking the original head",
        State::Sending => "Waiting for GitHub to confirm the merge",
        State::OutcomeUnknown => "Outcome unknown",
        State::Waiting => "Waiting to retry the original intent",
        State::NeedsAttention => match status.reason.as_deref() {
            Some("required_merge_queue_unsupported") => {
                "Needs attention · repository requires a merge queue"
            }
            Some("expected_merge_revision_changed") => {
                "Needs attention · pull request head changed"
            }
            _ => "Needs attention · inspect in the Outbox",
        },
        State::Confirmed => "Merge confirmed",
        State::Cancelled => "Local intent canceled",
    }
}

impl ReaderWorkspace {
    pub(super) fn merge_view(
        &self,
        pane: usize,
        tab: &Tab,
        pr: &PullRequestView,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let revision = &pr.data.revision;
        let expanded = self.merge_views.get(&tab.id);
        let active = expanded.filter(|view| &view.revision == revision);
        let id = format!("pr-{pane}-merge-open");
        let target = tab.id.clone();
        let expected = revision.clone();
        let open = control(
            id.clone(),
            if active.is_some() {
                "Hide direct merge"
            } else {
                "Direct merge"
            },
        )
        .disabled(expanded.is_some_and(|view| view.busy))
        .on_click(cx.listener(move |this, _, _, cx| {
            if this
                .merge_views
                .get(&target)
                .is_some_and(|view| view.revision == expected)
            {
                this.merge_views.remove(&target);
            } else {
                this.merge_views.insert(
                    target.clone(),
                    MergeView {
                        revision: expected.clone(),
                        method: None,
                        busy: false,
                    },
                );
            }
            cx.notify();
        }));
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(self.observed_control(id, open, false));
        if let Some(progress) = pr.data.merge_status.first() {
            panel = panel.child(
                div()
                    .id(format!("pr-{pane}-merge-state"))
                    .role(Role::Status)
                    .text_xs()
                    .text_color(palette.muted)
                    .child(format!(
                        "{} · {} · {}",
                        method_name(progress.method),
                        progress.head.as_str(),
                        merge_progress(progress)
                    )),
            );
        }
        let Some(view) = active else {
            return panel.into_any_element();
        };

        let id = format!("pr-{pane}-merge-refresh");
        let expected = revision.clone();
        let refresh = control(id.clone(), "Check merge options")
            .disabled(!self.can_fetch() || view.busy)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.refresh_merge_policy(expected.clone(), window, cx);
            }));
        panel = panel.child(self.observed_control(id, refresh, false));
        let Some(cached) = pr.data.merge_policy.as_ref() else {
            return panel
                .child("Download the current repository’s merge methods to prepare a direct merge.")
                .into_any_element();
        };
        panel = panel.child(div().text_xs().text_color(palette.muted).child(format!(
            "Options checked {}",
            time_label(Some(cached.observed_at_ms))
        )));
        if !available(&cached.policy) {
            return panel
                .child(if cached.policy.queue == QueueRequirement::Required {
                    "This repository requires a merge queue. Direct merge is unavailable."
                } else {
                    "The saved pull request state or account permissions prevent a direct merge."
                })
                .into_any_element();
        }

        let mut choices = div().flex().flex_wrap().gap_1();
        for method in [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase]
            .into_iter()
            .filter(|method| cached.policy.permits_method(*method))
        {
            let id = format!("pr-{pane}-merge-{}", method.github_name());
            let target = tab.id.clone();
            let button = control(id.clone(), method_name(method))
                .selected(view.method == Some(method))
                .disabled(view.busy)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(view) = this.merge_views.get_mut(&target).filter(|view| !view.busy)
                    {
                        view.method = Some(method);
                        cx.notify();
                    }
                }));
            choices = choices.child(self.observed_control(id, button, false));
        }
        panel = panel.child(choices);
        if let Some(method) = view
            .method
            .filter(|method| cached.policy.permits_method(*method))
        {
            let id = format!("pr-{pane}-merge-confirm");
            let target = tab.id.clone();
            let expected = revision.clone();
            let confirm = control(id.clone(), "Queue direct merge")
                .disabled(view.busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.queue_direct_merge(&target, expected.clone(), method, window, cx);
                }));
            panel = panel
                .child(div().text_sm().child(format!(
                    "{} head {}. Threadline will recheck GitHub’s merge rules before sending.",
                    method_name(method),
                    revision.head.as_str(),
                )))
                .child(self.observed_control(id, confirm, false));
        }
        panel.into_any_element()
    }

    fn refresh_merge_policy(
        &mut self,
        revision: PrRevision,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = revision.pr;
        self.remote(
            window,
            cx,
            move |service, account, cancel| async move {
                if account != revision.account {
                    return Err(crate::reader::ReaderError::ScopeChanged);
                }
                service.fetch_merge_policy(&revision, &cancel).await?;
                Ok(((), true))
            },
            move |this, result, window, cx| {
                if result.is_some() {
                    this.refresh_write_views(item, window, cx);
                }
            },
        );
    }

    fn queue_direct_merge(
        &mut self,
        tab: &str,
        expected: PrRevision,
        method: MergeMethod,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self
            .merge_views
            .get_mut(tab)
            .filter(|view| view.revision == expected && view.method == Some(method) && !view.busy)
        else {
            return;
        };
        let current = self
            .tab_views
            .get(tab)
            .and_then(|view| view.pr.as_ref())
            .map(|pr| &pr.data.revision);
        if self.account != Some(expected.account)
            || current != Some(&expected)
            || self.departure.is_some()
        {
            return;
        }
        let tab = tab.to_owned();
        let item = expected.pr;
        let revision_id = expected.id;
        view.busy = true;
        self.launch(
            window,
            cx,
            move |store| {
                store.submit_merge(
                    &expected,
                    MergeSubmission {
                        id: &store.new_operation_id()?,
                        method,
                        dependencies: &[],
                        submitted_at_ms: now_ms(),
                    },
                )
            },
            move |this, outcome, window, cx| {
                if let Some(view) = this
                    .merge_views
                    .get_mut(&tab)
                    .filter(|view| view.revision.id == revision_id)
                {
                    view.busy = false;
                    if outcome.is_ok() {
                        view.method = None;
                    }
                }
                if outcome.is_ok() {
                    this.status = Some(
                        "Direct merge queued for the original head. Check delivery in the Outbox.",
                    );
                    this.refresh_write_views(item, window, cx);
                    if let Some(worker) = this.sync_worker.as_ref() {
                        worker.wake(None);
                    }
                } else {
                    this.status = Some(
                        "Could not queue the merge. Check the current head and merge options.",
                    );
                }
                this.settle_departure(window, cx);
                cx.notify();
            },
        );
        cx.notify();
    }
}

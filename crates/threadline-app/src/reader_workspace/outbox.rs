use super::*;
use crate::palette::Palette;
use gpui_kit::{
    AnyElement, InteractiveElement, IntoElement, ParentElement, Role, StatefulInteractiveElement,
    Styled,
    component::{Disableable, Selectable},
    div,
};
use std::num::NonZeroU64;
use threadline_storage::{
    queue::{ActionKind, Attempt, CancelResult, DraftSubmissionStatus, Operation, Receipt, State},
    writes::{IssueChanges, WriteConflict, WriteIntent},
};

#[derive(Clone, Copy, Default, Eq, PartialEq)]
enum Filter {
    #[default]
    Pending,
    Attention,
    History,
    All,
}

pub(super) struct OutboxState {
    filter: Filter,
    desired: Option<String>,
    selected: Option<Inspection>,
    paused: bool,
    changing: bool,
    inspection_generation: u64,
    controls_generation: u64,
    draft_status_generation: u64,
    pub(super) queue_generation: u64,
}

impl Default for OutboxState {
    fn default() -> Self {
        Self {
            filter: Filter::Pending,
            desired: None,
            selected: None,
            paused: false,
            changing: false,
            inspection_generation: 0,
            controls_generation: 0,
            draft_status_generation: 0,
            queue_generation: 0,
        }
    }
}

#[cfg(feature = "e2e")]
impl OutboxState {
    pub(super) fn selected_state(&self) -> Option<String> {
        self.selected
            .as_ref()
            .map(|inspection| format!("{:?}", inspection.operation.state))
    }

    pub(super) fn selected_merge_queue_required(&self) -> bool {
        self.selected.as_ref().is_some_and(|inspection| {
            inspection.operation.reason.as_deref() == Some("required_merge_queue_unsupported")
        })
    }

    pub(super) fn cancel_requested(&self) -> bool {
        self.selected
            .as_ref()
            .is_some_and(|inspection| inspection.operation.cancel_requested)
    }

    pub(super) fn is_paused(&self) -> bool {
        self.paused
    }
}

struct Inspection {
    operation: Operation,
    intent: Option<WriteIntent>,
    attempts: Vec<Attempt>,
    receipts: Vec<Receipt>,
    conflict: Option<WriteConflict>,
    repository: Option<String>,
}

impl Inspection {
    fn read(store: &Store, account: AccountId, id: &str) -> Outcome<Option<Self>> {
        let Some(operation) = store.operation(account, id)? else {
            return Ok(None);
        };
        let repository = store
            .github_repository_views(account)?
            .into_iter()
            .find(|repository| repository.id == operation.original_repository)
            .map(|repository| format!("{}/{}", repository.owner, repository.name));
        Ok(Some(Self {
            operation,
            intent: store.write_intent(account, id)?,
            attempts: store.attempts(account, id)?,
            receipts: store.receipts(account, id)?,
            conflict: store.write_conflict(account, id)?,
            repository,
        }))
    }
}

impl Filter {
    fn accepts(self, row: &QueueRow) -> bool {
        match self {
            Self::Pending => !matches!(row.state, State::Confirmed | State::Cancelled),
            Self::Attention => {
                matches!(row.state, State::NeedsAttention | State::OutcomeUnknown)
                    || row.cancel_requested
                        && !matches!(row.state, State::Confirmed | State::Cancelled)
            }
            Self::History => matches!(row.state, State::Confirmed | State::Cancelled),
            Self::All => true,
        }
    }
}

#[derive(Clone, Copy)]
enum Action {
    Cancel,
    Retry,
}

struct DraftStatusLookup {
    id: String,
    lease: String,
    revision: i64,
    review_item: Option<ItemId>,
}

impl DraftStatusLookup {
    fn matches(&self, id: &str, lease: &str, revision: i64) -> bool {
        self.id == id && self.lease == lease && self.revision == revision
    }
}

impl ReaderWorkspace {
    pub(super) fn refresh_draft_submissions(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        let writes = self.drafts.values().filter_map(|editor| {
            editor.current_submission().map(|status| DraftStatusLookup {
                id: editor.id.clone(),
                lease: editor.lease.clone(),
                revision: status.revision,
                review_item: None,
            })
        });
        let reviews = self.review_editors.iter().filter_map(|(&item, editor)| {
            editor.current_submission().map(|status| DraftStatusLookup {
                id: editor.id().to_owned(),
                lease: editor.lease_token().to_owned(),
                revision: status.revision,
                review_item: Some(item),
            })
        });
        let lookups = writes.chain(reviews).collect::<Vec<_>>();
        if lookups.is_empty() {
            return;
        }
        self.outbox.draft_status_generation += 1;
        let generation = self.outbox.draft_status_generation;
        self.launch(
            window,
            cx,
            move |store| {
                lookups
                    .into_iter()
                    .map(|lookup| {
                        let status =
                            store.draft_submission_status(account, &lookup.id, lookup.revision)?;
                        Ok((lookup, status))
                    })
                    .collect::<Outcome<Vec<_>>>()
            },
            move |this, result, _, _| {
                if this.outbox.draft_status_generation != generation {
                    return;
                }
                let Ok(statuses) = result else { return };
                for (lookup, status) in statuses {
                    if let Some(item) = lookup.review_item {
                        if let Some(editor) = this.review_editors.get_mut(&item).filter(|editor| {
                            lookup.matches(editor.id(), editor.lease_token(), editor.version)
                        }) {
                            editor.submitted = status;
                        }
                    } else if let Some(editor) = this.drafts.get_mut(&lookup.id).filter(|editor| {
                        lookup.matches(&editor.id, &editor.lease, editor.durable_revision)
                    }) {
                        editor.submitted = status;
                    }
                }
            },
        );
    }

    pub(super) fn refresh_sidebar_counts(&mut self, window: &Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        let scope = self.scope;
        self.launch(
            window,
            cx,
            move |store| store.sidebar_counts(account, scope),
            move |this, result, _, _| {
                if this.scope == scope
                    && let Ok(counts) = result
                {
                    this.counts = Some(counts);
                }
            },
        );
    }

    pub(super) fn refresh_outbox_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        self.outbox.controls_generation += 1;
        let generation = self.outbox.controls_generation;
        self.launch(
            window,
            cx,
            move |store| store.sync_settings(account),
            move |this, result, _, _| {
                if this.outbox.controls_generation == generation
                    && let Ok(settings) = result
                {
                    this.outbox.paused = settings.controls.pause_queued_actions;
                }
            },
        );
        if let Some(id) = self.outbox.desired.clone() {
            self.inspect_outbox(id, window, cx);
        }
    }

    fn inspect_outbox(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        if self.outbox.desired.as_ref() != Some(&id) {
            self.outbox.selected = None;
        }
        self.outbox.desired = Some(id.clone());
        self.outbox.inspection_generation += 1;
        let generation = self.outbox.inspection_generation;
        let selected = id.clone();
        self.launch(
            window,
            cx,
            move |store| Inspection::read(store, account, &id),
            move |this, result, _, _| {
                if this.outbox.inspection_generation != generation
                    || this.outbox.desired.as_ref() != Some(&selected)
                {
                    return;
                }
                match result {
                    Ok(value) => this.outbox.selected = value,
                    Err(_) => this.status = Some("The saved Outbox action could not be read."),
                }
            },
        );
    }

    fn set_outbox_paused(&mut self, paused: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        if self.outbox.changing {
            return;
        }
        self.outbox.controls_generation += 1;
        self.surface_focus.focus(window, cx);
        self.outbox.changing = true;
        self.launch(
            window,
            cx,
            move |store| {
                store.update_sync_settings(account, |settings| {
                    settings.controls.pause_queued_actions = paused
                })?;
                if !paused {
                    store.resume_writes(account, now_ms())?;
                }
                Ok(())
            },
            move |this, result, window, cx| {
                this.outbox.changing = false;
                if result.is_ok() {
                    this.outbox.paused = paused;
                    this.status = Some(if paused {
                        "Outbox dispatch paused."
                    } else {
                        "Outbox dispatch resumed."
                    });
                    if let Some(worker) = this.sync_worker.as_ref() {
                        worker.wake(None);
                    }
                } else {
                    this.status = Some("The Outbox setting could not be saved.");
                }
                this.refresh_outbox_state(window, cx);
                this.refresh_sidebar_counts(window, cx);
            },
        );
    }

    fn outbox_command(
        &mut self,
        operation: &Operation,
        action: Action,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account.filter(|account| *account == operation.account) else {
            return;
        };
        if self.outbox.changing {
            return;
        }
        self.surface_focus.focus(window, cx);
        self.outbox.changing = true;
        let id = operation.id.clone();
        let item = operation.item;
        self.launch(window,cx,move |store| {
            Ok(match action {
                Action::Cancel=>match store.cancel_operation(account,&id)? {
                    CancelResult::Cancelled=>"Action cancelled locally. Its text is saved in History.",
                    CancelResult::MayAlreadyBeSent=>"Cancellation requested. GitHub may already have received this action.",
                    CancelResult::AlreadyConfirmed=>"GitHub already confirmed this action.",
                },
                Action::Retry if store.retry_write_now(account,&id,now_ms())?=>"The definitely unsent action is eligible for retry when online and authorized.",
                Action::Retry=>"This action cannot safely be retried now.",
            })
        },move |this,result,window,cx| {
            this.outbox.changing=false;
            this.status=Some(result.unwrap_or("The saved action could not be updated."));
            this.refresh_write_views(item,window,cx);
            if let Some(worker)=this.sync_worker.as_ref() {worker.wake(None);}
        });
    }

    pub(super) fn outbox_view(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut filters = div().flex().flex_wrap().gap_1();
        for (key, title, filter) in [
            ("pending", "Pending", Filter::Pending),
            ("attention", "Attention", Filter::Attention),
            ("history", "History", Filter::History),
            ("all", "All", Filter::All),
        ] {
            let id = format!("reader-outbox-filter-{key}");
            let button = super::view::control(id.clone(), title)
                .selected(self.outbox.filter == filter)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.outbox.filter = filter;
                    cx.notify();
                }));
            filters = filters.child(self.observed_control(id, button, false));
        }
        let pause = !self.outbox.paused;
        let pause_button = super::view::control(
            "reader-outbox-pause",
            if pause {
                "Pause Outbox"
            } else {
                "Resume Outbox"
            },
        )
        .disabled(self.outbox.changing)
        .on_click(
            cx.listener(move |this, _, window, cx| this.set_outbox_paused(pause, window, cx)),
        );
        let mut view = div()
            .id("reader-outbox-list")
            .role(Role::Group)
            .aria_label("Submitted Outbox actions")
            .overflow_y_scroll()
            .min_h_0()
            .flex_1()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().text_lg().child("Outbox"))
            .child(
                div()
                    .text_sm()
                    .text_color(palette.muted)
                    .child(self.counts.map_or_else(
                        || "Pending count unavailable".into(),
                        |counts| format!("{} unresolved submitted actions", counts.outbox),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(filters)
                    .child(self.observed_control("reader-outbox-pause", pause_button, false)),
            );
        if let Some(inspection) = &self.outbox.selected {
            view = view.child(self.outbox_inspection(inspection, palette, cx));
        }
        let matching = self
            .queue
            .iter()
            .enumerate()
            .filter(|(_, row)| self.outbox.filter.accepts(row));
        for (index, row) in matching {
            let id = format!("reader-outbox-action-{index}");
            let saved = row.id.clone();
            let button = super::view::control(
                id.clone(),
                format!(
                    "{} · {}",
                    action_label(row.kind),
                    state_label(row.state, row.cancel_requested)
                ),
            )
            .selected(self.outbox.desired.as_ref() == Some(&saved))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.inspect_outbox(saved.clone(), window, cx)
            }));
            view = view.child(self.observed_control(id, button, false));
        }
        if !self.queue.iter().any(|row| self.outbox.filter.accepts(row)) {
            view = view.child(
                div()
                    .text_sm()
                    .text_color(palette.muted)
                    .child("This page has no actions matching the filter."),
            );
        }
        if self.queue.len() >= usize::from(PAGE) {
            view = view.child(
                super::view::control("reader-more-outbox", "More saved actions").on_click(
                    cx.listener(|this, _, window, cx| this.show_outbox(true, window, cx)),
                ),
            );
        }
        view.into_any_element()
    }

    fn outbox_inspection(
        &self,
        detail: &Inspection,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let operation = &detail.operation;
        let route = detail
            .repository
            .as_deref()
            .unwrap_or("Original repository");
        let target = operation.item;
        let retry = operation.clone();
        let cancel = operation.clone();
        let title = format!(
            "{} · {route} #{}",
            action_label(operation.kind),
            operation.original_number
        );
        let mut actions = div()
            .flex()
            .flex_wrap()
            .gap_2()
            .child(self.observed_control(
                "reader-outbox-open",
                super::view::control("reader-outbox-open", "Open item").on_click(cx.listener(
                    move |this, _, window, cx| this.open(target, None, false, false, window, cx),
                )),
                false,
            ));
        if !matches!(operation.state, State::Confirmed | State::Cancelled)
            && !operation.cancel_requested
        {
            actions = actions.child(
                self.observed_control(
                    "reader-outbox-cancel",
                    super::view::control("reader-outbox-cancel", "Cancel action")
                        .disabled(self.outbox.changing)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.outbox_command(&cancel, Action::Cancel, window, cx)
                        })),
                    false,
                ),
            );
        }
        if operation.state == State::Queued
            && detail.intent.is_some()
            && !operation.cancel_requested
        {
            actions = actions.child(
                self.observed_control(
                    "reader-outbox-retry",
                    super::view::control("reader-outbox-retry", "Retry now")
                        .disabled(self.outbox.changing)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.outbox_command(&retry, Action::Retry, window, cx)
                        })),
                    false,
                ),
            );
        }
        let mut panel = div()
            .id("reader-outbox-inspection")
            .role(Role::Group)
            .aria_label("Saved action details")
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .border_1()
            .border_color(palette.border)
            .child(div().text_sm().child(title))
            .child(
                div()
                    .text_sm()
                    .child(state_label(operation.state, operation.cancel_requested)),
            )
            .children(operation.reason.as_deref().map(|reason| {
                div()
                    .text_xs()
                    .text_color(palette.muted)
                    .child(reason_label(reason))
            }))
            .children(operation.draft.as_ref().map(|(_, revision)| {
                div()
                    .text_xs()
                    .text_color(palette.muted)
                    .child(format!("Submitted draft revision {revision}"))
            }))
            .children((!operation.dependencies.is_empty()).then(|| {
                div()
                    .text_sm()
                    .child(format!("Depends on {}", operation.dependencies.join(", ")))
            }))
            .children(detail.intent.as_ref().map(|intent| {
                let preview = intent_preview(intent);
                div()
                    .id("reader-outbox-intent")
                    .role(Role::Group)
                    .aria_label(preview.replace('\n', ". "))
                    .text_sm()
                    .child(preview)
            }))
            .children(
                detail
                    .conflict
                    .as_ref()
                    .map(|conflict| conflict_view(conflict, palette)),
            )
            .child(actions);
        for attempt in &detail.attempts {
            panel = panel.child(div().text_xs().text_color(palette.muted).child(format!(
                    "Attempt {} · claimed {} · sent {} · {}",
                    attempt.number,
                    super::view::time_label(Some(attempt.claimed_at)),
                    super::view::time_label(attempt.sending_at),
                    attempt
                        .outcome
                        .as_deref()
                        .map_or_else(|| "in progress".into(), reason_label)
                )));
        }
        for receipt in &detail.receipts {
            panel = panel.child(div().text_xs().text_color(palette.muted).child(format!(
                "Confirmed {} {}",
                receipt.object_kind, receipt.remote_id
            )));
        }
        panel.into_any_element()
    }

    pub(super) fn pending_write_projection(
        &self,
        pane: usize,
        index: usize,
        tab: &Tab,
        projection: &Projection,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut row = div().p_2().border_1().border_color(palette.accent).child(
            div().text_xs().text_color(palette.accent).child(format!(
                "Pending local action · {}",
                state_label(projection.state, false)
            )),
        );
        match serde_json::from_str::<WriteIntent>(&projection.value) {
            Ok(intent) => {
                row = row.child(div().text_sm().child(intent_preview(&intent)));
                if let WriteIntent::CreateComment { body } = intent
                    && matches!(
                        projection.state,
                        State::Queued | State::Validating | State::Sending | State::Waiting
                    )
                {
                    let tab = tab.clone();
                    let predecessor = projection.operation_id.clone();
                    let id = format!("reader-edit-pending-comment-{pane}-{index}");
                    row = row.child(self.observed_control(
                        id.clone(),
                        super::view::control(id, "Draft a follow-up edit").on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.edit_pending_comment(
                                    tab.clone(),
                                    predecessor.clone(),
                                    body.clone(),
                                    window,
                                    cx,
                                )
                            },
                        )),
                        false,
                    ));
                }
            }
            Err(_) => row = row.child(div().text_sm().child(projection.value.clone())),
        }
        row.into_any_element()
    }
}

pub(super) fn editable_comments(
    store: &Store,
    account: AccountId,
    item: ItemId,
    actor: Option<NonZeroU64>,
    page: &DiscussionPage,
) -> Outcome<HashMap<String, NonZeroU64>> {
    let Some(actor) = actor else {
        return Ok(HashMap::new());
    };
    let mut result = HashMap::new();
    for metadata in &page.metadata {
        if let Ok(comment) = metadata.key.parse::<NonZeroU64>()
            && store.comment_author_id(account, item, comment)? == Some(actor)
        {
            result.insert(metadata.key.clone(), comment);
        }
    }
    Ok(result)
}

fn action_label(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::EditIssue => "Issue edit",
        ActionKind::Comment => "Comment",
        ActionKind::Review => "Review",
        ActionKind::Reply => "Reply",
        ActionKind::ResolveThread => "Resolve thread",
        ActionKind::Merge => "Merge pull request",
    }
}

pub(super) fn state_label(state: State, cancelled: bool) -> &'static str {
    match state {
        State::Confirmed => "Confirmed",
        State::Cancelled => "Cancelled",
        _ if cancelled => "Cancellation requested",
        State::Queued => "Queued",
        State::Validating => "Validating target",
        State::Sending => "Sending",
        State::OutcomeUnknown => "Outcome unknown",
        State::Waiting => "Waiting for confirmation",
        State::NeedsAttention => "Needs attention",
    }
}

pub(super) fn draft_submission_label(status: DraftSubmissionStatus) -> &'static str {
    match (status.state, status.cancel_requested) {
        (State::Queued, false) => "Saved to Outbox",
        (State::OutcomeUnknown, true) => "Outcome unknown · cancellation requested",
        (state, cancelled) => state_label(state, cancelled),
    }
}

fn reason_label(value: &str) -> String {
    match value {
        "remote_edit_conflict" | "changed_fields" | "issue_fields_conflict" => {
            "The GitHub issue or comment changed before this edit was sent.".into()
        }
        "mutation_access_rejected" | "write_permission_unavailable" => {
            "GitHub write permission is unavailable.".into()
        }
        "retry_burst_exhausted" => {
            "Automatic retries paused after repeated connection failures.".into()
        }
        "required_merge_queue_unsupported" => {
            "GitHub requires a merge queue for this branch. Direct merge is unavailable.".into()
        }
        _ => value.replace('_', " "),
    }
}

fn intent_preview(intent: &WriteIntent) -> String {
    match intent {
        WriteIntent::CreateComment { body } => format!("New comment\n{body}"),
        WriteIntent::EditComment { body, .. } => format!("Comment edit\n{}", body.wanted),
        WriteIntent::EditIssue(fields) => issue_preview(fields),
    }
}

fn issue_preview(fields: &IssueChanges) -> String {
    let mut lines = Vec::new();
    if let Some(value) = &fields.title {
        lines.push(format!("Title · {}", value.wanted));
    }
    if let Some(value) = &fields.body {
        lines.push(format!(
            "Description · {}",
            value.wanted.as_deref().unwrap_or("Clear description")
        ));
    }
    if let Some(value) = &fields.labels {
        lines.push(format!(
            "Labels · {}",
            value.wanted.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if let Some(value) = &fields.assignees {
        lines.push(format!(
            "Assignees · {}",
            value.wanted.iter().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    if let Some(value) = &fields.milestone {
        lines.push(value.wanted.map_or_else(
            || "Clear milestone".to_owned(),
            |number| format!("Milestone #{number}"),
        ));
    }
    if let Some(value) = &fields.closed {
        lines.push(if value.wanted {
            "Close issue".into()
        } else {
            "Reopen issue".into()
        });
    }
    lines.join("\n")
}

fn conflict_view(conflict: &WriteConflict, palette: Palette) -> AnyElement {
    let mut view = div()
        .p_2()
        .border_1()
        .border_color(palette.accent)
        .text_sm()
        .child(reason_label(&conflict.reason));
    if let Some(current) = &conflict.current {
        match &conflict.intended {
            WriteIntent::EditComment { body, .. } => {
                if let Some(comment) = &current.comment {
                    view = view
                        .child(format!("Original comment\n{}", body.base))
                        .child(format!("Current GitHub comment\n{}", comment.body));
                }
            }
            WriteIntent::EditIssue(fields) => {
                if let Some(edit) = &fields.title {
                    view = view.child(conflict_value(
                        "reader-conflict-original-title",
                        "Original title",
                        &edit.base,
                    ));
                }
                view = view.child(conflict_value(
                    "reader-conflict-remote-title",
                    "Current GitHub title",
                    current.fields.title.as_deref().unwrap_or("Unavailable"),
                ));
                if let Some(edit) = &fields.body {
                    view = view.child(conflict_value(
                        "reader-conflict-original-description",
                        "Original description",
                        edit.base.as_deref().unwrap_or("No description"),
                    ));
                }
                let observed = current
                    .fields
                    .body
                    .as_ref()
                    .map(|body| body.as_deref().unwrap_or("No description"))
                    .unwrap_or("Unavailable");
                view = view.child(conflict_value(
                    "reader-conflict-remote-description",
                    "Current GitHub description",
                    observed,
                ));
            }
            WriteIntent::CreateComment { .. } => {}
        }
    }
    view.into_any_element()
}

fn conflict_value(id: &'static str, field: &str, value: &str) -> AnyElement {
    let presentation = format!("{field} · {value}");
    div()
        .id(id)
        .role(Role::Group)
        .aria_label(presentation.clone())
        .child(presentation)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::DraftStatusLookup;

    #[test]
    fn submitted_status_from_a_closed_editor_cannot_replace_a_reopened_editor() {
        let delayed = DraftStatusLookup {
            id: "review-1".into(),
            lease: "old-editor".into(),
            revision: 6,
            review_item: None,
        };
        assert!(delayed.matches("review-1", "old-editor", 6));
        assert!(!delayed.matches("review-1", "reopened-editor", 6));
        assert!(!delayed.matches("review-1", "old-editor", 7));
        assert!(!delayed.matches("review-2", "old-editor", 6));
    }
}

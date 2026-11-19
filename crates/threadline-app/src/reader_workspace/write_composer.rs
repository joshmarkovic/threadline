use super::*;
use crate::palette::Palette;
use gpui_kit::{
    AnyElement, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    TestSupportExt,
    component::{Disableable, Selectable, input::Input},
    div,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, num::NonZeroU64};
use threadline_storage::{
    queue::{DraftContent, DraftSubmission},
    writes::{
        CachedRepositoryChoices, CommentTarget, FieldEdit, IssueChanges, IssueValues, WriteIntent,
        WriteSubmission,
    },
};

const DRAFT_FORMAT: &str = "threadline-existing-item-edit-v1:";

#[derive(Clone, Serialize, Deserialize)]
pub(super) enum WriteBase {
    Issue {
        fields: Box<IssueValues>,
        provider_revision: String,
    },
    Comment {
        target: CommentTarget,
        body: String,
    },
    ReviewReply {
        revision_id: i64,
        thread_id: String,
        root_comment_id: NonZeroU64,
    },
}

#[derive(Clone, Default, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct IssueOptions {
    title: Option<String>,
    labels: Option<BTreeSet<String>>,
    assignees: Option<BTreeSet<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "optional_milestone"
    )]
    milestone: Option<Option<NonZeroU64>>,
    closed: Option<bool>,
}

pub(super) struct IssueEditor {
    pub(super) title: Option<Entity<InputState>>,
    options: IssueOptions,
}

impl WriteBase {
    pub(super) fn encode(&self) -> Outcome<String> {
        Ok(format!(
            "{DRAFT_FORMAT}{}",
            serde_json::to_string(self).map_err(|_| threadline_storage::Error::Corrupt)?
        ))
    }

    pub(super) fn decode(content: &DraftContent, issue: bool) -> Option<Self> {
        if let Some(encoded) = content.base.strip_prefix(DRAFT_FORMAT) {
            return serde_json::from_str(encoded).ok();
        }
        if issue && !content.anchor.is_empty() {
            return Some(Self::Issue {
                fields: Box::new(IssueValues {
                    body: Some(Some(content.anchor.clone())),
                    ..Default::default()
                }),
                provider_revision: content.base.clone(),
            });
        }
        None
    }
}

impl IssueEditor {
    pub(super) fn new(
        base: &IssueValues,
        stored: &str,
        window: &mut Window,
        cx: &mut Context<ReaderWorkspace>,
    ) -> Self {
        let options = serde_json::from_str::<IssueOptions>(stored).unwrap_or_default();
        let title = base.title.as_ref().map(|base| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(options.title.as_ref().unwrap_or(base).clone())
            })
        });
        Self { title, options }
    }

    pub(super) fn stored_options(&self, cx: &gpui_kit::App) -> String {
        serde_json::to_string(&self.desired(cx)).expect("issue draft fields serialize")
    }

    fn desired(&self, cx: &gpui_kit::App) -> IssueOptions {
        let mut options = self.options.clone();
        options.title = self
            .title
            .as_ref()
            .map(|title| title.read(cx).value().to_string());
        options
    }
}

fn changed<T: PartialEq + Clone>(base: Option<&T>, wanted: Option<&T>) -> Option<FieldEdit<T>> {
    let (base, wanted) = (base?, wanted?);
    (base != wanted).then(|| FieldEdit {
        base: base.clone(),
        wanted: wanted.clone(),
    })
}

fn body_for_write<'a>(base: &IssueValues, body: &'a str) -> Option<&'a str> {
    if body.is_empty() && !matches!(&base.body, Some(Some(original)) if original.is_empty()) {
        None
    } else {
        Some(body)
    }
}

fn differs<T: PartialEq>(wanted: &Option<T>, base: &Option<T>) -> bool {
    wanted
        .as_ref()
        .zip(base.as_ref())
        .is_some_and(|(wanted, base)| wanted != base)
}

impl drafts::Editor {
    fn has_write_changes(&self, cx: &gpui_kit::App) -> bool {
        match self.write_base.as_ref() {
            Some(WriteBase::Issue { fields, .. }) => {
                let requested = self
                    .issue_choices
                    .as_ref()
                    .map(|option| option.desired(cx))
                    .unwrap_or_default();
                let title = requested
                    .title
                    .as_ref()
                    .zip(fields.title.as_ref())
                    .is_some_and(|(wanted, base)| wanted != base && !wanted.trim().is_empty());
                let body = fields
                    .body
                    .as_ref()
                    .is_some_and(|base| base.as_deref() != body_for_write(fields, &self.body));
                title
                    || body
                    || differs(&requested.labels, &fields.labels)
                    || differs(&requested.assignees, &fields.assignees)
                    || differs(&requested.milestone, &fields.milestone)
                    || differs(&requested.closed, &fields.closed)
            }
            Some(WriteBase::Comment { body, .. }) => {
                &self.body != body && !self.body.trim().is_empty()
            }
            Some(WriteBase::ReviewReply { .. }) => false,
            None => !self.issue_draft && !self.comment_edit && !self.body.trim().is_empty(),
        }
    }

    fn write_intent(&self, cx: &gpui_kit::App) -> Option<WriteIntent> {
        let intent = match self.write_base.as_ref() {
            Some(WriteBase::Issue { fields, .. }) => {
                let requested = self
                    .issue_choices
                    .as_ref()
                    .map(|option| option.desired(cx))
                    .unwrap_or_default();
                let body = body_for_write(fields, &self.body).map(str::to_owned);
                WriteIntent::EditIssue(IssueChanges {
                    title: changed(fields.title.as_ref(), requested.title.as_ref()),
                    body: changed(fields.body.as_ref(), Some(&body)),
                    labels: changed(fields.labels.as_ref(), requested.labels.as_ref()),
                    assignees: changed(fields.assignees.as_ref(), requested.assignees.as_ref()),
                    milestone: changed(fields.milestone.as_ref(), requested.milestone.as_ref()),
                    closed: changed(fields.closed.as_ref(), requested.closed.as_ref()),
                })
            }
            Some(WriteBase::Comment { target, body }) => WriteIntent::EditComment {
                target: target.clone(),
                body: changed(Some(body), Some(&self.body))?,
            },
            Some(WriteBase::ReviewReply { .. }) => return None,
            None if !self.issue_draft && !self.comment_edit => WriteIntent::CreateComment {
                body: self.body.clone(),
            },
            None => return None,
        };
        intent.validate().ok().map(|_| intent)
    }
}

#[derive(Clone)]
enum OptionChange {
    Label(String),
    Assignee(String),
    Milestone(Option<NonZeroU64>),
    Closed(bool),
}

impl ReaderWorkspace {
    fn change_write_option(&mut self, id: &str, change: OptionChange, cx: &mut Context<Self>) {
        let Some(editor) = self.drafts.get_mut(id) else {
            return;
        };
        let (Some(WriteBase::Issue { fields, .. }), Some(form)) =
            (editor.write_base.as_ref(), editor.issue_choices.as_mut())
        else {
            return;
        };
        match change {
            OptionChange::Label(name) => {
                if let Some(base) = fields.labels.as_ref() {
                    toggle(
                        form.options.labels.get_or_insert_with(|| base.clone()),
                        name,
                    );
                }
            }
            OptionChange::Assignee(name) => {
                if let Some(base) = fields.assignees.as_ref() {
                    toggle(
                        form.options.assignees.get_or_insert_with(|| base.clone()),
                        name,
                    );
                }
            }
            OptionChange::Milestone(value) => form.options.milestone = Some(value),
            OptionChange::Closed(value) => form.options.closed = Some(value),
        }
        editor.capture(cx);
        cx.notify();
    }

    pub(super) fn submit_editor(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.drafts.get_mut(&id) else {
            return;
        };
        if editor.submitting {
            return;
        }
        editor.capture(cx);
        if !editor.saved() {
            self.status = Some("The local draft is still saving.");
            cx.notify();
            return;
        }
        let Some(intent) = editor.write_intent(cx) else {
            self.status = Some("Enter a valid change based on the downloaded issue or comment.");
            cx.notify();
            return;
        };
        let (account, item, version, lease) = (
            editor.account,
            editor.item,
            editor.durable_revision,
            editor.lease.clone(),
        );
        if editor.current_submission().is_some() {
            return;
        }
        editor.submitting = true;
        let draft_id = id.clone();
        let owned = lease.clone();
        self.launch(
            window,
            cx,
            move |store| {
                let request_id = store.new_operation_id()?;
                let now = now_ms();
                store.submit_write(
                    account,
                    WriteSubmission {
                        id: &request_id,
                        item,
                        intent: &intent,
                        draft: Some(DraftSubmission {
                            id: &draft_id,
                            revision: version,
                            editor: &owned,
                            now,
                        }),
                        dependencies: &[],
                        submitted_at_ms: now,
                    },
                )
            },
            move |this, result, window, cx| {
                if let Some(editor) = this
                    .drafts
                    .get_mut(&id)
                    .filter(|editor| editor.account == account && editor.lease == lease)
                {
                    editor.submitting = false;
                    if result.is_ok() {
                        editor.submitted = Some(
                            threadline_storage::queue::DraftSubmissionStatus::queued(version),
                        );
                    }
                }
                match result {
                    Ok(_) => {
                        this.status = Some("Saved to the Outbox.");
                        this.refresh_write_views(item, window, cx);
                        if let Some(worker) = this.sync_worker.as_ref() {
                            worker.wake(None);
                        }
                    }
                    Err(_) => {
                        this.status =
                            Some("The action could not be queued. Your local draft is still saved.")
                    }
                }
                this.settle_departure(window, cx);
            },
        );
        cx.notify();
    }

    pub(super) fn refresh_write_views(
        &mut self,
        item: ItemId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_draft_submissions(window, cx);
        let tabs = self
            .layout
            .panes
            .iter()
            .filter_map(|pane| pane.active.and_then(|index| pane.tabs.get(index)))
            .filter(|tab| tab.item == item)
            .cloned()
            .collect::<Vec<_>>();
        for tab in tabs {
            self.load_tab(tab, window, cx);
        }
        if self.surface == Surface::Outbox {
            self.show_outbox(false, window, cx);
        }
        self.refresh_sidebar_counts(window, cx);
    }

    pub(super) fn fetch_editing_options(
        &mut self,
        tab: Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = tab.item;
        self.remote(
            window,
            cx,
            move |service, account, cancel| async move {
                service.fetch_write_choices(account, item, &cancel).await?;
                Ok(((), true))
            },
            move |this, result, window, cx| {
                if result.is_some() {
                    this.status =
                        Some("Downloaded the repository's available issue editing choices.");
                    this.refresh_write_views(item, window, cx);
                }
            },
        );
    }

    pub(super) fn issue_option_controls(
        &self,
        pane: usize,
        editor: &drafts::Editor,
        choices: Option<&CachedRepositoryChoices>,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (Some(WriteBase::Issue { fields, .. }), Some(form)) =
            (editor.write_base.as_ref(), editor.issue_choices.as_ref())
        else {
            return div().into_any_element();
        };
        let list = div().flex().flex_col().gap_1();
        let list = list.children(form.title.as_ref().map(|title| {
            let id = format!("reader-issue-title-{pane}");
            self.observed_control(
                id.clone(),
                div().id(id.clone()).test_support().child(
                    Input::new(title)
                        .accessibility_id(id)
                        .aria_label("Issue title"),
                ),
                true,
            )
        }));
        let mut options = div()
            .id(format!("reader-issue-options-{pane}"))
            .max_h(px(112.))
            .overflow_y_scroll()
            .flex()
            .flex_wrap()
            .gap_1();
        if let Some(base) = fields.closed {
            let closed = form.options.closed.unwrap_or(base);
            options = options.child(self.write_option(
                pane,
                "issue-state",
                if closed {
                    "Reopen issue"
                } else {
                    "Close issue"
                },
                editor,
                OptionChange::Closed(!closed),
                false,
                cx,
            ));
        }
        if let (Some(base), Some(available)) = (
            fields.labels.as_ref(),
            choices.and_then(|choices| choices.labels.as_ref()),
        ) {
            let current = form.options.labels.as_ref().unwrap_or(base);
            for (index, name) in available.values.iter().enumerate() {
                options = options.child(self.write_option(
                    pane,
                    &format!("label-{index}"),
                    name.clone(),
                    editor,
                    OptionChange::Label(name.clone()),
                    current.contains(name),
                    cx,
                ));
            }
        }
        if let (Some(base), Some(available)) = (
            fields.assignees.as_ref(),
            choices.and_then(|choices| choices.assignees.as_ref()),
        ) {
            let current = form.options.assignees.as_ref().unwrap_or(base);
            for (index, name) in available.values.iter().enumerate() {
                options = options.child(self.write_option(
                    pane,
                    &format!("assignee-{index}"),
                    format!("Assign {name}"),
                    editor,
                    OptionChange::Assignee(name.clone()),
                    current.contains(name),
                    cx,
                ));
            }
        }
        if let (Some(base), Some(available)) = (
            fields.milestone,
            choices.and_then(|choices| choices.milestones.as_ref()),
        ) {
            let current = form.options.milestone.unwrap_or(base);
            options = options.child(self.write_option(
                pane,
                "milestone-clear",
                "Clear milestone",
                editor,
                OptionChange::Milestone(None),
                current.is_none(),
                cx,
            ));
            for (index, choice) in available.values.iter().enumerate() {
                options = options.child(self.write_option(
                    pane,
                    &format!("milestone-{index}"),
                    format!("Milestone {}", choice.title),
                    editor,
                    OptionChange::Milestone(Some(choice.number)),
                    current == Some(choice.number),
                    cx,
                ));
            }
        }
        let coverage = choices.map_or_else(
            || "Repository choices have not been downloaded".to_owned(),
            |choices| {
                let observed = [
                    choices.labels.as_ref().map(|list| list.complete),
                    choices.assignees.as_ref().map(|list| list.complete),
                    choices.milestones.as_ref().map(|list| list.complete),
                ];
                format!(
                    "Repository choices · labels {} · assignees {} · milestones {}",
                    choice_coverage(observed[0]),
                    choice_coverage(observed[1]),
                    choice_coverage(observed[2])
                )
            },
        );
        list.child(options)
            .child(div().text_xs().text_color(palette.muted).child(coverage))
            .into_any_element()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The native control has exact pane identity, edit choice and selection state"
    )]
    fn write_option(
        &self,
        pane: usize,
        suffix: &str,
        label: impl Into<gpui_kit::SharedString>,
        editor: &drafts::Editor,
        choice: OptionChange,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = format!("reader-issue-{suffix}-{pane}");
        let editor = editor.id.clone();
        let button = super::view::control(id.clone(), label)
            .selected(selected)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.change_write_option(&editor, choice.clone(), cx);
            }));
        self.observed_control(id, button, false)
    }

    pub(super) fn submit_control(
        &self,
        pane: usize,
        editor: &drafts::Editor,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = format!("reader-submit-{pane}");
        let editor_id = editor.id.clone();
        let submitted = editor.current_submission();
        let action = if editor.issue_draft {
            "Queue issue edit"
        } else if editor.comment_edit {
            "Queue comment edit"
        } else {
            "Queue comment"
        };
        self.observed_control(
            id.clone(),
            super::view::control(
                id,
                submitted.map_or(action, super::outbox::draft_submission_label),
            )
            .disabled(
                editor.submitting
                    || submitted.is_some()
                    || !editor.saved()
                    || !editor.has_write_changes(cx),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.submit_editor(editor_id.clone(), window, cx)
            })),
            false,
        )
    }
}

fn toggle(set: &mut BTreeSet<String>, item: String) {
    if !set.remove(&item) {
        set.insert(item);
    }
}

fn optional_milestone<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Option<NonZeroU64>>, D::Error> {
    Option::<NonZeroU64>::deserialize(deserializer).map(Some)
}

fn choice_coverage(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "available",
        Some(false) => "partial",
        None => "unknown",
    }
}

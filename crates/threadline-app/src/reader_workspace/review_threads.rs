use std::num::NonZeroU64;

use super::{view::control, write_composer::WriteBase, *};
use gpui_kit::{
    AnyElement, IntoElement, ParentElement, Styled,
    component::{Disableable, input::Textarea},
    div,
};
use threadline_storage::{
    pr::{CodeViewportAnchor, PrRecord, PrRevision},
    queue::DraftSubmission,
    reviews::{ReviewIntent, ReviewSubmission},
};

pub(super) struct Thread<'a> {
    id: &'a str,
    root_comment: Option<NonZeroU64>,
    resolved: Option<bool>,
}

impl<'a> Thread<'a> {
    pub(super) fn from_record(record: &'a PrRecord) -> Option<Self> {
        let PrRecord::Thread {
            node_id,
            comments,
            resolved,
            ..
        } = record
        else {
            return None;
        };
        Some(Self {
            id: node_id,
            root_comment: comments
                .iter()
                .find(|comment| comment.in_reply_to.is_none())
                .and_then(|comment| NonZeroU64::new(comment.id)),
            resolved: *resolved,
        })
    }
}

impl ReaderWorkspace {
    pub(super) fn thread_controls(
        &self,
        (pane, position): (usize, usize),
        tab: &Tab,
        revision: &PrRevision,
        thread: Thread<'_>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut actions = div().flex().flex_col().items_start().gap_1().pb_1();
        let item = tab.item;
        if let Some(resolved) = thread.resolved {
            let id = format!("pr-{pane}-thread-{position}-resolution");
            let revision = revision.clone();
            let thread_id = thread.id.to_owned();
            let button = control(
                id.clone(),
                if resolved {
                    "Queue reopening"
                } else {
                    "Queue resolution"
                },
            )
            .disabled(self.review_controls_busy(item))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.queue_resolution(revision.clone(), thread_id.clone(), !resolved, window, cx);
            }));
            actions = actions.child(self.observed_control(id, button, false));
        }
        let Some(root_comment_id) = thread.root_comment else {
            return actions.into_any_element();
        };
        let key = format!("review-reply-{}-{root_comment_id}", item.0);
        let Some(editor) = self
            .drafts
            .get(&key)
            .filter(|editor| editor.tab_id == tab.id)
        else {
            let id = format!("pr-{pane}-thread-{position}-reply");
            let target = tab.clone();
            let base = WriteBase::ReviewReply {
                revision_id: revision.id,
                thread_id: thread.id.to_owned(),
                root_comment_id,
            };
            let button = control(id.clone(), "Reply")
                .disabled(self.pending_draft.is_some() || self.review_controls_busy(item))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.edit_review_reply(target.clone(), base.clone(), window, cx);
                }));
            return actions
                .child(self.observed_control(id, button, false))
                .into_any_element();
        };
        let current = revision.id;
        let bound = matches!(
            &editor.write_base,
            Some(WriteBase::ReviewReply { revision_id, thread_id, .. })
                if *revision_id == current && thread_id == thread.id
        );
        let input = format!("pr-{pane}-thread-{position}-reply-input");
        let submit = format!("pr-{pane}-thread-{position}-reply-submit");
        let submitted = editor.current_submission();
        let button = control(
            submit.clone(),
            submitted.map_or("Queue reply", super::outbox::draft_submission_label),
        )
        .disabled(
            !bound
                || !editor.saved()
                || editor.lease_lost
                || editor.submitting
                || editor.body.trim().is_empty()
                || submitted.is_some(),
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            this.queue_reply(&key, current, window, cx);
        }));
        actions
            .child(
                self.observed_control(
                    input.clone(),
                    Textarea::new(&editor.view)
                        .accessibility_id(input)
                        .aria_label("Reply to review thread"),
                    true,
                ),
            )
            .child(self.observed_control(submit, button, false))
            .children(
                editor
                    .lease_lost
                    .then(|| div().text_sm().child(drafts::EDITOR_LEASE_LOST)),
            )
            .children((!bound).then(|| {
                div()
                    .text_sm()
                    .child("This reply is saved against the original review revision.")
            }))
            .into_any_element()
    }

    fn queue_reply(
        &mut self,
        key: &str,
        current: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.drafts.get_mut(key) else {
            return;
        };
        let Some(WriteBase::ReviewReply {
            revision_id,
            thread_id,
            root_comment_id,
        }) = editor.write_base.clone()
        else {
            return;
        };
        if editor.submitting || editor.lease_lost || revision_id != current {
            return;
        }
        editor.capture(cx);
        let version = editor.durable_revision;
        if !editor.saved() || editor.body.trim().is_empty() || editor.current_submission().is_some()
        {
            return;
        }
        let (account, item, lease, key) = (
            editor.account,
            editor.item,
            editor.lease.clone(),
            key.to_owned(),
        );
        let intent = ReviewIntent::Reply {
            thread_id,
            root_comment_id,
            body: editor.body.clone(),
        };
        editor.submitting = true;
        let edit_key = key.clone();
        let edit_lease = lease.clone();
        self.launch(
            window,
            cx,
            move |store| {
                let (revision, _) = store
                    .pr_revision(account, item, revision_id)?
                    .ok_or(threadline_storage::Error::NotFound)?;
                let now = now_ms();
                if !store.renew_editor(
                    account,
                    &edit_key,
                    &edit_lease,
                    version,
                    now,
                    now + drafts::EDITOR_LEASE_MS,
                )? {
                    return Ok(None);
                }
                store
                    .submit_review(
                        &revision,
                        ReviewSubmission {
                            id: &store.new_operation_id()?,
                            intent: &intent,
                            draft: Some(DraftSubmission {
                                id: &edit_key,
                                revision: version,
                                editor: &edit_lease,
                                now,
                            }),
                            dependencies: &[],
                            submitted_at_ms: now,
                        },
                    )
                    .map(Some)
            },
            move |this, result, window, cx| {
                if let Some(editor) = this
                    .drafts
                    .get_mut(&key)
                    .filter(|editor| editor.lease == lease)
                {
                    editor.submitting = false;
                    if matches!(&result, Ok(Some(_))) {
                        editor.submitted = Some(
                            threadline_storage::queue::DraftSubmissionStatus::queued(version),
                        );
                    } else if matches!(&result, Ok(None)) {
                        editor.lease_lost = true;
                    }
                }
                if matches!(result, Ok(None)) {
                    this.status = Some(drafts::EDITOR_LEASE_LOST);
                    this.settle_departure(window, cx);
                } else {
                    this.finish_thread_action(item, result.is_ok(), window, cx);
                }
            },
        );
    }

    fn queue_resolution(
        &mut self,
        revision: PrRevision,
        thread_id: String,
        resolved: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = revision.pr;
        if self.departure.is_some() || !self.review_pending.insert(item) {
            return;
        }
        self.launch(
            window,
            cx,
            move |store| {
                store.submit_review(
                    &revision,
                    ReviewSubmission {
                        id: &store.new_operation_id()?,
                        intent: &ReviewIntent::SetThreadResolved {
                            thread_id,
                            resolved,
                        },
                        draft: None,
                        dependencies: &[],
                        submitted_at_ms: now_ms(),
                    },
                )
            },
            move |this, result, window, cx| {
                this.review_pending.remove(&item);
                this.finish_thread_action(item, result.is_ok(), window, cx);
            },
        );
    }

    fn finish_thread_action(
        &mut self,
        item: ItemId,
        queued: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status = Some(if queued {
            "Review thread action queued. GitHub permissions will be checked before sending."
        } else {
            "The review thread action could not be queued."
        });
        if queued {
            self.refresh_write_views(item, window, cx);
            if let Some(worker) = self.sync_worker.as_ref() {
                worker.wake(None);
            }
        }
        self.settle_departure(window, cx);
        cx.notify();
    }
}

use super::pr::PullRequestView;
use super::pr::SelectedFile;
use gpui_kit::{
    ElementId, SharedString,
    component::diff::{DiffAnnotation, DiffLinePosition, DiffSide},
};
use threadline_storage::pr::{PrFamily, ReviewAnchor, ReviewThreadLocation, SourceSide};

pub(super) struct AnchoredAnnotations {
    pub rows: Vec<DiffAnnotation>,
    pub labels: HashMap<ElementId, SharedString>,
    pub unavailable: bool,
}

impl AnchoredAnnotations {
    fn add(&mut self, key: String, path: &str, side: SourceSide, line: u32, note: String) {
        let position = DiffLinePosition::new(path.to_owned(), diff_side(side), line as usize);
        let annotation = DiffAnnotation::line(key, position);
        self.labels.insert(annotation.id().clone(), note.into());
        self.rows.push(annotation);
    }
}

fn matches_source(revision: &PrRevision, side: SourceSide, oid: &str) -> bool {
    match side {
        SourceSide::Left => revision
            .merge_base
            .as_ref()
            .is_some_and(|base| base.as_str() == oid),
        SourceSide::Right => revision.head.as_str() == oid,
    }
}

fn root_anchor(record: &PrRecord) -> Option<&ReviewAnchor> {
    let PrRecord::Thread { comments, .. } = record else {
        return None;
    };
    comments
        .iter()
        .find(|comment| comment.in_reply_to.is_none())?
        .anchor
        .as_ref()
}

fn diff_side(side: SourceSide) -> DiffSide {
    match side {
        SourceSide::Left => DiffSide::Original,
        SourceSide::Right => DiffSide::Modified,
    }
}

fn current_anchor(revision: &PrRevision, record: &PrRecord) -> Option<ReviewAnchor> {
    let PrRecord::Thread {
        outdated: Some(false),
        location: Some(ReviewThreadLocation::Line { path, side, line }),
        ..
    } = record
    else {
        return None;
    };
    let source = match side {
        SourceSide::Left => revision.merge_base.as_ref()?,
        SourceSide::Right => &revision.head,
    };
    Some(ReviewAnchor {
        path: path.clone(),
        side: *side,
        line: *line,
        original_oid: source.as_str().to_owned(),
    })
}

fn downloaded_line(file: &SelectedFile, anchor: &ReviewAnchor) -> bool {
    file.summary.path == anchor.path
        && file.patch.is_some()
        && file.has_source_line(anchor.side, anchor.line)
}

impl ReaderWorkspace {
    pub(super) fn review_annotations(
        &self,
        tab: &Tab,
        pr: &PullRequestView,
        file: &SelectedFile,
    ) -> AnchoredAnnotations {
        let mut result = AnchoredAnnotations {
            rows: Vec::new(),
            labels: HashMap::new(),
            unavailable: false,
        };
        let revision = &pr.data.revision;
        for (key, record) in pr
            .data
            .pages
            .iter()
            .filter(|(family, _)| *family == PrFamily::Threads)
            .flat_map(|(_, page)| &page.rows)
        {
            let PrRecord::Thread {
                comments,
                resolved,
                location,
                replies_complete,
                ..
            } = record
            else {
                continue;
            };
            let path = match location {
                Some(
                    ReviewThreadLocation::Line { path, .. } | ReviewThreadLocation::File { path },
                ) => Some(path.as_str()),
                None => root_anchor(record).map(|anchor| anchor.path.as_str()),
            };
            if path != Some(file.summary.path.as_str()) || comments.is_empty() {
                continue;
            }
            let Some(anchor) =
                current_anchor(revision, record).filter(|anchor| downloaded_line(file, anchor))
            else {
                result.unavailable = true;
                continue;
            };
            let Some(patch) = &file.patch else { continue };
            let state = match resolved {
                Some(true) => "Resolved",
                Some(false) => "Unresolved",
                None => "Resolution unknown",
            };
            let mut body = state.to_owned();
            for comment in comments {
                body.push_str("\n\n");
                if comment.in_reply_to.is_some() {
                    body.push_str("Reply from ");
                }
                body.push_str(comment.author.as_deref().unwrap_or("Unknown reviewer"));
                body.push('\n');
                body.push_str(&comment.body);
            }
            if !replies_complete {
                body.push_str("\n\nSome replies are unavailable.");
            }
            result.add(key.clone(), patch.path(), anchor.side, anchor.line, body);
        }
        if let Some(editor) = self
            .review_editors
            .get(&tab.item)
            .filter(|editor| editor.revision().id == revision.id)
            && let Some(patch) = &file.patch
        {
            for draft in &editor.inline {
                let anchor = &draft.anchor;
                let full_path = match anchor.side {
                    SourceSide::Left => file
                        .summary
                        .previous_path
                        .as_ref()
                        .unwrap_or(&file.summary.path),
                    SourceSide::Right => &file.summary.path,
                };
                if &anchor.path == full_path
                    && matches_source(revision, anchor.side, &anchor.source_oid)
                {
                    result.add(
                        draft.key.clone(),
                        patch.path(),
                        anchor.side,
                        anchor.last_line,
                        "Local inline review draft".into(),
                    );
                }
            }
        }
        result
    }

    pub(super) fn unresolved_thread_control(
        &self,
        pane: usize,
        tab: &Tab,
        pr: &PullRequestView,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let revision = &pr.data.revision;
        let anchors = pr
            .data
            .pages
            .iter()
            .filter(|(family, _)| *family == PrFamily::Threads)
            .flat_map(|(_, page)| &page.rows)
            .filter(|(_, record)| {
                matches!(
                    record,
                    PrRecord::Thread {
                        resolved: Some(false),
                        ..
                    }
                )
            })
            .filter_map(|(_, record)| current_anchor(revision, record))
            .filter(|anchor| {
                pr.data.files.as_ref().is_some_and(|files| {
                    files
                        .selected
                        .as_ref()
                        .filter(|file| file.summary.path == anchor.path)
                        .map_or_else(
                            || files.rows.iter().any(|file| file.path == anchor.path),
                            |file| downloaded_line(file, anchor),
                        )
                })
            })
            .collect::<Vec<_>>();
        let current = tab
            .view
            .pr_reader
            .as_ref()
            .and_then(|settings| settings.code_anchor.as_ref());
        let next = current
            .and_then(|current| anchors.iter().position(|anchor| anchor == current))
            .map_or(0, |position| (position + 1) % anchors.len());
        let anchor = anchors.get(next)?.clone();
        let revision_id = pr.data.revision.id;
        let target = tab.clone();
        let id = format!("pr-{pane}-next-unresolved-thread");
        let button = control(id.clone(), "Next unresolved thread")
            .disabled(self.layout_busy)
            .on_click(cx.listener(move |this, _, window, cx| {
                if let Some(diff) = this
                    .tab_views
                    .get(&target.id)
                    .and_then(|view| view.pr.as_ref())
                    .and_then(|pr| {
                        let selected = pr.data.files.as_ref()?.selected.as_ref()?;
                        (selected.summary.path == anchor.path).then_some(pr.diff.as_ref()?)
                    })
                {
                    diff.update(cx, |state, cx| {
                        state.scroll_to_line(
                            DiffLinePosition::new(
                                anchor.path.clone(),
                                diff_side(anchor.side),
                                anchor.line as usize,
                            ),
                            cx,
                        )
                    });
                }
                let anchor = anchor.clone();
                this.edit_pr_tab(
                    &target,
                    |settings| {
                        settings.selected_path = Some(anchor.path.clone());
                        settings.code_viewport_anchor = Some(CodeViewportAnchor {
                            revision_id,
                            source: anchor.clone(),
                        });
                        settings.code_anchor = Some(anchor);
                    },
                    window,
                    cx,
                );
            }));
        Some(self.observed_control(id, button, false))
    }
}

use super::write_composer::{IssueEditor, WriteBase};
use super::*;
use gpui_kit::base::input::TextareaState;
use gpui_kit::{Subscription, component::input::InputEvent};
use threadline_storage::{
    queue::{DraftContent, DraftKind},
    resources::{ResourceFamily, ResourceKey, ResourceTarget},
    writes::CommentTarget,
};
use tokio::{sync::watch, task::JoinHandle};

#[derive(Clone)]
struct DraftUpdate {
    serial: u64,
    content: DraftContent,
}

struct Acknowledgement {
    serial: Option<u64>,
    saved: bool,
    revision: Option<i64>,
}

struct OwnedDraft {
    account: AccountId,
    id: String,
    lease: String,
    revision: i64,
}

pub(super) struct Editor {
    pub(super) id: String,
    pub(super) tab_id: String,
    pub(super) view: Entity<TextareaState>,
    pub(super) preview: Option<media::CachedMarkdown>,
    pub(super) failed: bool,
    pub(super) account: AccountId,
    pub(super) item: ItemId,
    pub(super) lease: String,
    pub(super) body: String,
    anchor: String,
    base: String,
    pub(super) durable_revision: i64,
    pub(super) submitting: bool,
    pub(super) submitted: Option<i64>,
    pub(super) write_base: Option<WriteBase>,
    pub(super) issue_choices: Option<IssueEditor>,
    pub(super) issue_draft: bool,
    pub(super) comment_edit: bool,
    serial: u64,
    persisted: u64,
    updates: watch::Sender<Option<Arc<DraftUpdate>>>,
    task: JoinHandle<()>,
    _subscriptions: Vec<Subscription>,
}

impl Editor {
    pub(super) fn saved(&self) -> bool {
        self.serial == self.persisted && !self.failed
    }

    pub(super) fn capture(&mut self, cx: &gpui_kit::App) {
        let body = self.view.read(cx).text().to_string();
        let anchor = self
            .issue_choices
            .as_ref()
            .map_or_else(|| self.anchor.clone(), |form| form.stored_options(cx));
        if self.body == body && self.anchor == anchor && !self.failed {
            return;
        }
        self.serial += 1;
        self.body = body.clone();
        self.anchor = anchor.clone();
        self.preview = None;
        self.failed = self
            .updates
            .send(Some(Arc::new(DraftUpdate {
                serial: self.serial,
                content: DraftContent {
                    body,
                    base: self.base.clone(),
                    anchor,
                },
            })))
            .is_err();
    }
}

async fn write_editor(
    store: Arc<Store>,
    draft: OwnedDraft,
    mut changes: watch::Receiver<Option<Arc<DraftUpdate>>>,
    acknowledgements: watch::Sender<Acknowledgement>,
) {
    let OwnedDraft {
        account,
        id,
        lease,
        mut revision,
    } = draft;
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_millis(2000));
    loop {
        match tokio::select! {
            result=changes.changed()=> match result {
                Ok(())=>changes.borrow_and_update().clone(),
                Err(_)=>break,
            },
            _=heartbeat.tick()=>None,
        } {
            Some(update) => {
                let store = Arc::clone(&store);
                let id = id.clone();
                let lease = lease.clone();
                let serial = update.serial;
                let pending = update.content.clone();
                let execution = tokio::task::spawn_blocking(move || {
                    let now = now_ms();
                    store.acquire_editor(account, &id, &lease, now, now + 10_000)?;
                    store.save_draft(account, &id, &lease, now, revision, &pending)
                })
                .await;
                let saved = matches!(&execution, Ok(Ok(_)));
                if let Ok(Ok(latest)) = execution {
                    revision = latest;
                }
                acknowledgements.send_replace(Acknowledgement {
                    serial: Some(serial),
                    saved,
                    revision: saved.then_some(revision),
                });
            }
            None => {
                let store = Arc::clone(&store);
                let id = id.clone();
                let lease = lease.clone();
                let owned = tokio::task::spawn_blocking(move || {
                    let now = now_ms();
                    store.acquire_editor(account, &id, &lease, now, now + 10_000)
                })
                .await;
                if !matches!(owned, Ok(Ok(true))) {
                    acknowledgements.send_replace(Acknowledgement {
                        serial: None,
                        saved: false,
                        revision: None,
                    });
                }
            }
        }
    }
    let _ = tokio::task::spawn_blocking(move || store.release_editor(account, &id, &lease)).await;
}

struct DraftEditorRequest {
    kind: DraftKind,
    id: String,
    initial: Option<WriteBase>,
    restore_only: bool,
}

impl ReaderWorkspace {
    pub(super) fn restore_active_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self
            .layout
            .panes
            .get(self.layout.focused_pane)
            .and_then(|pane| pane.active.and_then(|index| pane.tabs.get(index)))
            .cloned()
        else {
            return;
        };
        let Some(draft_id) = tab
            .view
            .draft
            .as_deref()
            .filter(|draft_id| !self.drafts.contains_key(*draft_id))
        else {
            return;
        };
        let kind = if draft_id == format!("issue-{}", tab.item.0) {
            DraftKind::IssueEdit
        } else if draft_id == format!("comment-{}", tab.item.0)
            || draft_id.starts_with(&format!("comment-edit-{}-", tab.item.0))
        {
            DraftKind::Comment
        } else if draft_id.starts_with(&format!("review-reply-{}-", tab.item.0)) {
            DraftKind::Reply
        } else {
            return;
        };
        let id = draft_id.to_owned();
        self.edit_draft(
            tab,
            DraftEditorRequest {
                kind,
                id,
                initial: None,
                restore_only: true,
            },
            window,
            cx,
        );
    }

    pub(super) fn edit(
        &mut self,
        tab: Tab,
        kind: DraftKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = match kind {
            DraftKind::IssueEdit => format!("issue-{}", tab.item.0),
            DraftKind::Comment => format!("comment-{}", tab.item.0),
            _ => return,
        };
        self.edit_draft(
            tab,
            DraftEditorRequest {
                kind,
                id,
                initial: None,
                restore_only: false,
            },
            window,
            cx,
        );
    }

    pub(super) fn edit_comment(
        &mut self,
        tab: Tab,
        remote: std::num::NonZeroU64,
        body: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_draft(
            tab.clone(),
            DraftEditorRequest {
                kind: DraftKind::Comment,
                id: format!("comment-edit-{}-{remote}", tab.item.0),
                initial: Some(WriteBase::Comment {
                    target: CommentTarget::Remote(remote),
                    body,
                }),
                restore_only: false,
            },
            window,
            cx,
        );
    }

    pub(super) fn edit_pending_comment(
        &mut self,
        tab: Tab,
        predecessor: String,
        body: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.edit_draft(
            tab.clone(),
            DraftEditorRequest {
                kind: DraftKind::Comment,
                id: format!("comment-edit-{}-{}", tab.item.0, predecessor),
                initial: Some(WriteBase::Comment {
                    target: CommentTarget::CreatedBy(predecessor),
                    body,
                }),
                restore_only: false,
            },
            window,
            cx,
        );
    }

    pub(super) fn edit_review_reply(
        &mut self,
        tab: Tab,
        base: WriteBase,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let WriteBase::ReviewReply {
            root_comment_id, ..
        } = &base
        else {
            return;
        };
        self.edit_draft(
            tab.clone(),
            DraftEditorRequest {
                kind: DraftKind::Reply,
                id: format!("review-reply-{}-{root_comment_id}", tab.item.0),
                initial: Some(base),
                restore_only: false,
            },
            window,
            cx,
        );
    }

    fn edit_draft(
        &mut self,
        tab: Tab,
        request: DraftEditorRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        if self.pending_draft.is_some() || self.departure.is_some() || self.layout_busy {
            return;
        }
        let DraftEditorRequest {
            kind,
            id,
            initial,
            restore_only,
        } = request;
        if let Some(editor) = self.drafts.get(&id) {
            let target = editor.tab_id.clone();
            let mut layout = self.layout.clone();
            if let Some((pane, position)) = layout.panes.iter().enumerate().find_map(|(pane, p)| {
                p.tabs
                    .iter()
                    .position(|tab| tab.id == target)
                    .map(|pos| (pane, pos))
            }) {
                layout.focused_pane = pane;
                layout.panes[pane].active = Some(position);
                layout.panes[pane].tabs[position].view.draft = Some(id.clone());
                self.editor_to_focus = Some(id);
                self.save_layout(layout, window, cx);
            }
            return;
        }
        let item = tab.item;
        let lease = view_id();
        self.pending_draft = Some(id.clone());
        let draft_id = id.clone();
        let editor_lease = lease.clone();
        self.launch(
            window,
            cx,
            move |store| {
                if store.draft(account, &draft_id)?.is_none() {
                    if restore_only {
                        return Err(threadline_storage::Error::NotFound);
                    }
                    let content = match initial {
                        Some(base @ WriteBase::Comment { .. }) => {
                            let WriteBase::Comment { body, .. } = &base else {
                                unreachable!()
                            };
                            DraftContent {
                                body: body.clone(),
                                base: base.encode()?,
                                anchor: String::new(),
                            }
                        }
                        Some(base @ WriteBase::ReviewReply { .. }) => DraftContent {
                            body: String::new(),
                            base: base.encode()?,
                            anchor: String::new(),
                        },
                        _ => draft_base(store, account, item, kind)?,
                    };
                    store.create_draft(account, &draft_id, item, kind, &content)?;
                }
                let now = now_ms();
                if !store.acquire_editor(account, &draft_id, &editor_lease, now, now + 10_000)? {
                    return Ok(None);
                }
                store.draft(account, &draft_id)
            },
            move |this, result, window, cx| {
                this.pending_draft = None;
                let draft = match result {
                    Ok(Some(draft)) => draft,
                    Ok(None) => {
                        this.status = Some("This draft has an active editor in another window.");
                        this.settle_departure(window, cx);
                        return;
                    }
                    Err(_) => {
                        this.status = Some("The draft could not be opened.");
                        this.settle_departure(window, cx);
                        return;
                    }
                };
                if !this
                    .layout
                    .panes
                    .iter()
                    .any(|pane| pane.tabs.iter().any(|candidate| candidate.id == tab.id))
                {
                    let store = Arc::clone(&this.store);
                    this.runtime
                        .spawn_blocking(move || store.release_editor(account, &id, &lease));
                    this.settle_departure(window, cx);
                    return;
                }
                let issue_draft = kind == DraftKind::IssueEdit;
                let write_base = WriteBase::decode(&draft.content, issue_draft);
                let initial_base = write_base
                    .as_ref()
                    .and_then(|base| base.encode().ok())
                    .unwrap_or_else(|| draft.content.base.clone());
                let issue_choices = match write_base.as_ref() {
                    Some(WriteBase::Issue { fields, .. }) => {
                        Some(IssueEditor::new(fields, &draft.content.anchor, window, cx))
                    }
                    _ => None,
                };
                let saved_body = draft.content.body.clone();
                let saved_anchor = issue_choices.as_ref().map_or_else(
                    || draft.content.anchor.clone(),
                    |form| form.stored_options(cx),
                );
                let editor = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .auto_grow(5, 12)
                        .default_value(saved_body.clone())
                });
                let (updates, receiver) = watch::channel(None);
                let (signals, mut acknowledgements) = watch::channel(Acknowledgement {
                    serial: Some(0),
                    saved: true,
                    revision: Some(draft.revision),
                });
                let task = this.runtime.spawn(write_editor(
                    Arc::clone(&this.store),
                    OwnedDraft {
                        account,
                        id: id.clone(),
                        lease: lease.clone(),
                        revision: draft.revision,
                    },
                    receiver,
                    signals,
                ));
                let listening = id.clone();
                let current_lease = lease.clone();
                cx.spawn_in(window, async move |view, cx| {
                    while acknowledgements.changed().await.is_ok() {
                        let Acknowledgement {
                            serial,
                            saved,
                            revision,
                        } = *acknowledgements.borrow_and_update();
                        if view
                            .update_in(cx, |this, window, cx| {
                                if let Some(editor) = this.drafts.get_mut(&listening)
                                    && editor.account == account
                                    && editor.lease == current_lease
                                {
                                    if saved {
                                        editor.persisted =
                                            editor.persisted.max(serial.unwrap_or_default());
                                        if let Some(revision) = revision {
                                            editor.durable_revision = revision;
                                        }
                                    }
                                    if serial.is_none_or(|latest| latest == editor.serial) {
                                        editor.failed = !saved;
                                    }
                                    this.settle_departure(window, cx);
                                    cx.notify();
                                }
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                })
                .detach();
                let mut subscriptions = Vec::new();
                let observing = id.clone();
                let started = lease.clone();
                subscriptions.push(cx.subscribe(&editor, move |this, _, event, cx| {
                    this.draft_changed(account, &observing, &started, event, cx);
                }));
                if let Some(title) = issue_choices.as_ref().and_then(|form| form.title.as_ref()) {
                    let observing = id.clone();
                    let started = lease.clone();
                    subscriptions.push(cx.subscribe(title, move |this, _, event, cx| {
                        this.draft_changed(account, &observing, &started, event, cx);
                    }));
                }
                this.drafts.insert(
                    id.clone(),
                    Editor {
                        id: id.clone(),
                        tab_id: tab.id.clone(),
                        view: editor.clone(),
                        preview: None,
                        failed: false,
                        account,
                        item,
                        lease,
                        body: saved_body,
                        anchor: saved_anchor,
                        base: initial_base,
                        durable_revision: draft.revision,
                        submitting: false,
                        submitted: None,
                        write_base,
                        issue_choices,
                        issue_draft,
                        comment_edit: id.starts_with("comment-edit-"),
                        serial: 0,
                        persisted: 0,
                        updates,
                        task,
                        _subscriptions: subscriptions,
                    },
                );
                let mut layout = this.layout.clone();
                if let Some(slot) = layout
                    .panes
                    .iter_mut()
                    .flat_map(|pane| &mut pane.tabs)
                    .find(|candidate| candidate.id == tab.id)
                {
                    slot.view.draft = Some(id.clone());
                }
                this.editor_to_focus = Some(id);
                this.save_layout(layout, window, cx);
            },
        );
    }

    fn draft_changed(
        &mut self,
        account: AccountId,
        id: &str,
        lease: &str,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Change)
            && let Some(editor) = self.drafts.get_mut(id)
            && editor.account == account
            && editor.lease == lease
        {
            editor.capture(cx);
            cx.notify();
        }
    }

    pub(super) fn release_drafts(&mut self) {
        self.drafts.clear();
    }

    pub(super) fn stop_editors(&mut self) -> Vec<JoinHandle<()>> {
        self.drafts
            .drain()
            .map(|(_, editor)| {
                let Editor { task, updates, .. } = editor;
                drop(updates);
                task
            })
            .collect()
    }

    pub(super) fn may_close(&mut self, tab_ids: &[String], cx: &gpui_kit::App) -> bool {
        self.capture_review_editors(cx);
        for editor in self
            .drafts
            .values_mut()
            .filter(|editor| tab_ids.contains(&editor.tab_id))
        {
            editor.capture(cx);
        }
        if self.pending_draft.is_some()
            || tab_ids
                .iter()
                .any(|id| self.merge_views.get(id).is_some_and(|merge| merge.busy))
            || self.drafts.values().any(|editor| {
                tab_ids.contains(&editor.tab_id) && (!editor.saved() || editor.submitting)
            })
            || !self.review_pending.is_empty()
            || self.review_editors.values().any(|editor| {
                tab_ids.contains(&editor.tab_id) && (!editor.saved() || editor.sending)
            })
        {
            self.status = Some("The draft is still saving. Its editor remains open.");
            return false;
        }
        true
    }

    pub(super) fn capture_drafts(&mut self, cx: &gpui_kit::App) {
        for editor in self.drafts.values_mut() {
            editor.capture(cx);
        }
    }

    pub(super) fn preview(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(editor) = self.drafts.get_mut(id) {
            let text = editor.view.read(cx).value().to_string();
            editor.preview = Some(media::CachedMarkdown::new(&text, cx));
            cx.notify();
        }
    }
}

pub(super) fn draft_base(
    store: &Store,
    account: AccountId,
    item: ItemId,
    kind: DraftKind,
) -> Outcome<DraftContent> {
    let mut draft = DraftContent {
        body: String::new(),
        base: String::new(),
        anchor: String::new(),
    };
    if kind == DraftKind::IssueEdit {
        let fields = store
            .issue_write_base(account, item)?
            .ok_or(threadline_storage::Error::NotFound)?;
        draft.body = fields
            .body
            .as_ref()
            .and_then(|body| body.clone())
            .unwrap_or_default();
        let key = ResourceKey {
            account,
            target: ResourceTarget::Item(item),
            family: ResourceFamily::Metadata,
        };
        let state = store.resource_state(key)?;
        // Preserve a provider-supplied revision or validator with the original issue body.
        draft.base = state
            .validators
            .revision
            .or(state.validators.etag)
            .or(state.validators.last_modified)
            .unwrap_or_default();
        draft.base = WriteBase::Issue {
            fields: Box::new(fields),
            provider_revision: draft.base,
        }
        .encode()?;
    }
    Ok(draft)
}

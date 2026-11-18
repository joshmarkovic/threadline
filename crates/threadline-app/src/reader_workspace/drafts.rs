use super::*;
use gpui_kit::base::input::TextareaState;
use gpui_kit::{Subscription, component::input::InputEvent};
use threadline_storage::{
    queue::{DraftContent, DraftKind},
    resources::{ResourceFamily, ResourceKey, ResourceTarget},
};
use tokio::{sync::watch, task::JoinHandle};

#[derive(Clone)]
struct DraftUpdate {
    serial: u64,
    body: String,
}

struct Acknowledgement {
    serial: Option<u64>,
    saved: bool,
}

struct OwnedDraft {
    account: AccountId,
    id: String,
    lease: String,
    revision: i64,
    content: DraftContent,
}

pub(super) struct Editor {
    pub(super) id: String,
    pub(super) tab_id: String,
    pub(super) view: Entity<TextareaState>,
    pub(super) preview: Option<media::CachedMarkdown>,
    pub(super) failed: bool,
    account: AccountId,
    lease: String,
    body: String,
    serial: u64,
    persisted: u64,
    updates: watch::Sender<Option<Arc<DraftUpdate>>>,
    task: JoinHandle<()>,
    _subscription: Subscription,
}

impl Editor {
    pub(super) fn saved(&self) -> bool {
        self.serial == self.persisted && !self.failed
    }

    fn capture(&mut self, cx: &gpui_kit::App) {
        let body = self.view.read(cx).text().to_string();
        if self.body == body && !self.failed {
            return;
        }
        self.serial += 1;
        self.body = body.clone();
        self.preview = None;
        self.failed = self
            .updates
            .send(Some(Arc::new(DraftUpdate {
                serial: self.serial,
                body,
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
        content,
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
                let pending = DraftContent {
                    body: update.body.clone(),
                    base: content.base.clone(),
                    anchor: content.anchor.clone(),
                };
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
                    });
                }
            }
        }
    }
    let _ = tokio::task::spawn_blocking(move || store.release_editor(account, &id, &lease)).await;
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
        } else if draft_id == format!("comment-{}", tab.item.0) {
            DraftKind::Comment
        } else {
            return;
        };
        self.edit(tab, kind, window, cx);
    }

    pub(super) fn edit(
        &mut self,
        tab: Tab,
        kind: DraftKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        if self.pending_draft.is_some() || self.departure.is_some() || self.layout_busy {
            return;
        }
        let kind_key = match kind {
            DraftKind::IssueEdit => "issue",
            DraftKind::Comment => "comment",
            _ => return,
        };
        let id = format!("{kind_key}-{}", tab.item.0);
        if let Some(editor) = self.drafts.get(&id) {
            let target = editor.tab_id.clone();
            let focus = editor.view.clone();
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
                self.save_layout(layout, window, cx);
                window.on_next_frame(move |window, cx| focus.focus_handle(cx).focus(window, cx));
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
                let content = draft_base(store, account, item, kind)?;
                store.create_draft(account, &draft_id, item, kind, &content)?;
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
                let saved_body = draft.content.body.clone();
                let editor = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .auto_grow(5, 12)
                        .default_value(saved_body.clone())
                });
                let (updates, receiver) = watch::channel(None);
                let (signals, mut acknowledgements) = watch::channel(Acknowledgement {
                    serial: Some(0),
                    saved: true,
                });
                let task = this.runtime.spawn(write_editor(
                    Arc::clone(&this.store),
                    OwnedDraft {
                        account,
                        id: id.clone(),
                        lease: lease.clone(),
                        revision: draft.revision,
                        content: draft.content,
                    },
                    receiver,
                    signals,
                ));
                let listening = id.clone();
                let current_lease = lease.clone();
                cx.spawn_in(window, async move |view, cx| {
                    while acknowledgements.changed().await.is_ok() {
                        let Acknowledgement { serial, saved } =
                            *acknowledgements.borrow_and_update();
                        if view
                            .update_in(cx, |this, window, cx| {
                                if let Some(editor) = this.drafts.get_mut(&listening)
                                    && editor.account == account
                                    && editor.lease == current_lease
                                {
                                    if saved {
                                        editor.persisted =
                                            editor.persisted.max(serial.unwrap_or_default());
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
                let observing = id.clone();
                let started = lease.clone();
                let subscription = cx.subscribe(&editor, move |this, _, event, cx| {
                    if matches!(event, InputEvent::Change)
                        && let Some(editor) = this.drafts.get_mut(&observing)
                        && editor.account == account
                        && editor.lease == started
                    {
                        editor.capture(cx);
                        cx.notify();
                    }
                });
                this.drafts.insert(
                    id.clone(),
                    Editor {
                        id: id.clone(),
                        tab_id: tab.id.clone(),
                        view: editor.clone(),
                        preview: None,
                        failed: false,
                        account,
                        lease,
                        body: saved_body,
                        serial: 0,
                        persisted: 0,
                        updates,
                        task,
                        _subscription: subscription,
                    },
                );
                let mut layout = this.layout.clone();
                if let Some(slot) = layout
                    .panes
                    .iter_mut()
                    .flat_map(|pane| &mut pane.tabs)
                    .find(|candidate| candidate.id == tab.id)
                {
                    slot.view.draft = Some(id);
                }
                this.save_layout(layout, window, cx);
                window.on_next_frame(move |window, cx| editor.focus_handle(cx).focus(window, cx));
            },
        );
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
        for editor in self
            .drafts
            .values_mut()
            .filter(|editor| tab_ids.contains(&editor.tab_id))
        {
            editor.capture(cx);
        }
        if self.pending_draft.is_some()
            || self
                .drafts
                .values()
                .any(|editor| tab_ids.contains(&editor.tab_id) && !editor.saved())
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

fn draft_base(
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
        let issue = store
            .reader_item_metadata(account, item)?
            .ok_or(threadline_storage::Error::NotFound)?;
        if let Change::Value(body) = issue.body {
            draft.body = body;
        }
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
            .or_else(|| {
                issue
                    .remote_updated_at_ms
                    .map(|revision| revision.to_string())
            })
            .unwrap_or_default();
        draft.anchor = draft.body.clone();
    }
    Ok(draft)
}

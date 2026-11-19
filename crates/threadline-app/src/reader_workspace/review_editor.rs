use super::*;
use gpui_kit::base::input::TextareaState;
use gpui_kit::component::diff::{DiffLineRange, DiffSide};
use std::time::Duration;
use threadline_storage::{
    pr::{PrRevision, SourceSide},
    queue::DraftSubmission,
    reviews::{
        DraftInline, InlineAnchor, ReviewDraft, ReviewEvent, ReviewIntent, ReviewSubmission,
        SavedReviewDraft,
    },
};
use tokio::{sync::watch, task::JoinHandle};

#[derive(Clone)]
struct ReviewSnapshot {
    sequence: u64,
    value: ReviewDraft,
    reanchor: Option<PrRevision>,
}

#[derive(Clone)]
struct ReviewSaved {
    sequence: Option<u64>,
    version: Option<i64>,
    reanchored: Option<PrRevision>,
    reanchor_attempted: bool,
}

#[derive(Clone)]
struct ReviewLease {
    id: String,
    token: String,
    revision: PrRevision,
}

#[derive(Clone)]
pub(super) struct SourceRange {
    path: String,
    side: SourceSide,
    start: u32,
    end: u32,
}

impl SourceRange {
    fn from_selection(selection: &DiffLineRange) -> Option<Self> {
        if selection.side() != selection.end_side() {
            return None;
        }
        let side = match selection.side() {
            DiffSide::Original => SourceSide::Left,
            DiffSide::Modified => SourceSide::Right,
        };
        let start = u32::try_from(selection.start()).ok()?;
        let end = u32::try_from(selection.end()).ok()?;
        Some(Self {
            path: selection.path().to_string(),
            side,
            start: start.min(end),
            end: start.max(end),
        })
    }

    fn capture(&self, store: &Store, revision: &PrRevision) -> Outcome<InlineAnchor> {
        store.capture_inline_source_anchor(revision, &self.path, self.side, self.start, self.end)
    }
}

pub(super) struct InlineEditor {
    pub key: String,
    pub anchor: InlineAnchor,
    pub input: Entity<TextareaState>,
    _subscription: Subscription,
}

pub(super) struct ReviewEditor {
    lease: ReviewLease,
    pub tab_id: String,
    pub event: ReviewEvent,
    pub summary: Entity<TextareaState>,
    pub inline: Vec<InlineEditor>,
    pub focused_inline: Option<usize>,
    pub collapsed: bool,
    pub failed: bool,
    pub sending: bool,
    pub(crate) version: i64,
    pub(crate) submitted: Option<i64>,
    pub remapped: HashMap<String, InlineAnchor>,
    pub remap_revision: Option<i64>,
    rebinding: bool,
    sequence: u64,
    persisted: u64,
    last: ReviewDraft,
    updates: watch::Sender<Option<Arc<ReviewSnapshot>>>,
    task: JoinHandle<()>,
    _summary_watch: Subscription,
}

impl ReviewEditor {
    pub(super) fn revision(&self) -> &PrRevision {
        &self.lease.revision
    }
    #[cfg(feature = "e2e")]
    pub(super) fn id(&self) -> &str {
        &self.lease.id
    }

    pub(super) fn saved(&self) -> bool {
        self.sequence == self.persisted && !self.failed
    }

    pub(super) fn inputs(&self, cx: &gpui_kit::App) -> ReviewDraft {
        ReviewDraft {
            event: self.event,
            summary: self.summary.read(cx).text().to_string(),
            comments: self
                .inline
                .iter()
                .map(|text| DraftInline {
                    key: text.key.clone(),
                    body: text.input.read(cx).text().to_string(),
                    anchor: text.anchor.clone(),
                })
                .collect(),
        }
    }

    fn capture(&mut self, cx: &gpui_kit::App) {
        if self.rebinding {
            return;
        }
        let value = self.inputs(cx);
        if value == self.last && !self.failed {
            return;
        }
        self.sequence += 1;
        self.last = value.clone();
        self.failed = self
            .updates
            .send(Some(Arc::new(ReviewSnapshot {
                sequence: self.sequence,
                value,
                reanchor: None,
            })))
            .is_err();
    }
}

#[cfg(feature = "e2e")]
#[derive(serde::Serialize)]
pub(super) struct ReviewObservation {
    id: String,
    revision: i64,
    head: String,
    owner_tab: bool,
    collapsed: bool,
    event: String,
    saved: bool,
    queued: bool,
    summary_nonempty: bool,
    anchors: Vec<ReviewAnchorObservation>,
}

#[cfg(feature = "e2e")]
#[derive(serde::Serialize)]
struct ReviewAnchorObservation {
    path: String,
    side: String,
    first_line: u32,
    last_line: u32,
    source_oid: String,
    comment_nonempty: bool,
}

#[cfg(feature = "e2e")]
impl ReviewEditor {
    pub(super) fn observation(&self, tab: &Tab) -> ReviewObservation {
        ReviewObservation {
            id: self.id().to_owned(),
            revision: self.version,
            head: self.revision().head.as_str().to_owned(),
            owner_tab: self.tab_id == tab.id,
            collapsed: self.collapsed,
            event: format!("{:?}", self.event),
            saved: self.saved(),
            queued: self.submitted == Some(self.version),
            summary_nonempty: !self.last.summary.trim().is_empty(),
            anchors: self
                .last
                .comments
                .iter()
                .map(|comment| ReviewAnchorObservation {
                    path: comment.anchor.path.clone(),
                    side: format!("{:?}", comment.anchor.side),
                    first_line: comment.anchor.first_line,
                    last_line: comment.anchor.last_line,
                    source_oid: comment.anchor.source_oid.clone(),
                    comment_nonempty: !comment.body.trim().is_empty(),
                })
                .collect(),
        }
    }
}

async fn persist_review(
    store: Arc<Store>,
    mut lease: ReviewLease,
    mut version: i64,
    mut updates: watch::Receiver<Option<Arc<ReviewSnapshot>>>,
    receipts: watch::Sender<ReviewSaved>,
) {
    let mut heartbeat = tokio::time::interval(Duration::from_millis(2000));
    loop {
        let update = tokio::select! {
            change=updates.changed()=>match change {Ok(())=>updates.borrow_and_update().clone(),Err(_)=>break},
            _=heartbeat.tick()=>None,
        };
        let db = Arc::clone(&store);
        let proof = lease.clone();
        let expected = version;
        let edit = update.clone();
        let outcome = tokio::task::spawn_blocking(move || {
            let now = now_ms();
            if !db.acquire_editor(
                proof.revision.account,
                &proof.id,
                &proof.token,
                now,
                now + 10_000,
            )? {
                return Err(threadline_storage::Error::Conflict(
                    "Review editor lease changed",
                ));
            }
            match edit {
                Some(edit) => {
                    let stamp = DraftSubmission {
                        id: &proof.id,
                        revision: expected,
                        editor: &proof.token,
                        now,
                    };
                    match edit.reanchor.as_ref() {
                        Some(new) => db.reanchor_review_draft(new, stamp, &edit.value),
                        None => db.save_review_draft(&proof.revision, stamp, &edit.value),
                    }
                }
                None => Ok(expected),
            }
        })
        .await;
        match (update, outcome) {
            (Some(update), Ok(Ok(saved))) => {
                version = saved;
                let reanchored = update.reanchor.clone();
                if let Some(new) = reanchored.as_ref() {
                    lease.revision = new.clone();
                }
                receipts.send_replace(ReviewSaved {
                    sequence: Some(update.sequence),
                    version: Some(version),
                    reanchor_attempted: reanchored.is_some(),
                    reanchored,
                });
            }
            (update, err) if !matches!(err, Ok(Ok(_))) => {
                receipts.send_replace(ReviewSaved {
                    sequence: update.as_ref().map(|update| update.sequence),
                    version: None,
                    reanchored: None,
                    reanchor_attempted: update.is_some_and(|update| update.reanchor.is_some()),
                });
            }
            _ => {}
        }
    }
    let _ = tokio::task::spawn_blocking(move || {
        store.release_editor(lease.revision.account, &lease.id, &lease.token)
    })
    .await;
}

struct ReviewOpen {
    saved: SavedReviewDraft,
    proof: ReviewLease,
    extra: Option<InlineAnchor>,
}

fn review_id(item: ItemId) -> String {
    format!("review-{}", item.0)
}

fn load_review(
    store: &Store,
    revision: PrRevision,
    token: String,
    anchor: Option<SourceRange>,
    restore: bool,
) -> Outcome<Option<ReviewOpen>> {
    let id = review_id(revision.pr);
    let existing = store.review_draft(revision.account, &id)?;
    if restore && existing.is_none() {
        return Ok(None);
    }
    let saved_revision = match &existing {
        Some(saved) if saved.revision_id != revision.id => {
            store
                .pr_revision(revision.account, revision.pr, saved.revision_id)?
                .ok_or(threadline_storage::Error::NotFound)?
                .0
        }
        _ => revision.clone(),
    };
    let extra = anchor
        .filter(|_| saved_revision == revision)
        .map(|anchor| anchor.capture(store, &revision))
        .transpose()?;
    if existing.is_none() {
        let initial = ReviewDraft {
            event: ReviewEvent::Comment,
            summary: String::new(),
            comments: Vec::new(),
        };
        store.create_review_draft(&saved_revision, &id, &initial)?;
    }
    let now = now_ms();
    if !store.acquire_editor(saved_revision.account, &id, &token, now, now + 10_000)? {
        return Err(threadline_storage::Error::Conflict(
            "The review draft is being edited in another window",
        ));
    }
    let saved = store
        .review_draft(saved_revision.account, &id)?
        .ok_or(threadline_storage::Error::NotFound)?;
    Ok(Some(ReviewOpen {
        saved,
        proof: ReviewLease {
            id,
            token,
            revision: saved_revision,
        },
        extra,
    }))
}

impl ReaderWorkspace {
    pub(super) fn review_controls_busy(&self, item: ItemId) -> bool {
        self.review_pending.contains(&item)
    }

    pub(super) fn begin_inline(
        &mut self,
        tab: Tab,
        selected: &DiffLineRange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(range) = SourceRange::from_selection(selected) else {
            self.status = Some("Select a contiguous range on one side of the diff.");
            cx.notify();
            return;
        };
        if self.review_editors.get(&tab.item).is_some_and(|editor| {
            self.tab_views
                .get(&tab.id)
                .and_then(|view| view.pr.as_ref())
                .is_some_and(|pr| pr.data.revision.id != editor.revision().id)
        }) {
            self.reanchor_selection(tab, range, window, cx);
        } else {
            self.open_review(tab, Some(range), false, window, cx);
        }
    }

    pub(super) fn restore_review(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        if tab
            .view
            .draft
            .as_ref()
            .is_some_and(|id| id == &review_id(tab.item))
            && !self.review_editors.contains_key(&tab.item)
        {
            self.open_review(tab, None, true, window, cx);
        }
    }

    pub(super) fn open_review(
        &mut self,
        tab: Tab,
        range: Option<SourceRange>,
        restore: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.departure.is_some() || self.review_pending.contains(&tab.item) {
            return;
        }
        let Some(pr) = self
            .tab_views
            .get(&tab.id)
            .and_then(|view| view.pr.as_ref())
        else {
            return;
        };
        let revision = pr.data.revision.clone();
        let item = tab.item;
        if let Some(editor) = self.review_editors.get(&item) {
            if let Some(range) = range
                && editor.revision().id == revision.id
            {
                let token = editor.lease.token.clone();
                let key = view_id();
                self.review_pending.insert(item);
                self.launch(
                    window,
                    cx,
                    move |store| range.capture(store, &revision),
                    move |this, result, window, cx| {
                        this.review_pending.remove(&item);
                        match result {
                            Ok(anchor)
                                if this
                                    .review_editors
                                    .get(&item)
                                    .is_some_and(|editor| editor.lease.token == token) =>
                            {
                                this.add_review_inline(item, key.clone(), anchor, window, cx)
                            }
                            Ok(_) => {}
                            Err(_) => {
                                this.status = Some(
                                    "The selected source range is unavailable for this revision.",
                                )
                            }
                        }
                        this.settle_departure(window, cx);
                    },
                );
            } else if !restore {
                self.focus_review_tab(item, window, cx);
            }
            return;
        }
        self.review_pending.insert(item);
        let token = view_id();
        self.launch(window,cx,move |store|load_review(store,revision,token,range,restore),move |this,result,window,cx| {
            this.review_pending.remove(&item);
            match result {
                Ok(Some(open)) => {
                    if this.layout.panes.iter().flat_map(|pane|&pane.tabs).any(|current|current.id==tab.id&&current.item==item) {
                        let ReviewOpen{saved,proof,extra}=open;
                        this.install_review_editor(&tab,saved,proof,window,cx);
                        if let Some(anchor)=extra {
                            this.add_review_inline(item,view_id(),anchor,window,cx);
                        } else if !restore {
                            this.focus_review_tab(item,window,cx);
                        }
                    } else {
                        let store=Arc::clone(&this.store);
                        this.runtime.spawn_blocking(move ||store.release_editor(open.proof.revision.account,&open.proof.id,&open.proof.token));
                    }
                }
                Ok(None)=>{},
                Err(_)=>this.status=Some("The local review could not be opened. Its saved text remains available."),
            }
            this.settle_departure(window,cx);
        });
    }

    fn install_review_editor(
        &mut self,
        tab: &Tab,
        saved: SavedReviewDraft,
        lease: ReviewLease,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = tab.item;
        let summary = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 4)
                .default_value(saved.draft.summary.clone())
        });
        let owner = lease.token.clone();
        let subscription = cx.subscribe(&summary, move |this, _, event, cx| {
            this.review_text_changed(item, &owner, event, cx)
        });
        let inline = saved
            .draft
            .comments
            .iter()
            .map(|inline| self.inline_editor(item, lease.token.clone(), inline.clone(), window, cx))
            .collect();
        let (changes, updates) = watch::channel(None);
        let (result, mut receipts) = watch::channel(ReviewSaved {
            sequence: Some(0),
            version: Some(saved.revision),
            reanchored: None,
            reanchor_attempted: false,
        });
        let task = self.runtime.spawn(persist_review(
            Arc::clone(&self.store),
            lease.clone(),
            saved.revision,
            updates,
            result,
        ));
        let token = lease.token.clone();
        let id = lease.id.clone();
        cx.spawn_in(window, async move |view, cx| {
            while receipts.changed().await.is_ok() {
                let receipt = receipts.borrow_and_update().clone();
                if view
                    .update_in(cx, |this, window, cx| {
                        if let Some(editor) = this
                            .review_editors
                            .get_mut(&item)
                            .filter(|editor| editor.lease.token == token)
                        {
                            if let Some(version) = receipt.version {
                                editor.version = version;
                                editor.persisted =
                                    editor.persisted.max(receipt.sequence.unwrap_or_default());
                            }
                            if receipt.reanchor_attempted && receipt.sequence == Some(editor.sequence) {
                                editor.sending = false;
                                editor.rebinding = false;
                                if receipt.version.is_none() {
                                    editor.persisted = editor.sequence;
                                    this.status = Some("The source changed again. The original review draft is still saved.");
                                }
                            }
                            if !receipt.reanchor_attempted && receipt
                                .sequence
                                .is_none_or(|sequence| sequence == editor.sequence)
                            {
                                editor.failed = receipt.version.is_none();
                            }
                            if let Some(revision) = receipt.reanchored {
                                editor.lease.revision = revision;
                                for inline in &mut editor.inline {
                                    if let Some(anchor) = editor.remapped.get(&inline.key) {
                                        inline.anchor = anchor.clone();
                                    }
                                }
                                for inline in &mut editor.last.comments {
                                    inline.anchor = editor.remapped[&inline.key].clone();
                                }
                                editor.remapped.clear();
                                editor.remap_revision = None;
                                editor.submitted = None;
                                this.status = Some(
                                    "Review draft moved to the explicitly selected source ranges.",
                                );
                            }
                            if receipt.reanchor_attempted {
                                editor.capture(cx);
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
        self.review_editors.insert(
            item,
            ReviewEditor {
                lease,
                tab_id: tab.id.clone(),
                event: saved.draft.event,
                summary,
                inline,
                focused_inline: None,
                collapsed: false,
                failed: false,
                sending: false,
                version: saved.revision,
                submitted: None,
                remapped: HashMap::new(),
                remap_revision: None,
                rebinding: false,
                sequence: 0,
                persisted: 0,
                last: saved.draft,
                updates: changes,
                task,
                _summary_watch: subscription,
            },
        );
        self.capture_scroll();
        let mut layout = self.layout.clone();
        if let Some(current) = layout
            .panes
            .iter_mut()
            .flat_map(|pane| &mut pane.tabs)
            .find(|current| current.id == tab.id)
        {
            current.view.draft = Some(id);
        }
        self.save_layout(layout, window, cx);
        cx.notify();
    }

    fn inline_editor(
        &self,
        item: ItemId,
        token: String,
        comment: DraftInline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> InlineEditor {
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 6)
                .default_value(comment.body)
        });
        let subscription = cx.subscribe(&input, move |this, _, event, cx| {
            this.review_text_changed(item, &token, event, cx)
        });
        InlineEditor {
            key: comment.key,
            anchor: comment.anchor,
            input,
            _subscription: subscription,
        }
    }

    fn review_text_changed(
        &mut self,
        item: ItemId,
        token: &str,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::Change)
            && let Some(editor) = self
                .review_editors
                .get_mut(&item)
                .filter(|editor| editor.lease.token == token)
        {
            editor.capture(cx);
            cx.notify();
        }
    }

    pub(super) fn toggle_review_editor(
        &mut self,
        item: ItemId,
        tab: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self
            .review_editors
            .get_mut(&item)
            .filter(|editor| editor.tab_id == tab)
        else {
            return;
        };
        if editor.collapsed {
            self.focus_review_tab(item, window, cx);
        } else {
            editor.capture(cx);
            editor.collapsed = true;
            cx.notify();
        }
    }

    fn focus_review_tab(&mut self, item: ItemId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.review_editors.get_mut(&item) else {
            return;
        };
        editor.collapsed = false;
        let id = editor.tab_id.clone();
        let focus = editor
            .focused_inline
            .and_then(|index| editor.inline.get(index))
            .map_or_else(
                || editor.summary.focus_handle(cx),
                |inline| inline.input.focus_handle(cx),
            );
        if self.active_tab().is_none_or(|active| active.id != id) {
            let mut layout = self.layout.clone();
            if let Some((index, pane, tab)) =
                layout
                    .panes
                    .iter_mut()
                    .enumerate()
                    .find_map(|(index, pane)| {
                        pane.tabs
                            .iter()
                            .position(|tab| tab.id == id)
                            .map(|tab| (index, pane, tab))
                    })
            {
                layout.focused_pane = index;
                pane.active = Some(tab);
                self.save_layout(layout, window, cx);
            }
        }
        window.on_next_frame(move |window, cx| focus.focus(window, cx));
        cx.notify();
    }

    fn add_review_inline(
        &mut self,
        item: ItemId,
        key: String,
        anchor: InlineAnchor,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(owner) = self
            .review_editors
            .get(&item)
            .map(|editor| editor.lease.token.clone())
        else {
            return;
        };
        let inline = self.inline_editor(
            item,
            owner,
            DraftInline {
                key,
                body: String::new(),
                anchor,
            },
            window,
            cx,
        );
        let Some(editor) = self.review_editors.get_mut(&item) else {
            return;
        };
        editor.focused_inline = Some(editor.inline.len());
        editor.inline.push(inline);
        editor.capture(cx);
        self.focus_review_tab(item, window, cx);
        cx.notify();
    }

    pub(super) fn select_review_inline(
        &mut self,
        item: ItemId,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.review_editors.get_mut(&item)
            && index < editor.inline.len()
        {
            editor.focused_inline = Some(index);
            self.focus_review_tab(item, window, cx);
            cx.notify();
        }
    }

    pub(super) fn set_review_event(
        &mut self,
        item: ItemId,
        event: ReviewEvent,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.review_editors.get_mut(&item) {
            editor.event = event;
            editor.capture(cx);
            cx.notify();
        }
    }

    fn reanchor_selection(
        &mut self,
        tab: Tab,
        range: SourceRange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.review_editors.get(&tab.item) else {
            return;
        };
        let Some(key) = editor
            .focused_inline
            .and_then(|index| editor.inline.get(index))
            .map(|comment| comment.key.clone())
        else {
            self.status =
                Some("Choose an original inline draft before assigning its replacement range.");
            return;
        };
        if editor.tab_id != tab.id || editor.sending || self.review_pending.contains(&tab.item) {
            return;
        }
        let Some(pr) = self
            .tab_views
            .get(&tab.id)
            .and_then(|view| view.pr.as_ref())
        else {
            return;
        };
        let revision = pr.data.revision.clone();
        let item = tab.item;
        let version = revision.id;
        let token = editor.lease.token.clone();
        let tab_id = tab.id;
        self.review_pending.insert(item);
        self.launch(window,cx,move |store|range.capture(store,&revision),move |this,result,window,cx| {
            this.review_pending.remove(&item);
            let current=this.tab_views.get(&tab_id).and_then(|view|view.pr.as_ref()).map(|pr|pr.data.revision.id);
            if current==Some(version) && let Some(editor)=this.review_editors.get_mut(&item).filter(|editor|editor.lease.token==token) {
                match result {
                    Ok(anchor)=>{
                        if editor.remap_revision != Some(version) {
                            editor.remapped.clear();
                            editor.remap_revision = Some(version);
                        }
                        editor.remapped.insert(key,anchor);
                        this.status=Some("Replacement range selected. Confirm re-anchoring after reviewing all comments.");
                    }
                    Err(_)=>this.status=Some("The selected replacement range is unavailable."),
                }
            }
            this.settle_departure(window,cx);
        });
    }

    pub(super) fn confirm_review_reanchor(
        &mut self,
        item: ItemId,
        revision: PrRevision,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.review_editors.get_mut(&item) else {
            return;
        };
        if editor.sending
            || !editor.saved()
            || editor.lease.revision.id == revision.id
            || (!editor.inline.is_empty() && editor.remap_revision != Some(revision.id))
            || editor
                .inline
                .iter()
                .any(|comment| !editor.remapped.contains_key(&comment.key))
        {
            return;
        }
        let mut value = editor.inputs(cx);
        for inline in &mut value.comments {
            inline.anchor = editor.remapped[&inline.key].clone();
        }
        editor.sequence += 1;
        editor.sending = true;
        editor.rebinding = true;
        if editor
            .updates
            .send(Some(Arc::new(ReviewSnapshot {
                sequence: editor.sequence,
                value,
                reanchor: Some(revision),
            })))
            .is_err()
        {
            editor.failed = true;
            editor.sending = false;
            editor.rebinding = false;
        }
        cx.notify();
    }

    pub(super) fn remove_review_inline(
        &mut self,
        item: ItemId,
        index: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = self.review_editors.get_mut(&item)
            && !editor.sending
            && index < editor.inline.len()
        {
            let old = editor.inline.remove(index);
            editor.remapped.remove(&old.key);
            editor.focused_inline =
                (!editor.inline.is_empty()).then(|| index.min(editor.inline.len() - 1));
            editor.capture(cx);
            cx.notify();
        }
    }

    pub(super) fn submit_review_editor(
        &mut self,
        item: ItemId,
        current: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.review_editors.get_mut(&item) else {
            return;
        };
        if editor.lease.revision.id != current || editor.sending {
            return;
        }
        editor.capture(cx);
        if !editor.saved() || editor.submitted == Some(editor.version) {
            return;
        }
        let lease = editor.lease.clone();
        let token = lease.token.clone();
        let version = editor.version;
        let intent = ReviewIntent::Review(editor.inputs(cx));
        editor.sending = true;
        self.launch(
            window,
            cx,
            move |store| {
                let now = now_ms();
                let operation = store.new_operation_id()?;
                store.submit_review(
                    &lease.revision,
                    ReviewSubmission {
                        id: &operation,
                        intent: &intent,
                        draft: Some(DraftSubmission {
                            id: &lease.id,
                            revision: version,
                            editor: &lease.token,
                            now,
                        }),
                        dependencies: &[],
                        submitted_at_ms: now,
                    },
                )
            },
            move |this, result, window, cx| {
                if let Some(editor) = this
                    .review_editors
                    .get_mut(&item)
                    .filter(|editor| editor.lease.token == token)
                {
                    editor.sending = false;
                    if result.is_ok() {
                        editor.submitted = Some(version);
                    }
                }
                this.status = Some(if result.is_ok() {
                    "Review saved to the Outbox."
                } else {
                    "The review was not queued. Check its text and source revision."
                });
                if result.is_ok() {
                    this.refresh_write_views(item, window, cx);
                    if let Some(worker) = this.sync_worker.as_ref() {
                        worker.wake(None);
                    }
                }
                this.settle_departure(window, cx);
            },
        );
        cx.notify();
    }

    pub(super) fn capture_review_editors(&mut self, cx: &gpui_kit::App) {
        for editor in self.review_editors.values_mut() {
            editor.capture(cx);
        }
    }

    pub(super) fn close_review_editors(&mut self) -> Vec<JoinHandle<()>> {
        self.review_editors
            .drain()
            .map(|(_, editor)| {
                let ReviewEditor { task, updates, .. } = editor;
                drop(updates);
                task
            })
            .collect()
    }
}

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use gpui_kit::base::text::TextViewState;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AppContext, Context, Entity, FocusHandle, Focusable, ScrollHandle, Subscription, Window, point,
    px,
};
use threadline_core::{github::Reachability, sync::Cancellation};
use threadline_storage::{
    AccountId, Change, Item, ItemId, Store,
    access::{AccessState, RepositoryView},
    queue::{Pane, Projection, QueueRow, Tab, TabView, Workspace},
    reader::{
        BrowseFilter, BrowsePage, CacheUsage, DiscussionPage, IssueDownload, ReaderItemMetadata,
        ReaderScope, SearchHit, SidebarCounts, TimelinePage,
    },
};

mod activity_view;
mod drafts;
mod history_view;
mod media;
mod merge_view;
mod outbox;
#[cfg(feature = "e2e")]
pub(crate) mod performance;
mod pr;
mod pr_view;
mod remote;
mod review_editor;
mod review_threads;
mod review_view;
mod scheduled;
mod sync_bridge;
#[cfg(all(test, feature = "e2e"))]
mod tests;
mod view;
mod write_composer;
pub(crate) use view::bind_shortcuts;

const PAGE: u16 = 40;
const MAX_PANES: usize = 3;
type Outcome<T> = threadline_storage::Result<T>;
type DepartureAction = Box<dyn FnOnce(&mut Window, &mut gpui_kit::App)>;

struct Departure {
    quit: bool,
    after: DepartureAction,
}

pub(crate) struct ReaderAccount {
    pub account: Option<AccountId>,
    pub profile: Option<threadline_storage::access::AccessProfile>,
    pub offline: bool,
    pub reachability: Reachability,
    pub repositories: Vec<RepositoryView>,
}

#[cfg(feature = "e2e")]
#[derive(serde::Serialize)]
pub(crate) struct ReaderObservation {
    pub pane_count: usize,
    pub tab_counts: Vec<usize>,
    pub focused_pane: usize,
    pub active_item: Option<i64>,
    pub active_number: Option<u64>,
    pub cached_content_ready: bool,
    pub cached_visible_panes: usize,
    pub search_results: Option<usize>,
    pub search_generation: Option<u64>,
    pub first_search_item: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub performance_timing: Option<performance::PaintTiming>,
    pub comments_coverage: Option<String>,
    pub comment_fetch_state: Option<String>,
    pub comment_count: Option<usize>,
    pub download_state: Option<String>,
    pub active_draft_saved: Option<bool>,
    pub active_draft_chars: Option<usize>,
    pub active_draft_sha256: Option<String>,
    pub signed_out: bool,
    pub offline: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_sync: Option<SyncObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<pr_view::PrObservation>,
    pub pending_pr_reads: usize,
    pub history: Option<history_view::HistoryObservation>,
    pub notification: activity_view::NotificationObservation,
    pub outbox: Option<u64>,
    pub outbox_visible: bool,
    pub outbox_action_state: Option<String>,
    pub outbox_action_cancel_requested: bool,
    pub outbox_paused: bool,
    pub active_draft_submitted: Option<bool>,
}

#[cfg(feature = "e2e")]
#[derive(serde::Serialize)]
pub(crate) struct SyncObservation {
    panel_open: bool,
    schedule: Option<threadline_storage::sync::WorkSchedule>,
    controls: Option<threadline_storage::sync::SyncControls>,
    cache_generation: u64,
    eligible_outbox: u64,
    connection: String,
    reason: Option<String>,
    pending_reads: Option<u64>,
}

pub(crate) struct ReaderWorkspace {
    store: Arc<Store>,
    runtime: tokio::runtime::Handle,
    service: Option<crate::reader::ReaderService>,
    workspace_visible: bool,
    visible_subscriptions: Option<scheduled::VisibleSubscriptions>,
    sync_worker: Option<Arc<crate::sync_worker::SyncWorker>>,
    sync_update: Option<crate::sync_worker::SyncUpdate>,
    sync_started: bool,
    network: Cancellation,
    network_pending: usize,
    network_started: u64,
    network_observed: u64,
    remote_search: Option<remote::SearchResults>,
    remote_generation: u64,
    remote_error: Option<String>,
    images: HashMap<media::ImageKey, media::ImageState>,
    downloads: HashMap<ItemId, Cancellation>,
    pr_reads: std::collections::HashSet<pr_view::PrReadKey>,
    pr_resume_attempts: std::collections::HashSet<(i64, i64)>,
    merge_views: HashMap<String, merge_view::MergeView>,
    account: Option<AccountId>,
    login: String,
    actor: Option<std::num::NonZeroU64>,
    access: AccessState,
    offline: bool,
    reachability: Reachability,
    checked: Option<i64>,
    contact: Option<i64>,
    repositories: Vec<RepositoryView>,
    scope: ReaderScope,
    back: Vec<ItemId>,
    forward: Vec<ItemId>,
    layout: Workspace,
    layout_busy: bool,
    pending_tab_selection: Option<String>,
    surface_focus: FocusHandle,
    focus: Vec<FocusHandle>,
    focus_events: Vec<Subscription>,
    maximized: bool,
    tab_views: HashMap<String, ItemView>,
    pending_tabs: HashMap<String, u64>,
    tab_generation: u64,
    generation: u64,
    list_generation: u64,
    filter: BrowseFilter,
    saved: Vec<(String, BrowseFilter)>,
    rows: Vec<threadline_storage::reader::BrowseRow>,
    next: Option<ItemId>,
    coverage: Option<(threadline_storage::resources::Coverage, Option<i64>)>,
    list_busy: bool,
    search: Entity<InputState>,
    search_hits: Option<Vec<SearchHit>>,
    search_reply: Option<u64>,
    #[cfg(feature = "e2e")]
    performance: performance::ReaderTiming,
    url: Entity<InputState>,
    saved_name: Entity<InputState>,
    author: Entity<InputState>,
    label: Entity<InputState>,
    assignee: Entity<InputState>,
    reviewer: Entity<InputState>,
    dates_after: Entity<InputState>,
    dates_before: Entity<InputState>,
    counts: Option<SidebarCounts>,
    filters_open: bool,
    surface: Surface,
    history: history_view::HistoryState,
    activity: activity_view::ActivityState,
    sync_panel: Option<Entity<crate::sync_panel::SyncPanel>>,
    sync_watch: Option<Subscription>,
    #[cfg(feature = "e2e")]
    sync_render_watch: Option<Subscription>,
    cache: Option<CacheUsage>,
    queue: Vec<QueueRow>,
    outbox: outbox::OutboxState,
    drafts: HashMap<String, drafts::Editor>,
    pending_draft: Option<String>,
    review_editors: HashMap<ItemId, review_editor::ReviewEditor>,
    review_pending: HashSet<ItemId>,
    editor_to_focus: Option<String>,
    departure: Option<Departure>,
    #[cfg(feature = "e2e")]
    observer: Option<crate::observation::Observer>,
    status: Option<&'static str>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Surface {
    Browse,
    Activity,
    NotificationSettings,
    Outbox,
    Storage,
    Sync,
    History,
}

struct ItemData {
    item: Option<Item>,
    metadata: Option<ReaderItemMetadata>,
    pr: Option<pr::PullRequestData>,
    anchor_missing: bool,
    discussion: DiscussionPage,
    timeline: TimelinePage,
    linked_items: HashMap<String, ItemId>,
    projections: Vec<Projection>,
    write_choices: Option<threadline_storage::writes::CachedRepositoryChoices>,
    editable_comments: std::collections::HashMap<String, std::num::NonZeroU64>,
    download: Option<IssueDownload>,
}

impl ItemData {
    fn read(
        store: &Store,
        account: AccountId,
        actor: Option<std::num::NonZeroU64>,
        tab: &Tab,
    ) -> Outcome<Self> {
        let item = tab.item;
        let (discussion, anchor_missing) = match tab
            .view
            .scroll_anchor
            .as_deref()
            .filter(|key| !key.starts_with("thread:"))
        {
            Some(anchor) => match store.discussion_page_at(account, item, anchor, PAGE) {
                Ok(page) => (page, false),
                Err(threadline_storage::Error::NotFound) => {
                    (store.discussion_page(account, item, None, PAGE)?, true)
                }
                Err(error) => return Err(error),
            },
            None => (store.discussion_page(account, item, None, PAGE)?, false),
        };
        let timeline = store.timeline_page(account, item, None, PAGE)?;
        let mut linked_items = HashMap::new();
        for entry in &timeline.entries {
            if let Some(link) = &entry.linked_item
                && let Some(target) = store.resolve_linked_item(account, link)?
            {
                linked_items.insert(entry.remote_key.clone(), target);
            }
        }
        let cached = store.item(account, item)?;
        let pr = if cached
            .as_ref()
            .is_some_and(|item| item.kind == threadline_storage::ItemKind::PullRequest)
        {
            pr::PullRequestData::read(store, account, tab)?
        } else {
            None
        };
        let write_choices = if cached
            .as_ref()
            .is_some_and(|item| item.kind == threadline_storage::ItemKind::Issue)
        {
            Some(store.write_choices(
                account,
                cached.as_ref().expect("cached issue").locator.repository,
            )?)
        } else {
            None
        };
        let editable_comments =
            outbox::editable_comments(store, account, item, actor, &discussion)?;
        Ok(ItemData {
            item: cached,
            pr,
            metadata: store.reader_item_metadata(account, item)?,
            anchor_missing,
            discussion,
            timeline,
            linked_items,
            projections: store.projections(account, item)?,
            write_choices,
            editable_comments,
            download: store.issue_download(account, item)?,
        })
    }
}

struct ItemView {
    data: ItemData,
    pr: Option<Box<pr::PullRequestView>>,
    body: Option<media::CachedMarkdown>,
    notes: Vec<(String, media::CachedMarkdown)>,
    scroll: ScrollHandle,
}

impl ItemView {
    fn use_cached_text(
        data: &mut ItemData,
        mut previous: Option<Self>,
        cx: &mut Context<ReaderWorkspace>,
    ) -> (
        Option<media::CachedMarkdown>,
        Vec<(String, media::CachedMarkdown)>,
    ) {
        let value = data.item.as_ref().map(|item| &item.body);
        let body = if let Some(previous) = previous.as_mut()
            && previous.data.item.as_ref().map(|item| &item.body) == value
        {
            previous.body.take()
        } else {
            match value {
                Some(Change::Value(markdown)) => Some(media::CachedMarkdown::new(markdown, cx)),
                _ => None,
            }
        };
        let notes = if let Some(mut previous) = previous
            && previous.data.discussion.state == data.discussion.state
            && previous.data.discussion.records == data.discussion.records
        {
            std::mem::swap(&mut data.discussion, &mut previous.data.discussion);
            previous.notes
        } else {
            data.discussion
                .records
                .iter()
                .map(|record| {
                    (
                        record.key.clone(),
                        media::CachedMarkdown::new(&record.body, cx),
                    )
                })
                .collect()
        };
        (body, notes)
    }
}

impl ReaderWorkspace {
    #[cfg(feature = "e2e")]
    pub(crate) fn observation(&self, cx: &gpui_kit::App) -> ReaderObservation {
        use sha2::{Digest, Sha256};
        let active = self
            .layout
            .panes
            .get(self.layout.focused_pane)
            .and_then(|pane| pane.active.and_then(|index| pane.tabs.get(index)));
        let draft = active
            .and_then(|tab| tab.view.draft.as_ref())
            .and_then(|id| self.drafts.get(id));
        let cached = active.and_then(|tab| self.tab_views.get(&tab.id));
        ReaderObservation {
            pane_count: self.layout.panes.len(),
            tab_counts: self
                .layout
                .panes
                .iter()
                .map(|pane| pane.tabs.len())
                .collect(),
            focused_pane: self.layout.focused_pane,
            active_item: active.map(|tab| tab.item.0),
            active_number: cached
                .and_then(|view| view.data.item.as_ref())
                .map(|item| item.locator.number),
            cached_content_ready: active
                .and_then(|tab| self.tab_views.get(&tab.id))
                .is_some_and(|view| view.data.item.is_some()),
            cached_visible_panes: self
                .layout
                .panes
                .iter()
                .filter(|pane| {
                    pane.active
                        .and_then(|index| pane.tabs.get(index))
                        .and_then(|tab| self.tab_views.get(&tab.id))
                        .is_some_and(|view| view.data.item.is_some())
                })
                .count(),
            search_results: self.search_hits.as_ref().map(Vec::len),
            search_generation: self.search_reply,
            performance_timing: self.performance.ready(
                active.map(|tab| tab.item.0),
                cached.is_some_and(|view| view.data.item.is_some()),
                self.search_reply,
            ),
            first_search_item: self
                .search_hits
                .as_ref()
                .and_then(|hits| hits.first())
                .map(|hit| hit.row.item.0),
            comments_coverage: cached
                .map(|view| format!("{:?}", view.data.discussion.state.coverage)),
            comment_fetch_state: cached
                .map(|view| format!("{:?}", view.data.discussion.state.fetch)),
            comment_count: cached.map(|view| view.notes.len()),
            download_state: cached
                .and_then(|view| view.data.download.as_ref())
                .map(|download| format!("{:?}", download.state)),
            active_draft_saved: draft.map(drafts::Editor::saved),
            active_draft_chars: draft.map(|draft| draft.view.read(cx).value().chars().count()),
            active_draft_sha256: draft.map(|draft| {
                let value = draft.view.read(cx).value();
                format!("{:x}", Sha256::digest(value.as_bytes()))
            }),
            signed_out: self.access == AccessState::SignedOut,
            offline: self.offline,
            shared_sync: self.sync_update.as_ref().map(|update| {
                let settings = self
                    .sync_panel
                    .as_ref()
                    .and_then(|panel| panel.read(cx).saved_settings());
                SyncObservation {
                    panel_open: self.surface == Surface::Sync,
                    schedule: settings.map(|settings| settings.schedule.clone()),
                    controls: settings.map(|settings| settings.controls),
                    cache_generation: update.cache_generation,
                    eligible_outbox: update.eligible_outbox,
                    connection: format!("{:?}", update.connection.status),
                    reason: update.connection.reason.map(|reason| format!("{reason:?}")),
                    pending_reads: update.status.as_ref().map(|status| status.pending),
                }
            }),
            pull_request: active.and_then(|tab| self.pr_observation(tab, cx)),
            pending_pr_reads: self.pr_reads.len(),
            history: (self.surface == Surface::History).then(|| self.history.observation()),
            notification: self.notification_observation(),
            outbox: self.counts.map(|counts| counts.outbox),
            outbox_visible: self.surface == Surface::Outbox,
            outbox_action_state: self.outbox.selected_state(),
            outbox_action_cancel_requested: self.outbox.cancel_requested(),
            outbox_paused: self.outbox.is_paused(),
            active_draft_submitted: draft
                .map(|draft| draft.submitted == Some(draft.durable_revision)),
        }
    }
    pub(crate) fn new(
        store: Arc<Store>,
        runtime: tokio::runtime::Handle,
        service: Option<crate::reader::ReaderService>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut field = |label| cx.new(|cx| InputState::new(window, cx).placeholder(label));
        let [
            search,
            url,
            saved_name,
            author,
            label,
            assignee,
            reviewer,
            dates_after,
            dates_before,
        ] = [
            field("Search downloaded content"),
            field("https://github.com/owner/repo/issues/123"),
            field("Saved view name"),
            field("Author"),
            field("Label"),
            field("Assignee"),
            field("Reviewer"),
            field("Updated after (YYYY-MM-DD)"),
            field("Updated before (YYYY-MM-DD)"),
        ];
        let mut reader = Self {
            store,
            runtime,
            service,
            workspace_visible: true,
            visible_subscriptions: None,
            sync_worker: None,
            sync_update: None,
            sync_started: false,
            network: Cancellation::new(),
            network_pending: 0,
            network_started: 0,
            network_observed: 0,
            remote_search: None,
            remote_generation: 0,
            remote_error: None,
            images: HashMap::new(),
            downloads: HashMap::new(),
            pr_reads: std::collections::HashSet::new(),
            pr_resume_attempts: std::collections::HashSet::new(),
            merge_views: HashMap::new(),
            account: None,
            login: String::new(),
            actor: None,
            access: AccessState::Unchecked,
            offline: false,
            reachability: Reachability::NotAttempted,
            checked: None,
            contact: None,
            repositories: Vec::new(),
            scope: ReaderScope::Selected,
            back: Vec::new(),
            forward: Vec::new(),
            layout: empty_workspace(),
            layout_busy: false,
            pending_tab_selection: None,
            surface_focus: cx.focus_handle(),
            focus: Vec::new(),
            focus_events: Vec::new(),
            maximized: false,
            tab_views: HashMap::new(),
            pending_tabs: HashMap::new(),
            tab_generation: 0,
            generation: 0,
            list_generation: 0,
            filter: BrowseFilter::default(),
            saved: Vec::new(),
            rows: Vec::new(),
            next: None,
            coverage: None,
            list_busy: false,
            search,
            search_hits: None,
            search_reply: None,
            #[cfg(feature = "e2e")]
            performance: performance::ReaderTiming::new(),
            url,
            saved_name,
            author,
            label,
            assignee,
            reviewer,
            dates_after,
            dates_before,
            counts: None,
            filters_open: false,
            surface: Surface::Browse,
            history: history_view::HistoryState::default(),
            activity: activity_view::ActivityState::default(),
            sync_panel: None,
            sync_watch: None,
            #[cfg(feature = "e2e")]
            sync_render_watch: None,
            cache: None,
            queue: Vec::new(),
            outbox: outbox::OutboxState::default(),
            drafts: HashMap::new(),
            pending_draft: None,
            review_editors: HashMap::new(),
            review_pending: HashSet::new(),
            editor_to_focus: None,
            departure: None,
            #[cfg(feature = "e2e")]
            observer: None,
            status: None,
        };
        reader.ensure_focus_handles(MAX_PANES, window, cx);
        reader.focus_events.push(
            cx.observe_global_in::<crate::activity_delivery::ActivityInbox>(
                window,
                |reader, window, cx| reader.drain_notification_inbox(window, cx),
            ),
        );
        reader.focus_events.push(cx.subscribe_in(
            &reader.search,
            window,
            |reader, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    reader.local_search(window, cx);
                }
            },
        ));
        reader.focus_events.push(cx.subscribe_in(
            &reader.url,
            window,
            |reader, _, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    reader.open_url(window, cx);
                }
            },
        ));
        reader
    }

    fn ensure_focus_handles(&mut self, count: usize, window: &mut Window, cx: &mut Context<Self>) {
        while self.focus.len() < count {
            let pane = self.focus.len();
            let focus = cx.focus_handle();
            self.focus_events
                .push(cx.on_focus_in(&focus, window, move |this, window, cx| {
                    this.remember_pane_focus(pane, window, cx);
                }));
            self.focus.push(focus);
        }
    }

    fn remember_pane_focus(&mut self, pane: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.layout_busy || self.layout.focused_pane == pane || pane >= self.layout.panes.len() {
            return;
        }
        self.capture_scroll();
        let mut layout = self.layout.clone();
        layout.focused_pane = pane;
        self.save_layout(layout, window, cx);
    }

    pub(crate) fn has_account(&self) -> bool {
        self.account.is_some()
    }

    pub(crate) fn focus_active(&self, window: &mut Window) {
        let focus = if self.surface == Surface::Browse {
            self.focus
                .get(self.layout.focused_pane)
                .unwrap_or(&self.surface_focus)
        } else {
            &self.surface_focus
        }
        .clone();
        window.on_next_frame(move |window, cx| focus.focus(window, cx));
    }

    pub(crate) fn set_account(
        &mut self,
        state: ReaderAccount,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ReaderAccount {
            account,
            profile,
            offline,
            reachability,
            repositories,
        } = state;
        let profile = profile.as_ref();
        let changed = self.account != account;
        let offline_changed = self.offline != offline;
        let previous_access = self.access;
        let can_fetch_before = self.can_fetch();
        if offline && !self.offline {
            self.network.cancel();
            self.network = Cancellation::new();
            self.network_pending = 0;
        }
        let scope_changed = !self
            .repositories
            .iter()
            .map(|repo| (repo.id, repo.selected))
            .eq(repositories.iter().map(|repo| (repo.id, repo.selected)));
        self.offline = offline;
        self.access = profile.map_or(AccessState::Unchecked, |item| item.access);
        self.actor = profile.map(|profile| profile.user_id);
        self.login = profile.map_or_else(String::new, |item| item.login.clone());
        if changed || can_fetch_before != self.can_fetch() {
            self.pr_resume_attempts.clear();
        }
        if !changed && !can_fetch_before && self.can_fetch() {
            self.resume_visible_pr_downloads(window, cx);
        }
        let checked = profile.and_then(|item| item.checked_at_ms);
        if changed || (reachability != Reachability::NotAttempted && checked >= self.checked) {
            self.reachability = reachability;
            self.checked = checked;
            if reachability == Reachability::ReceivedResponse {
                self.contact = checked.or(self.contact);
            }
        }
        if changed {
            self.network.cancel();
            self.network = Cancellation::new();
            self.network_pending = 0;
            self.sync_update = None;
            self.remote_search = None;
            self.remote_error = None;
            self.images.clear();
            self.downloads.clear();
            self.pr_reads.clear();
            self.merge_views.clear();
            self.release_drafts();
            self.review_editors.clear();
            self.review_pending.clear();
            self.account = account;
            self.generation += 1;
            self.contact = if reachability == Reachability::ReceivedResponse {
                checked
            } else {
                None
            };
            self.layout = empty_workspace();
            self.maximized = false;
            self.filter = BrowseFilter::default();
            self.surface = Surface::Browse;
            self.history = history_view::HistoryState::default();
            self.activity = activity_view::ActivityState::default();
            self.sync_panel = None;
            self.sync_watch = None;
            #[cfg(feature = "e2e")]
            {
                self.sync_render_watch = None;
            }
            self.rows.clear();
            self.queue.clear();
            self.outbox = outbox::OutboxState::default();
            self.tab_views.clear();
            self.pending_tabs.clear();
            self.pending_tab_selection = None;
            self.search_hits = None;
            self.search_reply = None;
            self.pending_draft = None;
            self.editor_to_focus = None;
            self.counts = None;
            self.coverage = None;
            self.cache = None;
            self.status = None;
            self.scope = ReaderScope::Selected;
            self.back.clear();
            self.forward.clear();
            self.layout_busy = account.is_some();
        }
        self.repositories = repositories;
        if changed {
            self.adopt_sync_snapshot(window, cx);
        }
        if let Some(worker) = self.sync_worker.as_ref() {
            if changed || offline_changed {
                worker.interrupt(None);
            } else if scope_changed || self.access != previous_access {
                worker.wake(None);
            }
        }
        if let Some(account) = account {
            if changed {
                self.launch(
                    window,
                    cx,
                    move |store| store.reader_layout(account),
                    |this, result, window, cx| {
                        this.layout_busy = false;
                        match result {
                            Ok(Some((layout, maximized))) => {
                                this.ensure_focus_handles(layout.panes.len(), window, cx);
                                this.layout = layout;
                                this.maximized = maximized.is_some();
                            }
                            Ok(None) => {}
                            Err(_) => {
                                this.status = Some("The saved workspace could not be loaded.")
                            }
                        }
                        this.load_visible(window, cx);
                        this.maybe_start_sync();
                        this.restore_active_draft(window, cx);
                        this.focus_active(window);
                        this.drain_notification_inbox(window, cx);
                        this.settle_departure(window, cx);
                    },
                );
            }
            if changed || scope_changed {
                self.browse(false, window, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn request_departure(
        &mut self,
        quit: bool,
        after: impl FnOnce(&mut Window, &mut gpui_kit::App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .departure
            .as_ref()
            .is_some_and(|departure| departure.quit || !quit)
        {
            return;
        }
        self.capture_drafts(cx);
        self.capture_review_editors(cx);
        self.departure = Some(Departure {
            quit,
            after: Box::new(after),
        });
        self.capture_history_scroll(window, cx);
        self.settle_departure(window, cx);
    }

    fn settle_departure(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.departure.is_none()
            || self.layout_busy
            || self.pending_draft.is_some()
            || !self.review_pending.is_empty()
            || self.merge_views.values().any(|merge| merge.busy)
            || self.history.busy
            || self.activity.busy
        {
            return;
        }
        if self.drafts.values().any(|editor| editor.failed)
            || self.review_editors.values().any(|editor| editor.failed)
        {
            self.departure.take();
            self.status = Some("A draft could not be saved. Your text is still in its editor.");
            cx.notify();
            return;
        }
        if self
            .drafts
            .values()
            .any(|editor| !editor.saved() || editor.submitting)
            || self
                .review_editors
                .values()
                .any(|editor| !editor.saved() || editor.sending)
        {
            self.status = Some("Saving drafts before leaving the workspace…");
            cx.notify();
            return;
        }
        let Some(account) = self.account else { return };
        self.capture_scroll();
        let layout = self.layout.clone();
        let maximized = self.maximized.then_some(layout.focused_pane);
        self.layout_busy = true;
        self.launch(
            window,
            cx,
            move |store| store.save_reader_layout(account, &layout, maximized),
            |this, result, window, cx| {
                this.layout_busy = false;
                if result.is_err() {
                    this.departure.take();
                    this.status =
                        Some("Workspace state could not be saved. Your tabs remain open.");
                    return;
                }
                this.capture_drafts(cx);
                this.capture_review_editors(cx);
                if this.drafts.values().any(|editor| !editor.saved())
                    || this
                        .review_editors
                        .values()
                        .any(|editor| !editor.saved() || editor.sending)
                {
                    this.settle_departure(window, cx);
                    return;
                }
                let Some(Departure { quit, after }) = this.departure.take() else {
                    return;
                };
                this.status = None;
                if quit {
                    let mut handles = this.stop_editors();
                    handles.extend(this.close_review_editors());
                    let visibility = this
                        .visible_subscriptions
                        .take()
                        .map(scheduled::VisibleSubscriptions::finish);
                    cx.spawn_in(window, async move |_, cx| {
                        for handle in handles {
                            let _ = handle.await;
                        }
                        if let Some(visibility) = visibility {
                            let _ = visibility.await;
                        }
                        let _ = cx.update(after);
                    })
                    .detach();
                } else {
                    window.defer(cx, after);
                }
            },
        );
    }

    fn launch<R, F, H>(&self, window: &Window, cx: &mut Context<Self>, operation: F, handle: H)
    where
        R: Send + 'static,
        F: FnOnce(&Store) -> Outcome<R> + Send + 'static,
        H: FnOnce(&mut Self, Outcome<R>, &mut Window, &mut Context<Self>) + 'static,
    {
        let store = Arc::clone(&self.store);
        let epoch = self.generation;
        let work = self.runtime.spawn_blocking(move || operation(&store));
        cx.spawn_in(window, async move |view, cx| {
            let result = work
                .await
                .unwrap_or(Err(threadline_storage::Error::Conflict(
                    "local operation interrupted",
                )));
            let _ = view.update_in(cx, move |this, window, cx| {
                if epoch == this.generation {
                    handle(this, result, window, cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn browse(&mut self, more: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        if more && (self.next.is_none() || self.list_busy) {
            return;
        }
        let after = more.then_some(self.next).flatten();
        self.surface = Surface::Browse;
        let scope = self.scope;
        let filter = self.filter.clone();
        self.list_generation += 1;
        let generation = self.list_generation;
        self.list_busy = true;
        self.search_hits = None;
        self.search_reply = None;
        self.launch(
            window,
            cx,
            move |store| {
                Ok((
                    store.browse_page(account, scope, &filter, after, PAGE)?,
                    store.sidebar_counts(account, scope)?,
                    store.reader_views(account)?,
                ))
            },
            move |this, result, _, _| {
                if generation != this.list_generation {
                    return;
                }
                this.list_busy = false;
                match result {
                    Ok((
                        BrowsePage {
                            items,
                            next,
                            coverage,
                            last_validated_at_ms,
                        },
                        counts,
                        saved,
                    )) => {
                        if !more {
                            this.rows.clear();
                        }
                        this.rows.extend(items);
                        this.next = next;
                        this.coverage = Some((coverage, last_validated_at_ms));
                        this.counts = Some(counts);
                        this.saved = saved;
                    }
                    Err(_) => {
                        this.status = Some("This view could not be read from the local cache.")
                    }
                }
            },
        );
        cx.notify();
    }

    fn local_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        let phrase = self.search.read(cx).value().to_string();
        if phrase.trim().is_empty() {
            self.browse(false, window, cx);
            return;
        }
        self.surface = Surface::Browse;
        self.list_generation += 1;
        let generation = self.list_generation;
        self.list_busy = true;
        self.search_reply = None;
        #[cfg(feature = "e2e")]
        self.performance
            .start(performance::Action::Search(generation));
        let scope = self.scope;
        self.launch(
            window,
            cx,
            move |store| store.search_downloaded(account, scope, &phrase, PAGE),
            move |this, result, _, _| {
                if generation != this.list_generation {
                    return;
                }
                this.list_busy = false;
                match result {
                    Ok(hits) => {
                        this.search_hits = Some(hits);
                        this.search_reply = Some(generation);
                    }
                    Err(_) => this.status = Some("The downloaded-content search could not finish."),
                }
            },
        );
        cx.notify();
    }

    fn show_outbox(&mut self, more: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        if self.surface != Surface::Outbox {
            self.surface = Surface::Outbox;
            self.surface_focus.focus(window, cx);
        }
        self.refresh_outbox_state(window, cx);
        self.outbox.queue_generation += 1;
        let generation = self.outbox.queue_generation;
        let after = if more {
            self.queue.last().map_or(0, |row| row.sequence)
        } else {
            0
        };
        self.launch(
            window,
            cx,
            move |store| store.queue_page(account, after, PAGE),
            move |this, result, _, _| {
                if this.outbox.queue_generation != generation {
                    return;
                }
                match result {
                    Ok(rows) => {
                        if !more {
                            this.queue.clear();
                        }
                        this.queue.extend(rows);
                    }
                    Err(_) => this.status = Some("The local Outbox could not be read."),
                }
            },
        );
        cx.notify();
    }

    fn show_cache(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        self.surface = Surface::Storage;
        self.surface_focus.focus(window, cx);
        self.launch(
            window,
            cx,
            move |store| store.cache_usage(account),
            |this, result, _, _| {
                this.cache = result.ok();
            },
        );
        cx.notify();
    }

    fn show_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        if self.sync_panel.is_none() {
            let store = Arc::clone(&self.store);
            let runtime = self.runtime.clone();
            let panel =
                cx.new(|cx| crate::sync_panel::SyncPanel::new(store, runtime, account, window, cx));
            #[cfg(feature = "e2e")]
            if let Some(observer) = self.observer.as_ref() {
                panel.update(cx, |panel, _| panel.set_observer(Some(observer.clone())));
                self.sync_render_watch = Some(cx.observe(&panel, |_, _, cx| cx.notify()));
            }
            self.sync_watch = Some(cx.subscribe_in(
                &panel,
                window,
                |this, _, change: &crate::sync_panel::SettingsChanged, window, cx| {
                    let offline = change.current.work_offline;
                    let offline_changed = change.previous.work_offline != offline;
                    if offline_changed {
                        this.pr_resume_attempts.clear();
                    }
                    if offline && !this.offline {
                        this.network.cancel();
                        this.network = Cancellation::new();
                    }
                    this.offline = offline;
                    if offline_changed && !offline {
                        this.resume_visible_pr_downloads(window, cx);
                    }
                    if let Some(worker) = this.sync_worker.as_ref() {
                        if offline_changed {
                            worker.interrupt((!offline).then_some(
                                threadline_core::connectivity::ProbeTrigger::NetworkChanged,
                            ));
                        } else if !change.previous.pause_background
                            && change.current.pause_background
                        {
                            worker.interrupt(None);
                        } else {
                            worker.wake(None);
                        }
                    }
                    window.dispatch_action(Box::new(crate::workspace::ReloadLocalAccount), cx);
                    cx.notify();
                },
            ));
            self.sync_panel = Some(panel);
        }
        self.surface = Surface::Sync;
        self.surface_focus.focus(window, cx);
        cx.notify();
    }

    fn load_visible(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tabs: Vec<_> = self
            .layout
            .panes
            .iter()
            .filter_map(|pane| pane.active.and_then(|i| pane.tabs.get(i)))
            .cloned()
            .collect();
        for tab in tabs {
            if !self.tab_views.contains_key(&tab.id) && !self.pending_tabs.contains_key(&tab.id) {
                self.load_tab(tab, window, cx);
            }
        }
    }

    fn prepare_tab_read(&mut self, tab: &Tab) -> u64 {
        self.tab_generation += 1;
        self.pending_tabs
            .insert(tab.id.clone(), self.tab_generation);
        self.tab_generation
    }

    fn load_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        let actor = self.actor;
        let request = self.prepare_tab_read(&tab);
        let source = tab.clone();
        self.launch(
            window,
            cx,
            move |store| ItemData::read(store, account, actor, &source),
            move |this, result, window, cx| this.apply_tab_read(&tab, request, result, window, cx),
        );
    }

    fn apply_tab_read(
        &mut self,
        requested: &Tab,
        request: u64,
        result: Outcome<ItemData>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = requested.id.clone();
        let item = requested.item;
        if self.pending_tabs.get(&id) != Some(&request) {
            return;
        }
        self.pending_tabs.remove(&id);
        let Some(tab) = self
            .layout
            .panes
            .iter()
            .flat_map(|pane| &pane.tabs)
            .find(|tab| tab.id == id && tab.item == item)
            .cloned()
        else {
            return;
        };
        match result {
            Ok(mut data) => {
                let mut old = self.tab_views.remove(&id);
                let scroll = old
                    .as_ref()
                    .map_or_else(ScrollHandle::new, |view| view.scroll.clone());
                if old.is_none() && tab.view.scroll_anchor.is_some() && !data.anchor_missing {
                    let discussion = 5
                        + usize::from(data.metadata.is_some())
                        + usize::from(data.discussion.state.error.is_some());
                    scroll.scroll_to_item(discussion);
                } else if old.is_none() && tab.view.scroll_offset > 0 {
                    let handle = scroll.clone();
                    window.on_next_frame(move |_, _| {
                        handle.set_offset(point(px(0.), px(-(tab.view.scroll_offset as f32))))
                    });
                }
                if let Some(set) = data.pr.as_ref().and_then(|pr| pr.references.as_ref()) {
                    for current in self
                        .layout
                        .panes
                        .iter_mut()
                        .flat_map(|pane| &mut pane.tabs)
                        .filter(|tab| tab.item == item)
                    {
                        if let Some(context) = current.view.context.as_mut() {
                            pr::refresh_context(set, context);
                        }
                    }
                }
                let prior = old.as_mut().and_then(|view| view.pr.take().map(|pr| *pr));
                let pr = data
                    .pr
                    .take()
                    .map(|data| Box::new(pr::PullRequestView::new(data, &tab, prior, window, cx)));
                let (body, notes) = ItemView::use_cached_text(&mut data, old, cx);
                self.tab_views.insert(
                    id,
                    ItemView {
                        data,
                        pr,
                        body,
                        notes,
                        scroll,
                    },
                );
                self.trim_inactive_views();
                self.restore_review(tab, window, cx);
                self.resume_visible_pr_downloads(window, cx);
            }
            Err(_) => self.status = Some("The cached item could not be loaded."),
        }
        self.maybe_start_sync();
    }

    fn capture_scroll(&mut self) {
        for tab in self.layout.panes.iter_mut().flat_map(|pane| &mut pane.tabs) {
            if let Some(view) = self.tab_views.get(&tab.id) {
                tab.view.scroll_offset = (-view.scroll.offset().y.as_f32()).max(0.) as u64;
                if let Some(pr_view) = view.pr.as_ref() {
                    let settings = tab.view.pr_reader.get_or_insert_with(Default::default);
                    if pr_view.context_scroll.bounds().size.height > px(0.) {
                        settings.context_scroll =
                            (-pr_view.context_scroll.offset().y.as_f32()).max(0.) as u32;
                    }
                    if pr_view.issue_scroll.bounds().size.height > px(0.) {
                        settings.issue_scroll =
                            (-pr_view.issue_scroll.offset().y.as_f32()).max(0.) as u32;
                    }
                }
            }
        }
    }

    fn save_layout(&mut self, layout: Workspace, window: &mut Window, cx: &mut Context<Self>) {
        self.save_layout_with_tab(layout, None, false, None, window, cx);
    }

    fn save_layout_with_tab(
        &mut self,
        layout: Workspace,
        target: Option<Tab>,
        show_cached: bool,
        recent: Option<ItemId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        if self.layout_busy {
            return;
        }
        self.layout_busy = true;
        let state = layout.clone();
        let maximized = self.maximized.then_some(layout.focused_pane);
        let optimistic = show_cached
            && target.as_ref().is_some_and(|tab| {
                !self.pending_tabs.contains_key(&tab.id)
                    && self
                        .tab_views
                        .get(&tab.id)
                        .is_some_and(|view| view.data.item.is_some())
            });
        let previous = optimistic.then(|| std::mem::replace(&mut self.layout, layout.clone()));
        let preload = target
            .filter(|tab| {
                !self.tab_views.contains_key(&tab.id) && !self.pending_tabs.contains_key(&tab.id)
            })
            .map(|tab| {
                let request = self.prepare_tab_read(&tab);
                (tab, request)
            });
        let pending = preload
            .as_ref()
            .map(|(tab, request)| (tab.id.clone(), *request));
        let actor = self.actor;
        self.launch(
            window,
            cx,
            move |store| {
                store.save_reader_layout(account, &state, maximized)?;
                Ok(preload.map(|(tab, request)| {
                    let result = ItemData::read(store, account, actor, &tab);
                    (tab, request, result)
                }))
            },
            move |this, result, window, cx| {
                this.layout_busy = false;
                let editor_to_focus = this.editor_to_focus.take();
                match result {
                    Ok(preloaded) => {
                        let reloading = layout
                            .panes
                            .iter()
                            .filter_map(|pane| pane.active.and_then(|index| pane.tabs.get(index)))
                            .filter(|tab| {
                                let previous = previous
                                    .as_ref()
                                    .unwrap_or(&this.layout)
                                    .panes
                                    .iter()
                                    .flat_map(|pane| &pane.tabs)
                                    .find(|old| old.id == tab.id);
                                previous.is_some_and(|old| {
                                    let old_pr = pr::tab_settings(old);
                                    let new_pr = pr::tab_settings(tab);
                                    old_pr.section != new_pr.section
                                        || old_pr.selected_path != new_pr.selected_path
                                        || old.view.context != tab.view.context
                                })
                            })
                            .cloned()
                            .collect::<Vec<_>>();
                        this.layout = layout;
                        this.tab_views.retain(|id, _| {
                            this.layout
                                .panes
                                .iter()
                                .any(|pane| pane.tabs.iter().any(|tab| tab.id == *id))
                        });
                        this.merge_views.retain(|id, _| {
                            this.layout
                                .panes
                                .iter()
                                .any(|pane| pane.tabs.iter().any(|tab| tab.id == *id))
                        });
                        this.drafts.retain(|_, editor| {
                            this.layout
                                .panes
                                .iter()
                                .any(|pane| pane.tabs.iter().any(|tab| tab.id == editor.tab_id))
                        });
                        this.review_editors.retain(|_, editor| {
                            this.layout
                                .panes
                                .iter()
                                .any(|pane| pane.tabs.iter().any(|tab| tab.id == editor.tab_id))
                        });
                        for tab in reloading {
                            this.load_tab(tab, window, cx);
                        }
                        this.load_visible(window, cx);
                        if let Some((tab, request, result)) = preloaded {
                            this.apply_tab_read(&tab, request, result, window, cx);
                        }
                        if let Some(item) = recent {
                            let store = Arc::clone(&this.store);
                            this.runtime.spawn_blocking(move || {
                                store.record_recent_item(account, item, now_ms())
                            });
                        }
                        this.restore_active_draft(window, cx);
                        this.finish_notification_item(window, cx);
                        if let Some(editor) = editor_to_focus.and_then(|id| this.drafts.get(&id)) {
                            let focus = editor.view.focus_handle(cx);
                            window.on_next_frame(move |window, cx| focus.focus(window, cx));
                        }
                    }
                    Err(_) => {
                        if let Some(previous) = previous {
                            this.layout = previous;
                            this.focus_active(window);
                        }
                        if let Some((tab, request)) = pending
                            && this.pending_tabs.get(&tab) == Some(&request)
                        {
                            this.pending_tabs.remove(&tab);
                        }
                        this.status =
                            Some("Workspace changes could not be saved. Your tabs are still open.")
                    }
                }
                this.apply_pending_tab_selection(window, cx);
                this.settle_departure(window, cx);
            },
        );
        cx.notify();
    }

    fn open(
        &mut self,
        item: ItemId,
        anchor: Option<String>,
        beside: bool,
        duplicate: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.layout_busy {
            return;
        }
        let current = self.active_tab().map(|tab| tab.item);
        self.capture_scroll();
        let mut layout = self.layout.clone();
        let index = if beside {
            if layout.panes.len() == 1 {
                layout.panes.push(Pane {
                    active: None,
                    tabs: Vec::new(),
                });
                layout.proportions = vec![0.5; 2];
            }
            (layout.focused_pane + 1) % layout.panes.len()
        } else {
            layout.focused_pane
        };
        let pane = &mut layout.panes[index];
        let existing = if duplicate {
            None
        } else {
            pane.active
                .filter(|i| pane.tabs[*i].item == item)
                .or_else(|| pane.tabs.iter().rposition(|tab| tab.item == item))
        };
        let tab = if let Some(existing) = existing {
            pane.active = Some(existing);
            &mut pane.tabs[existing]
        } else {
            pane.tabs.push(Tab {
                id: view_id(),
                item,
                view: TabView::default(),
            });
            pane.active = Some(pane.tabs.len() - 1);
            pane.tabs.last_mut().expect("just added a tab")
        };
        let preserve_view = anchor.is_none();
        if let Some(anchor) = anchor {
            tab.view.scroll_anchor = Some(anchor);
            self.tab_views.remove(&tab.id);
        }
        let target = tab.clone();
        layout.focused_pane = index;
        self.surface = Surface::Browse;
        if let Some(previous) = current.filter(|previous| *previous != item) {
            push_history(&mut self.back, previous);
            self.forward.clear();
        }
        self.save_layout_with_tab(
            layout,
            Some(target),
            existing.is_some() && preserve_view,
            Some(item),
            window,
            cx,
        );
    }

    fn active_tab(&self) -> Option<&Tab> {
        let pane = self.layout.panes.get(self.layout.focused_pane)?;
        pane.tabs.get(pane.active?)
    }

    fn navigate_history(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.layout_busy {
            return;
        }
        let Some(current) = self.active_tab().map(|tab| tab.item) else {
            return;
        };
        let (source, target) = if forward {
            (&mut self.forward, &mut self.back)
        } else {
            (&mut self.back, &mut self.forward)
        };
        let Some(item) = source.pop() else { return };
        push_history(target, current);
        self.capture_scroll();
        let mut layout = self.layout.clone();
        let pane = &mut layout.panes[layout.focused_pane];
        let index = pane
            .tabs
            .iter()
            .rposition(|tab| tab.item == item)
            .unwrap_or_else(|| {
                pane.tabs.push(Tab {
                    id: view_id(),
                    item,
                    view: TabView::default(),
                });
                pane.tabs.len() - 1
            });
        pane.active = Some(index);
        self.surface = Surface::Browse;
        self.save_layout(layout, window, cx);
    }

    fn reopen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        if self.layout_busy {
            return;
        }
        self.layout_busy = true;
        self.launch(
            window,
            cx,
            move |store| {
                store.reopen_last_tab(account, None)?;
                store.workspace(account)
            },
            |this, result, window, cx| {
                this.layout_busy = false;
                if let Ok(Some(layout)) = result {
                    this.layout = layout;
                    this.load_visible(window, cx);
                    this.restore_active_draft(window, cx);
                }
                this.apply_pending_tab_selection(window, cx);
                this.settle_departure(window, cx);
            },
        );
    }
}

fn empty_workspace() -> Workspace {
    Workspace {
        focused_pane: 0,
        proportions: vec![1.],
        panes: vec![Pane {
            active: None,
            tabs: Vec::new(),
        }],
    }
}

fn push_history(history: &mut Vec<ItemId>, item: ItemId) {
    if history.last() == Some(&item) {
        return;
    }
    if history.len() >= 100 {
        history.remove(0);
    }
    history.push(item);
}

fn view_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "view-{}-{}-{}",
        std::process::id(),
        now_ms(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_millis()).ok())
        .unwrap_or_default()
}

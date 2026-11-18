use std::{collections::HashMap, sync::Arc};

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

mod drafts;
mod media;
mod remote;
#[cfg(all(test, feature = "e2e"))]
mod tests;
mod view;
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
    pub cached_content_ready: bool,
    pub search_results: Option<usize>,
    pub search_generation: Option<u64>,
    pub first_search_item: Option<i64>,
    pub comments_coverage: Option<String>,
    pub comment_fetch_state: Option<String>,
    pub comment_count: Option<usize>,
    pub download_state: Option<String>,
    pub active_draft_saved: Option<bool>,
    pub active_draft_chars: Option<usize>,
    pub active_draft_sha256: Option<String>,
    pub signed_out: bool,
    pub offline: bool,
    pub outbox: Option<u64>,
}

pub(crate) struct ReaderWorkspace {
    store: Arc<Store>,
    runtime: tokio::runtime::Handle,
    service: Option<crate::reader::ReaderService>,
    network: Cancellation,
    network_pending: usize,
    network_started: u64,
    network_observed: u64,
    remote_search: Option<remote::SearchResults>,
    remote_generation: u64,
    remote_error: Option<String>,
    images: HashMap<media::ImageKey, media::ImageState>,
    downloads: HashMap<ItemId, Cancellation>,
    account: Option<AccountId>,
    login: String,
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
    cache: Option<CacheUsage>,
    queue: Vec<QueueRow>,
    drafts: HashMap<String, drafts::Editor>,
    pending_draft: Option<String>,
    editor_to_focus: Option<String>,
    departure: Option<Departure>,
    status: Option<&'static str>,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Surface {
    Browse,
    Outbox,
    Storage,
}

struct ItemData {
    item: Option<Item>,
    metadata: Option<ReaderItemMetadata>,
    anchor_missing: bool,
    discussion: DiscussionPage,
    timeline: TimelinePage,
    linked_items: HashMap<String, ItemId>,
    projections: Vec<Projection>,
    download: Option<IssueDownload>,
}

struct ItemView {
    data: ItemData,
    body: Option<media::CachedMarkdown>,
    notes: Vec<(String, media::CachedMarkdown)>,
    scroll: ScrollHandle,
    _scroll_restore: u64,
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
            cached_content_ready: active
                .and_then(|tab| self.tab_views.get(&tab.id))
                .is_some_and(|view| view.data.item.is_some()),
            search_results: self.search_hits.as_ref().map(Vec::len),
            search_generation: self.search_reply,
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
            outbox: self.counts.map(|counts| counts.outbox),
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
            network: Cancellation::new(),
            network_pending: 0,
            network_started: 0,
            network_observed: 0,
            remote_search: None,
            remote_generation: 0,
            remote_error: None,
            images: HashMap::new(),
            downloads: HashMap::new(),
            account: None,
            login: String::new(),
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
            cache: None,
            queue: Vec::new(),
            drafts: HashMap::new(),
            pending_draft: None,
            editor_to_focus: None,
            departure: None,
            status: None,
        };
        reader.ensure_focus_handles(MAX_PANES, window, cx);
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
                    if this.layout_busy
                        || this.layout.focused_pane == pane
                        || pane >= this.layout.panes.len()
                    {
                        return;
                    }
                    this.capture_scroll();
                    let mut layout = this.layout.clone();
                    layout.focused_pane = pane;
                    this.save_layout(layout, window, cx);
                }));
            self.focus.push(focus);
        }
    }

    pub(crate) fn has_account(&self) -> bool {
        self.account.is_some()
    }

    pub(crate) fn focus_active(&self, window: &mut Window) {
        if let Some(focus) = self.focus.get(self.layout.focused_pane) {
            let focus = focus.clone();
            window.on_next_frame(move |window, cx| focus.focus(window, cx));
        }
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
        self.login = profile.map_or_else(String::new, |item| item.login.clone());
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
            self.remote_search = None;
            self.remote_error = None;
            self.images.clear();
            self.downloads.clear();
            self.release_drafts();
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
            self.rows.clear();
            self.queue.clear();
            self.tab_views.clear();
            self.pending_tabs.clear();
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
                        this.restore_active_draft(window, cx);
                        this.focus_active(window);
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
        self.departure = Some(Departure {
            quit,
            after: Box::new(after),
        });
        self.settle_departure(window, cx);
    }

    fn settle_departure(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.departure.is_none() || self.layout_busy || self.pending_draft.is_some() {
            return;
        }
        if self.drafts.values().any(|editor| editor.failed) {
            self.departure.take();
            self.status = Some("A draft could not be saved. Your text is still in its editor.");
            cx.notify();
            return;
        }
        if self.drafts.values().any(|editor| !editor.saved()) {
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
                if this.drafts.values().any(|editor| !editor.saved()) {
                    this.settle_departure(window, cx);
                    return;
                }
                let Some(Departure { quit, after }) = this.departure.take() else {
                    return;
                };
                this.status = None;
                if quit {
                    let handles = this.stop_editors();
                    cx.spawn_in(window, async move |_, cx| {
                        for handle in handles {
                            let _ = handle.await;
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
        self.surface = Surface::Outbox;
        let after = if more {
            self.queue.last().map_or(0, |row| row.sequence)
        } else {
            0
        };
        self.launch(
            window,
            cx,
            move |store| store.queue_page(account, after, PAGE),
            move |this, result, _, _| match result {
                Ok(rows) => {
                    if !more {
                        this.queue.clear();
                    }
                    this.queue.extend(rows);
                }
                Err(_) => this.status = Some("The local Outbox could not be read."),
            },
        );
        cx.notify();
    }

    fn show_cache(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        self.surface = Surface::Storage;
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

    fn load_tab(&mut self, tab: Tab, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        let item = tab.item;
        let id = tab.id.clone();
        self.tab_generation += 1;
        let request = self.tab_generation;
        self.pending_tabs.insert(id.clone(), request);
        self.launch(
            window,
            cx,
            move |store| {
                let (discussion, anchor_missing) = match tab.view.scroll_anchor.as_deref() {
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
                Ok(ItemData {
                    item: store.item(account, item)?,
                    metadata: store.reader_item_metadata(account, item)?,
                    anchor_missing,
                    discussion,
                    timeline,
                    linked_items,
                    projections: store.projections(account, item)?,
                    download: store.issue_download(account, item)?,
                })
            },
            move |this, result, window, cx| {
                if this.pending_tabs.get(&id) != Some(&request) {
                    return;
                }
                this.pending_tabs.remove(&id);
                let Some(tab) = this
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
                    Ok(data) => {
                        let old = this.tab_views.remove(&id);
                        let scroll = old
                            .as_ref()
                            .map_or_else(ScrollHandle::new, |view| view.scroll.clone());
                        if old.is_none() && tab.view.scroll_anchor.is_some() && !data.anchor_missing
                        {
                            let discussion = 5 + usize::from(data.metadata.is_some());
                            scroll.scroll_to_item(discussion);
                        } else if old.is_none() && tab.view.scroll_offset > 0 {
                            let handle = scroll.clone();
                            window.on_next_frame(move |_, _| {
                                handle
                                    .set_offset(point(px(0.), px(-(tab.view.scroll_offset as f32))))
                            });
                        }
                        let body = match data.item.as_ref().map(|row| &row.body) {
                            Some(Change::Value(markdown)) => {
                                Some(media::CachedMarkdown::new(markdown, cx))
                            }
                            _ => None,
                        };
                        let notes = data
                            .discussion
                            .records
                            .iter()
                            .map(|record| {
                                (
                                    record.key.clone(),
                                    media::CachedMarkdown::new(&record.body, cx),
                                )
                            })
                            .collect();
                        this.tab_views.insert(
                            id,
                            ItemView {
                                data,
                                body,
                                notes,
                                scroll,
                                _scroll_restore: tab.view.scroll_offset,
                            },
                        );
                    }
                    Err(_) => this.status = Some("The cached item could not be loaded."),
                }
            },
        );
    }

    fn capture_scroll(&mut self) {
        for tab in self.layout.panes.iter_mut().flat_map(|pane| &mut pane.tabs) {
            if let Some(view) = self.tab_views.get(&tab.id) {
                tab.view.scroll_offset = (-view.scroll.offset().y.as_f32()).max(0.) as u64;
            }
        }
    }

    fn save_layout(&mut self, layout: Workspace, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        if self.layout_busy {
            return;
        }
        self.layout_busy = true;
        let state = layout.clone();
        let maximized = self.maximized.then_some(layout.focused_pane);
        self.launch(
            window,
            cx,
            move |store| store.save_reader_layout(account, &state, maximized),
            move |this, result, window, cx| {
                this.layout_busy = false;
                let editor_to_focus = this.editor_to_focus.take();
                match result {
                    Ok(()) => {
                        this.layout = layout;
                        this.tab_views.retain(|id, _| {
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
                        this.load_visible(window, cx);
                        this.restore_active_draft(window, cx);
                        if let Some(editor) = editor_to_focus.and_then(|id| this.drafts.get(&id)) {
                            let focus = editor.view.focus_handle(cx);
                            window.on_next_frame(move |window, cx| focus.focus(window, cx));
                        }
                    }
                    Err(_) => {
                        this.status =
                            Some("Workspace changes could not be saved. Your tabs are still open.")
                    }
                }
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
        if let Some(anchor) = anchor {
            tab.view.scroll_anchor = Some(anchor);
            self.tab_views.remove(&tab.id);
        }
        layout.focused_pane = index;
        self.surface = Surface::Browse;
        if let Some(previous) = current.filter(|previous| *previous != item) {
            push_history(&mut self.back, previous);
            self.forward.clear();
        }
        if let Some(account) = self.account {
            let store = Arc::clone(&self.store);
            self.runtime
                .spawn_blocking(move || store.record_recent_item(account, item, now_ms()));
        }
        self.save_layout(layout, window, cx);
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

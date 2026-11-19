use super::{
    pr::{FileSummary, parse_patch},
    view::{control, time_label},
    *,
};
use crate::palette::Palette;
use gpui_kit::{
    AnyElement, InteractiveElement, IntoElement, ParentElement, PathBuilder, Role,
    StatefulInteractiveElement, Styled, canvas,
    component::{
        Disableable, Selectable,
        diff::{Diff, DiffState},
        input::Input,
    },
    div,
};
use threadline_storage::{
    RepositoryId,
    history::{
        ComparisonOptions, HistoryComparison, HistoryCoverage, HistoryDiffPage, HistoryMonitoring,
        HistoryPage, HistoryRef, HistoryRefState, HistoryRow, HistoryView,
    },
    pr::PrRecord,
    resources::{CommitOid, ComparisonMode},
};

const VIEW_ID: &str = "reader-commit-history";
const GRAPH_LIMIT: u16 = 48;
const ROW_H: f32 = 58.;

pub(super) struct HistoryState {
    pub busy: bool,
    fetching: bool,
    refreshing: bool,
    refresh_pending: bool,
    repository: Option<RepositoryId>,
    request: u64,
    scroll: ScrollHandle,
    data: Option<HistoryData>,
    diff: Option<VisibleDiff>,
    monitoring_input: Option<Entity<InputState>>,
}
impl Default for HistoryState {
    fn default() -> Self {
        Self {
            busy: false,
            fetching: false,
            refreshing: false,
            refresh_pending: false,
            repository: None,
            request: 0,
            scroll: ScrollHandle::new(),
            data: None,
            diff: None,
            monitoring_input: None,
        }
    }
}

#[cfg(all(test, feature = "e2e"))]
impl HistoryState {
    pub(super) fn cached_graph_rows(&self) -> usize {
        self.data
            .as_ref()
            .and_then(|data| data.page.as_ref())
            .map_or(0, |page| page.rows.len())
    }

    pub(super) fn cached_ref_tip(&self, reference: &HistoryRef) -> Option<Option<&CommitOid>> {
        self.data
            .as_ref()?
            .refs
            .iter()
            .find(|entry| &entry.reference == reference)
            .map(|entry| entry.tip.as_ref())
    }

    pub(super) fn plan(&self) -> Option<&HistoryComparison> {
        self.data
            .as_ref()?
            .comparison
            .as_ref()
            .map(|(plan, _)| plan)
    }
}

struct HistoryData {
    refs: Vec<HistoryRefState>,
    monitoring: HistoryMonitoring,
    generation: Option<i64>,
    view: Option<HistoryView>,
    page: Option<HistoryPage>,
    after: Option<i64>,
    comparison: Option<(HistoryComparison, HistoryDiffPage)>,
}

struct VisibleDiff {
    state: Entity<DiffState>,
    path: String,
    comparison: HistoryComparison,
    proof: threadline_storage::pr::PatchEvidence,
}

#[cfg(feature = "e2e")]
#[derive(serde::Serialize)]
pub(crate) struct HistoryObservation {
    pub repository: Option<i64>,
    pub refs: usize,
    pub commits: usize,
    pub coverage: Option<String>,
    pub unloaded_parents: usize,
    pub selected: Option<String>,
    pub comparison: Option<&'static str>,
    pub comparison_base: Option<String>,
    pub comparison_head: Option<String>,
    pub comparison_coverage: Option<String>,
    pub parent: Option<usize>,
    pub files: usize,
    pub displayed_file: Option<String>,
}

#[cfg(feature = "e2e")]
impl HistoryState {
    pub(super) fn observation(&self) -> HistoryObservation {
        let data = self.data.as_ref();
        let view = data.and_then(|data| data.view.as_ref());
        let page = data.and_then(|data| data.page.as_ref());
        let comparison = data.and_then(|data| data.comparison.as_ref());
        let (comparison_kind, parent) = match comparison.map(|(plan, _)| plan) {
            Some(HistoryComparison::Parent { parent_index, .. }) => {
                (Some("Parent"), Some(parent_index + 1))
            }
            Some(HistoryComparison::Root { .. }) => (Some("Root"), None),
            Some(HistoryComparison::Pair { .. }) => (Some("Pair"), None),
            None => (None, None),
        };
        HistoryObservation {
            repository: self.repository.map(|id| id.0),
            refs: data.map_or(0, |data| data.refs.len()),
            commits: page.map_or(0, |page| page.rows.len()),
            coverage: page.map(|page| format!("{:?}", page.coverage)),
            unloaded_parents: page.map_or(0, |page| {
                page.rows
                    .iter()
                    .flat_map(|row| &row.parents)
                    .filter(|edge| !edge.loaded)
                    .count()
            }),
            selected: view
                .and_then(|view| view.selected.as_ref())
                .map(|oid| oid.as_str().to_owned()),
            comparison: comparison_kind,
            comparison_base: comparison
                .and_then(|(_, page)| page.identity.base.as_ref())
                .map(|oid| oid.as_str().to_owned()),
            comparison_head: comparison.map(|(_, page)| page.identity.head.as_str().to_owned()),
            comparison_coverage: comparison.map(|(_, page)| format!("{:?}", page.coverage)),
            parent,
            files: comparison.map_or(0, |(_, page)| page.files.len()),
            displayed_file: self.diff.as_ref().map(|diff| diff.path.clone()),
        }
    }
}

#[derive(Clone)]
enum ChangeHistory {
    Reference(HistoryRef, CommitOid),
    Select(CommitOid),
    CompareWith(CommitOid),
    Parent(CommitOid),
    File(String),
    Page(Option<i64>),
}

#[derive(Clone)]
enum HistoryFetch {
    References(Option<HistoryRef>),
    Walk(HistoryRef),
    Comparison(HistoryComparison),
}

enum MonitorHistory {
    Branch(String),
    Pattern(String),
    Pull(std::num::NonZeroU64),
}

fn bounded_history_refs(
    store: &Store,
    account: AccountId,
    repository: RepositoryId,
    inspected: Option<&HistoryRef>,
) -> Outcome<Vec<HistoryRefState>> {
    const REF_LIMIT: u16 = 100;
    let mut refs = store.history_refs(account, repository, None, REF_LIMIT)?;
    if let Some(reference) = inspected
        && !refs.iter().any(|entry| &entry.reference == reference)
        && let Some(inspected) = store.history_ref(account, repository, reference)?
    {
        refs.insert(0, inspected);
        refs.truncate(usize::from(REF_LIMIT));
    }
    Ok(refs)
}

impl HistoryData {
    fn load(
        store: &Store,
        account: AccountId,
        repository: RepositoryId,
        request: Option<(Option<i64>, HistoryView, Option<i64>)>,
    ) -> Outcome<Self> {
        let saved = if request.is_none() {
            store.history_view(account, repository, VIEW_ID)?
        } else {
            None
        };
        let inspected = request
            .as_ref()
            .map(|(_, view, _)| &view.reference)
            .or_else(|| saved.as_ref().map(|saved| &saved.state.reference));
        let refs = bounded_history_refs(store, account, repository, inspected)?;
        let monitoring = store.history_monitoring(account, repository)?;
        let (generation, view, after) = if let Some((generation, view, after)) = request {
            (generation, Some(view), after)
        } else if let Some(saved) = saved {
            let after = match saved.state.scroll_anchor.as_ref() {
                Some(anchor) => store
                    .history_ordinal(account, repository, &saved.state.tip, anchor)?
                    .map(|ordinal| ordinal.saturating_sub((ordinal % i64::from(GRAPH_LIMIT)) + 1)),
                None => None,
            };
            (Some(saved.generation), Some(saved.state), after)
        } else {
            let first = monitoring
                .default_branch
                .as_ref()
                .and_then(|default| {
                    refs.iter().find(|entry| {
                        entry.reference == HistoryRef::Branch(default.clone())
                            && entry.tip.is_some()
                    })
                })
                .or_else(|| refs.iter().find(|entry| entry.tip.is_some()));
            (
                None,
                first.and_then(|entry| {
                    entry
                        .tip
                        .clone()
                        .map(|tip| new_view(entry.reference.clone(), tip))
                }),
                None,
            )
        };
        let page = view
            .as_ref()
            .map(|view| store.history_page(account, repository, &view.tip, after, GRAPH_LIMIT))
            .transpose()?;
        let comparison = view
            .as_ref()
            .and_then(|view| view.selected.as_ref().map(|oid| (view, oid)))
            .map(|(view, selected)| -> Outcome<_> {
                let options = ComparisonOptions {
                    ignore_whitespace: view.ignore_whitespace,
                    context_lines: view.context_lines,
                };
                let comparison = if let Some(second) = view.second.as_ref() {
                    store.comparison_between(
                        account,
                        repository,
                        second,
                        selected,
                        ComparisonMode::Direct,
                        options,
                    )?
                } else {
                    store.comparison_for_parent(
                        account,
                        repository,
                        selected,
                        view.parent.as_ref(),
                        options,
                    )?
                };
                let files = if let Some(file) = view.file.as_deref() {
                    store.history_diff_page_at_file(account, &comparison, file, 25)?
                } else {
                    store.history_diff_page(account, &comparison, None, 25)?
                };
                Ok((comparison, files))
            })
            .transpose()?;
        Ok(Self {
            refs,
            monitoring,
            generation,
            view,
            page,
            after,
            comparison,
        })
    }
}

fn new_view(reference: HistoryRef, tip: CommitOid) -> HistoryView {
    HistoryView {
        reference,
        tip,
        selected: None,
        second: None,
        parent: None,
        scroll_anchor: None,
        scroll_offset: 0,
        file: None,
        file_scroll: 0,
        ignore_whitespace: false,
        context_lines: 3,
    }
}

fn ref_name(reference: &HistoryRef) -> String {
    match reference {
        HistoryRef::Branch(name) => format!("Branch {name}"),
        HistoryRef::Tag(name) => format!("Tag {name}"),
        HistoryRef::FollowedPr(number) => format!("Followed PR #{number}"),
    }
}

impl ReaderWorkspace {
    fn fetch_history(
        &mut self,
        repository: RepositoryId,
        fetch: HistoryFetch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.history.repository != Some(repository)
            || self.history.fetching
            || !self.can_fetch()
            || self.service.is_none()
        {
            return;
        }
        self.history.fetching = true;
        let finished = fetch.clone();
        self.remote(
            window,
            cx,
            move |service, account, cancel| async move {
                let page = match fetch {
                    HistoryFetch::References(inspected) => {
                        service
                            .fetch_history_refs(account, repository, inspected.as_ref(), &cancel)
                            .await?
                    }
                    HistoryFetch::Walk(reference) => {
                        service
                            .fetch_history_page(account, repository, &reference, &cancel)
                            .await?
                    }
                    HistoryFetch::Comparison(comparison) => {
                        service.fetch_history_diff(&comparison, &cancel).await?
                    }
                };
                Ok((page, page.contacted))
            },
            move |this, result, window, cx| {
                this.history.fetching = false;
                if this.history.repository != Some(repository) {
                    return;
                }
                let same_route = match &finished {
                    HistoryFetch::References(_) => true,
                    HistoryFetch::Walk(reference) => this
                        .history
                        .data
                        .as_ref()
                        .and_then(|data| data.view.as_ref())
                        .is_some_and(|view| &view.reference == reference),
                    HistoryFetch::Comparison(comparison) => this
                        .history
                        .data
                        .as_ref()
                        .and_then(|data| data.comparison.as_ref())
                        .is_some_and(|(current, _)| current == comparison),
                };
                if let Some(state) = result.filter(|_| same_route) {
                    let profile = this.history.data.as_ref().and_then(|data| {
                        data.view.as_ref().cloned().map(|mut view| {
                            remember_history_scroll(&this.history, &mut view);
                            (data.generation, view, data.after)
                        })
                    });
                    this.status = Some(if state.has_more {
                        "Downloaded this page. More provider history is available."
                    } else if state.complete {
                        "GitHub finished the requested history resource."
                    } else {
                        "The provider could supply only part of the requested history."
                    });
                    this.load_history(repository, profile, window, cx);
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    pub(super) fn open_activity_comparison(
        &mut self,
        repository: RepositoryId,
        reference: String,
        base: CommitOid,
        head: CommitOid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(account) = self.account.filter(|_| !self.history.busy) else {
            return false;
        };
        if !self
            .repositories
            .iter()
            .any(|entry| entry.id == repository && entry.selected)
        {
            return false;
        }
        let Some(reference) = HistoryRef::from_key(&reference)
            .filter(|candidate| candidate.key().is_ok_and(|key| key == reference))
        else {
            return false;
        };
        self.surface = Surface::History;
        self.history.repository = Some(repository);
        self.history.request += 1;
        let generation = self.history.request;
        self.history.busy = true;
        self.history.data = None;
        self.history.diff = None;
        self.launch(
            window,
            cx,
            move |store| {
                let view_generation = store
                    .history_view(account, repository, VIEW_ID)?
                    .map(|saved| saved.generation);
                let mut view = new_view(reference, head.clone());
                view.selected = Some(head);
                view.second = Some(base);
                HistoryData::load(
                    store,
                    account,
                    repository,
                    Some((view_generation, view, None)),
                )
            },
            move |this, result, window, cx| {
                if this.history.request != generation || this.history.repository != Some(repository)
                {
                    return;
                }
                this.history.busy = false;
                match result {
                    Ok(data) => {
                        this.set_history_diff(&data, cx);
                        this.history.data = Some(data);
                        this.history.scroll.set_offset(point(px(0.), px(0.)));
                    }
                    Err(_) => {
                        this.show_activity(false, window, cx);
                        this.status = Some(
                            "The notification comparison is unavailable for the saved commit pair.",
                        );
                    }
                }
                this.complete_notification_open(window, cx);
                this.settle_departure(window, cx);
            },
        );
        true
    }

    pub(super) fn show_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.surface = Surface::History;
        if self.history.monitoring_input.is_none() {
            self.history.monitoring_input = Some(cx.new(|cx| InputState::new(window, cx)));
        }
        let selected = match self.scope {
            ReaderScope::Repository(repository) => Some(repository),
            ReaderScope::Selected => self.history.repository.or_else(|| {
                self.repositories
                    .iter()
                    .find(|repo| repo.selected)
                    .map(|repo| repo.id)
            }),
        };
        if let Some(repository) = selected {
            self.load_history(repository, None, window, cx);
        } else {
            cx.notify();
        }
    }

    fn load_history(
        &mut self,
        repository: RepositoryId,
        request: Option<(Option<i64>, HistoryView, Option<i64>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        if !self
            .repositories
            .iter()
            .any(|repo| repo.id == repository && repo.selected)
            || self.history.busy
        {
            return;
        }
        if self.history.repository != Some(repository) {
            self.history.data = None;
            self.history.diff = None;
            self.history.scroll.set_offset(point(px(0.), px(0.)));
        }
        self.history.repository = Some(repository);
        self.history.request += 1;
        let generation = self.history.request;
        self.history.busy = true;
        self.launch(window, cx, move |store| {
            let persist = request.is_some();
            let mut data = HistoryData::load(store, account, repository, request)?;
            if persist && let Some(view) = data.view.as_mut() {
                if view.scroll_anchor.is_none() {
                    view.scroll_anchor = data.page.as_ref().and_then(|page| page.rows.first())
                        .map(|row| row.entry.commit.oid.clone());
                }
                data.generation = Some(store.save_history_view(account, repository, VIEW_ID, data.generation, view)?);
            }
            Ok(data)
        }, move |this, outcome, window, cx| {
            if this.history.request != generation || this.history.repository != Some(repository) {
                return;
            }
            this.history.busy = false;
            match outcome {
                Ok(data) => {
                    let offset = data.view.as_ref().map_or(0, |view| view.scroll_offset);
                    this.set_history_diff(&data, cx);
                    this.history.data = Some(data);
                    let scroll = this.history.scroll.clone();
                    window.on_next_frame(move |_, _| scroll.set_offset(point(px(0.), -px(offset as f32))));
                }
                Err(_) => this.status = Some("History could not be loaded or saved. The previous view remains available."),
            }
            if std::mem::take(&mut this.history.refresh_pending) {
                this.refresh_cached_history(window, cx);
            }
            this.settle_departure(window, cx);
            this.drain_notification_inbox(window, cx);
            cx.notify();
        });
        cx.notify();
    }

    pub(super) fn refresh_cached_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface != Surface::History {
            return;
        }
        let (Some(account), Some(repository)) = (self.account, self.history.repository) else {
            return;
        };
        if self.history.busy || self.history.refreshing {
            self.history.refresh_pending = true;
            return;
        }
        let Some(data) = self.history.data.as_ref() else {
            return;
        };
        let Some((tip, inspected)) = data
            .view
            .as_ref()
            .map(|view| (view.tip.clone(), view.reference.clone()))
        else {
            self.load_history(repository, None, window, cx);
            return;
        };
        let after = data.after;
        let request = self.history.request;
        self.history.refreshing = true;
        self.launch(
            window,
            cx,
            move |store| {
                let refs = bounded_history_refs(store, account, repository, Some(&inspected))?;
                let page = store.history_page(account, repository, &tip, after, GRAPH_LIMIT)?;
                Ok((refs, page))
            },
            move |this, result, window, cx| {
                this.history.refreshing = false;
                if this.surface == Surface::History
                    && this.history.repository == Some(repository)
                    && this.history.request == request
                    && let Ok((refs, page)) = result
                    && let Some(data) = this.history.data.as_mut()
                {
                    data.refs = refs;
                    data.page = Some(page);
                }
                if std::mem::take(&mut this.history.refresh_pending) {
                    this.refresh_cached_history(window, cx);
                }
            },
        );
    }

    fn set_history_diff(&mut self, data: &HistoryData, cx: &mut Context<Self>) {
        let next = data.comparison.as_ref().and_then(|(comparison, page)| {
            let view = data.view.as_ref()?;
            let file = view.file.as_deref()?;
            let selected = page.files.iter().find_map(|row| match &row.record {
                PrRecord::File {
                    path,
                    previous_path,
                    status,
                    additions,
                    deletions,
                    patch,
                } if path == file => Some((
                    FileSummary {
                        path: path.clone(),
                        previous_path: previous_path.clone(),
                        status: status.clone(),
                        additions: *additions,
                        deletions: *deletions,
                        viewed: false,
                    },
                    patch,
                )),
                _ => None,
            })?;
            Some((comparison, file, selected))
        });
        if let Some((comparison, path, (summary, patch))) = next {
            if self.history.diff.as_ref().is_some_and(|diff| {
                &diff.comparison == comparison && diff.path == path && &diff.proof == patch
            }) {
                return;
            }
            self.history.diff = parse_patch(&summary, patch).0.map(|diff| VisibleDiff {
                state: cx.new(|cx| DiffState::new([diff], cx)),
                path: path.to_owned(),
                comparison: comparison.clone(),
                proof: patch.clone(),
            });
        } else {
            self.history.diff = None;
        }
    }

    fn history_action(
        &mut self,
        action: ChangeHistory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (Some(repository), Some(data)) = (self.history.repository, self.history.data.as_ref())
        else {
            return;
        };
        if self.history.busy {
            return;
        }
        let mut after = data.after;
        let mut view = match action {
            ChangeHistory::Reference(reference, tip) => {
                after = None;
                new_view(reference, tip)
            }
            other => {
                let Some(mut view) = data.view.clone() else {
                    return;
                };
                remember_history_scroll(&self.history, &mut view);
                match other {
                    ChangeHistory::Select(oid) => {
                        view.selected = Some(oid);
                        view.second = None;
                        view.parent = None;
                        view.file = None;
                    }
                    ChangeHistory::CompareWith(oid) => {
                        view.second = Some(oid);
                        view.parent = None;
                        view.file = None;
                    }
                    ChangeHistory::Parent(oid) => {
                        view.second = None;
                        view.parent = Some(oid);
                        view.file = None;
                    }
                    ChangeHistory::File(path) => {
                        view.file = Some(path);
                        view.file_scroll = 0;
                    }
                    ChangeHistory::Page(request) => {
                        after = request;
                        view.scroll_anchor = None;
                        view.scroll_offset = 0;
                    }
                    ChangeHistory::Reference(..) => unreachable!(),
                }
                view
            }
        };
        if view.second == view.selected {
            view.second = None;
        }
        let generation = data.generation;
        self.load_history(repository, Some((generation, view, after)), window, cx);
    }

    pub(super) fn capture_history_scroll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.surface != Surface::History || self.history.busy {
            return;
        }
        let (Some(repository), Some(data)) = (self.history.repository, self.history.data.as_ref())
        else {
            return;
        };
        let Some(mut view) = data.view.clone() else {
            return;
        };
        remember_history_scroll(&self.history, &mut view);
        if data.view.as_ref() != Some(&view) {
            self.load_history(
                repository,
                Some((data.generation, view, data.after)),
                window,
                cx,
            );
        }
    }

    pub(super) fn history_surface(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let mut header = div().flex().flex_wrap().gap_1().p_2().child(
            control("history-back", "Back to workspace").on_click(cx.listener(
                |this, _, window, cx| {
                    this.capture_history_scroll(window, cx);
                    this.browse(false, window, cx);
                },
            )),
        );
        for (index, repo) in self
            .repositories
            .iter()
            .filter(|repo| repo.selected)
            .enumerate()
        {
            let repository = repo.id;
            let id = format!("history-repository-{index}");
            let button = control(id.clone(), format!("{}/{}", repo.owner, repo.name))
                .selected(self.history.repository == Some(repository))
                .disabled(self.history.busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.load_history(repository, None, window, cx)
                }));
            header = header.child(self.observed_control(id, button, false));
        }
        if let Some(repository) = self.history.repository {
            let inspected = self
                .history
                .data
                .as_ref()
                .and_then(|data| data.view.as_ref())
                .map(|view| view.reference.clone());
            header = header.child(self.history_download_control(
                "history-fetch-refs",
                "Download branches and tags",
                repository,
                HistoryFetch::References(inspected),
                cx,
            ));
        }
        let Some(data) = self.history.data.as_ref() else {
            return div()
                .flex_1()
                .child(header)
                .child(if self.history.busy {
                    "Loading downloaded history…"
                } else {
                    "No selected repository history is available."
                })
                .into_any_element();
        };
        let mut refs = div().flex().flex_wrap().gap_1().p_2();
        for (index, entry) in data.refs.iter().enumerate() {
            let Some(tip) = entry.tip.as_ref() else {
                refs = refs.child(
                    div()
                        .id(format!("history-ref-deleted-{index}"))
                        .accessibility_id(format!("history-ref-deleted-{index}"))
                        .role(Role::Status)
                        .aria_label(format!("{} deleted", ref_name(&entry.reference)))
                        .text_xs()
                        .child(format!("{} deleted", ref_name(&entry.reference))),
                );
                continue;
            };
            let reference = entry.reference.clone();
            let tip = tip.clone();
            let id = format!("history-ref-{index}");
            let button = control(id.clone(), ref_name(&reference))
                .selected(
                    data.view
                        .as_ref()
                        .is_some_and(|view| view.reference == reference && view.tip == tip),
                )
                .disabled(self.history.busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.history_action(
                        ChangeHistory::Reference(reference.clone(), tip.clone()),
                        window,
                        cx,
                    )
                }));
            refs = refs.child(self.observed_control(id, button, false));
        }
        refs = refs.child(self.history_monitoring_controls(data, cx));
        if let Some(view) = data.view.as_ref()
            && let Some(current) = data
                .refs
                .iter()
                .find(|entry| entry.reference == view.reference)
            && current.tip.as_ref() != Some(&view.tip)
        {
            let description = if let Some(latest) = &current.tip {
                format!(
                    "Showing saved tip {}. Select this branch or tag again to view its new tip {}.",
                    view.tip.as_str(),
                    latest.as_str()
                )
            } else {
                format!(
                    "The remote reference was removed. Its saved commits at {} remain available.",
                    view.tip.as_str()
                )
            };
            refs = refs.child(
                div()
                    .id("history-inspected-ref-status")
                    .accessibility_id("history-inspected-ref-status")
                    .role(Role::Status)
                    .aria_label(description.clone())
                    .text_xs()
                    .text_color(palette.muted)
                    .child(description),
            );
        }
        let Some(page) = data.page.as_ref() else {
            return div()
                .flex_1()
                .child(header)
                .child(refs)
                .child("This repository has no downloaded branch or tag references.")
                .into_any_element();
        };
        let graph = self.history_graph(data, page, palette, cx);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(header)
            .child(refs)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(graph)
                    .child(self.history_comparison(data, palette, cx)),
            )
            .into_any_element()
    }

    fn history_graph(
        &self,
        data: &HistoryData,
        page: &HistoryPage,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut body = div().relative();
        let lanes = page
            .rows
            .iter()
            .map(|row| row.lane)
            .chain(
                page.rows
                    .iter()
                    .flat_map(|row| row.parents.iter().map(|parent| parent.lane)),
            )
            .max()
            .unwrap_or(0);
        let width = 30. + lanes as f32 * 12.;
        body = body.child(
            graph_edges(&page.rows, palette)
                .absolute()
                .top_0()
                .left_0()
                .w(px(width))
                .h_full(),
        );
        for row in &page.rows {
            let commit = &row.entry.commit;
            let id = format!("history-commit-{}", row.ordinal);
            let oid = commit.oid.clone();
            let selected = data.view.as_ref().and_then(|view| view.selected.as_ref()) == Some(&oid);
            let button = control(
                id.clone(),
                format!("{} · {}", &oid.as_str()[..12], commit.title),
            )
            .selected(selected)
            .disabled(self.history.busy)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.history_action(ChangeHistory::Select(oid.clone()), window, cx)
            }));
            let labels = row
                .labels
                .iter()
                .map(ref_name)
                .collect::<Vec<_>>()
                .join(" · ");
            let unloaded = row.parents.iter().filter(|parent| !parent.loaded).count();
            let mut row_element = div()
                .h(px(ROW_H))
                .relative()
                .pl(px(width))
                .py_1()
                .border_b_1()
                .border_color(palette.border)
                .flex()
                .flex_col()
                .justify_center()
                .min_w_0()
                .child(
                    div()
                        .absolute()
                        .top(px(ROW_H * 0.5 - 4.))
                        .left(px(10. + row.lane as f32 * 12.))
                        .size(px(8.))
                        .rounded_full()
                        .bg(if row.lane == 0 {
                            palette.good
                        } else {
                            palette.accent
                        }),
                )
                .child(self.observed_control(id, button, false))
                .child(div().text_xs().text_color(palette.muted).child(format!(
                    "{} · {}{}{}",
                    row.entry.author.as_deref().unwrap_or("Unknown author"),
                    time_label(row.entry.authored_at_ms.or(Some(commit.committed_at_ms))),
                    if labels.is_empty() {
                        String::new()
                    } else {
                        format!(" · {labels}")
                    },
                    if unloaded == 0 {
                        String::new()
                    } else {
                        format!(" · {unloaded} parent links beyond the downloaded history")
                    },
                )));
            if data
                .view
                .as_ref()
                .is_some_and(|view| view.selected.is_some() && !selected)
            {
                let id = format!("history-compare-{}", row.ordinal);
                let base = commit.oid.clone();
                let button = control(id.clone(), "Use as comparison base")
                    .disabled(self.history.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.history_action(ChangeHistory::CompareWith(base.clone()), window, cx)
                    }));
                row_element = row_element.child(self.observed_control(id, button, false));
            }
            body = body.child(row_element);
        }
        let mut pagination = div().flex().flex_wrap().gap_1();
        if data.after.is_some() {
            pagination = pagination.child(
                control("history-first-page", "Newest downloaded commits")
                    .disabled(self.history.busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.history_action(ChangeHistory::Page(None), window, cx);
                    })),
            );
        }
        if let Some(after) = page.next_ordinal {
            pagination = pagination.child(
                control("history-older-page", "Older downloaded commits")
                    .disabled(self.history.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.history_action(ChangeHistory::Page(Some(after)), window, cx);
                    })),
            );
        }
        if let (Some(repository), Some(view)) = (self.history.repository, data.view.as_ref()) {
            let label = if page.rows.is_empty() {
                "Download commit history"
            } else {
                "Check branch tip or fetch older history"
            };
            pagination = pagination.child(self.history_download_control(
                "history-fetch-older",
                label,
                repository,
                HistoryFetch::Walk(view.reference.clone()),
                cx,
            ));
        }
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .p_2()
            .child(
                div()
                    .id("history-coverage")
                    .role(Role::Status)
                    .child(format!(
                        "{} · {}",
                        coverage_label(page.coverage),
                        time_label(page.fetched_at_ms)
                    )),
            )
            .child(pagination)
            .child(
                div()
                    .id("history-graph")
                    .role(Role::Group)
                    .aria_label("Commit ancestry")
                    .overflow_y_scroll()
                    .overflow_x_scroll()
                    .track_scroll(&self.history.scroll)
                    .flex_1()
                    .min_h_0()
                    .child(body),
            )
            .into_any_element()
    }

    fn history_comparison(
        &self,
        data: &HistoryData,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some((comparison, page)) = data.comparison.as_ref() else {
            return div()
                .flex_1()
                .p_3()
                .child("Select a commit to view its changes.")
                .into_any_element();
        };
        let mut panel = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .border_l_1()
            .border_color(palette.border);
        let summary = match comparison {
            HistoryComparison::Root { commit, .. } => format!("Root commit {}", commit.as_str()),
            HistoryComparison::Parent {
                key,
                parent_index,
                parent_count,
            } => format!(
                "Parent {} of {parent_count} · {} → {}",
                parent_index + 1,
                key.base.as_str(),
                key.head.as_str()
            ),
            HistoryComparison::Pair { key } => {
                format!(
                    "Direct comparison · {} → {}",
                    key.base.as_str(),
                    key.head.as_str()
                )
            }
        };
        panel = panel
            .child(
                div()
                    .id("history-comparison-mode")
                    .accessibility_id("history-comparison-mode")
                    .role(Role::Status)
                    .aria_label(summary.clone())
                    .text_sm()
                    .child(summary),
            )
            .child(
                div()
                    .id("history-comparison-coverage")
                    .accessibility_id("history-comparison-coverage")
                    .role(Role::Status)
                    .aria_label(coverage_label(page.coverage))
                    .child(coverage_label(page.coverage)),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(palette.muted)
                    .child("The stored patch retains whitespace and original source lines."),
            );
        if let Some(repository) = self.history.repository
            && page.coverage != HistoryCoverage::Complete
        {
            panel = panel.child(self.history_download_control(
                "history-fetch-comparison",
                "Download comparison files",
                repository,
                HistoryFetch::Comparison(comparison.clone()),
                cx,
            ));
        }
        if let Some(parents) = data
            .view
            .as_ref()
            .and_then(|view| view.selected.as_ref())
            .and_then(|selected| {
                data.page
                    .as_ref()?
                    .rows
                    .iter()
                    .find(|row| &row.entry.commit.oid == selected)
            })
            .map(|row| &row.entry.commit.parents)
        {
            let mut controls = div().flex().flex_wrap().gap_1();
            for (index, parent) in parents.iter().enumerate().take(16) {
                let id = format!("history-parent-{index}");
                let target = parent.clone();
                let button = control(
                    id.clone(),
                    format!("Parent {} · {}", index + 1, &parent.as_str()[..12]),
                )
                .disabled(self.history.busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.history_action(ChangeHistory::Parent(target.clone()), window, cx)
                }));
                controls = controls.child(self.observed_control(id, button, false));
            }
            panel = panel.child(controls);
        }
        let mut files = div().flex().flex_wrap().gap_1();
        for row in &page.files {
            let PrRecord::File { path, status, .. } = &row.record else {
                continue;
            };
            let id = format!("history-file-{}", row.ordinal);
            let selected = data.view.as_ref().and_then(|view| view.file.as_deref()) == Some(path);
            let target = path.clone();
            let button = control(id.clone(), format!("{status} · {path}"))
                .selected(selected)
                .disabled(self.history.busy)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.history_action(ChangeHistory::File(target.clone()), window, cx);
                }));
            files = files.child(self.observed_control(id, button, false));
        }
        panel = panel.child(files);
        if let Some(diff) = &self.history.diff {
            panel = panel.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(Diff::new(&diff.state).size_full()),
            );
        } else if data.view.as_ref().is_some_and(|view| view.file.is_some()) {
            panel = panel.child("A text patch is unavailable for this file.");
        }
        panel.into_any_element()
    }

    fn history_download_control(
        &self,
        id: &str,
        label: &str,
        repository: RepositoryId,
        fetch: HistoryFetch,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let button = control(id.to_owned(), label.to_owned())
            .disabled(self.history.fetching || self.history.busy || !self.can_fetch())
            .on_click(cx.listener(move |this, _, window, cx| {
                this.fetch_history(repository, fetch.clone(), window, cx);
            }));
        self.observed_control(id, button, false)
    }

    fn history_monitoring_controls(
        &self,
        data: &HistoryData,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut choices = div().flex().flex_wrap().gap_1().items_center();
        let Some(repository) = self.history.repository else {
            return choices.into_any_element();
        };
        if let Some(view) = data.view.as_ref()
            && let HistoryRef::Branch(name) = &view.reference
        {
            let name = name.clone();
            let always = data.monitoring.default_branch.as_ref() == Some(&name);
            let watched = data.monitoring.branches.contains(&name);
            let button = control(
                "history-monitor-branch",
                if always {
                    "Monitoring default branch"
                } else if watched {
                    "Stop monitoring branch"
                } else {
                    "Monitor this branch"
                },
            )
            .disabled(always || self.history.busy)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.change_history_monitoring(
                    repository,
                    MonitorHistory::Branch(name.clone()),
                    window,
                    cx,
                );
            }));
            choices = choices.child(self.observed_control("history-monitor-branch", button, false));
        }
        if let Some(field) = self.history.monitoring_input.as_ref() {
            choices = choices.child(
                div().w(px(155.)).child(
                    Input::new(field)
                        .accessibility_id("history-monitor-input")
                        .aria_label("Branch pattern or pull request number"),
                ),
            );
            let br = field.clone();
            choices = choices.child(
                control("history-monitor-pattern", "Toggle branch pattern")
                    .disabled(self.history.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.change_history_monitoring(
                            repository,
                            MonitorHistory::Pattern(br.read(cx).value().trim().to_owned()),
                            window,
                            cx,
                        );
                    })),
            );
            let pr = field.clone();
            choices = choices.child(
                control("history-monitor-pr", "Toggle followed PR")
                    .disabled(self.history.busy)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Ok(number) = pr.read(cx).value().trim().parse() {
                            this.change_history_monitoring(
                                repository,
                                MonitorHistory::Pull(number),
                                window,
                                cx,
                            );
                        } else {
                            this.status = Some("Enter the positive pull request number to follow.");
                            cx.notify();
                        }
                    })),
            );
        }
        choices
            .child(div().text_xs().child(format!(
                "{} chosen branches · {} branch patterns · {} followed PRs",
                data.monitoring.branches.len(),
                data.monitoring.branch_patterns.len(),
                data.monitoring.followed_prs.len()
            )))
            .into_any_element()
    }

    fn change_history_monitoring(
        &mut self,
        repository: RepositoryId,
        action: MonitorHistory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self
            .account
            .filter(|_| self.history.repository == Some(repository) && !self.history.busy)
        else {
            return;
        };
        self.history.busy = true;
        self.launch(window, cx, move |store| {
            let mut options = store.history_monitoring(account, repository)?;
            match action {
                MonitorHistory::Branch(name) => toggle(&mut options.branches, name),
                MonitorHistory::Pattern(pattern) => toggle(&mut options.branch_patterns, pattern),
                MonitorHistory::Pull(number) => toggle(&mut options.followed_prs, number),
            }
            store.set_history_monitoring(account, repository, &options)?;
            Ok(options)
        }, move |this, result, window, cx| {
            this.history.busy = false;
            if this.history.repository == Some(repository) {
                match result {
                    Ok(options) => {
                        if let Some(data) = this.history.data.as_mut() { data.monitoring = options; }
                        if let Some(worker) = this.sync_worker.as_ref() { worker.wake(None); }
                    }
                    Err(_) => this.status = Some("The branch pattern or history monitoring choice could not be saved."),
                }
            }
            this.settle_departure(window, cx);
        });
    }
}

fn toggle<T: Ord>(set: &mut std::collections::BTreeSet<T>, member: T) {
    if !set.remove(&member) {
        set.insert(member);
    }
}

fn remember_history_scroll(history: &HistoryState, view: &mut HistoryView) {
    view.scroll_offset = (-history.scroll.offset().y.as_f32()).max(0.) as u32;
    if let Some(row) = history
        .data
        .as_ref()
        .and_then(|data| data.page.as_ref())
        .and_then(|page| page.rows.get((view.scroll_offset as f32 / ROW_H) as usize))
    {
        view.scroll_anchor = Some(row.entry.commit.oid.clone());
    }
}

fn coverage_label(coverage: HistoryCoverage) -> &'static str {
    match coverage {
        HistoryCoverage::Missing => "Not downloaded",
        HistoryCoverage::Partial => "Partial download",
        HistoryCoverage::Complete => "Complete for this selection",
        HistoryCoverage::Limited => "Provider limit reached",
    }
}

fn graph_edges(rows: &[HistoryRow], palette: Palette) -> impl IntoElement + Styled + use<> {
    let row_count = rows.len();
    let positions = rows
        .iter()
        .enumerate()
        .map(|(index, row)| (row.entry.commit.oid.clone(), index))
        .collect::<HashMap<_, _>>();
    let positions = &positions;
    let edges = rows
        .iter()
        .enumerate()
        .flat_map(|(index, row)| {
            row.parents.iter().map(move |parent| {
                (
                    index,
                    row.lane,
                    positions.get(&parent.oid).copied(),
                    parent.lane,
                    parent.loaded,
                )
            })
        })
        .collect::<Vec<_>>();
    canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let anchor = |row: usize, lane: usize| {
                point(
                    bounds.left() + px(14. + lane as f32 * 12.),
                    bounds.top() + px(ROW_H * (row as f32 + 0.5)),
                )
            };
            for &(index, lane, parent, parent_lane, loaded) in &edges {
                let start = anchor(index, lane);
                let end = parent.map_or_else(
                    || anchor(row_count, parent_lane),
                    |row| anchor(row, parent_lane),
                );
                let mut path = PathBuilder::stroke(px(1.5));
                path.move_to(start);
                let bend = start.y + (end.y - start.y) * 0.5;
                path.cubic_bezier_to(end, point(start.x, bend), point(end.x, bend));
                if let Ok(line) = path.build() {
                    window.paint_path(
                        line,
                        if !loaded {
                            palette.muted
                        } else if lane == 0 && parent_lane == 0 {
                            palette.good
                        } else {
                            palette.accent
                        },
                    );
                }
            }
        },
    )
}

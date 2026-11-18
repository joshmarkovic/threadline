use super::*;
use gpui_kit::component::diff::{
    DiffFile, DiffLinePosition, DiffMode, DiffParseOptions, DiffSide, DiffState,
};
use threadline_storage::{
    pr::{
        BodyReference, NarrowSurface, PatchEvidence, PrContextMode, PrDiffMode, PrDownload,
        PrFamily, PrHeader, PrPage, PrRecord, PrRevision, PrSection, PrTabView, ReviewAnchor,
        SourceSide, Verification,
    },
    queue::PrContext,
    resources::ResourceState,
};

pub(super) struct PullRequestData {
    pub revision: PrRevision,
    pub header: PrHeader,
    body: Change<String>,
    pub references: Option<(String, Vec<BodyReference>)>,
    pub linked: Option<LinkedIssue>,
    pub remembered_context: Option<PrContext>,
    pub files: Option<FilePage>,
    pub pages: Vec<(PrFamily, PrPage)>,
    pub download: Option<PrDownload>,
    pub merge_policy: Option<threadline_storage::merges::CachedMergePolicy>,
    pub merge_status: Vec<threadline_storage::merges::MergeStatus>,
}

pub(super) struct LinkedIssue {
    pub item: Item,
    pub metadata: Option<ReaderItemMetadata>,
    pub discussion: DiscussionPage,
}

pub(super) struct FilePage {
    pub rows: Vec<FileSummary>,
    pub next: Option<String>,
    pub state: ResourceState,
    pub selected: Option<SelectedFile>,
}

#[derive(Clone)]
pub(super) struct FileSummary {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub viewed: bool,
}

pub(super) struct SelectedFile {
    pub summary: FileSummary,
    pub patch: Option<DiffFile>,
    pub problem: Option<&'static str>,
    evidence: PatchEvidence,
}

pub(super) struct PullRequestView {
    pub data: PullRequestData,
    pub body: Option<media::CachedMarkdown>,
    pub linked_body: Option<media::CachedMarkdown>,
    pub linked_comments: Vec<media::CachedMarkdown>,
    pub context_scroll: ScrollHandle,
    pub issue_scroll: ScrollHandle,
    pub diff: Option<Entity<DiffState>>,
}

impl PullRequestData {
    pub fn read(store: &Store, account: AccountId, tab: &Tab) -> Outcome<Option<Self>> {
        let Some((revision, header)) = store.current_pr_revision(account, tab.item)? else {
            return Ok(None);
        };
        let view = tab.view.pr_reader.as_ref().cloned().unwrap_or_default();
        let (body, references) = store.pr_revision_context(&revision)?;
        let remembered_context = store.pr_context(account, tab.item)?;
        let target = tab
            .view
            .context
            .as_ref()
            .or(remembered_context.as_ref())
            .and_then(|context| {
                let mut current = context.clone();
                let set = references.as_ref()?;
                refresh_context(set, &mut current);
                verified_reference(set, &current)
            });
        let linked = if let Some(target) = target
            && let Some(item) = store.item(account, target)?
            && item.kind == threadline_storage::ItemKind::Issue
        {
            Some(LinkedIssue {
                item,
                metadata: store.reader_item_metadata(account, target)?,
                discussion: store.discussion_page(account, target, None, 20)?,
            })
        } else {
            None
        };
        let families: &[PrFamily] = match view.section {
            PrSection::Conversation => &[PrFamily::Reviews, PrFamily::Threads],
            PrSection::Commits => &[PrFamily::Commits],
            PrSection::Files => &[PrFamily::Context, PrFamily::Threads],
            PrSection::Checks => &[PrFamily::Checks, PrFamily::Statuses],
        };
        let mut pages: Vec<(PrFamily, PrPage)> = families
            .iter()
            .map(|family| Ok((*family, store.pr_page(&revision, *family, None, PAGE)?)))
            .collect::<Outcome<_>>()?;
        if let Some(key) = tab
            .view
            .scroll_anchor
            .as_deref()
            .filter(|key| key.starts_with("thread:"))
            && let Some((_, threads)) = pages
                .iter_mut()
                .find(|(family, _)| *family == PrFamily::Threads)
        {
            if let Some(index) = threads
                .rows
                .iter()
                .position(|(existing, _)| existing == key)
            {
                let target = threads.rows.remove(index);
                threads.rows.insert(0, target);
            } else if let Some(target) = store.pr_record(&revision, PrFamily::Threads, key)? {
                threads.rows.insert(0, (key.to_owned(), target));
            }
        }
        let files = (view.section == PrSection::Files)
            .then(|| FilePage::read(store, &revision, view.selected_path.as_deref(), None))
            .transpose()?;
        let download = store.pr_download(&revision)?;
        let merge_policy = store.merge_policy(&revision)?;
        let merge_status = store.merge_statuses(account, tab.item)?;
        Ok(Some(Self {
            revision,
            header,
            body,
            references,
            linked,
            remembered_context,
            files,
            pages,
            download,
            merge_policy,
            merge_status,
        }))
    }
}

impl FilePage {
    pub fn read(
        store: &Store,
        revision: &PrRevision,
        selected: Option<&str>,
        after: Option<&str>,
    ) -> Outcome<Self> {
        let page = store.pr_page(revision, PrFamily::Files, after, 20)?;
        let viewed = store.pr_viewed_files(revision)?;
        let mut exact = None;
        let mut rows = Vec::with_capacity(page.rows.len());
        for (_, record) in page.rows {
            let Some((summary, patch)) = file_parts(record, &viewed) else {
                continue;
            };
            if selected.map_or(rows.is_empty(), |selected| selected == summary.path) {
                exact = Some((summary.clone(), patch));
            }
            rows.push(summary);
        }
        if exact.is_none()
            && let Some(path) = selected
        {
            exact = store
                .pr_file(revision, path)?
                .and_then(|record| file_parts(record, &viewed));
        }
        let selected = exact.map(|(summary, evidence)| {
            let (patch, problem) = parse_patch(&summary, &evidence);
            SelectedFile {
                summary,
                patch,
                problem,
                evidence,
            }
        });
        Ok(Self {
            rows,
            next: page.next,
            state: page.state,
            selected,
        })
    }
}

fn file_parts(record: PrRecord, viewed: &[String]) -> Option<(FileSummary, PatchEvidence)> {
    let PrRecord::File {
        path,
        previous_path,
        status,
        additions,
        deletions,
        patch,
    } = record
    else {
        return None;
    };
    let viewed = viewed.iter().any(|candidate| candidate == &path);
    Some((
        FileSummary {
            path,
            previous_path,
            status,
            additions,
            deletions,
            viewed,
        },
        patch,
    ))
}

pub(super) fn parse_patch(
    summary: &FileSummary,
    evidence: &PatchEvidence,
) -> (Option<DiffFile>, Option<&'static str>) {
    let (fragment, truncated) = match evidence {
        PatchEvidence::Text(text) => (text.as_str(), false),
        PatchEvidence::Truncated(text) => (text.as_str(), true),
        PatchEvidence::Binary => return (None, Some("Binary file. No text patch is available.")),
        PatchEvidence::ApiCapped => {
            return (None, Some("GitHub omitted this patch at its API limit."));
        }
        PatchEvidence::Unavailable => {
            return (
                None,
                Some("GitHub did not supply a text patch for this file."),
            );
        }
    };
    let left = if summary.status == "added" {
        "/dev/null".into()
    } else {
        git_path(
            'a',
            summary.previous_path.as_deref().unwrap_or(&summary.path),
        )
    };
    let right = if summary.status == "removed" {
        "/dev/null".into()
    } else {
        git_path('b', &summary.path)
    };
    let patch = format!(
        "--- {left}\n+++ {right}\n{}\n",
        fragment.trim_end_matches('\n')
    );
    let parsed = DiffFile::parse_with(&patch, DiffParseOptions::new().truncated_hunks(truncated));
    let result = parsed
        .ok()
        .filter(|files| files.len() == 1)
        .and_then(|mut files| files.pop());
    if result.is_none() {
        return (None, Some("The saved text patch is incomplete or invalid."));
    }
    (
        result,
        truncated.then_some("Partial patch. GitHub did not supply every changed line."),
    )
}

fn git_path(prefix: char, path: &str) -> String {
    use std::fmt::Write as _;
    let mut quoted = format!("\"{prefix}/");
    for byte in path.bytes() {
        match byte {
            b'"' | b'\\' => {
                quoted.push('\\');
                quoted.push(char::from(byte));
            }
            b' '..=b'~' => quoted.push(char::from(byte)),
            _ => write!(quoted, "\\{byte:03o}").expect("write to String"),
        }
    }
    quoted.push('"');
    quoted
}

pub(super) fn verified_reference(
    set: &(String, Vec<BodyReference>),
    context: &PrContext,
) -> Option<ItemId> {
    if set.0 != context.body_revision {
        return None;
    }
    let item = context.selected_issue?;
    set.1
        .iter()
        .any(|reference| {
            reference.provenance == context.provenance
                && reference.verification == Verification::Verified
                && reference.verified_item == Some(item)
        })
        .then_some(item)
}

pub(super) fn refresh_context(set: &(String, Vec<BodyReference>), context: &mut PrContext) {
    if context.selected_issue.is_none() {
        return;
    }
    context.body_revision.clone_from(&set.0);
    if verified_reference(set, context).is_none() {
        context.selected_issue = None;
    }
}

impl PullRequestView {
    pub fn new(
        data: PullRequestData,
        tab: &Tab,
        old: Option<Self>,
        window: &mut Window,
        cx: &mut Context<ReaderWorkspace>,
    ) -> Self {
        let mut old = old.filter(|old| old.data.revision == data.revision);
        let recipe = tab.view.pr_reader.as_ref().cloned().unwrap_or_default();
        let body = match &data.body {
            Change::Value(body) => old
                .as_mut()
                .filter(|old| old.data.body == data.body)
                .and_then(|old| old.body.take())
                .or_else(|| Some(media::CachedMarkdown::new(body, cx))),
            _ => None,
        };
        let linked_body = data.linked.as_ref().and_then(|link| {
            if let Some(old) = old
                .as_mut()
                .filter(|old| old.data.linked.as_ref().map(|issue| &issue.item) == Some(&link.item))
            {
                return old.linked_body.take();
            }
            match &link.item.body {
                Change::Value(body) => Some(media::CachedMarkdown::new(body, cx)),
                _ => None,
            }
        });
        let linked_comments = data.linked.as_ref().map_or_else(Vec::new, |link| {
            if let Some(old) = old.as_mut().filter(|old| {
                old.data.linked.as_ref().is_some_and(|issue| {
                    issue.item.id == link.item.id
                        && issue.discussion.records == link.discussion.records
                })
            }) {
                return std::mem::take(&mut old.linked_comments);
            }
            link.discussion
                .records
                .iter()
                .map(|record| media::CachedMarkdown::new(&record.body, cx))
                .collect()
        });
        let context_scroll = old
            .as_ref()
            .map_or_else(ScrollHandle::new, |old| old.context_scroll.clone());
        let issue_scroll = old
            .as_ref()
            .map_or_else(ScrollHandle::new, |old| old.issue_scroll.clone());
        if old.is_none() {
            restore_offset(&context_scroll, recipe.context_scroll, window);
            restore_offset(&issue_scroll, recipe.issue_scroll, window);
        }
        let selected = data
            .files
            .as_ref()
            .and_then(|files| files.selected.as_ref());
        let diff = if recipe.section != PrSection::Files {
            old.and_then(|old| old.diff)
        } else {
            selected.and_then(|selected| {
                let patch = selected.patch.as_ref()?;
                if let Some(old) = old.as_mut()
                    && old.diff.as_ref().is_some_and(|view| {
                        view.read(cx)
                            .files()
                            .first()
                            .is_some_and(|file| file.path() == patch.path())
                    })
                    && old
                        .data
                        .files
                        .as_ref()
                        .and_then(|files| files.selected.as_ref())
                        .is_none_or(|prior| prior.evidence == selected.evidence)
                {
                    return old.diff.take();
                }
                Some(new_diff(&data.revision, &recipe, selected, patch, cx))
            })
        };
        Self {
            data,
            body,
            linked_body,
            linked_comments,
            context_scroll,
            issue_scroll,
            diff,
        }
    }

    pub fn replace_files(&mut self, files: FilePage, tab: &Tab, cx: &mut Context<ReaderWorkspace>) {
        let old = self
            .data
            .files
            .as_ref()
            .and_then(|files| files.selected.as_ref());
        let selected = files.selected.as_ref();
        if old.zip(selected).is_none_or(|(old, new)| {
            old.summary.path != new.summary.path || old.evidence != new.evidence
        }) {
            self.diff = selected.and_then(|selected| {
                selected.patch.as_ref().map(|patch| {
                    new_diff(&self.data.revision, &tab_settings(tab), selected, patch, cx)
                })
            });
        }
        self.data.files = Some(files);
    }
}

fn restore_offset(scroll: &ScrollHandle, offset: u32, window: &mut Window) {
    if offset > 0 {
        let scroll = scroll.clone();
        window.on_next_frame(move |_, _| scroll.set_offset(point(px(0.), px(-(offset as f32)))));
    }
}

fn new_diff(
    revision: &PrRevision,
    recipe: &PrTabView,
    selected: &SelectedFile,
    patch: &DiffFile,
    cx: &mut Context<ReaderWorkspace>,
) -> Entity<DiffState> {
    let diff =
        cx.new(|cx| DiffState::new([patch.clone()], cx).with_mode(diff_mode(recipe.diff_mode)));
    if let Some(anchor) = recipe
        .code_anchor
        .as_ref()
        .filter(|anchor| anchor.path == selected.summary.path)
    {
        let (side, oid) = match anchor.side {
            SourceSide::Left => (DiffSide::Original, revision.merge_base.as_ref()),
            SourceSide::Right => (DiffSide::Modified, Some(&revision.head)),
        };
        if oid.is_some_and(|oid| oid.as_str() == anchor.original_oid) {
            diff.update(cx, |state, cx| {
                state.scroll_to_line(
                    DiffLinePosition::new(patch.path().clone(), side, anchor.line as usize),
                    cx,
                )
            });
        }
    }
    diff
}

pub(super) fn diff_mode(mode: PrDiffMode) -> DiffMode {
    match mode {
        PrDiffMode::Unified => DiffMode::Unified,
        PrDiffMode::Split => DiffMode::Split,
    }
}

pub(super) const fn section_resources(section: PrSection) -> &'static [PrFamily] {
    match section {
        PrSection::Conversation => &[PrFamily::Reviews, PrFamily::Threads],
        PrSection::Files => &[PrFamily::Files],
        PrSection::Commits => &[PrFamily::Commits],
        PrSection::Checks => &[PrFamily::Checks, PrFamily::Statuses],
    }
}

pub(super) fn tab_settings(tab: &Tab) -> PrTabView {
    tab.view.pr_reader.as_ref().cloned().unwrap_or_default()
}

impl ReaderWorkspace {
    pub(super) fn edit_pr_tab(
        &mut self,
        tab: &Tab,
        change: impl FnOnce(&mut PrTabView),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.layout_busy {
            return;
        }
        self.capture_scroll();
        let mut layout = self.layout.clone();
        let Some(target) = layout
            .panes
            .iter_mut()
            .flat_map(|pane| &mut pane.tabs)
            .find(|candidate| candidate.id == tab.id && candidate.item == tab.item)
        else {
            return;
        };
        change(target.view.pr_reader.get_or_insert_with(PrTabView::default));
        self.save_layout(layout, window, cx);
    }

    pub(super) fn set_context_mode(
        &mut self,
        tab: Tab,
        mode: PrContextMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if mode == PrContextMode::Issue
            && tab
                .view
                .context
                .as_ref()
                .is_none_or(|context| context.selected_issue.is_none())
            && let Some(pr) = self
                .tab_views
                .get(&tab.id)
                .and_then(|view| view.pr.as_ref())
            && let Some(set) = pr.data.references.as_ref()
            && let Some(reference) = set.1.iter().find(|reference| {
                reference.verification == Verification::Verified
                    && reference.verified_item.is_some()
            })
        {
            let ratio = tab
                .view
                .context
                .as_ref()
                .map_or(0.33, |context| context.ratio);
            let context = pr
                .data
                .remembered_context
                .as_ref()
                .filter(|context| verified_reference(set, context).is_some())
                .map_or_else(
                    || PrContext {
                        selected_issue: reference.verified_item,
                        body_revision: set.0.clone(),
                        provenance: reference.provenance.clone(),
                        ratio,
                    },
                    |context| PrContext {
                        ratio,
                        ..context.clone()
                    },
                );
            self.choose_linked_issue(tab, context, window, cx);
            return;
        }
        self.edit_pr_tab(
            &tab,
            |view| {
                view.context_mode = mode;
                view.context_collapsed = false;
                view.narrow_surface = NarrowSurface::Context;
            },
            window,
            cx,
        );
    }

    pub(super) fn choose_linked_issue(
        &mut self,
        tab: Tab,
        context: PrContext,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        if self.layout_busy {
            return;
        }
        self.capture_scroll();
        let id = tab.id;
        let item = tab.item;
        self.layout_busy = true;
        let saved = context.clone();
        self.launch(
            window,
            cx,
            {
                let id = id.clone();
                move |store| {
                    store.set_pr_tab_context(account, item, &id, &saved, PrContextMode::Issue)
                }
            },
            move |this, result, window, cx| {
                this.layout_busy = false;
                if result.is_err() {
                    this.status = Some(
                        "The linked issue changed. Refresh the pull request and choose again.",
                    );
                } else if let Some(tab) = this
                    .layout
                    .panes
                    .iter_mut()
                    .flat_map(|pane| &mut pane.tabs)
                    .find(|tab| tab.id == id && tab.item == item)
                {
                    tab.view.context = Some(context);
                    let view = tab.view.pr_reader.get_or_insert_with(PrTabView::default);
                    view.context_mode = PrContextMode::Issue;
                    view.context_collapsed = false;
                    view.narrow_surface = NarrowSurface::Context;
                    let updated = this.layout.clone();
                    this.save_layout(updated, window, cx);
                    this.reload_pr(item, window, cx);
                }
                this.settle_departure(window, cx);
            },
        );
        cx.notify();
    }

    pub(super) fn code_selection(
        &mut self,
        tab: Tab,
        revision: &PrRevision,
        position: DiffLinePosition,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (side, oid) = match position.side() {
            DiffSide::Original => (SourceSide::Left, revision.merge_base.as_ref()),
            DiffSide::Modified => (SourceSide::Right, Some(&revision.head)),
        };
        let (Some(oid), Ok(line)) = (oid, u32::try_from(position.line())) else {
            return;
        };
        let anchor = ReviewAnchor {
            path: position.path().to_string(),
            side,
            line,
            original_oid: oid.as_str().to_owned(),
        };
        self.edit_pr_tab(&tab, |view| view.code_anchor = Some(anchor), window, cx);
    }
}

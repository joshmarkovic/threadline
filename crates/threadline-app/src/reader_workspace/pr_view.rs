use super::{
    pr::*,
    view::{control, coverage_label, resource_problem, time_label},
    *,
};
use crate::palette::Palette;
use gpui_kit::{
    AnyElement, Element, InteractiveElement, IntoElement, ParentElement, Role,
    StatefulInteractiveElement, Styled,
    base::ElementExt,
    component::{
        Disableable, Selectable,
        diff::{Diff, DiffLineRange, DiffSide},
    },
    div,
};
use threadline_storage::{
    pr::{
        BodyReference, NarrowSurface, PrContextMode, PrDiffMode, PrFamily, PrRecord, PrRevision,
        PrSection, SourceSide, Verification,
    },
    queue::PrContext,
    resources::{ResourceError, ResourceState},
};

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct PrReadKey(ItemId, &'static str, String);

fn pr_status(id: String, text: impl Into<String>, palette: Palette) -> impl Element {
    let text = text.into();
    div()
        .id(id.clone())
        .accessibility_id(id)
        .role(Role::Status)
        .aria_label(text.clone())
        .text_xs()
        .text_color(palette.muted)
        .child(text)
}

struct FamilyCoverage<'a> {
    state: &'a ResourceState,
    next: Option<&'a str>,
    api_capped: bool,
}

#[derive(Clone)]
enum Command {
    Section(PrSection),
    Mode(PrContextMode),
    Collapse(bool),
    Surface(NarrowSurface),
    Resize(f64),
    File(String),
    DiffMode(PrDiffMode),
    Change {
        next: bool,
        revision: i64,
        path: String,
    },
    Viewed(String, bool),
    Choose(PrContext),
    Verify(String, i64),
    CachedPage(PrFamily, Option<String>),
    Fetch(PrFamily),
    Source(String, SourceSide, u32),
    Download {
        context: bool,
        media: bool,
    },
    CancelDownload(i64),
}

#[cfg(feature = "e2e")]
#[derive(serde::Serialize)]
pub(crate) struct PrObservation {
    section: String,
    context_mode: String,
    context_collapsed: bool,
    context_ratio: f64,
    diff_mode: String,
    files_coverage: Option<String>,
    files_on_page: Option<usize>,
    threads_coverage: Option<String>,
    threads_on_page: Option<usize>,
    reviews_on_page: Option<usize>,
    checks_coverage: Option<String>,
    patch_change_lines: Option<usize>,
    source_anchor: Option<PrSourceObservation>,
    selected_source_line: Option<usize>,
    narrow_surface: String,
    selected_file: Option<String>,
    revision_id: Option<i64>,
    head: Option<String>,
    merge_base: Option<String>,
    merged: Option<bool>,
    closed: Option<bool>,
    issue_rest_id: Option<u64>,
    pull_request_rest_id: Option<u64>,
    linked_issue: Option<i64>,
    verified_targets: usize,
    download_state: Option<String>,
    review_draft: Option<super::review_editor::ReviewObservation>,
}

#[cfg(feature = "e2e")]
#[derive(serde::Serialize)]
struct PrSourceObservation {
    side: String,
    line: u32,
    path: String,
}

#[cfg(feature = "e2e")]
#[derive(serde::Serialize)]
pub(crate) struct SharedPrPaneObservation {
    pane: usize,
    tab: String,
    item: i64,
    provider_draft: Option<bool>,
    review_coverage: Option<String>,
    review_rows: Option<usize>,
    conversation_offset: Option<u32>,
    files_context_offset: Option<u32>,
    view: PrObservation,
}

impl ReaderWorkspace {
    #[cfg(feature = "e2e")]
    pub(super) fn shared_pr_observation(&self, cx: &gpui_kit::App) -> Vec<SharedPrPaneObservation> {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if self.maximized
            || !*ENABLED.get_or_init(|| {
                std::env::var_os("THREADLINE_E2E_SHARED_PR").is_some_and(|value| value == "1")
            })
        {
            return Vec::new();
        }
        self.layout
            .panes
            .iter()
            .enumerate()
            .filter_map(|(pane, slot)| {
                let tab = slot.active.and_then(|index| slot.tabs.get(index))?;
                let item_view = self.tab_views.get(&tab.id)?;
                let cached = item_view.pr.as_ref()?;
                let reviews = cached
                    .data
                    .pages
                    .iter()
                    .find_map(|(family, page)| (*family == PrFamily::Reviews).then_some(page));
                Some(SharedPrPaneObservation {
                    pane,
                    tab: tab.id.clone(),
                    item: tab.item.0,
                    provider_draft: cached.data.header.draft,
                    review_coverage: reviews.map(|page| format!("{:?}", page.state.coverage)),
                    review_rows: reviews.map(|page| page.rows.len()),
                    conversation_offset: (item_view.scroll.bounds().size.height > px(0.))
                        .then(|| (-item_view.scroll.offset().y.as_f32()).max(0.) as u32),
                    files_context_offset: (cached.context_scroll.bounds().size.height > px(0.))
                        .then(|| (-cached.context_scroll.offset().y.as_f32()).max(0.) as u32),
                    view: self.pr_observation(tab, cx)?,
                })
            })
            .collect()
    }

    #[cfg(feature = "e2e")]
    pub(super) fn pr_observation(&self, tab: &Tab, cx: &gpui_kit::App) -> Option<PrObservation> {
        let cached = self.tab_views.get(&tab.id)?;
        (cached.data.item.as_ref()?.kind == threadline_storage::ItemKind::PullRequest).then(|| {
            let settings = tab_settings(tab);
            let pr = cached.pr.as_deref();
            let page = |target| {
                pr.and_then(|pr| {
                    pr.data
                        .pages
                        .iter()
                        .find_map(|(family, page)| (*family == target).then_some(page))
                })
            };
            let threads = page(PrFamily::Threads);
            PrObservation {
                section: format!("{:?}", settings.section),
                context_mode: format!("{:?}", settings.context_mode),
                context_collapsed: settings.context_collapsed,
                context_ratio: tab
                    .view
                    .context
                    .as_ref()
                    .map_or(0.33, |context| context.ratio),
                diff_mode: format!("{:?}", settings.diff_mode),
                files_coverage: pr
                    .and_then(|pr| pr.data.files.as_ref())
                    .map(|files| format!("{:?}", files.state.coverage)),
                files_on_page: pr
                    .and_then(|pr| pr.data.files.as_ref())
                    .map(|files| files.rows.len()),
                threads_coverage: threads.map(|page| format!("{:?}", page.state.coverage)),
                threads_on_page: threads.map(|page| page.rows.len()),
                reviews_on_page: page(PrFamily::Reviews).map(|page| page.rows.len()),
                checks_coverage: page(PrFamily::Checks)
                    .map(|page| format!("{:?}", page.state.coverage)),
                patch_change_lines: pr
                    .and_then(|pr| pr.data.files.as_ref()?.selected.as_ref()?.patch.as_ref())
                    .map(|patch| patch.additions() + patch.deletions()),
                source_anchor: settings.code_anchor.map(|anchor| PrSourceObservation {
                    side: format!("{:?}", anchor.side),
                    line: anchor.line,
                    path: anchor.path,
                }),
                selected_source_line: pr
                    .and_then(|pr| pr.diff.as_ref()?.read(cx).selected_lines())
                    .map(|selection| selection.start()),
                narrow_surface: format!("{:?}", settings.narrow_surface),
                selected_file: pr.and_then(|pr| {
                    pr.data
                        .files
                        .as_ref()?
                        .selected
                        .as_ref()
                        .map(|file| file.summary.path.clone())
                }),
                revision_id: pr.map(|pr| pr.data.revision.id),
                head: pr.map(|pr| pr.data.revision.head.as_str().to_owned()),
                merge_base: pr
                    .and_then(|pr| pr.data.revision.merge_base.as_ref())
                    .map(|oid| oid.as_str().to_owned()),
                merged: cached.data.item.as_ref().and_then(|item| {
                    match &item.pull_request.as_ref()?.fields.merged {
                        Change::Value(value) => Some(*value),
                        _ => None,
                    }
                }),
                closed: cached
                    .data
                    .item
                    .as_ref()
                    .and_then(|item| match item.closed {
                        Change::Value(value) => Some(value),
                        _ => None,
                    }),
                issue_rest_id: cached
                    .data
                    .item
                    .as_ref()
                    .and_then(|item| item.issue_id)
                    .map(|id| id.get()),
                pull_request_rest_id: cached
                    .data
                    .item
                    .as_ref()
                    .and_then(|item| item.pull_request.as_ref())
                    .map(|pull| pull.id.get()),
                linked_issue: pr
                    .and_then(|pr| pr.data.linked.as_ref())
                    .map(|issue| issue.item.id.0),
                verified_targets: pr.and_then(|pr| pr.data.references.as_ref()).map_or(
                    0,
                    |(_, refs)| {
                        refs.iter()
                            .filter(|reference| reference.verification == Verification::Verified)
                            .count()
                    },
                ),
                download_state: pr
                    .and_then(|pr| pr.data.download.as_ref())
                    .map(|job| format!("{:?}", job.state)),
                review_draft: self
                    .review_editors
                    .get(&tab.item)
                    .map(|editor| editor.observation(tab)),
            }
        })
    }

    pub(super) fn pull_request_view(
        &self,
        pane: usize,
        tab: &Tab,
        item: &Item,
        pane_width: f32,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = tab_settings(tab);
        let pr = self
            .tab_views
            .get(&tab.id)
            .and_then(|view| view.pr.as_deref());
        let sections = [
            ("conversation", "Conversation", PrSection::Conversation),
            ("commits", "Commits", PrSection::Commits),
            ("files", "Files", PrSection::Files),
            ("checks", "Checks", PrSection::Checks),
        ];
        let mut top = div().flex().flex_wrap().items_center().gap_1();
        for (key, label, section) in sections {
            top = top.child(self.pr_control(
                &format!("pr-{pane}-{key}"),
                label,
                tab,
                Command::Section(section),
                settings.section == section,
                false,
                cx,
            ));
        }
        let mut panel = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(self.pr_header(pane, tab, item, pr, palette, cx))
            .child(
                top.px_2()
                    .py_1()
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(palette.border),
            );
        let Some(pr) = pr else {
            return panel
                .child(
                    div()
                        .p_2()
                        .text_sm()
                        .child("The comparison and review records have not been downloaded.")
                        .child(self.pr_control(
                            &format!("pr-{pane}-refresh"),
                            "Load comparison",
                            tab,
                            Command::Fetch(PrFamily::Files),
                            false,
                            !self.can_fetch(),
                            cx,
                        )),
                )
                .children(
                    (settings.section == PrSection::Conversation)
                        .then(|| self.item_view(pane, tab, palette, cx)),
                )
                .into_any_element();
        };
        if settings.section != PrSection::Files {
            panel = panel.child(self.review_view(pane, tab, pr, palette, cx));
        }
        panel = panel.child(match settings.section {
            PrSection::Conversation => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .child(self.pr_families(pane, tab, pr, palette, cx))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .flex_grow(2.)
                        .min_h_0()
                        .child(self.item_view(pane, tab, palette, cx)),
                )
                .into_any_element(),
            PrSection::Commits | PrSection::Checks => self.pr_families(pane, tab, pr, palette, cx),
            PrSection::Files => self.pr_files(pane, tab, pr, pane_width < 725., palette, cx),
        });
        panel.into_any_element()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "Button identity, state, and the bound tab command are explicit at the render site"
    )]
    fn pr_control(
        &self,
        id: &str,
        title: impl Into<gpui_kit::SharedString>,
        tab: &Tab,
        command: Command,
        selected: bool,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tab = tab.clone();
        let button = control(id.to_owned(), title)
            .selected(selected)
            .disabled(disabled || self.layout_busy)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.pr_command(&tab, command.clone(), window, cx)
            }));
        self.observed_control(id, button, false)
    }

    fn pr_header(
        &self,
        pane: usize,
        tab: &Tab,
        item: &Item,
        pr: Option<&PullRequestView>,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title = match &item.title {
            Change::Value(title) => title.as_str(),
            _ => "Title not downloaded",
        };
        let state = match (
            &item.closed,
            item.pull_request.as_ref().map(|pr| &pr.fields.merged),
        ) {
            (_, Some(Change::Value(true))) => "Merged",
            (Change::Value(true), Some(Change::Value(false))) => "Closed",
            (Change::Value(true), _) => "Closed · merge state unknown",
            (Change::Value(false), _) => "Open",
            _ => "State not downloaded",
        };
        let mut header = div().flex().flex_col().flex_shrink_0().gap_1().p_2().child(
            div()
                .text_sm()
                .child(format!("Pull request #{} · {title}", item.locator.number)),
        );
        if let Some(pr) = pr {
            let h = &pr.data.header;
            let draft = match h.draft {
                Some(true) => "Draft",
                Some(false) => "Ready for review",
                None => "Draft state unknown",
            };
            let reviewers = h.requested_reviewers.as_ref().map_or_else(
                || "Requested reviewers unknown".into(),
                |names| {
                    if names.is_empty() {
                        "No outstanding review requests".into()
                    } else {
                        format!("Requested reviewers {}", names.join(", "))
                    }
                },
            );
            let refs = format!(
                "{} → {}",
                h.head_ref.as_deref().unwrap_or("Unknown source branch"),
                h.base_ref.as_deref().unwrap_or("Unknown target branch")
            );
            header = header
                .child(pr_status(
                    format!("pr-{pane}-merge-state"),
                    format!(
                        "{state} · {draft} · {} · {refs}",
                        h.author.as_deref().unwrap_or("Unknown author")
                    ),
                    palette,
                ))
                .child(div().text_xs().text_color(palette.muted).child(format!(
                        "{reviewers} · {}",
                        h.review_decision
                            .as_deref()
                            .unwrap_or("Review decision unknown")
                    )))
                .child(div().text_xs().text_color(palette.muted).child(format!(
                    "Opened {} · merged {} · Head {}",
                    time_label(h.created_at_ms),
                    time_label(h.merged_at_ms),
                    &pr.data.revision.head.as_str()[..12],
                )))
                .child(self.pr_download_header(pane, tab, pr, palette, cx))
                .child(self.merge_view(pane, tab, pr, palette, cx));
        } else {
            header = header.child(pr_status(format!("pr-{pane}-merge-state"), state, palette));
        }
        header.into_any_element()
    }

    fn pr_download_header(
        &self,
        pane: usize,
        tab: &Tab,
        pr: &PullRequestView,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use threadline_storage::pr::PrDownloadState;
        let running = pr.data.download.as_ref().filter(|download| {
            matches!(
                download.state,
                PrDownloadState::Pending | PrDownloadState::Active
            )
        });
        let mut controls = div().flex().items_center().gap_2().flex_wrap();
        if let Some(previous) = &pr.data.previous_download {
            controls = controls.child(pr_status(
                format!("pr-{pane}-previous-download"),
                format!(
                    "Previous head {} · Offline pin {:?}",
                    &previous.head.as_str()[..12],
                    previous.state,
                ),
                palette,
            ));
        }
        if let Some(job) = &pr.data.download {
            let complete = job
                .families
                .iter()
                .filter(|(_, state)| {
                    state.coverage == threadline_storage::resources::Coverage::Complete
                })
                .count()
                + usize::from(job.body_downloaded)
                + usize::from(
                    job.conversation.coverage == threadline_storage::resources::Coverage::Complete,
                );
            let scope = if job.include_context {
                " · includes selected source and media"
            } else if job.include_media {
                " · includes selected media"
            } else {
                ""
            };
            let coverage = if job.incomplete_patches_or_threads {
                " · some patches or replies are unavailable"
            } else if job.state == PrDownloadState::Partial
                && job.unselected_optional_resources().next().is_some()
            {
                " · select available source or media to keep"
            } else {
                ""
            };
            controls = controls.child(pr_status(
                format!("pr-{pane}-download-state"),
                format!(
                    "Offline download {:?} · {complete}/{} resources complete{scope}{coverage}",
                    job.state,
                    job.families.len() + 2
                ),
                palette,
            ));
            if !matches!(job.state, PrDownloadState::Cancelled) {
                controls = controls.child(self.pr_control(
                    &format!("pr-{pane}-cancel-download"),
                    if running.is_some() {
                        "Cancel download"
                    } else {
                        "Release offline pin"
                    },
                    tab,
                    Command::CancelDownload(job.generation),
                    false,
                    false,
                    cx,
                ));
            }
        }
        if running.is_none()
            && pr
                .data
                .download
                .as_ref()
                .is_none_or(|job| job.state != PrDownloadState::Complete)
        {
            controls = controls
                .child(self.pr_control(
                    &format!("pr-{pane}-download"),
                    "Download PR for offline",
                    tab,
                    Command::Download {
                        context: false,
                        media: false,
                    },
                    false,
                    false,
                    cx,
                ))
                .child(self.pr_control(
                    &format!("pr-{pane}-download-media"),
                    "Download and keep cached media",
                    tab,
                    Command::Download {
                        context: false,
                        media: true,
                    },
                    false,
                    false,
                    cx,
                ))
                .child(self.pr_control(
                    &format!("pr-{pane}-download-context"),
                    "Download and keep cached source and media",
                    tab,
                    Command::Download {
                        context: true,
                        media: true,
                    },
                    false,
                    false,
                    cx,
                ));
        }
        controls.into_any_element()
    }

    fn pr_families(
        &self,
        pane: usize,
        tab: &Tab,
        pr: &PullRequestView,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = tab_settings(tab);
        let mut viewport = div()
            .id(format!("pr-{pane}-records"))
            .overflow_y_scroll()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .p_2()
            .flex()
            .flex_col()
            .gap_2();
        if settings.section == PrSection::Conversation {
            viewport = viewport.max_h(px(220.));
        }
        if settings.section == PrSection::Checks {
            viewport = viewport.child(div().text_xs().text_color(palette.muted).child(format!(
                "Checks for head {}",
                pr.data.revision.head.as_str()
            )));
        }
        for (family, page) in &pr.data.pages {
            viewport = viewport.child(self.pr_family_bar(
                pane,
                tab,
                *family,
                FamilyCoverage {
                    state: &page.state,
                    next: page.next.as_deref(),
                    api_capped: page.api_capped,
                },
                palette,
                cx,
            ));
            for (index, (_, record)) in page.rows.iter().enumerate() {
                viewport = viewport.child(pr_record(record, palette));
                if let Some(thread) = super::review_threads::Thread::from_record(record) {
                    viewport = viewport.child(self.thread_controls(
                        (pane, index),
                        tab,
                        &pr.data.revision,
                        thread,
                        cx,
                    ));
                }
            }
        }
        #[cfg(feature = "e2e")]
        if settings.section == PrSection::Conversation
            && let Some(observer) = self.observer.as_ref()
        {
            return div()
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .max_h(px(220.))
                .min_h_0()
                .child(viewport)
                .child(observer.visible_control_bounds(format!("pr-{pane}-conversation-records")))
                .into_any_element();
        }
        viewport.into_any_element()
    }

    fn pr_family_bar(
        &self,
        pane: usize,
        tab: &Tab,
        family: PrFamily,
        FamilyCoverage {
            state,
            next,
            api_capped,
        }: FamilyCoverage<'_>,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = match family {
            PrFamily::Files => "Files",
            PrFamily::Commits => "Commits",
            PrFamily::Reviews => "Reviews",
            PrFamily::Threads => "Review threads",
            PrFamily::Checks => "Checks",
            PrFamily::Statuses => "Commit status",
            PrFamily::Context => "Surrounding source",
            PrFamily::Media => "Media",
        };
        let name = family.name();
        let coverage_id = format!("pr-{pane}-{name}-coverage");
        let coverage = pr_status(
            coverage_id.clone(),
            coverage_label(label, state.coverage, state.last_validation_at_ms),
            palette,
        );
        let limitation = api_capped.then(|| {
            pr_status(
                format!("pr-{pane}-{name}-api-cap"),
                "GitHub returned its 300-file comparison limit. Other changed files may be missing.",
                palette,
            )
        });
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(self.observed_control(coverage_id, coverage, false))
                    .children(next.map(|cursor| {
                        self.pr_control(
                            &format!("pr-{pane}-{name}-cached-next"),
                            "Next downloaded page",
                            tab,
                            Command::CachedPage(family, Some(cursor.to_owned())),
                            false,
                            false,
                            cx,
                        )
                    }))
                    .child(self.pr_control(
                        &format!("pr-{pane}-{name}-cached-first"),
                        "First page",
                        tab,
                        Command::CachedPage(family, None),
                        false,
                        false,
                        cx,
                    ))
                    .child(self.pr_control(
                        &format!("pr-{pane}-{name}-fetch"),
                        "Fetch from GitHub",
                        tab,
                        Command::Fetch(family),
                        false,
                        !self.can_fetch(),
                        cx,
                    )),
            )
            .children(limitation)
            .children(
                (state.error.is_some()
                    && (!api_capped || state.error != Some(ResourceError::InvalidResponse)))
                .then(|| resource_problem(label, state, palette)),
            )
            .into_any_element()
    }

    fn pr_files(
        &self,
        pane: usize,
        tab: &Tab,
        pr: &PullRequestView,
        narrow: bool,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = tab_settings(tab);
        let mut strip = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .p_1()
            .flex_shrink_0();
        if narrow {
            strip = strip
                .child(self.pr_control(
                    &format!("pr-{pane}-show-context"),
                    "Context",
                    tab,
                    Command::Surface(NarrowSurface::Context),
                    settings.narrow_surface == NarrowSurface::Context,
                    false,
                    cx,
                ))
                .child(self.pr_control(
                    &format!("pr-{pane}-show-code"),
                    "Code",
                    tab,
                    Command::Surface(NarrowSurface::Code),
                    settings.narrow_surface == NarrowSurface::Code,
                    false,
                    cx,
                ));
        } else {
            strip = strip.child(self.pr_control(
                &format!("pr-{pane}-context-collapse"),
                if settings.context_collapsed {
                    "Show context"
                } else {
                    "Collapse context"
                },
                tab,
                Command::Collapse(!settings.context_collapsed),
                false,
                false,
                cx,
            ));
        }
        let ratio = tab
            .view
            .context
            .as_ref()
            .map_or(0.33, |context| context.ratio)
            .clamp(0.25, 0.48);
        let show_context = if narrow {
            settings.narrow_surface == NarrowSurface::Context
        } else {
            !settings.context_collapsed
        };
        let show_code = !narrow || settings.narrow_surface == NarrowSurface::Code;
        let mut body = div().flex().flex_1().min_h_0().min_w_0();
        if show_context {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .min_w_0()
                    .flex_basis(px(0.))
                    .flex_grow(if show_code { ratio as f32 } else { 1. })
                    .border_r_1()
                    .border_color(palette.border)
                    .child(self.pr_context(pane, tab, pr, narrow, palette, cx)),
            );
        }
        if show_code {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .min_w_0()
                    .flex_basis(px(0.))
                    .flex_grow(if show_context { 1. - ratio as f32 } else { 1. })
                    .child(self.pr_code(pane, tab, pr, palette, cx)),
            );
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(
                strip
                    .relative()
                    .keep_text_selection(format!("pr-{pane}-layout-controls")),
            )
            .child(body)
            .into_any_element()
    }

    fn pr_context(
        &self,
        pane: usize,
        tab: &Tab,
        pr: &PullRequestView,
        narrow: bool,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mode = tab_settings(tab).context_mode;
        let mut toolbar = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_1()
            .p_1()
            .child(self.pr_control(
                &format!("pr-{pane}-context-body"),
                "View PR body",
                tab,
                Command::Mode(PrContextMode::Body),
                mode == PrContextMode::Body,
                false,
                cx,
            ))
            .child(self.pr_control(
                &format!("pr-{pane}-context-issue"),
                "View issue",
                tab,
                Command::Mode(PrContextMode::Issue),
                mode == PrContextMode::Issue,
                false,
                cx,
            ));
        if !narrow {
            toolbar = toolbar
                .child(self.pr_control(
                    &format!("pr-{pane}-context-less"),
                    "Narrow context",
                    tab,
                    Command::Resize(-0.04),
                    false,
                    false,
                    cx,
                ))
                .child(self.pr_control(
                    &format!("pr-{pane}-context-more"),
                    "Widen context",
                    tab,
                    Command::Resize(0.04),
                    false,
                    false,
                    cx,
                ));
        }
        let content = match mode {
            PrContextMode::Body => pr.body.as_ref().map_or_else(
                || {
                    div()
                        .text_sm()
                        .child("No pull request body is available in the cache.")
                        .into_any_element()
                },
                |body| self.markdown_element(body, Some(pr.data.revision.repository), palette, cx),
            ),
            PrContextMode::Issue => self.pr_issue_context(pane, tab, pr, palette, cx),
        };
        let context_id = format!("pr-{pane}-context-content");
        let viewport = div()
            .id(context_id.clone())
            .role(Role::Group)
            .aria_label(match mode {
                PrContextMode::Body => "Pull request body",
                PrContextMode::Issue => "Linked issue",
            })
            .track_scroll(match mode {
                PrContextMode::Body => &pr.context_scroll,
                PrContextMode::Issue => &pr.issue_scroll,
            })
            .overflow_y_scroll()
            .flex_1()
            .min_h_0()
            .p_2()
            .child(content);
        #[cfg(feature = "e2e")]
        let viewport = if let Some(observer) = self.observer.as_ref() {
            div()
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(viewport)
                .child(observer.visible_control_bounds(context_id))
                .into_any_element()
        } else {
            viewport.into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .child(
                toolbar
                    .flex_shrink_0()
                    .relative()
                    .keep_text_selection(format!("pr-{pane}-context-controls")),
            )
            .child(viewport)
            .into_any_element()
    }

    fn pr_issue_context(
        &self,
        pane: usize,
        tab: &Tab,
        pr: &PullRequestView,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(set) = &pr.data.references else {
            return div()
                .id(format!("pr-{pane}-reference-state"))
                .text_sm()
                .role(Role::Status)
                .child("Linked issues have not been checked for this body.")
                .into_any_element();
        };
        if set.1.is_empty() {
            return div()
                .id(format!("pr-{pane}-reference-state"))
                .text_sm()
                .role(Role::Status)
                .child("This pull request body has no closing issue references.")
                .into_any_element();
        }
        let chosen = tab
            .view
            .context
            .as_ref()
            .and_then(|context| verified_reference(set, context));
        let mut context = div().min_w_0().flex().flex_col().gap_2();
        if let Some(link) = pr
            .data
            .linked
            .as_ref()
            .filter(|issue| Some(issue.item.id) == chosen)
        {
            let item = &link.item;
            let title = match &item.title {
                Change::Value(title) => title.as_str(),
                _ => "Title not downloaded",
            };
            let route = set
                .1
                .iter()
                .find(|reference| reference.verified_item == chosen)
                .map_or_else(
                    || "Repository unavailable".to_owned(),
                    |reference| format!("{}/{}", reference.owner, reference.repository),
                );
            let state = &link.discussion.state;
            context = context
                .child(
                    div()
                        .text_sm()
                        .child(format!("Issue {route} #{} · {title}", item.locator.number)),
                )
                .children(
                    link.metadata
                        .as_ref()
                        .map(|metadata| super::view::item_metadata(metadata, palette)),
                )
                .child(pr.linked_body.as_ref().map_or_else(
                    || {
                        div()
                            .text_sm()
                            .child("The linked issue body is unavailable in the cache.")
                            .into_any_element()
                    },
                    |body| self.markdown_element(body, Some(item.locator.repository), palette, cx),
                ))
                .child(
                    div()
                        .id(format!("pr-{pane}-linked-coverage"))
                        .role(Role::Status)
                        .text_xs()
                        .child(coverage_label(
                            "Linked issue conversation",
                            state.coverage,
                            state.last_validation_at_ms,
                        )),
                )
                .children(
                    pr.linked_comments
                        .iter()
                        .map(|comment| {
                            self.markdown_element(
                                comment,
                                Some(item.locator.repository),
                                palette,
                                cx,
                            )
                        })
                        .collect::<Vec<_>>(),
                );
        } else if tab
            .view
            .context
            .as_ref()
            .is_some_and(|context| !context.provenance.is_empty())
            && chosen.is_none()
        {
            context = context.child(div().id(format!("pr-{pane}-reference-state")).text_sm().role(Role::Status).child(
                "The previously selected closing reference has changed. Choose a verified issue.",
            ));
        } else {
            context = context.child(
                div()
                    .id(format!("pr-{pane}-reference-state"))
                    .text_sm()
                    .role(Role::Status)
                    .child("Choose a verified closing issue."),
            );
        }
        let mut picker = div()
            .id(format!("pr-{pane}-linked-picker"))
            .role(Role::Group)
            .aria_label("Verified closing references")
            .flex()
            .flex_col()
            .gap_1();
        for (index, reference) in set.1.iter().enumerate() {
            let label = format!(
                "{}/{} #{}",
                reference.owner, reference.repository, reference.number
            );
            let status = reference_status(reference, self.offline);
            let verified = reference
                .verified_item
                .filter(|_| reference.verification == Verification::Verified);
            let control = match verified {
                Some(issue) => {
                    let ratio = tab
                        .view
                        .context
                        .as_ref()
                        .map_or(0.33, |context| context.ratio);
                    self.pr_control(
                        &format!("pr-{pane}-linked-{index}"),
                        label,
                        tab,
                        Command::Choose(PrContext {
                            selected_issue: Some(issue),
                            body_revision: set.0.clone(),
                            provenance: reference.provenance.clone(),
                            ratio,
                        }),
                        Some(issue) == chosen,
                        false,
                        cx,
                    )
                }
                None => self.pr_control(
                    &format!("pr-{pane}-verify-{index}"),
                    format!("Check {label}"),
                    tab,
                    Command::Verify(set.0.clone(), reference.position),
                    false,
                    !self.can_fetch(),
                    cx,
                ),
            };
            picker = picker.child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1()
                    .child(control)
                    .child(div().text_xs().text_color(palette.muted).child(status)),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                picker
                    .relative()
                    .keep_text_selection(format!("pr-{pane}-issue-picker-controls")),
            )
            .child(context)
            .into_any_element()
    }

    fn pr_code(
        &self,
        pane: usize,
        tab: &Tab,
        pr: &PullRequestView,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(files) = &pr.data.files else {
            return div()
                .p_2()
                .text_sm()
                .child("Loading the cached file list…")
                .into_any_element();
        };
        let file_panel = self.pr_file_list(pane, tab, files, palette, cx);
        let diff_panel = self.pr_diff_panel(pane, tab, pr, files, palette, cx);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(self.review_view(pane, tab, pr, palette, cx))
            .child(div().p_1().flex_shrink_0().child(self.pr_family_bar(
                pane,
                tab,
                PrFamily::Files,
                FamilyCoverage {
                    state: &files.state,
                    next: files.next.as_deref(),
                    api_capped: files.api_capped,
                },
                palette,
                cx,
            )))
            .children(self.unresolved_thread_control(pane, tab, pr, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(file_panel)
                    .child(diff_panel),
            )
            .into_any_element()
    }

    fn pr_file_list(
        &self,
        pane: usize,
        tab: &Tab,
        files: &FilePage,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut contents = div()
            .id(format!("pr-{pane}-file-list"))
            .role(Role::Group)
            .aria_label("Changed files")
            .w(px(152.))
            .flex_none()
            .min_h_0()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(palette.border)
            .p_1()
            .flex()
            .flex_col()
            .gap_1();
        let selected = files.selected.as_ref().map(|file| &file.summary.path);
        for (index, file) in files.rows.iter().enumerate() {
            let name = file.path.rsplit('/').next().unwrap_or(&file.path);
            let label = if file.viewed {
                format!("✓ {name}")
            } else {
                name.to_owned()
            };
            contents = contents
                .child(self.pr_control(
                    &format!("pr-{pane}-file-{index}"),
                    label,
                    tab,
                    Command::File(file.path.clone()),
                    selected == Some(&file.path),
                    false,
                    cx,
                ))
                .child(div().text_xs().text_color(palette.muted).child(format!(
                    "{} · {}",
                    file.status,
                    file_counts(file)
                )));
        }
        contents.into_any_element()
    }

    fn pr_diff_panel(
        &self,
        pane: usize,
        tab: &Tab,
        pr: &PullRequestView,
        files: &FilePage,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(file) = &files.selected else {
            return div()
                .p_2()
                .child(pr_status(
                    format!("pr-{pane}-patch-state"),
                    "No downloadable file patch is selected.",
                    palette,
                ))
                .into_any_element();
        };
        let settings = tab_settings(tab);
        let title = file.summary.previous_path.as_ref().map_or_else(
            || file.summary.path.clone(),
            |previous| format!("{previous} → {}", file.summary.path),
        );
        let mut panel = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .child(
                div()
                    .p_1()
                    .text_xs()
                    .child(format!("{title} · {}", file_counts(&file.summary))),
            )
            .child(self.pr_diff_toolbar(pane, tab, pr, file, palette, cx));
        if pr.diff.is_some() {
            panel = panel.child(div().p_1().text_xs().text_color(palette.muted).child(
                "Whitespace filtering is unavailable. Review anchors use the full source patch.",
            ));
        }
        if let Some(problem) = file.problem {
            panel = panel.child(div().p_1().child(pr_status(
                format!("pr-{pane}-patch-state"),
                problem,
                palette,
            )));
        }
        if settings
            .selected_path
            .as_ref()
            .is_some_and(|selected| selected != &file.summary.path)
        {
            return panel
                .child(div().text_xs().p_1().child("Loading the selected file…"))
                .into_any_element();
        }
        if let Some(diff) = &pr.diff {
            let annotations = self.review_annotations(tab, pr, &file.summary.path);
            let tab = tab.clone();
            let revision = pr.data.revision.clone();
            let entity = cx.weak_entity();
            let annotate = cx.weak_entity();
            let annotation_tab = tab.clone();
            let comparison = div()
                .id(format!("pr-{pane}-diff"))
                .relative()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .child(
                    Diff::new(diff)
                        .size_full()
                        .annotations(annotations.rows)
                        .render_annotation(move |annotation, _, _| {
                            div()
                                .p_1()
                                .text_xs()
                                .children(annotations.labels.get(annotation.id()).cloned())
                        })
                        .on_line_click(move |position, _, window, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                this.code_selection(
                                    tab.clone(),
                                    &revision,
                                    position.clone(),
                                    window,
                                    cx,
                                )
                            });
                        })
                        .on_add_annotation(move |range, window, cx| {
                            let _ = annotate.update(cx, |this, cx| {
                                this.begin_inline(annotation_tab.clone(), range, window, cx);
                            });
                        }),
                );
            #[cfg(feature = "e2e")]
            let comparison = comparison.children(
                self.observer
                    .as_ref()
                    .map(|observer| observer.visible_control_bounds(format!("pr-{pane}-diff"))),
            );
            panel = panel.child(comparison);
        }
        panel
            .child(self.pr_source_cache(pane, pr, file, palette))
            .into_any_element()
    }

    fn pr_diff_toolbar(
        &self,
        pane: usize,
        tab: &Tab,
        pr: &PullRequestView,
        file: &SelectedFile,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = tab_settings(tab);
        let revision = &pr.data.revision;
        let source_ref = revision.merge_base.as_ref().map_or_else(
            || "merge base unavailable".to_owned(),
            |oid| format!("merge base {}", oid.as_str()),
        );
        let comparison = format!(
            "Pull request comparison · {source_ref} → head {}",
            revision.head.as_str()
        );
        let mut buttons = div().flex().flex_wrap().items_center().gap_1();
        let path = file.summary.path.clone();
        if let Some(diff) = &pr.diff {
            let id = format!("pr-{pane}-review-add-inline");
            let diff = diff.clone();
            let target = tab.clone();
            let code = file.summary.path.clone();
            let revision = pr.data.revision.clone();
            let note = control(id.clone(), "Comment on selected source lines")
                .disabled(self.review_controls_busy(tab.item))
                .on_click(cx.listener(move |this, _, window, cx| {
                    let range = diff
                        .read(cx)
                        .selected_lines()
                        .or_else(|| this.saved_diff_selection(&target, &code, &revision));
                    if let Some(range) = range {
                        this.begin_inline(target.clone(), &range, window, cx);
                    } else {
                        this.status = Some("Select one or more source lines in the diff first.");
                        cx.notify();
                    }
                }));
            buttons = buttons.child(
                div()
                    .keep_text_selection(format!("pr-{pane}-review-add-control"))
                    .child(self.observed_control(id, note, false)),
            );
            for (mode, label) in [
                (PrDiffMode::Unified, "Unified"),
                (PrDiffMode::Split, "Split"),
            ] {
                buttons = buttons.child(self.pr_control(
                    &format!("pr-{pane}-diff-{}", label.to_ascii_lowercase()),
                    label,
                    tab,
                    Command::DiffMode(mode),
                    settings.diff_mode == mode,
                    false,
                    cx,
                ));
            }
            buttons = buttons
                .child(self.pr_control(
                    &format!("pr-{pane}-previous-change"),
                    "Previous change",
                    tab,
                    Command::Change {
                        next: false,
                        revision: pr.data.revision.id,
                        path: path.clone(),
                    },
                    false,
                    file.changes.is_empty(),
                    cx,
                ))
                .child(self.pr_control(
                    &format!("pr-{pane}-next-change"),
                    "Next change",
                    tab,
                    Command::Change {
                        next: true,
                        revision: pr.data.revision.id,
                        path: path.clone(),
                    },
                    false,
                    file.changes.is_empty(),
                    cx,
                ));
        }
        buttons = buttons.child(self.pr_control(
            &format!("pr-{pane}-viewed"),
            if file.summary.viewed {
                "Mark unviewed"
            } else {
                "Mark viewed"
            },
            tab,
            Command::Viewed(path.clone(), !file.summary.viewed),
            false,
            false,
            cx,
        ));
        if let Some(anchor) = settings.code_anchor.filter(|anchor| anchor.path == path) {
            buttons = buttons.child(self.pr_control(
                &format!("pr-{pane}-load-source"),
                "Fetch surrounding source",
                tab,
                Command::Source(path, anchor.side, anchor.line.saturating_sub(10).max(1)),
                false,
                !self.can_fetch(),
                cx,
            ));
        }
        div()
            .p_1()
            .flex()
            .flex_col()
            .gap_1()
            .flex_shrink_0()
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .id(format!("pr-{pane}-comparison-mode"))
                    .accessibility_id(format!("pr-{pane}-comparison-mode"))
                    .role(Role::Status)
                    .aria_label(comparison.clone())
                    .text_xs()
                    .text_color(palette.muted)
                    .child(comparison),
            )
            .child(buttons)
            .into_any_element()
    }

    fn pr_source_cache(
        &self,
        pane: usize,
        pr: &PullRequestView,
        file: &SelectedFile,
        palette: Palette,
    ) -> AnyElement {
        let mut sources = div()
            .id(format!("pr-{pane}-source-cache"))
            .role(Role::Group)
            .aria_label("Downloaded surrounding source")
            .text_xs()
            .text_color(palette.muted)
            .overflow_y_scroll()
            .max_h(px(100.));
        for (_, source) in pr.data.pages.iter().flat_map(|(_, page)| &page.rows) {
            if let PrRecord::Context {
                oid,
                path,
                first_line,
                text,
            } = source
                && path == &file.summary.path
            {
                sources = sources.child(
                    div()
                        .p_1()
                        .child(format!("Source {oid} · line {first_line}"))
                        .child(div().font_family("monospace").child(text.clone())),
                );
            }
        }
        sources.into_any_element()
    }

    fn pr_command(
        &mut self,
        tab: &Tab,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.layout_busy
            || !self.layout.panes.iter().any(|pane| {
                pane.active
                    .and_then(|index| pane.tabs.get(index))
                    .is_some_and(|open| open.id == tab.id && open.item == tab.item)
            })
        {
            return;
        }
        match command {
            Command::Section(section) => {
                self.edit_pr_tab(tab, |view| view.section = section, window, cx)
            }
            Command::Mode(mode) => self.set_context_mode(tab.clone(), mode, window, cx),
            Command::Collapse(collapsed) => {
                self.edit_pr_tab(tab, |view| view.context_collapsed = collapsed, window, cx)
            }
            Command::Surface(surface) => {
                self.edit_pr_tab(tab, |view| view.narrow_surface = surface, window, cx)
            }
            Command::Resize(delta) => self.resize_pr_context(tab, delta, window, cx),
            Command::File(path) => self.edit_pr_tab(
                tab,
                |view| {
                    if view.selected_path.as_ref() != Some(&path) {
                        view.selected_path = Some(path);
                        view.code_anchor = None;
                        view.code_scroll = 0;
                    }
                },
                window,
                cx,
            ),
            Command::DiffMode(mode) => {
                if let Some(diff) = self
                    .tab_views
                    .get(&tab.id)
                    .and_then(|view| view.pr.as_ref()?.diff.as_ref())
                {
                    diff.update(cx, |state, cx| state.set_mode(diff_mode(mode), cx));
                }
                self.edit_pr_tab(tab, |view| view.diff_mode = mode, window, cx);
            }
            Command::Change {
                next,
                revision,
                path,
            } => {
                self.select_pr_change(tab, (revision, &path), next, window, cx);
            }
            Command::Choose(context) => self.choose_linked_issue(tab.clone(), context, window, cx),
            Command::Viewed(path, viewed) => self.set_pr_viewed(tab, path, viewed, window, cx),
            Command::CachedPage(family, after) => {
                self.load_pr_local_page(tab, family, after, window, cx)
            }
            Command::Fetch(family) => self.fetch_pr_resource(tab, family, window, cx),
            Command::Verify(body, position) => {
                self.verify_linked_issue(tab, body, position, window, cx)
            }
            Command::Source(file, side, line) => {
                self.fetch_pr_source(tab, file, side, line, window, cx)
            }
            Command::Download { context, media } => {
                self.pr_download(tab, DownloadAction::Queue { context, media }, window, cx)
            }
            Command::CancelDownload(generation) => {
                self.pr_download(tab, DownloadAction::Cancel(generation), window, cx)
            }
        }
    }

    fn select_pr_change(
        &mut self,
        tab: &Tab,
        target: (i64, &str),
        next: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pr) = self
            .tab_views
            .get(&tab.id)
            .and_then(|view| view.pr.as_ref())
        else {
            return;
        };
        let Some(file) = pr
            .data
            .files
            .as_ref()
            .and_then(|files| files.selected.as_ref())
        else {
            return;
        };
        let Some(diff) = pr.diff.as_ref() else { return };
        if target != (pr.data.revision.id, file.summary.path.as_str()) || file.changes.is_empty() {
            return;
        }
        let selected = diff
            .read(cx)
            .selected_lines()
            .or_else(|| self.saved_diff_selection(tab, target.1, &pr.data.revision));
        let position_index = file.changes.iter().position(|point| {
            selected.as_ref().is_some_and(|selection| {
                selection.path() == point.path()
                    && selection.side() == point.side()
                    && selection.start() == point.line()
                    && selection.end() == point.line()
            })
        });
        let last = file.changes.len() - 1;
        let index = match (next, position_index) {
            (true, Some(index)) if index < last => index + 1,
            (false, Some(index)) if index > 0 => index - 1,
            (true, _) => 0,
            (false, _) => last,
        };
        let position = file.changes[index].clone();
        let line = position.line();
        diff.update(cx, |state, cx| {
            let selection =
                DiffLineRange::new(position.path().clone(), position.side(), line, line);
            state.set_selected_lines(Some(selection), cx);
            state.scroll_to_line(position.clone(), cx);
        });
        let revision = pr.data.revision.clone();
        self.code_selection(tab.clone(), &revision, position, window, cx);
    }

    fn saved_diff_selection(
        &self,
        tab: &Tab,
        path: &str,
        revision: &PrRevision,
    ) -> Option<DiffLineRange> {
        let saved = self
            .layout
            .panes
            .iter()
            .flat_map(|pane| &pane.tabs)
            .find(|saved| saved.id == tab.id && saved.item == tab.item)?;
        let anchor = saved.view.pr_reader.as_ref()?.code_anchor.as_ref()?;
        if anchor.path != path {
            return None;
        }
        let (side, oid) = match anchor.side {
            SourceSide::Left => (DiffSide::Original, revision.merge_base.as_ref()),
            SourceSide::Right => (DiffSide::Modified, Some(&revision.head)),
        };
        let line = anchor.line as usize;
        oid.filter(|oid| oid.as_str() == anchor.original_oid)
            .map(|_| DiffLineRange::new(anchor.path.clone(), side, line, line))
    }

    fn resize_pr_context(
        &mut self,
        tab: &Tab,
        delta: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.capture_scroll();
        let mut layout = self.layout.clone();
        if let Some(current) = layout
            .panes
            .iter_mut()
            .flat_map(|pane| &mut pane.tabs)
            .find(|current| current.id == tab.id && current.item == tab.item)
        {
            let context = current.view.context.get_or_insert_with(|| PrContext {
                selected_issue: None,
                body_revision: String::new(),
                provenance: String::new(),
                ratio: 0.33,
            });
            context.ratio = (context.ratio + delta).clamp(0.25, 0.48);
            self.save_layout(layout, window, cx);
        }
    }

    fn current_pr(&self, id: &str) -> Option<&PullRequestView> {
        self.tab_views.get(id)?.pr.as_deref()
    }

    fn set_pr_viewed(
        &mut self,
        tab: &Tab,
        path: String,
        viewed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pr) = self.current_pr(&tab.id) else {
            return;
        };
        let revision = pr.data.revision.clone();
        let item = tab.item;
        self.launch(
            window,
            cx,
            move |store| store.set_pr_file_viewed(&revision, &path, viewed),
            move |this, result, window, cx| {
                if result.is_ok() {
                    this.reload_pr(item, window, cx);
                } else {
                    this.status = Some("Could not update the viewed-file status.");
                }
            },
        );
    }

    fn load_pr_local_page(
        &mut self,
        tab: &Tab,
        family: PrFamily,
        after: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pr) = self.current_pr(&tab.id) else {
            return;
        };
        let revision = pr.data.revision.clone();
        let revision_id = revision.id;
        let id = tab.id.clone();
        let current = tab_settings(tab);
        let selected = current.selected_path.clone().or_else(|| {
            pr.data
                .files
                .as_ref()?
                .selected
                .as_ref()
                .map(|file| file.summary.path.clone())
        });
        self.launch(
            window,
            cx,
            move |store| match family {
                PrFamily::Files => {
                    let selected = selected.as_deref();
                    FilePage::read(store, &revision, selected, after.as_deref())
                        .map(|page| (Some(page), None))
                }
                _ => store
                    .pr_page(&revision, family, after.as_deref(), PAGE)
                    .map(|page| (None, Some(page))),
            },
            move |this, result, _, cx| {
                let Some(tab) = this
                    .layout
                    .panes
                    .iter()
                    .filter_map(|pane| pane.active.and_then(|index| pane.tabs.get(index)))
                    .find(|tab| tab.id == id)
                    .cloned()
                else {
                    return;
                };
                let Some(tab_view) = this.tab_views.get_mut(&id) else {
                    return;
                };
                let Some(pr_view) = tab_view.pr.take() else {
                    return;
                };
                let settings = tab_settings(&tab);
                if pr_view.data.revision.id != revision_id
                    || settings.section != current.section
                    || settings.selected_path != current.selected_path
                {
                    tab_view.pr = Some(pr_view);
                    return;
                }
                match result {
                    Ok((Some(files), None)) => {
                        let mut pr = *pr_view;
                        pr.replace_files(files, &tab, cx);
                        tab_view.pr = Some(Box::new(pr));
                    }
                    Ok((None, Some(page))) => {
                        let mut pr = pr_view;
                        if let Some((_, entry)) =
                            pr.data.pages.iter_mut().find(|(name, _)| *name == family)
                        {
                            *entry = page;
                        }
                        tab_view.pr = Some(pr);
                    }
                    _ => {
                        tab_view.pr = Some(pr_view);
                        this.status =
                            Some("The downloaded page changed. Return to the first page.");
                    }
                }
            },
        );
    }

    pub(super) fn reload_pr(&mut self, item: ItemId, window: &mut Window, cx: &mut Context<Self>) {
        let tabs = self
            .layout
            .panes
            .iter()
            .flat_map(|pane| {
                pane.tabs.iter().enumerate().filter_map(|(index, tab)| {
                    (tab.item == item
                        && (pane.active == Some(index)
                            || self.tab_views.contains_key(&tab.id)
                            || self.pending_tabs.contains_key(&tab.id)))
                    .then_some(tab)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        for tab in tabs {
            self.load_tab(tab, window, cx);
        }
    }
}

fn file_counts(file: &FileSummary) -> String {
    match (file.additions, file.deletions) {
        (Some(added), Some(removed)) => format!("+{added} −{removed}"),
        _ => "Line counts unavailable".into(),
    }
}

fn reference_status(reference: &BodyReference, offline: bool) -> &'static str {
    match reference.verification {
        Verification::Pending if offline => "Verification pending · offline",
        Verification::Pending => "Verification pending",
        Verification::Verified => "Verified closing issue",
        Verification::PullRequest => "Target is a pull request",
        Verification::Missing => "Target was not found",
        Verification::Offline => "Target not downloaded · offline",
        Verification::Inaccessible => "No verified access to this repository",
        Verification::Failed => "Verification failed",
    }
}

fn pr_record(record: &PrRecord, palette: Palette) -> AnyElement {
    let mut card = div()
        .text_sm()
        .p_1()
        .border_b_1()
        .border_color(palette.border);
    card = match record {
        PrRecord::Commit {
            oid,
            message,
            author,
            authored_at_ms,
            committed_at_ms,
        } => card
            .child(format!(
                "{oid} · {}",
                author.as_deref().unwrap_or("Unknown author")
            ))
            .child(div().text_xs().text_color(palette.muted).child(format!(
                "Authored {} · committed {}",
                time_label(*authored_at_ms),
                time_label(*committed_at_ms)
            )))
            .child(message.clone()),
        PrRecord::Review {
            author,
            body,
            state,
            submitted_at_ms,
            commit_oid,
            ..
        } => card
            .child(format!(
                "{} · {state} · {}",
                author.as_deref().unwrap_or("Unknown reviewer"),
                time_label(*submitted_at_ms)
            ))
            .child(
                div().text_xs().text_color(palette.muted).child(
                    commit_oid
                        .clone()
                        .unwrap_or_else(|| "Review commit unknown".into()),
                ),
            )
            .child(body.clone()),
        PrRecord::Thread {
            resolved,
            outdated,
            comments,
            replies_complete,
            ..
        } => {
            let resolved = match resolved {
                Some(true) => "Resolved",
                Some(false) => "Unresolved",
                None => "Resolution unknown",
            };
            let outdated = match outdated {
                Some(true) => "Outdated",
                Some(false) => "Current",
                None => "Outdated state unknown",
            };
            let mut thread = card.child(format!("{resolved} · {outdated}"));
            for comment in comments {
                let author = comment.author.as_deref().unwrap_or("Unknown reviewer");
                thread = thread.child(
                    div()
                        .p_1()
                        .child(format!("{author} · {}", time_label(comment.created_at_ms)))
                        .children(comment.anchor.as_ref().map(|anchor| {
                            div().text_xs().text_color(palette.muted).child(format!(
                                "{} · {:?} line {}",
                                anchor.path, anchor.side, anchor.line
                            ))
                        }))
                        .child(comment.body.clone()),
                );
            }
            thread.children((!*replies_complete).then(|| {
                div()
                    .text_xs()
                    .text_color(palette.muted)
                    .child("Some replies are unavailable.")
            }))
        }
        PrRecord::Check {
            name,
            status,
            conclusion,
            completed_at_ms,
            ..
        } => card.child(format!(
            "{name} · {status} · {} · {}",
            conclusion.as_deref().unwrap_or("No conclusion reported"),
            time_label(*completed_at_ms)
        )),
        PrRecord::Status {
            context,
            state,
            updated_at_ms,
            ..
        } => card.child(format!(
            "{context} · {state} · {}",
            time_label(*updated_at_ms)
        )),
        _ => return div().into_any_element(),
    };
    card.into_any_element()
}

#[derive(Clone, Copy)]
enum DownloadAction {
    Queue { context: bool, media: bool },
    Cancel(i64),
}

impl ReaderWorkspace {
    fn start_pr_read(&mut self, key: PrReadKey) -> bool {
        self.can_fetch() && self.pr_reads.insert(key)
    }

    fn fetch_pr_resource(
        &mut self,
        tab: &Tab,
        family: PrFamily,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = tab.item;
        let revision = self.current_pr(&tab.id).map_or(0, |pr| pr.data.revision.id);
        let key = PrReadKey(item, family.name(), revision.to_string());
        if !self.start_pr_read(key.clone()) {
            return;
        }
        self.remote(
            window,
            cx,
            move |service, account, cancel| async move {
                let page = service
                    .fetch_pr_family(account, item, family, &cancel)
                    .await?;
                Ok(((), page.contacted))
            },
            move |this, _, window, cx| {
                this.pr_reads.remove(&key);
                this.reload_pr(item, window, cx);
            },
        );
    }

    fn verify_linked_issue(
        &mut self,
        tab: &Tab,
        body: String,
        position: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = tab.item;
        let key = PrReadKey(item, "linked-issue", format!("{body}:{position}"));
        if !self.start_pr_read(key.clone()) {
            return;
        }
        self.remote(
            window,
            cx,
            move |service, account, cancel| async move {
                service
                    .verify_pr_reference(account, item, &body, position, &cancel)
                    .await?;
                Ok(((), true))
            },
            move |this, _, window, cx| {
                this.pr_reads.remove(&key);
                this.reload_pr(item, window, cx);
            },
        );
    }

    fn fetch_pr_source(
        &mut self,
        tab: &Tab,
        file: String,
        side: SourceSide,
        line: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pr) = self.current_pr(&tab.id) else {
            return;
        };
        let revision = pr.data.revision.clone();
        let item = tab.item;
        let key = PrReadKey(
            item,
            "source",
            format!("{}:{side:?}:{line}:{file}", revision.id),
        );
        if !self.start_pr_read(key.clone()) {
            return;
        }
        self.remote(
            window,
            cx,
            move |service, account, cancel| async move {
                service
                    .fetch_pr_source(
                        account,
                        item,
                        crate::reader::PrSourceRequest {
                            revision: &revision,
                            file: &file,
                            side,
                            first_line: line,
                            lines: 30,
                        },
                        &cancel,
                    )
                    .await?;
                Ok(((), true))
            },
            move |this, _, window, cx| {
                this.pr_reads.remove(&key);
                this.reload_pr(item, window, cx);
            },
        );
    }

    fn pr_download(
        &mut self,
        tab: &Tab,
        action: DownloadAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pr) = self.current_pr(&tab.id) else {
            return;
        };
        let request = pr.data.revision.clone();
        let revision_id = request.id;
        let item = tab.item;
        self.launch(
            window,
            cx,
            move |store| match action {
                DownloadAction::Queue { context, media } => store
                    .request_pr_download(&request, context, media, now_ms())
                    .map(|_| true),
                DownloadAction::Cancel(generation) => {
                    store.cancel_pr_download(&request, generation)
                }
            },
            move |this, result, window, cx| {
                match result {
                    Ok(true) => {
                        if let Some(worker) = this.sync_worker.as_ref() {
                            match action {
                                DownloadAction::Queue { .. } => worker.wake(None),
                                DownloadAction::Cancel(generation) => {
                                    worker.cancel_pr_download(revision_id, generation);
                                }
                            }
                        }
                    }
                    Ok(false) => {}
                    Err(_) => this.status = Some("The offline download could not be updated."),
                }
                this.reload_pr(item, window, cx);
            },
        );
    }
}

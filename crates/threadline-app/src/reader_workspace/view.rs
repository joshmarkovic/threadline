use super::{remote::RemoteFamily, *};
use crate::palette::Palette;
use gpui_kit::base::TestTextExt;
use gpui_kit::{
    AnyElement, App, ClickEvent, Div, InteractiveElement, IntoElement, ParentElement, Render, Role,
    StatefulInteractiveElement, Styled, TestSupportExt,
    component::{
        Disableable, Selectable, Sizable,
        button::Button,
        input::{Input, Textarea},
    },
    div,
    prelude::FluentBuilder,
    rgb,
};
use threadline_storage::{
    ItemKind,
    queue::DraftKind,
    reader::{BrowseRow, DownloadState, KindFilter, KnownCount, StateFilter},
    resources::{Coverage, ResourceError, ResourceState},
};

#[derive(Clone, Copy)]
enum LayoutAction {
    Split,
    CloseTab,
    ClosePane,
    MoveTab,
    Reorder,
    Cycle,
    Focus,
    Resize(f64),
}

impl ReaderWorkspace {
    #[cfg(feature = "e2e")]
    pub(crate) fn set_observer(
        &mut self,
        observer: Option<crate::observation::Observer>,
        cx: &mut Context<Self>,
    ) {
        self.observer = observer;
        cx.notify();
    }

    pub(super) fn observed_control(
        &self,
        name: impl Into<String>,
        control: impl IntoElement,
        stretch: bool,
    ) -> AnyElement {
        #[cfg(feature = "e2e")]
        if let Some(observer) = &self.observer {
            return div()
                .relative()
                .when(stretch, |view| view.flex_1().min_w_0())
                .child(control)
                .child(observer.visible_control_bounds(name.into()))
                .into_any_element();
        }
        #[cfg(not(feature = "e2e"))]
        let _ = (name, stretch);
        control.into_any_element()
    }

    fn edit_layout(&mut self, action: LayoutAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.layout_busy {
            return;
        }
        self.capture_scroll(cx);
        let mut layout = self.layout.clone();
        let source = layout.focused_pane;
        let count = layout.panes.len();
        let pane = &mut layout.panes[source];
        match action {
            LayoutAction::Split if count < MAX_PANES => {
                layout.panes.push(Pane {
                    active: None,
                    tabs: vec![],
                });
                layout.proportions = vec![1. / (count + 1) as f64; count + 1];
                layout.focused_pane = count;
            }
            LayoutAction::CloseTab => {
                let Some(tab) = pane.active else { return };
                if !self.may_close(&[pane.tabs[tab].id.clone()], cx) {
                    cx.notify();
                    return;
                }
                pane.tabs.remove(tab);
                pane.active = last_tab(pane, tab);
            }
            LayoutAction::ClosePane if count > 1 => {
                let ids = pane
                    .tabs
                    .iter()
                    .map(|tab| tab.id.clone())
                    .collect::<Vec<_>>();
                if !self.may_close(&ids, cx) {
                    cx.notify();
                    return;
                }
                layout.panes.remove(source);
                layout.proportions = vec![1. / (count - 1) as f64; count - 1];
                layout.focused_pane = source.min(count - 2);
            }
            LayoutAction::MoveTab if count > 1 => {
                let Some(index) = pane.active else { return };
                let tab = pane.tabs.remove(index);
                pane.active = last_tab(pane, index);
                let target = (source + 1) % count;
                let adjacent = &mut layout.panes[target];
                adjacent.tabs.push(tab);
                adjacent.active = last_tab(adjacent, usize::MAX);
                layout.focused_pane = target;
            }
            LayoutAction::Reorder => {
                let Some(index) = pane.active.filter(|index| *index > 0) else {
                    return;
                };
                pane.tabs.swap(index, index - 1);
                pane.active = Some(index - 1);
            }
            LayoutAction::Cycle => {
                let Some(index) = pane.active else { return };
                pane.active = Some((index + 1) % pane.tabs.len());
            }
            LayoutAction::Focus => layout.focused_pane = (source + 1) % count,
            LayoutAction::Resize(delta) if count > 1 => {
                let adjacent = (source + 1) % count;
                let neighbor = layout.proportions[adjacent];
                let focused = layout.proportions[source];
                let applied = delta.clamp(0.17 - focused, neighbor - 0.17);
                layout.proportions[source] += applied;
                layout.proportions[adjacent] -= applied;
            }
            _ => return,
        }
        let focused = layout.focused_pane;
        self.save_layout(layout, window, cx);
        if let Some(focus) = self.focus.get(focused) {
            focus.focus(window, cx);
        }
    }

    fn select_tab(&mut self, tab_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.departure.is_some() {
            return;
        }
        if self.layout_busy {
            self.pending_tab_selection = Some(tab_id.to_owned());
            return;
        }
        self.activate_tab(tab_id, window, cx);
    }

    pub(super) fn apply_pending_tab_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab_id) = self.pending_tab_selection.take() {
            self.activate_tab(&tab_id, window, cx);
        }
    }

    fn activate_tab(&mut self, tab_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((pane, tab)) = self
            .layout
            .panes
            .iter()
            .enumerate()
            .find_map(|(index, pane)| {
                pane.tabs
                    .iter()
                    .position(|tab| tab.id == tab_id)
                    .map(|tab| (index, tab))
            })
        else {
            return;
        };
        if self.layout.focused_pane == pane && self.layout.panes[pane].active == Some(tab) {
            self.focus[pane].focus(window, cx);
            self.restore_pr_line_selection(&self.layout.panes[pane].tabs[tab], cx);
            return;
        }
        self.capture_scroll(cx);
        let mut layout = self.layout.clone();
        layout.focused_pane = pane;
        layout.panes[pane].active = Some(tab);
        let target = layout.panes[pane].tabs[tab].clone();
        self.restore_pr_line_selection(&target, cx);
        self.save_layout_with_tab(layout, Some(target), true, None, window, cx);
        self.focus[pane].focus(window, cx);
    }

    fn set_scope(&mut self, scope: ReaderScope, window: &mut Window, cx: &mut Context<Self>) {
        self.scope = scope;
        self.remote_search = None;
        self.remote_generation += 1;
        self.browse(false, window, cx);
    }

    fn pin(&mut self, row: BrowseRow, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        self.launch(
            window,
            cx,
            move |store| store.set_pinned_item(account, row.item, !row.pinned),
            |this, result, window, cx| {
                if result.is_ok() {
                    this.browse(false, window, cx)
                } else {
                    this.status = Some("The pin could not be saved.")
                }
            },
        );
    }

    pub(super) fn open_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let raw = self.url.read(cx).value().to_string();
        self.navigate_url(&raw, window, cx);
    }

    pub(super) fn navigate_url(&mut self, raw: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate_item_url(raw, OpenTab::Activation, window, cx);
    }

    pub(super) fn navigate_markdown_url(
        &mut self,
        raw: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.navigate_item_url(raw, OpenTab::CurrentPane, window, cx);
    }

    fn navigate_item_url(
        &mut self,
        raw: &str,
        mode: OpenTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(link) = parse_item_url(raw) else {
            self.status = Some("Enter an issue or pull request URL on github.com.");
            cx.notify();
            return;
        };
        self.store_route(link, mode, window, cx);
    }

    fn downloaded_action(
        &mut self,
        tab: Tab,
        job: Option<(i64, DownloadState)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        self.launch(
            window,
            cx,
            move |store| {
                let cancelled =
                    if let Some((generation, DownloadState::Pending | DownloadState::Active)) = job
                    {
                        store
                            .cancel_issue_download(account, tab.item, generation)?
                            .then_some(generation)
                    } else {
                        store.request_issue_download(account, tab.item, now_ms())?;
                        None
                    };
                Ok((cancelled, tab))
            },
            move |this, result, window, cx| match result {
                Ok((cancelled, tab)) => {
                    if let Some(worker) = this.sync_worker.as_ref() {
                        if let Some(generation) = cancelled {
                            worker.cancel_issue_download(account, tab.item, generation);
                        }
                        worker.wake(None);
                    }
                    this.load_tab(tab, window, cx);
                }
                Err(_) => this.status = Some("The offline download request could not be changed."),
            },
        );
    }

    fn load_comments(
        &mut self,
        tab: Tab,
        cursor: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        let actor = self.actor;
        let id = tab.id;
        let query = cursor.clone();
        self.launch(
            window,
            cx,
            move |store| {
                let page = store.discussion_page(account, tab.item, Some(&query), PAGE)?;
                let editable =
                    super::outbox::editable_comments(store, account, tab.item, actor, &page)?;
                Ok((page, editable))
            },
            move |this, result, _, cx| {
                let Some(view) = this.tab_views.get_mut(&id) else {
                    return;
                };
                if view.data.discussion.next.as_ref() != Some(&cursor) {
                    return;
                }
                if let Ok((page, editable)) = result {
                    for record in page.records {
                        if !view.notes.iter().any(|(key, _)| *key == record.key) {
                            view.notes.push((
                                record.key.clone(),
                                media::CachedMarkdown::new(&record.body, cx),
                            ));
                            view.data.discussion.records.push(record);
                        }
                    }
                    view.data.editable_comments.extend(editable);
                    view.data.discussion.metadata.extend(page.metadata);
                    view.data.discussion.next = page.next;
                    view.data.discussion.state = page.state;
                }
            },
        );
    }

    fn apply_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let get = |input: &Entity<InputState>| {
            let text = input.read(cx).value().trim().to_owned();
            (!text.is_empty()).then_some(text)
        };
        let since = get(&self.dates_after);
        let before = get(&self.dates_before);
        let updated_since_ms = since.as_deref().and_then(parse_date);
        let updated_before_ms = before.as_deref().and_then(parse_date);
        if since.is_some() != updated_since_ms.is_some()
            || before.is_some() != updated_before_ms.is_some()
        {
            self.status = Some("Use YYYY-MM-DD for date filters.");
            cx.notify();
            return;
        }
        self.filter.author = get(&self.author);
        self.filter.label = get(&self.label);
        self.filter.assignee = get(&self.assignee);
        self.filter.reviewer = get(&self.reviewer);
        self.filter.updated_since_ms = updated_since_ms;
        self.filter.updated_before_ms = updated_before_ms;
        self.browse(false, window, cx);
    }

    fn save_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        let name = self.saved_name.read(cx).value().trim().to_owned();
        let filter = self.filter.clone();
        if name.is_empty() {
            self.status = Some("Enter a name for this saved view.");
            cx.notify();
            return;
        }
        self.launch(
            window,
            cx,
            move |store| store.save_reader_view(account, &name, &filter),
            |this, result, window, cx| {
                if result.is_ok() {
                    this.browse(false, window, cx)
                } else {
                    this.status = Some("The view could not be saved.")
                }
            },
        );
    }

    fn clean_downloads(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(account) = self.account else { return };
        self.launch(window,cx,move |store|store.clear_cache(account),|this,result,window,cx|{
            if result.is_ok(){
                this.tab_views.clear();
                this.tab_summaries.clear();
                this.load_visible(window,cx);
                this.show_cache(window,cx);
                this.status=Some("Unpinned downloads were cleared. Draft and Outbox dependencies remain available.");
            } else {
                this.status=Some("Downloaded content could not be cleared.");
            }
        });
    }

    pub(crate) fn connection_label(&self) -> (&'static str, gpui_kit::Hsla) {
        use threadline_core::connectivity::ConnectionStatus;
        if self.offline {
            return ("Working offline", rgb(0xb93442).into());
        }
        if let Some(connection) = self.shared_connection() {
            return match connection.status {
                ConnectionStatus::Checking => ("Checking GitHub", rgb(0x8b785b).into()),
                ConnectionStatus::Stale => ("Connection status stale", rgb(0x8b785b).into()),
                ConnectionStatus::Reachable => ("GitHub reachable", rgb(0x28804a).into()),
                ConnectionStatus::Unreachable => ("Cannot reach GitHub", rgb(0xb93442).into()),
                ConnectionStatus::Offline => ("Working offline", rgb(0xb93442).into()),
            };
        }
        if self.network_pending > 0 && self.reachability == Reachability::NotAttempted {
            return ("Checking GitHub", rgb(0x8b785b).into());
        }
        if self.reachability == Reachability::NoResponse {
            return ("Cannot reach GitHub", rgb(0xb93442).into());
        }
        if self.reachability == Reachability::ReceivedResponse
            && self
                .contact
                .is_some_and(|time| now_ms().saturating_sub(time) < 300_000)
        {
            return ("GitHub reachable", rgb(0x28804a).into());
        }
        ("Connection status stale", rgb(0x8b785b).into())
    }

    fn sidebar(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let count_scope = match self.scope {
            ReaderScope::Selected => "selected repositories".into(),
            ReaderScope::Repository(id) => self
                .repositories
                .iter()
                .find(|repo| repo.id == id)
                .map_or_else(
                    || "current repository".into(),
                    |repo| format!("{}/{}", repo.owner, repo.name),
                ),
        };
        let [review, assign, outbox] = self.counts.map_or([None; 3], |counts| {
            [
                Some(counts.review_requested),
                Some(counts.assigned),
                Some(KnownCount {
                    known: counts.outbox,
                    complete: true,
                    last_validated_at_ms: None,
                }),
            ]
        });
        let overview = div()
            .flex()
            .flex_col()
            .gap_1()
            .child(rail(
                "reader-review",
                "Review requested",
                &count_scope,
                review,
                cx.listener(|this, _, window, cx| {
                    this.filter = BrowseFilter {
                        reviewer: Some(this.login.clone()),
                        ..BrowseFilter::default()
                    };
                    this.browse(false, window, cx);
                }),
            ))
            .child(rail(
                "reader-assigned",
                "Assigned to me",
                &count_scope,
                assign,
                cx.listener(|this, _, window, cx| {
                    this.filter = BrowseFilter {
                        assignee: Some(this.login.clone()),
                        ..BrowseFilter::default()
                    };
                    this.browse(false, window, cx);
                }),
            ))
            .child(self.observed_control(
                "reader-outbox",
                rail(
                    "reader-outbox",
                    "Outbox",
                    "active account",
                    outbox,
                    cx.listener(|this, _, window, cx| this.show_outbox(false, window, cx)),
                ),
                false,
            ));
        div()
            .id("reader-navigation")
            .role(Role::Group)
            .aria_label("Repository navigation")
            .w(px(221.))
            .flex_shrink_0()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .border_r_1()
            .border_color(palette.border)
            .bg(palette.panel)
            .child(overview)
            .child(self.observed_control(
                "reader-browse",
                control("reader-browse", "Selected repositories").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.filter = BrowseFilter::default();
                        this.set_scope(ReaderScope::Selected, window, cx);
                    },
                )),
                false,
            ))
            .children(
                self.repositories
                    .iter()
                    .filter(|repo| repo.selected)
                    .map(|repo| {
                        let id = repo.id;
                        let name = format!("reader-repository-{}", id.0);
                        let button = control(name.clone(), format!("{}/{}", repo.owner, repo.name))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.set_scope(ReaderScope::Repository(id), window, cx)
                            }));
                        self.observed_control(name, button, false)
                    })
                    .collect::<Vec<_>>(),
            )
            .child(control("reader-pinned", "Pinned").on_click(cx.listener(
                |this, _, window, cx| {
                    this.filter = BrowseFilter {
                        pinned_only: true,
                        ..Default::default()
                    };
                    this.browse(false, window, cx);
                },
            )))
            .child(control("reader-recent", "Recent").on_click(cx.listener(
                |this, _, window, cx| {
                    this.filter = BrowseFilter {
                        recent_only: true,
                        ..Default::default()
                    };
                    this.browse(false, window, cx);
                },
            )))
            .children(
                self.saved
                    .iter()
                    .enumerate()
                    .map(|(index, (name, filter))| {
                        let filter = filter.clone();
                        control(format!("reader-saved-{index}"), name.clone()).on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.filter = filter.clone();
                                this.browse(false, window, cx);
                            }),
                        )
                    })
                    .collect::<Vec<_>>(),
            )
            .child(self.observed_control(
                "reader-activity",
                control("reader-activity", "Activity").on_click(
                    cx.listener(|this, _, window, cx| this.show_activity(false, window, cx)),
                ),
                false,
            ))
            .child(
                self.observed_control(
                    "reader-history",
                    control("reader-history", "History")
                        .on_click(cx.listener(|this, _, window, cx| this.show_history(window, cx))),
                    false,
                ),
            )
            .child(
                control("reader-storage", "Storage usage")
                    .on_click(cx.listener(|this, _, window, cx| this.show_cache(window, cx))),
            )
            .child(
                self.observed_control(
                    "reader-sync-settings",
                    control("reader-sync-settings", "Synchronization")
                        .on_click(cx.listener(|this, _, window, cx| this.show_sync(window, cx))),
                    false,
                ),
            )
    }

    fn browse_view(&self, palette: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let filter_modes = [
            ("All items", KindFilter::All),
            ("Issues", KindFilter::Issues),
            ("Pull requests", KindFilter::PullRequests),
        ];
        let state_modes = [
            ("Any state", StateFilter::All),
            ("Open", StateFilter::Open),
            ("Closed", StateFilter::Closed),
            ("Merged", StateFilter::Merged),
        ];
        let mut hits = div()
            .id("reader-results")
            .role(Role::Group)
            .aria_label("Downloaded items")
            .max_h(px(245.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1();
        if let Some(found) = &self.search_hits {
            hits = hits
                .child(div().text_sm().child(if found.is_empty() {
                    "No matches in downloaded content."
                } else {
                    "Matches in downloaded content"
                }))
                .children(
                    found
                        .iter()
                        .enumerate()
                        .map(|(index, hit)| {
                            let anchor = hit.anchor.as_ref().map(|anchor| {
                                if hit.row.kind == ItemKind::PullRequest
                                    && hit.source == threadline_storage::reader::HitSource::Comment
                                {
                                    format!("{PR_DISCUSSION_ANCHOR}{anchor}")
                                } else {
                                    anchor.clone()
                                }
                            });
                            self.result_row(
                                index,
                                &hit.row,
                                anchor,
                                Some(hit.excerpt.clone()),
                                palette,
                                cx,
                            )
                        })
                        .collect::<Vec<_>>(),
                );
        } else {
            hits = hits.children(
                self.rows
                    .iter()
                    .enumerate()
                    .map(|(index, row)| self.result_row(index, row, None, None, palette, cx))
                    .collect::<Vec<_>>(),
            );
            if self.rows.is_empty() && !self.list_busy {
                let coverage = self.coverage.map(|(coverage, _)| coverage);
                hits=hits.child(div().text_sm().child(if coverage==Some(Coverage::Complete) {"No cached items in this view."} else {"This view has no downloaded items. Repository history might be incomplete."}));
            }
            if self.next.is_some() && !self.list_busy {
                hits = hits
                    .child(control("reader-more", "More downloaded items").on_click(
                        cx.listener(|this, _, window, cx| this.browse(true, window, cx)),
                    ));
            }
        }
        div()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(palette.panel)
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        self.observed_control(
                            "reader-search",
                            Input::new(&self.search)
                                .small()
                                .accessibility_id("reader-search")
                                .aria_label("Search downloaded content"),
                            true,
                        ),
                    )
                    .child(self.observed_control(
                        "reader-search-submit",
                        control("reader-search-submit", "Search downloaded").on_click(
                            cx.listener(|this, _, window, cx| this.local_search(window, cx)),
                        ),
                        false,
                    ))
                    .child(
                        control("reader-search-github", "Search GitHub")
                            .disabled(!self.can_fetch())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.search_github(false, window, cx)
                            })),
                    )
                    .child({
                        let enabled = self.can_fetch();
                        let button = control("reader-fetch-repository", "Download repository page")
                            .disabled(!enabled)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.load_repository(window, cx)),
                            );
                        if enabled {
                            self.observed_control("reader-fetch-repository", button, false)
                        } else {
                            button.into_any_element()
                        }
                    })
                    .child(control("reader-filters", "Filters").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.filters_open = !this.filters_open;
                            cx.notify();
                        },
                    ))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        Input::new(&self.url)
                            .small()
                            .accessibility_id("reader-url")
                            .aria_label("GitHub issue or pull request URL"),
                    )
                    .child(
                        control("reader-open-url", "Open URL")
                            .on_click(cx.listener(|this, _, window, cx| this.open_url(window, cx))),
                    ),
            )
            .when(self.filters_open, |element| {
                element.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_1()
                                .children(
                                    filter_modes
                                        .into_iter()
                                        .enumerate()
                                        .map(|(i, (label, mode))| {
                                            control(format!("reader-kind-{i}"), label)
                                                .selected(self.filter.kind == mode)
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.filter.kind = mode;
                                                        this.browse(false, window, cx)
                                                    },
                                                ))
                                        })
                                        .collect::<Vec<_>>(),
                                )
                                .children(
                                    state_modes
                                        .into_iter()
                                        .enumerate()
                                        .map(|(i, (label, mode))| {
                                            control(format!("reader-state-{i}"), label)
                                                .selected(self.filter.state == mode)
                                                .on_click(cx.listener(
                                                    move |this, _, window, cx| {
                                                        this.filter.state = mode;
                                                        this.browse(false, window, cx)
                                                    },
                                                ))
                                        })
                                        .collect::<Vec<_>>(),
                                ),
                        )
                        .child(
                            div().flex().gap_1().children(
                                [
                                    (&self.author, "Author"),
                                    (&self.label, "Label"),
                                    (&self.assignee, "Assignee"),
                                    (&self.reviewer, "Reviewer"),
                                    (&self.dates_after, "Updated after"),
                                    (&self.dates_before, "Updated before"),
                                ]
                                .into_iter()
                                .map(|(input, label)| Input::new(input).small().aria_label(label)),
                            ),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_1()
                                .child(control("reader-apply-filters", "Apply filters").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.apply_filters(window, cx)
                                    }),
                                ))
                                .child(
                                    Input::new(&self.saved_name)
                                        .small()
                                        .aria_label("Saved view name"),
                                )
                                .child(control("reader-save-filter", "Save view").on_click(
                                    cx.listener(|this, _, window, cx| this.save_filter(window, cx)),
                                )),
                        ),
                )
            })
            .children(self.list_busy.then(|| {
                div()
                    .text_xs()
                    .text_color(palette.muted)
                    .child("Reading the local cache…")
            }))
            .children(self.coverage.map(|(coverage, age)| {
                div()
                    .text_xs()
                    .text_color(palette.muted)
                    .child(coverage_label("Repository history", coverage, age))
            }))
            .child(hits)
            .children(self.remote_search.as_ref().map(|result| {
                let repo = result.repository;
                let coverage = if result.page.capped || result.page.incomplete {
                    " · results limited by GitHub"
                } else {
                    ""
                };
                div()
                    .id("reader-remote-results")
                    .role(Role::Group)
                    .aria_label("GitHub search results")
                    .max_h(px(156.))
                    .overflow_y_scroll()
                    .child(div().text_sm().child(format!(
                        "GitHub search · {} reported matches{coverage}",
                        result.page.reported_total
                    )))
                    .children(
                        result
                            .page
                            .hits
                            .iter()
                            .enumerate()
                            .map(|(index, hit)| {
                                let hit = hit.clone();
                                control(
                                    format!("reader-remote-result-{index}"),
                                    format!("#{} {}", hit.number.get(), hit.title),
                                )
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.open_remote(repo, hit.clone(), window, cx)
                                    },
                                ))
                            })
                            .collect::<Vec<_>>(),
                    )
                    .children(result.page.next_page.map(|_| {
                        control("reader-search-github-more", "More GitHub results").on_click(
                            cx.listener(|this, _, window, cx| this.search_github(true, window, cx)),
                        )
                    }))
            }))
    }

    fn result_row(
        &self,
        index: usize,
        row: &BrowseRow,
        anchor: Option<String>,
        snippet: Option<String>,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> Div {
        let item = row.item;
        let beside_anchor = anchor.clone();
        let pin_row = row.clone();
        let title = row.title.as_deref().unwrap_or("Title not downloaded");
        let label = format!(
            "{} {}/{} #{} · {title}",
            kind_label(row.kind),
            row.owner,
            row.name,
            row.number
        );
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .border_b_1()
            .border_color(palette.border)
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .items_start()
                    .child(self.observed_control(
                        format!("reader-result-{index}"),
                        control(format!("reader-result-{index}"), label).on_click(cx.listener(
                            move |this, _, window, cx| {
                                #[cfg(feature = "e2e")]
                                this.performance.start(performance::Action::Open(item.0));
                                this.open(item, anchor.clone(), OpenTab::CurrentPane, window, cx)
                            },
                        )),
                        false,
                    ))
                    .children(
                        snippet.map(|text| div().text_xs().text_color(palette.muted).child(text)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .child(self.observed_control(
                        format!("reader-beside-{index}"),
                        control(format!("reader-beside-{index}"), "Open beside").on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.open(item, beside_anchor.clone(), OpenTab::Beside, window, cx)
                            }),
                        ),
                        false,
                    ))
                    .child(
                        control(
                            format!("reader-pin-{index}"),
                            if row.pinned { "Unpin" } else { "Pin" },
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| this.pin(pin_row.clone(), window, cx),
                        )),
                    ),
            )
    }

    fn pane(
        &self,
        index: usize,
        pane: &Pane,
        pane_width: f32,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let focused = index == self.layout.focused_pane;
        let active = pane.active.and_then(|i| pane.tabs.get(i));
        let tab_width = px(((pane_width - 20.) / 2.).clamp(108., 220.));
        let tabs = div()
            .id(format!("reader-tabs-{index}"))
            .role(Role::Group)
            .aria_label(format!("Pane {} tabs", index + 1))
            .overflow_x_scroll()
            .flex()
            .gap_1()
            .flex_shrink_0()
            .p_1()
            .bg(palette.surface)
            .children(
                pane.tabs
                    .iter()
                    .enumerate()
                    .map(|(position, tab)| {
                        let mut label = self.tab_summaries.get(&tab.item).map_or_else(
                            || "Loading item…".to_owned(),
                            |row| {
                                format!(
                                    "{} {}/{} #{} {}",
                                    kind_label(row.kind),
                                    row.owner,
                                    row.name,
                                    row.number,
                                    row.title.as_deref().unwrap_or("No downloaded title")
                                )
                            },
                        );
                        if tab.view.draft.is_some() {
                            label.push_str(" · Draft");
                        }
                        let name = format!("reader-tab-{index}-{position}");
                        let tab_id = tab.id.clone();
                        #[cfg(feature = "e2e")]
                        let item = tab.item;
                        let selected = pane.active == Some(position);
                        let accessibility_label = if selected {
                            format!("{label} · Active tab")
                        } else {
                            label.clone()
                        };
                        let button = control(name.clone(), label)
                            .w(tab_width)
                            .flex_shrink_0()
                            .selected(selected)
                            .accessibility_label(accessibility_label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                #[cfg(feature = "e2e")]
                                this.performance.start(performance::Action::Tab(item.0));
                                this.select_tab(&tab_id, window, cx)
                            }));
                        self.observed_control(name, button, false)
                    })
                    .collect::<Vec<_>>(),
            );
        let current = active
            .map(|tab| {
                match self
                    .tab_views
                    .get(&tab.id)
                    .and_then(|view| view.data.item.as_ref())
                {
                    Some(item) if item.kind == ItemKind::PullRequest => {
                        self.pull_request_view(index, tab, item, pane_width, palette, cx)
                    }
                    _ => self.item_view(index, tab, palette, cx),
                }
            })
            .unwrap_or_else(|| {
                div()
                    .p_4()
                    .child("Open a downloaded issue or pull request.")
                    .into_any_element()
            });
        div()
            .id(format!("reader-pane-{index}"))
            .test_support()
            .role(Role::Group)
            .aria_label(format!("Pane {}", index + 1))
            .track_focus(&self.focus[index])
            .tab_stop(true)
            .min_w_0()
            .min_h_0()
            .h_full()
            .flex()
            .flex_col()
            .flex_basis(px(0.))
            .flex_grow(1.)
            .border_1()
            .border_color(if focused {
                palette.accent
            } else {
                palette.border
            })
            .bg(palette.panel)
            .child(tabs)
            .child(current)
    }

    pub(super) fn item_view(
        &self,
        pane: usize,
        tab: &Tab,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(view) = self.tab_views.get(&tab.id) else {
            return div()
                .p_4()
                .text_sm()
                .child(if self.pending_tabs.contains_key(&tab.id) {
                    "Loading downloaded item…"
                } else {
                    "This item is unavailable in the local cache."
                })
                .into_any_element();
        };
        let Some(item) = view.data.item.as_ref() else {
            return div()
                .p_4()
                .child("This item has not been downloaded. Its tab was retained.")
                .into_any_element();
        };
        let title = match &item.title {
            Change::Value(title) => title.as_str(),
            _ => "Title not downloaded",
        };
        let state = match item.closed {
            Change::Value(true) => "Closed",
            Change::Value(false) => "Open",
            _ => "State not downloaded",
        };
        let mut notes = div().flex().flex_col().gap_3();
        for (index, (key, text)) in view.notes.iter().enumerate() {
            let selected = tab.view.scroll_anchor.as_ref() == Some(key);
            let metadata = view
                .data
                .discussion
                .metadata
                .iter()
                .find(|entry| entry.key == *key);
            notes = notes.child(
                div()
                    .p_2()
                    .border_b_1()
                    .border_color(if selected {
                        palette.accent
                    } else {
                        palette.border
                    })
                    .children(metadata.map(|entry| {
                        div().text_xs().text_color(palette.muted).child(format!(
                            "{} · {}",
                            entry.author.as_deref().unwrap_or("Unknown author"),
                            time_label(Some(entry.created_at_ms))
                        ))
                    }))
                    .child(self.markdown_element(text, Some(item.locator.repository), palette, cx))
                    .children(
                        view.data
                            .editable_comments
                            .get(key)
                            .copied()
                            .zip(view.data.discussion.records.get(index))
                            .map(|(comment, original)| {
                                let id = format!("reader-edit-comment-{pane}-{index}");
                                let body = original.body.clone();
                                let tab = tab.clone();
                                self.observed_control(
                                    id.clone(),
                                    control(id, "Edit my comment").on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.edit_comment(
                                                tab.clone(),
                                                comment,
                                                body.clone(),
                                                window,
                                                cx,
                                            );
                                        },
                                    )),
                                    false,
                                )
                            }),
                    ),
            );
        }
        let job = view
            .data
            .download
            .as_ref()
            .map(|job| (job.generation, job.state));
        let download = tab.clone();
        let edit = tab.clone();
        let comment = tab.clone();
        let editor = tab
            .view
            .draft
            .as_ref()
            .and_then(|id| self.drafts.get(id))
            .filter(|editor| {
                !matches!(
                    editor.write_base,
                    Some(super::write_composer::WriteBase::ReviewReply { .. })
                )
            })
            .filter(|editor| editor.tab_id == tab.id);
        let title_id = format!("reader-issue-title-label-{pane}");
        let content = div()
            .id(format!("reader-conversation-{pane}"))
            .test_support()
            .role(Role::Group)
            .aria_label("Downloaded conversation")
            .track_scroll(&view.scroll)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .child(
                div()
                    .id(title_id.clone())
                    .accessibility_id(title_id)
                    .role(Role::Heading)
                    .aria_label(title.to_owned())
                    .text_lg()
                    .child(title.to_owned()),
            )
            .child(div().text_xs().text_color(palette.muted).child(format!(
                "{} #{} · {state}",
                kind_label(item.kind),
                item.locator.number
            )))
            .children(view.data.metadata.as_ref().map(|metadata|item_metadata(metadata,palette)))
            .children(
                view.body
                    .as_ref()
                    .map(|text| self.markdown_element(text,Some(item.locator.repository),palette,cx)),
            )
            .children(view.body.is_none().then(|| {
                div().text_sm().text_color(palette.muted).child(
                    if matches!(item.body, Change::Null) {
                        "No description."
                    } else {
                        "Description has not been downloaded."
                    },
                )
            }))
            .child(div().text_sm().child("Discussion"))
            .child(
                div()
                    .text_xs()
                    .text_color(palette.muted)
                    .child(coverage_label(
                        "Comments",
                        view.data.discussion.state.coverage,
                        view.data.discussion.state.last_fetch_at_ms,
                    )),
            )
            .children(view.data.discussion.state.error.map(|_|resource_problem("Comments",&view.data.discussion.state,palette)))
            .children(view.notes.is_empty().then(|| {
                div()
                    .text_sm()
                    .child(if view.data.discussion.state.shows_empty(0) {
                        "No comments."
                    } else {
                        "No comments have been downloaded."
                    })
            }))
            .child(notes)
            .children(view.data.anchor_missing.then(||div().text_sm().child("The selected discussion location is unavailable in the downloaded conversation.")))
            .child(div().text_sm().child("Timeline"))
            .child(div().text_xs().text_color(palette.muted).child(coverage_label("Timeline",view.data.timeline.state.coverage,view.data.timeline.state.last_fetch_at_ms)))
            .children(view.data.timeline.state.error.map(|_|resource_problem("Timeline",&view.data.timeline.state,palette)))
            .children(view.data.timeline.entries.iter().enumerate().map(|(position,event)|{
                let target=view.data.linked_items.get(&event.remote_key).copied();
                div().flex().flex_col().gap_1()
                    .child(div().text_sm().child(format!("{} · {} · {}",event.event,
                        event.actor.as_deref().unwrap_or("Unknown actor"),time_label(event.occurred_at_ms))))
                    .children(event.linked_item.as_ref().map(|linked|{
                        let label=format!("{}/{} #{}",linked.owner,linked.repository,linked.number);
                        match target {
                            Some(item)=>control(format!("reader-timeline-link-{pane}-{position}"),label)
                                .on_click(cx.listener(move|this,_,window,cx|this.open(item,None,OpenTab::CurrentPane,window,cx)))
                                .into_any_element(),
                            None=>div().text_xs().text_color(palette.muted)
                                .child(format!("Related item {label} · verification unavailable in the local cache"))
                                .into_any_element(),
                        }
                    }))
            }).collect::<Vec<_>>())
            .children(view.data.projections.iter().enumerate().map(|(index,projection)|
                self.pending_write_projection(pane,index,tab,projection,palette,cx)).collect::<Vec<_>>())
            .children(view.data.discussion.next.as_ref().map(|cursor| {
                let cursor = cursor.clone();
                let more = tab.clone();
                control(
                    format!("reader-more-comments-{pane}"),
                    "More downloaded comments",
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.load_comments(more.clone(), cursor.clone(), window, cx)
                }))
            }));
        let mut toolbar = div()
            .px_3()
            .pt_2()
            .flex()
            .flex_wrap()
            .gap_2()
            .children((item.kind == ItemKind::Issue).then(|| {
                let id = format!("reader-edit-issue-{pane}");
                self.observed_control(
                    id.clone(),
                    control(id, "Edit issue draft").on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.edit(edit.clone(), DraftKind::IssueEdit, window, cx)
                        },
                    )),
                    false,
                )
            }))
            .children((item.kind == ItemKind::Issue && self.can_fetch()).then(|| {
                let tab = tab.clone();
                let id = format!("reader-edit-choices-{pane}");
                self.observed_control(
                    id.clone(),
                    control(id, "Download editing choices").on_click(cx.listener(
                        move |this, _, window, cx| {
                            this.fetch_editing_options(tab.clone(), window, cx)
                        },
                    )),
                    false,
                )
            }))
            .child(self.observed_control(
                format!("reader-draft-comment-{pane}"),
                control(format!("reader-draft-comment-{pane}"), "Draft a comment").on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.edit(comment.clone(), DraftKind::Comment, window, cx)
                    }),
                ),
                false,
            ))
            .children((item.kind == ItemKind::Issue).then(|| {
                let name = format!("reader-download-{pane}");
                let disabled = job.is_some_and(|(_, state)| state == DownloadState::Complete);
                let button = control(
                    name.clone(),
                    match job {
                        Some((_, DownloadState::Pending | DownloadState::Active)) => {
                            "Cancel offline download"
                        }
                        Some((_, DownloadState::Complete)) => "Available offline",
                        _ => "Download for offline use",
                    },
                )
                .disabled(disabled)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.downloaded_action(download.clone(), job, window, cx)
                }));
                if disabled {
                    button.into_any_element()
                } else {
                    self.observed_control(name, button, false)
                }
            }));
        if let Some(download) = &view.data.download {
            let state = download.state;
            let detail = match download.last_error {
                Some(ResourceError::Unauthorized | ResourceError::Forbidden) => {
                    " · GitHub access is limited"
                }
                Some(ResourceError::NotFound | ResourceError::ConfirmedDeletion) => {
                    " · Issue unavailable on GitHub"
                }
                Some(ResourceError::RateLimited) => " · Waiting for GitHub's rate limit",
                Some(ResourceError::InvalidResponse) => {
                    " · GitHub did not provide all issue content"
                }
                Some(ResourceError::CursorExpired) => " · GitHub page cursor expired",
                Some(ResourceError::Transport | ResourceError::Unknown) => {
                    " · Download interrupted"
                }
                None => "",
            };
            toolbar = toolbar.child(div().text_xs().text_color(palette.muted).child(format!(
                "Offline download · {state:?} · {}/3 resources{detail}",
                download.completed_families()
            )));
            if self.can_fetch() && state == DownloadState::Pending && download.last_error.is_some()
            {
                let tab = tab.clone();
                let generation = download.generation;
                toolbar = toolbar.child(
                    self.observed_control(
                        format!("reader-resume-download-{pane}"),
                        control(
                            format!("reader-resume-download-{pane}"),
                            "Retry offline download",
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.resume_download(tab.clone(), generation, window, cx)
                            },
                        )),
                        false,
                    ),
                );
            }
        }
        if self.can_fetch() {
            let loads = [
                ("metadata", "Refresh details", RemoteFamily::Metadata),
                (
                    "comments",
                    "Download discussion page",
                    RemoteFamily::Comments,
                ),
                ("timeline", "Download timeline page", RemoteFamily::Timeline),
            ];
            toolbar = toolbar.child(
                div().flex().flex_wrap().gap_2().children(
                    loads
                        .into_iter()
                        .map(|(name, label, family)| {
                            let tab = tab.clone();
                            let id = format!("reader-fetch-{name}-{pane}");
                            let button = control(id.clone(), label).on_click(cx.listener(
                                move |this, _, window, cx| {
                                    this.fetch_family(tab.clone(), family, window, cx)
                                },
                            ));
                            self.observed_control(id, button, false)
                        })
                        .collect::<Vec<_>>(),
                ),
            );
        }
        let mut header = div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .p_2()
            .gap_2()
            .child(toolbar);
        if let Some(editor) = editor {
            let id = editor.id.clone();
            header = header.child(
                div()
                    .id(format!("reader-draft-{pane}"))
                    .role(Role::Group)
                    .aria_label("Local Markdown draft")
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(self.issue_option_controls(
                        pane,
                        editor,
                        view.data.write_choices.as_ref(),
                        palette,
                        cx,
                    ))
                    .child(
                        div()
                            .id(format!("reader-draft-input-{pane}"))
                            .test_support()
                            .child(
                                self.observed_control(
                                    format!("reader-draft-input-{pane}"),
                                    Textarea::new(&editor.view)
                                        .accessibility_id(format!("reader-draft-input-{pane}"))
                                        .aria_label("Local Markdown draft"),
                                    true,
                                ),
                            ),
                    )
                    .child(div().text_xs().child(if editor.lease_lost {
                        super::drafts::EDITOR_LEASE_LOST
                    } else if editor.failed {
                        "Draft not saved. Edit to retry."
                    } else if editor.saved() {
                        "Local draft saved"
                    } else {
                        "Saving local draft…"
                    }))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(
                                control(format!("reader-draft-preview-{pane}"), "Preview draft")
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| this.preview(&id, cx)),
                                    ),
                            )
                            .child(self.submit_control(pane, editor, cx)),
                    )
                    .children(editor.preview.as_ref().map(|text| {
                        self.markdown_element(text, Some(item.locator.repository), palette, cx)
                    })),
            );
        }
        #[cfg(feature = "e2e")]
        let content = match &self.observer {
            Some(observer) => div()
                .relative()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(content)
                .child(observer.visible_control_bounds(format!("reader-conversation-{pane}")))
                .into_any_element(),
            None => content.into_any_element(),
        };
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .child(header)
            .child(content)
            .into_any_element()
    }
}

gpui_kit::actions!(
    threadline,
    [
        ReaderFind,
        ReaderSplit,
        ReaderCycle,
        ReaderFocus,
        ReaderClose,
        ReaderReopen,
        ReaderMove,
        ReaderDraft
    ]
);

pub(crate) fn bind_shortcuts(cx: &mut App) {
    use gpui_kit::{Action, KeyBinding, KeyBindingContextPredicate};
    let context = std::rc::Rc::new(
        KeyBindingContextPredicate::parse("ThreadlineReader").expect("reader key context"),
    );
    let shortcuts: [(&str, Box<dyn Action>); 8] = [
        ("secondary-shift-f", Box::new(ReaderFind)),
        ("secondary-alt-t", Box::new(ReaderSplit)),
        ("ctrl-tab", Box::new(ReaderCycle)),
        ("secondary-alt-p", Box::new(ReaderFocus)),
        ("secondary-alt-w", Box::new(ReaderClose)),
        ("secondary-shift-t", Box::new(ReaderReopen)),
        ("secondary-alt-m", Box::new(ReaderMove)),
        ("secondary-alt-d", Box::new(ReaderDraft)),
    ];
    for (keys, action) in shortcuts {
        cx.bind_keys([
            KeyBinding::load(keys, action, Some(context.clone()), false, None, cx)
                .expect("reader key binding"),
        ]);
    }
}

impl ReaderWorkspace {
    fn workspace_connection(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        let (connection, color) = self.connection_label();
        let snapshot = self.shared_connection();
        let connection_details = format!(
            "{connection}. Last check {}. Last successful contact {}.",
            time_label(snapshot.map_or(self.checked, |state| state.last_checked_ms)),
            time_label(snapshot.map_or(self.contact, |state| state.last_contact_ms)),
        );

        div()
            .id("reader-connection")
            .test_support()
            .role(Role::Status)
            .aria_label(connection_details.clone())
            .p_2()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .bg(palette.surface)
            .child(div().size(px(9.)).rounded_full().bg(color))
            .child(div().text_sm().child(connection))
            .child(
                div()
                    .text_xs()
                    .text_color(palette.muted)
                    .child(connection_details),
            )
            .children(
                (self.access == AccessState::SignedOut)
                    .then(|| div().text_sm().child("Signed out · local profile")),
            )
            .children(
                self.connection_reason()
                    .map(|reason| div().text_xs().text_color(palette.muted).child(reason)),
            )
            .child(
                control("reader-check-now", "Check now")
                    .disabled(self.offline || self.access == AccessState::SignedOut)
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(crate::workspace::CheckAccountAccess), cx)
                    }),
            )
            .child(
                control(
                    "reader-connection-outbox",
                    self.counts.map_or_else(
                        || "Outbox".into(),
                        |counts| format!("Outbox · {} pending", counts.outbox),
                    ),
                )
                .on_click(cx.listener(|this, _, window, cx| this.show_outbox(false, window, cx))),
            )
            .into_any_element()
    }

    fn workspace_commands(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex_shrink_0()
            .flex()
            .flex_wrap()
            .gap_1()
            .p_2()
            .bg(palette.surface)
            .child(
                control("reader-back", "Back")
                    .disabled(self.back.is_empty() || self.layout_busy)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.navigate_history(false, window, cx)),
                    ),
            )
            .child(
                control("reader-forward", "Forward")
                    .disabled(self.forward.is_empty() || self.layout_busy)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.navigate_history(true, window, cx)),
                    ),
            )
            .children(
                [
                    ("reader-split", "Split pane", LayoutAction::Split),
                    ("reader-focus-pane", "Switch pane", LayoutAction::Focus),
                    ("reader-close-tab", "Close tab", LayoutAction::CloseTab),
                    ("reader-close-pane", "Close pane", LayoutAction::ClosePane),
                    ("reader-move-tab", "Move tab", LayoutAction::MoveTab),
                    ("reader-reorder-tab", "Tab left", LayoutAction::Reorder),
                    (
                        "reader-narrow-pane",
                        "Narrow pane",
                        LayoutAction::Resize(-0.06),
                    ),
                    (
                        "reader-widen-pane",
                        "Widen pane",
                        LayoutAction::Resize(0.06),
                    ),
                ]
                .into_iter()
                .map(|(id, label, action)| {
                    let button =
                        control(id, label)
                            .disabled(self.layout_busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.edit_layout(action, window, cx)
                            }));
                    if self.layout_busy {
                        button.into_any_element()
                    } else {
                        self.observed_control(id, button, false)
                    }
                })
                .collect::<Vec<_>>(),
            )
            .child(
                control("reader-reopen-tab", "Reopen tab")
                    .on_click(cx.listener(|this, _, window, cx| this.reopen(window, cx))),
            )
            .child(
                self.observed_control(
                    "reader-maximize-pane",
                    control(
                        "reader-maximize-pane",
                        if self.maximized {
                            "Restore panes"
                        } else {
                            "Maximize pane"
                        },
                    )
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.maximized = !this.maximized;
                        this.save_layout(this.layout.clone(), window, cx);
                    })),
                    false,
                ),
            )
            .child(
                control("reader-duplicate-tab", "Duplicate view beside").on_click(cx.listener(
                    |this, _, window, cx| {
                        if let Some(tab) = this
                            .layout
                            .panes
                            .get(this.layout.focused_pane)
                            .and_then(|pane| pane.active.and_then(|i| pane.tabs.get(i)))
                        {
                            this.open(tab.item, None, OpenTab::DuplicateBeside, window, cx);
                        }
                    },
                )),
            )
            .into_any_element()
    }

    fn workspace_upper_surface(&self, palette: Palette, cx: &mut Context<Self>) -> AnyElement {
        match self.surface {
            Surface::Sync=>div().flex_1().min_h_0().children(self.sync_panel.clone()).into_any_element(),
            Surface::Browse=>self.browse_view(palette,cx).into_any_element(),
            Surface::Outbox=>self.outbox_view(palette,cx),
            Surface::History=>self.history_surface(palette,cx),
            Surface::Activity|Surface::NotificationSettings=>self.activity_surface(palette,cx),
            Surface::Storage=>div().id("reader-storage-usage").flex().flex_col().gap_3().p_4()
                .child(div().text_lg().child("Downloaded content"))
                .children(self.cache.map(|cache|div().child(format!("{} summaries · {} discussion records · {} text bytes · {} offline downloads",
                    cache.downloaded_summaries,cache.discussion_records,cache.content_bytes,cache.offline_pins))))
                .children(self.repositories.iter().filter(|repo|repo.selected).map(|repo|div().text_sm().child(
                    format!("{}/{} · {} cached items · {} cached text bytes",repo.owner,repo.name,repo.cached_items,repo.cached_text_bytes))))
                .child({
                    let explanation = "Clearing removes unpinned downloads from this account. Offline pins, drafts, saved tabs, and Outbox dependencies are retained.";
                    div().id("reader-storage-policy").accessibility_id("reader-storage-policy").role(Role::Group).text_sm()
                        .aria_label(explanation).child(explanation)
                })
                .child(control("reader-clear-cache","Clear unpinned downloads")
                    .on_click(cx.listener(|this,_,window,cx|this.clean_downloads(window,cx))))
                .into_any_element(),
        }
    }

    fn workspace_content(
        &self,
        focused_only: bool,
        width: f32,
        palette: Palette,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let browsing_pr_conversation = self.surface == Surface::Browse
            && self.layout.panes.iter().any(|pane| {
                pane.active
                    .and_then(|slot| pane.tabs.get(slot))
                    .and_then(|tab| self.tab_views.get(&tab.id).map(|view| (tab, view)))
                    .is_some_and(|(tab, view)| match &tab.view.pr_reader {
                        Some(settings) => {
                            settings.section == threadline_storage::pr::PrSection::Conversation
                                && view
                                    .data
                                    .item
                                    .as_ref()
                                    .is_some_and(|item| item.kind == ItemKind::PullRequest)
                        }
                        None => view.pr.is_some(),
                    })
            });
        let mut upper = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .children(
                (self.surface != Surface::Browse || !self.maximized).then(|| {
                    let surface = self.workspace_upper_surface(palette, cx);
                    if browsing_pr_conversation {
                        div()
                            .id("reader-browse-scroll")
                            .overflow_y_scroll()
                            .max_h(gpui_kit::relative(0.3))
                            .child(surface)
                            .into_any_element()
                    } else {
                        surface
                    }
                }),
            );
        if self.surface == Surface::Browse {
            let panes = self
                .layout
                .panes
                .iter()
                .enumerate()
                .filter(|(index, _)| !focused_only || *index == self.layout.focused_pane)
                .map(|(index, pane)| {
                    let share = if focused_only {
                        1.
                    } else {
                        self.layout.proportions[index] as f32
                    };
                    div()
                        .min_w_0()
                        .min_h_0()
                        .h_full()
                        .flex_basis(px(0.))
                        .flex_grow(share)
                        .child(self.pane(index, pane, (width - 196.) * share, palette, cx))
                })
                .collect::<Vec<_>>();

            upper = upper.child(self.workspace_commands(palette, cx)).child(
                div()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .flex()
                    .gap_1()
                    .children(panes),
            );
        }
        div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .child(self.sidebar(palette, cx))
            .child(upper)
            .into_any_element()
    }
}

impl Render for ReaderWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = Palette::current(cx);
        let compact =
            window.viewport_size().width.as_f32() < 260. + 390. * self.layout.panes.len() as f32;
        self.publish_visible_panes(compact || self.maximized);
        div()
            .id("reader-workspace")
            .track_focus(&self.surface_focus)
            .tab_stop(true)
            .key_context("ThreadlineReader")
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .min_w_0()
            .on_action(cx.listener(|this, _: &ReaderFind, window, cx| {
                this.search.focus_handle(cx).focus(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReaderSplit, window, cx| {
                this.edit_layout(LayoutAction::Split, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReaderCycle, window, cx| {
                this.edit_layout(LayoutAction::Cycle, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReaderFocus, window, cx| {
                this.edit_layout(LayoutAction::Focus, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReaderClose, window, cx| {
                this.edit_layout(LayoutAction::CloseTab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReaderReopen, window, cx| this.reopen(window, cx)))
            .on_action(cx.listener(|this, _: &ReaderMove, window, cx| {
                this.edit_layout(LayoutAction::MoveTab, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ReaderDraft, window, cx| {
                if let Some(tab) = this
                    .layout
                    .panes
                    .get(this.layout.focused_pane)
                    .and_then(|pane| pane.active.and_then(|i| pane.tabs.get(i)))
                    .cloned()
                {
                    this.edit(tab, DraftKind::Comment, window, cx);
                }
            }))
            .child(self.workspace_connection(palette, cx))
            .children(self.remote_error.as_ref().map(|error| {
                div()
                    .id("reader-network-status")
                    .role(Role::Status)
                    .text_sm()
                    .child(error.clone())
            }))
            .children(self.status.map(|status| {
                div()
                    .id("reader-status")
                    .accessibility_id("reader-status")
                    .role(Role::Status)
                    .aria_label(status)
                    .p_2()
                    .text_sm()
                    .child(status)
            }))
            .child(self.workspace_content(
                compact || self.maximized,
                window.viewport_size().width.as_f32(),
                palette,
                cx,
            ))
    }
}

pub(super) fn control(
    id: impl Into<gpui_kit::SharedString>,
    label: impl Into<gpui_kit::SharedString>,
) -> Button {
    let id = id.into();
    let label = label.into();
    Button::new(id.clone())
        .label(label.clone())
        .tooltip(label)
        .small()
        .accessibility_id(id)
}

pub(super) const BADGE_BACKGROUND: u32 = 0x173e70;
pub(super) const BADGE_FOREGROUND: u32 = 0xffffff;

fn rail(
    id: &'static str,
    label: &'static str,
    scope: &str,
    count: Option<KnownCount>,
    listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let description = match count {
        None => format!("{label}. Cached count pending for {scope}."),
        Some(count) => {
            let detail = if count.complete {
                "complete cached count"
            } else {
                "known from partial downloads"
            };
            format!(
                "{label}. {} {detail} for {scope}. Last checked {}.",
                count.known,
                time_label(count.last_validated_at_ms)
            )
        }
    };
    let badge = count.and_then(KnownCount::badge).map(|total| {
        let digits = gpui_kit::SharedString::from(total.to_string());
        let diameter = (13. + digits.len() as f32 * 7.).max(23.);
        (digits, diameter)
    });
    let button = Button::new(id)
        .small()
        .w_full()
        .when_some(badge.as_ref(), |button, (_, diameter)| {
            // Small buttons clip children to their content box inside the border.
            button.min_h(px(*diameter + 4.))
        })
        .accessibility_id(id)
        .accessibility_label(description.clone())
        .tooltip(description.clone())
        .on_click(listener)
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(label.test_text(label))
                .children(badge.map(|(number, diameter)| {
                    div()
                        .id(format!("{id}-badge"))
                        .test_support()
                        .size(px(diameter))
                        .rounded_full()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .text_color(rgb(BADGE_FOREGROUND))
                        .bg(rgb(BADGE_BACKGROUND))
                        .child(number.clone().test_text(number))
                })),
        );
    div()
        .id(format!("{id}-scope"))
        .role(Role::Group)
        .aria_label(description)
        .child(button)
}

fn last_tab(pane: &Pane, preferred: usize) -> Option<usize> {
    (!pane.tabs.is_empty()).then(|| preferred.min(pane.tabs.len() - 1))
}

fn kind_label(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Issue => "Issue",
        ItemKind::PullRequest => "Pull request",
        ItemKind::Unknown => "Item",
    }
}

pub(super) fn item_metadata(metadata: &ReaderItemMetadata, palette: Palette) -> impl IntoElement {
    let labels = observed_people(metadata.labels.as_deref());
    let assignees = observed_people(metadata.assignees.as_deref());
    let milestone = match &metadata.milestone {
        Change::Value(title) => title.as_str(),
        Change::Null => "none",
        Change::Missing => "not downloaded",
    };
    let reactions = metadata.reactions.as_ref().map_or_else(
        || "not downloaded".into(),
        |counts| {
            let positive = counts.positive;
            let negative = counts.negative;
            let groups = [
                (positive, "+1"),
                (negative, "−1"),
                (counts.laugh, "Laugh"),
                (counts.hooray, "Hooray"),
                (counts.confused, "Confused"),
                (counts.heart, "Heart"),
                (counts.rocket, "Rocket"),
                (counts.eyes, "Eyes"),
            ];
            let detail = groups
                .into_iter()
                .filter(|(count, _)| *count > 0)
                .map(|(count, label)| format!("{label} {count}"))
                .collect::<Vec<_>>()
                .join(", ");
            let total = counts.total.map_or_else(
                || "Total unavailable".into(),
                |total| format!("{total} total"),
            );
            if detail.is_empty() {
                total
            } else {
                format!("{total} · {detail}")
            }
        },
    );
    div()
        .flex()
        .flex_col()
        .gap_1()
        .text_xs()
        .text_color(palette.muted)
        .child(format!(
            "{} · Created {} · Updated {}",
            metadata
                .author
                .as_deref()
                .unwrap_or("Author not downloaded"),
            time_label(metadata.created_at_ms),
            time_label(metadata.remote_updated_at_ms)
        ))
        .child(format!(
            "Labels {labels} · Assignees {assignees} · Milestone {milestone}"
        ))
        .child(format!("Reactions {reactions}"))
}

fn observed_people(values: Option<&[String]>) -> String {
    match values {
        None => "not downloaded".into(),
        Some([]) => "none".into(),
        Some(values) => values.join(", "),
    }
}

pub(super) fn time_label(value: Option<i64>) -> String {
    value
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map_or_else(
            || "not yet recorded".into(),
            |at| at.format("%Y-%m-%d %H:%M UTC").to_string(),
        )
}

pub(super) fn coverage_label(label: &str, coverage: Coverage, time: Option<i64>) -> String {
    let text = match coverage {
        Coverage::Missing => "not checked",
        Coverage::Partial => "partially downloaded",
        Coverage::Complete => "downloaded",
    };
    format!("{label} · {text} · Last saved {}", time_label(time))
}

pub(super) fn resource_problem(
    label: &str,
    state: &ResourceState,
    palette: Palette,
) -> impl IntoElement {
    let problem = match state.error {
        Some(ResourceError::Transport) => "Download interrupted by a connection error",
        Some(ResourceError::RateLimited) => "GitHub delayed further requests",
        Some(ResourceError::Unauthorized | ResourceError::Forbidden) => {
            "GitHub access is unavailable"
        }
        Some(ResourceError::NotFound | ResourceError::ConfirmedDeletion) => {
            "This content is unavailable on GitHub"
        }
        Some(ResourceError::CursorExpired) => "The next page must be located again",
        Some(_) | None => "The download could not finish",
    };
    let accessible = format!("{label}. {problem}. Downloaded content remains available.");
    div()
        .id(format!("{label}-resource-problem"))
        .test_support()
        .role(Role::Status)
        .aria_label(accessible.clone())
        .text_xs()
        .text_color(palette.muted)
        .child(accessible)
}

pub(super) const PR_DISCUSSION_ANCHOR: &str = "issue-comment:";
pub(super) const REVIEW_URL_ANCHOR: &str = "review-comment:";

pub(super) struct ItemUrl {
    pub route: threadline_core::domain::RepositoryName,
    pub kind: ItemKind,
    pub number: u64,
    pub anchor: Option<String>,
    pub unsupported_fragment: bool,
}

pub(super) fn parse_item_url(raw: &str) -> Option<ItemUrl> {
    let path = raw.trim().strip_prefix("https://github.com/")?;
    let (path, fragment) = path.split_once('#').unwrap_or((path, ""));
    let path = path.split('?').next()?;
    let mut parts = path.trim_end_matches('/').split('/');
    let route = threadline_core::domain::RepositoryName::new(parts.next()?, parts.next()?).ok()?;
    let kind = match parts.next()? {
        "issues" => ItemKind::Issue,
        "pull" => ItemKind::PullRequest,
        _ => return None,
    };
    let number = parts
        .next()?
        .parse::<u64>()
        .ok()
        .filter(|number| *number > 0)?;
    if parts.next().is_some() {
        return None;
    }
    let comment = fragment
        .strip_prefix("issuecomment-")
        .and_then(comment_number);
    let review = (kind == ItemKind::PullRequest)
        .then(|| {
            fragment
                .strip_prefix("discussion_r")
                .and_then(comment_number)
        })
        .flatten();
    let anchor = comment
        .map(|id| {
            let prefix = if kind == ItemKind::PullRequest {
                PR_DISCUSSION_ANCHOR
            } else {
                ""
            };
            format!("{prefix}{:020}", id.get())
        })
        .or_else(|| review.map(|id| format!("{REVIEW_URL_ANCHOR}{id}")));
    Some(ItemUrl {
        route,
        kind,
        number,
        unsupported_fragment: !fragment.is_empty() && anchor.is_none(),
        anchor,
    })
}

fn comment_number(value: &str) -> Option<std::num::NonZeroU64> {
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

fn parse_date(value: &str) -> Option<i64> {
    let date = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    date.and_hms_opt(0, 0, 0)
        .map(|time| time.and_utc().timestamp_millis())
}

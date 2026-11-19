use super::*;
use crate::reader::{ReaderError, ReaderService};
use std::future::Future;
use threadline_core::{
    domain::ItemKind as ProviderKind,
    github::reader::{IssueSummary, RemoteSearchPage},
    sync::Cancellation,
};
use threadline_storage::RepositoryId;

pub(super) struct SearchResults {
    pub(super) repository: RepositoryId,
    pub(super) query: String,
    pub(super) page: RemoteSearchPage,
}

#[derive(Clone, Copy)]
pub(super) enum RemoteFamily {
    Metadata,
    Comments,
    Timeline,
}

impl ReaderWorkspace {
    pub(super) fn remote<D, F, H>(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
        work: impl FnOnce(ReaderService, AccountId, Cancellation) -> F,
        done: H,
    ) where
        D: Send + 'static,
        F: Future<Output = Result<(D, bool), ReaderError>> + Send + 'static,
        H: FnOnce(&mut Self, Option<D>, &mut Window, &mut Context<Self>) + 'static,
    {
        let (Some(service), Some(account)) = (self.service.clone(), self.account) else {
            return;
        };
        if self.offline {
            self.status = Some("Working offline. Downloaded content is available.");
            cx.notify();
            return;
        }
        let generation = self.generation;
        self.network_pending += 1;
        self.network_started += 1;
        let observation = self.network_started;
        let shared_lease = self
            .sync_worker
            .as_ref()
            .and_then(|worker| worker.begin_foreground());
        self.remote_error = None;
        let cancel = self.network.clone();
        let task = self.runtime.spawn(work(service, account, cancel));
        cx.spawn_in(window, async move |entity, cx| {
            let result = task.await;
            let _ = entity.update_in(cx, move |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                this.network_pending = this.network_pending.saturating_sub(1);
                match result {
                    Ok(Ok((data, contacted))) => {
                        if contacted {
                            this.observe_contact(observation, Reachability::ReceivedResponse);
                            if let (Some(worker), Some(id)) =
                                (this.sync_worker.as_ref(), shared_lease)
                            {
                                worker.record_foreground(id, Reachability::ReceivedResponse, None);
                            }
                        }
                        done(this, Some(data), window, cx);
                    }
                    Ok(Err(error)) => {
                        let reachability = error.reachability();
                        if let (Some(worker), Some(id)) = (this.sync_worker.as_ref(), shared_lease)
                        {
                            let reason = match &error {
                                ReaderError::Remote(remote) => Some(remote.kind),
                                _ => None,
                            };
                            worker.record_foreground(id, reachability, reason);
                        }
                        this.observe_contact(observation, reachability);
                        this.remote_error = Some(error.to_string());
                        done(this, None, window, cx);
                    }
                    Err(_) => {
                        this.remote_error = Some("The GitHub request did not complete.".into());
                        done(this, None, window, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn observe_contact(&mut self, observation: u64, reachability: Reachability) {
        if reachability == Reachability::NotAttempted || observation < self.network_observed {
            return;
        }
        self.network_observed = observation;
        self.checked = Some(now_ms());
        self.reachability = reachability;
        if reachability == Reachability::ReceivedResponse {
            self.contact = self.checked;
        }
    }

    fn remote_repo(&self) -> Option<RepositoryId> {
        match self.scope {
            ReaderScope::Repository(repo) => Some(repo),
            ReaderScope::Selected => {
                let mut selected = self.repositories.iter().filter(|repo| repo.selected);
                let repository = selected.next()?.id;
                selected.next().is_none().then_some(repository)
            }
        }
    }

    pub(super) fn load_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.remote_repo() else {
            self.status = Some("Select one repository before downloading its list.");
            cx.notify();
            return;
        };
        self.remote(
            window,
            cx,
            move |service, account, cancel| async move {
                let page = service.fetch_list_page(account, repo, &cancel).await?;
                Ok((page, page.contacted))
            },
            move |this, page, window, cx| {
                let Some(page) = page else { return };
                this.refresh_tab_summaries(window, cx);
                this.status = if page.has_more {
                    Some("A repository page was saved. More items are available to download.")
                } else if page.complete {
                    Some("The repository list is complete.")
                } else {
                    Some("GitHub returned partial repository access.")
                };
                if this.remote_repo() == Some(repo) {
                    this.browse(false, window, cx);
                }
            },
        );
    }

    pub(super) fn search_github(
        &mut self,
        next: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(repo) = self.remote_repo() else {
            self.status = Some("Select a repository for GitHub search.");
            cx.notify();
            return;
        };
        let (query, page) = if next {
            let Some(existing) = self
                .remote_search
                .as_ref()
                .filter(|existing| existing.repository == repo)
            else {
                return;
            };
            let Some(page) = existing.page.next_page else {
                return;
            };
            (existing.query.clone(), page)
        } else {
            (self.search.read(cx).value().to_string(), 1)
        };
        self.remote_generation += 1;
        let request = self.remote_generation;
        let request_query = query.clone();
        self.remote(
            window,
            cx,
            move |service, account, cancel| async move {
                service
                    .search_remote(account, repo, &request_query, page, &cancel)
                    .await
                    .map(|page| (page, true))
            },
            move |this, page, _, _| {
                let Some(page) = page else { return };
                if this.remote_repo() == Some(repo) && this.remote_generation == request {
                    this.remote_search = Some(SearchResults {
                        repository: repo,
                        query,
                        page,
                    });
                }
            },
        );
    }

    pub(super) fn open_remote(
        &mut self,
        repo: RepositoryId,
        hit: IssueSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        let number = hit.number.get();
        self.launch(
            window,
            cx,
            move |store| {
                use threadline_core::github::reader::Reported;
                use threadline_storage::{
                    IssueClassification, IssueRecord, ItemLocator, ItemRepresentation, ItemUpdate,
                    NodeId, UtcMillis,
                };
                let classification = match hit.kind {
                    ProviderKind::Issue => IssueClassification::Issue,
                    ProviderKind::PullRequest => IssueClassification::PullRequest,
                };
                let body = match hit.body {
                    Reported::Absent => Change::Missing,
                    Reported::Null => Change::Null,
                    Reported::Value(value) => Change::Value(value),
                };
                store.ingest(
                    account,
                    ItemLocator {
                        repository: repo,
                        number,
                    },
                    ItemUpdate {
                        representation: ItemRepresentation::Issue(IssueRecord {
                            id: hit.issue_id,
                            node_id: Some(NodeId::new(hit.node_id)?),
                            classification,
                        }),
                        remote_updated_at: UtcMillis(hit.updated_at_ms),
                        title: Change::Value(hit.title),
                        body,
                        closed: Change::Value(hit.closed),
                    },
                )
            },
            |this, result, window, cx| {
                if let Ok(item) = result {
                    this.refresh_tab_summaries(window, cx);
                    this.open(item, None, OpenTab::CurrentPane, window, cx)
                } else {
                    this.status = Some("GitHub’s item could not be opened in this repository.")
                }
            },
        );
    }

    pub(super) fn fetch_family(
        &mut self,
        tab: Tab,
        family: RemoteFamily,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = tab.item;
        self.remote(
            window,
            cx,
            move |service, account, cancel| async move {
                let result = match family {
                    RemoteFamily::Metadata => {
                        service.fetch_item(account, item, &cancel).await?;
                        (false, true)
                    }
                    RemoteFamily::Comments => {
                        let page = service.fetch_comments(account, item, &cancel).await?;
                        (page.has_more, page.contacted)
                    }
                    RemoteFamily::Timeline => {
                        let page = service.fetch_timeline(account, item, &cancel).await?;
                        (page.has_more, page.contacted)
                    }
                };
                Ok(result)
            },
            move |this, remaining, window, cx| {
                if remaining == Some(true) {
                    this.status =
                        Some("One page was saved. More content is available from GitHub.");
                }
                if let Some(current) = this
                    .layout
                    .panes
                    .iter()
                    .flat_map(|pane| &pane.tabs)
                    .find(|current| current.id == tab.id && current.item == item)
                    .cloned()
                {
                    this.load_tab(current, window, cx);
                }
            },
        );
    }

    pub(super) fn can_fetch(&self) -> bool {
        !self.offline && self.service.is_some() && self.access == AccessState::Readable
    }

    pub(super) fn resume_download(
        &mut self,
        tab: Tab,
        generation: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        self.launch(
            window,
            cx,
            move |store| {
                store.retry_issue_download(account, tab.item, generation, now_ms())?;
                Ok(tab)
            },
            |this, result, window, cx| match result {
                Ok(tab) => {
                    if let Some(worker) = &this.sync_worker {
                        worker.wake(None);
                    }
                    this.load_tab(tab, window, cx);
                }
                Err(_) => this.status = Some("The offline download could not be retried."),
            },
        );
    }

    pub(super) fn store_route(
        &mut self,
        link: super::view::ItemUrl,
        mode: OpenTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = self.account else { return };
        self.launch(window,cx,move |store| store.resolve_downloaded_route(account,&link.route,link.number,Some(link.kind)),
            move |this,result,window,cx|match result {
                Ok(Some(item))=>{
                    this.open(item,link.anchor,mode,window,cx);
                    if link.unsupported_fragment {
                        this.status=Some("This link location is unavailable in the local reader.");
                    }
                }
                Ok(None)=>this.status=Some("This issue or pull request has not been downloaded for this account. The link remains available."),
                Err(_)=>this.status=Some("The link could not be looked up in the local profile."),
            });
    }
}

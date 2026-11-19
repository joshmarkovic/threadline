use super::*;
use crate::sync_worker::SyncWorker;
use threadline_storage::{
    resources::ResourceFamily,
    sync::{ReadTarget, VisibleRead},
};
use tokio::{sync::watch, task::JoinHandle};

#[derive(Clone)]
struct ViewportDemand {
    account: AccountId,
    targets: Vec<VisibleRead>,
}

pub(super) struct VisibleSubscriptions {
    sender: watch::Sender<Option<ViewportDemand>>,
    task: JoinHandle<()>,
}

impl VisibleSubscriptions {
    pub(super) fn start(
        runtime: &tokio::runtime::Handle,
        store: Arc<Store>,
        worker: Arc<SyncWorker>,
    ) -> Self {
        let (sender, mut receiver) = watch::channel::<Option<ViewportDemand>>(None);
        let task = runtime.spawn(async move {
            let mut account = None;
            while receiver.changed().await.is_ok() {
                let desired = receiver.borrow_and_update().clone();
                let next_account = desired.as_ref().map(|demand| demand.account);
                let old = account.filter(|old| Some(*old) != next_account);
                let database = Arc::clone(&store);
                let saved = tokio::task::spawn_blocking(move || {
                    let mut removed = Vec::new();
                    if let Some(old) = old {
                        removed.push((old, database.replace_visible_read_subscriptions(old, &[])?));
                    }
                    if let Some(demand) = desired {
                        removed.push((
                            demand.account,
                            database.replace_visible_read_subscriptions(
                                demand.account,
                                &demand.targets,
                            )?,
                        ));
                    }
                    Ok::<_, threadline_storage::Error>(removed)
                })
                .await;
                if let Ok(Ok(removed)) = saved {
                    account = next_account;
                    for (account, targets) in removed {
                        for orphan in targets {
                            worker.cancel_read(account, &orphan.target, orphan.generation);
                        }
                    }
                    worker.wake(None);
                }
            }
            if let Some(account) = account {
                if let Ok(Ok(orphans)) = tokio::task::spawn_blocking(move || {
                    store.replace_visible_read_subscriptions(account, &[])
                })
                .await
                {
                    for orphan in orphans {
                        worker.cancel_read(account, &orphan.target, orphan.generation);
                    }
                }
                worker.wake(None);
            }
        });
        Self { sender, task }
    }

    fn publish(&self, demand: Option<ViewportDemand>) {
        self.sender.send_if_modified(|latest| {
            if equivalent(latest.as_ref(), demand.as_ref()) {
                return false;
            }
            *latest = demand;
            true
        });
    }

    pub(super) fn finish(self) -> JoinHandle<()> {
        drop(self.sender);
        self.task
    }
}

fn equivalent(left: Option<&ViewportDemand>, right: Option<&ViewportDemand>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => {
            left.account == right.account
                && left
                    .targets
                    .iter()
                    .map(read_key)
                    .eq(right.targets.iter().map(read_key))
        }
        _ => false,
    }
}

fn read_key(read: &VisibleRead) -> (&str, &ReadTarget) {
    (&read.subscriber, &read.target)
}

impl ReaderWorkspace {
    pub(crate) fn set_workspace_visible(&mut self, visible: bool) {
        self.workspace_visible = visible;
        if !visible && let Some(subscriptions) = self.visible_subscriptions.as_ref() {
            subscriptions.publish(None);
        }
    }

    pub(super) fn publish_visible_panes(&self, focused_only: bool) {
        let Some(subscriptions) = self.visible_subscriptions.as_ref() else {
            return;
        };
        let Some(account) = self.account else {
            subscriptions.publish(None);
            return;
        };
        let visible = self.workspace_visible && self.surface == Surface::Browse;
        let mut targets = Vec::new();
        let due_ms = now_ms();
        if visible {
            for (pane_index, pane) in self.layout.panes.iter().enumerate() {
                if focused_only && pane_index != self.layout.focused_pane {
                    continue;
                }
                let Some(tab) = pane.active.and_then(|index| pane.tabs.get(index)) else {
                    continue;
                };
                let Some(view) = self.tab_views.get(&tab.id) else {
                    continue;
                };
                if let Some(item) = view.data.item.as_ref() {
                    targets.push(VisibleRead {
                        subscriber: format!("pane-{pane_index}"),
                        target: ReadTarget {
                            repository: item.locator.repository,
                            item: Some(item.id),
                            family: ResourceFamily::Metadata,
                            revision: String::new(),
                        },
                        due_ms,
                    });
                }
                if let Some(pr) = view.pr.as_ref() {
                    let section = super::pr::tab_settings(tab).section;
                    for family in super::pr::section_resources(section) {
                        targets.push(VisibleRead {
                            subscriber: format!("pane-{pane_index}-{}", family.name()),
                            target: pr.data.revision.read_target(*family),
                            due_ms,
                        });
                    }
                }
            }
        }
        subscriptions.publish(Some(ViewportDemand { account, targets }));
    }

    pub(super) fn trim_inactive_views(&mut self, cx: &gpui_kit::App) {
        const ACTIVE_PLUS_RECENT: usize = 8;
        let surplus = self.tab_views.len().saturating_sub(ACTIVE_PLUS_RECENT);
        if surplus == 0 {
            return;
        }
        self.capture_scroll(cx);
        let active = self
            .layout
            .panes
            .iter()
            .filter_map(|pane| pane.active.and_then(|index| pane.tabs.get(index)))
            .map(|tab| tab.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        let inactive = self
            .tab_views
            .keys()
            .filter(|id| !active.contains(id.as_str()))
            .take(surplus)
            .cloned()
            .collect::<Vec<_>>();
        for id in inactive {
            self.tab_views.remove(&id);
        }
    }
}

use super::*;
use crate::sync_worker::{SyncUpdate, SyncWorker};
use threadline_core::{
    connectivity::{ConnectionSnapshot, ConnectionStatus, ProbeTrigger},
    github::AccessKind,
};

impl ReaderWorkspace {
    pub(crate) fn attach_sync(
        &mut self,
        worker: Arc<SyncWorker>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.visible_subscriptions = Some(scheduled::VisibleSubscriptions::start(
            &self.runtime,
            Arc::clone(&self.store),
            Arc::clone(&worker),
        ));
        let mut updates = worker.watch();
        self.sync_worker = Some(worker);
        cx.spawn_in(window, async move |view, cx| {
            while updates.changed().await.is_ok() {
                let update = updates.borrow_and_update().clone();
                if view
                    .update_in(cx, move |this, window, cx| {
                        this.update_sync(update, window, cx)
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn adopt_sync_snapshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(worker) = self.sync_worker.as_ref() {
            let snapshot = worker.watch().borrow().clone();
            self.update_sync(snapshot, window, cx);
        }
    }

    pub(super) fn maybe_start_sync(&mut self) {
        if !self.sync_started
            && self.account.is_some()
            && !self.layout_busy
            && self.pending_tabs.is_empty()
            && self.wake_sync(ProbeTrigger::Startup)
        {
            self.sync_started = true;
        }
    }

    pub(super) fn update_sync(
        &mut self,
        update: SyncUpdate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.account.is_none() || update.account != self.account {
            return;
        }
        let (published, history) = self.sync_update.as_ref().map_or((0, 0), |previous| {
            (previous.cache_generation, previous.history_generation)
        });
        let history_changes = update.history_generation.saturating_sub(history);
        let cache_changes = update.cache_generation.saturating_sub(published);
        let reached = update.connection.status == ConnectionStatus::Reachable
            && self.sync_update.as_ref().is_some_and(|previous| {
                previous.connection.status == ConnectionStatus::Unreachable
            });
        if let Some(panel) = self.sync_panel.as_ref() {
            panel.update(cx, |panel, cx| panel.set_status(update.status.clone(), cx));
        }
        self.sync_update = Some(update);
        if reached {
            self.pr_resume_attempts.clear();
            self.resume_visible_pr_downloads(window, cx);
        }
        if history_changes > 0 {
            self.refresh_cached_history(window, cx);
        }
        if cache_changes > history_changes {
            self.refresh_tab_summaries(window, cx);
            self.refresh_sidebar_counts(window, cx);
            self.refresh_draft_submissions(window, cx);
            if self.surface == Surface::Outbox {
                self.show_outbox(false, window, cx);
            }
            let visible = self
                .layout
                .panes
                .iter()
                .filter_map(|pane| pane.active.and_then(|index| pane.tabs.get(index)))
                .cloned()
                .collect::<Vec<_>>();
            for tab in visible {
                self.load_tab(tab, window, cx);
            }
            if self.surface == Surface::Browse && self.search_hits.is_none() && !self.list_busy {
                self.browse(false, window, cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn wake_sync(&self, trigger: ProbeTrigger) -> bool {
        let Some(worker) = self.sync_worker.as_ref() else {
            return false;
        };
        worker.wake(Some(trigger));
        true
    }

    pub(super) fn shared_connection(&self) -> Option<ConnectionSnapshot> {
        self.sync_update.as_ref().map(|update| update.connection)
    }

    pub(super) fn connection_reason(&self) -> Option<&'static str> {
        match self.shared_connection()?.reason? {
            AccessKind::Unauthorized | AccessKind::InvalidCredential => {
                Some("GitHub credential expired or was revoked")
            }
            AccessKind::PermissionDenied | AccessKind::MissingOrInaccessible => {
                Some("Repository access is limited")
            }
            AccessKind::SsoRequired => Some("GitHub needs SSO authorization"),
            AccessKind::ApprovalPending => Some("Organization token approval is pending"),
            AccessKind::RateLimited => Some("GitHub limited further requests"),
            AccessKind::ServiceFailure => Some("GitHub reported a service error"),
            AccessKind::Transport | AccessKind::Timeout => Some("No verified GitHub API response"),
            _ => None,
        }
    }
}

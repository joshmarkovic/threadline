use super::*;
use crate::{
    Change, ItemLocator, ItemRepresentation, ItemUpdate, NodeId, PullRequestRecord,
    PullRequestUpdate, UtcMillis,
    access::{
        AccessState, CredentialKind, DiscoveryState, RepositoryObservation,
        RepositoryObservationSource,
    },
    history::{HistoryMonitoring, HistoryRef},
    notifications::{ActorRule, DeliveryMode, RuleActor, RuleScope},
    reader::ReaderScope,
};
use std::num::NonZeroU64;

pub const ACTIVITY_FIXTURE_REVISION: &str = "s11-native-activity-v1";
pub const WATCH_BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub fn prepare_native_activity(store: &Store, now: i64) -> Result<()> {
    if !store.github_access_profiles()?.is_empty() {
        return Err(Error::Conflict("activity fixture needs a new profile"));
    }
    let user = NonZeroU64::new(BACKLOG_USER).expect("fixture user");
    let account = store.ensure_account("github.com", BACKLOG_USER)?;
    let profile = store.reserve_access_profile_credential(
        account,
        user,
        BACKLOG_LOGIN,
        BACKLOG_OWNER,
        CredentialKind::Classic,
        None,
    )?;
    store.install_current_credential(&profile, || Ok::<_, Error>(()))??;
    let repository = store.observe_github_repository(
        &profile,
        &RepositoryObservation {
            provider_id: NonZeroU64::new(BACKLOG_REPOSITORY).expect("fixture repository"),
            owner: BACKLOG_OWNER,
            name: BACKLOG_REPO_NAME,
            private: true,
            source: RepositoryObservationSource::Direct,
        },
        now,
    )?;
    store.record_access_result(
        &profile,
        CredentialKind::Classic,
        AccessState::Readable,
        DiscoveryState::Complete,
        now,
    )?;
    store.activate_local_profile(account)?;
    store.select_github_repository(account, repository, true)?;
    let old = now - 3_600_000;
    for (number, merged) in [(42, false), (43, true)] {
        store.ingest(
            account,
            ItemLocator { repository, number },
            ItemUpdate {
                representation: ItemRepresentation::PullRequest(PullRequestRecord {
                    id: (720 + number).try_into()?,
                    node_id: Some(NodeId::new(format!("PR_{number}"))?),
                    details: PullRequestUpdate {
                        merged: Change::Value(merged),
                        base_oid: Change::Value(WATCH_BASE.into()),
                        head_oid: Change::Value("b".repeat(40)),
                        merge_oid: Change::Missing,
                    },
                }),
                remote_updated_at: UtcMillis(old),
                title: Change::Value(format!("Activity fixture pull request {number}")),
                body: Change::Missing,
                closed: Change::Value(merged),
            },
        )?;
    }
    store.set_history_monitoring(
        account,
        repository,
        &HistoryMonitoring {
            branches: ["main".into()].into(),
            ..HistoryMonitoring::default()
        },
    )?;
    let base = CommitOid::parse(WATCH_BASE)?;
    store.save_commits(
        account,
        repository,
        &[GraphCommit {
            oid: base.clone(),
            title: "Previous monitored branch head".into(),
            committed_at_ms: old,
            parents: Vec::new(),
        }],
    )?;
    let profile = store
        .github_access_profile(account, BACKLOG_OWNER)?
        .ok_or(Error::NotFound)?;
    store.observe_history_ref(
        &profile,
        repository,
        &HistoryRef::Branch("main".into()),
        Some(&base),
        old,
    )?;
    store.schedule_selected_activity(account, old)?;
    store.write(|db| {
        db.execute(
            "UPDATE scheduled_reads SET watermark=?2, last_success=?2 WHERE account_id=?1",
            params![account, old],
        )?;
        Ok(())
    })?;
    store.update_notification_settings(account, |settings| {
        settings.fallback = DeliveryMode::Popup;
        settings.actor_rules.push(ActorRule {
            actor: RuleActor::User(NonZeroU64::new(91).expect("fixture bot")),
            scope: RuleScope::ReviewsAndInline,
        });
    })?;
    store.update_sync_settings(account, |settings| {
        settings.controls.work_offline = true;
        settings.schedule.timezone = Some("UTC".into());
        settings.schedule.work_interval_secs = 2;
        settings.schedule.off_hours_interval_secs = 2;
        settings.schedule.visible_interval_secs = 2;
    })?;
    Ok(())
}

pub fn inspect_native_activity(store: &Store) -> Result<serde_json::Value> {
    let account = store.active_github_account()?.ok_or(Error::NotFound)?;
    let [repo] = store
        .github_repository_views(account)?
        .try_into()
        .map_err(|_| Error::NotFound)?;
    if repo.owner != BACKLOG_OWNER || repo.provider_id.get() != BACKLOG_REPOSITORY || !repo.selected
    {
        return Err(Error::Conflict("Activity fixture repository changed"));
    }
    let feed = store.notification_feed(account, None, 100)?;
    let details = feed.into_iter().map(|entry| {
        Ok(serde_json::json!({
            "id":entry.id,
            "key":entry.event_key,
            "kind":entry.event_kind,
            "categories":entry.categories,
            "target":entry.target,
            "delivery":store.load_delivery_state(account, &entry.event_key)?.map(|status| format!("{:?}", status.phase)),
        }))
    }).collect::<Result<Vec<_>>>()?;
    let (groups, suppressed): (i64, i64) = store.read(|db| {
        Ok(db.query_row(
            "SELECT (SELECT count(*) FROM native_notification_groups WHERE account_id=?1 AND state='delivered'),
             (SELECT count(*) FROM notification_events WHERE account_id=?1 AND published=0 AND decision IS NOT NULL)",
            [account], |row| Ok((row.get(0)?,row.get(1)?)),
        )?)
    })?;
    let opened_tabs = store.workspace(account)?.map_or(0, |workspace| {
        workspace.panes.iter().map(|pane| pane.tabs.len()).sum()
    });
    let bot_content = store
        .search_downloaded(account, ReaderScope::Selected, "fixture bot retained", 10)?
        .len();
    let human_content = store
        .search_downloaded(account, ReaderScope::Selected, "fixture human retained", 10)?
        .len();
    Ok(serde_json::json!({
        "fixture_revision": ACTIVITY_FIXTURE_REVISION,
        "feed": details,
        "delivered_groups": groups,
        "suppressed": suppressed,
        "opened_tabs": opened_tabs,
        "bot_content": bot_content,
        "human_content": human_content,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_activity_fixture_seeds_only_predecessors() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open_profile(dir.path()).unwrap();
        let now = 1_700_000_000_000;
        prepare_native_activity(&store, now).unwrap();
        let account = store.active_github_account().unwrap().unwrap();
        let repo = store.github_repository_views(account).unwrap()[0].id;
        assert!(store.sync_settings(account).unwrap().controls.work_offline);
        assert!(store.claim_sync_read(account, now).unwrap().is_none());
        for (number, expected) in [(42, false), (43, true)] {
            let item = store
                .resolve(
                    account,
                    ItemLocator {
                        repository: repo,
                        number,
                    },
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                store
                    .item(account, item)
                    .unwrap()
                    .unwrap()
                    .pull_request
                    .unwrap()
                    .fields
                    .merged,
                Change::Value(expected)
            );
        }
        let receipt = inspect_native_activity(&store).unwrap();
        assert_eq!(receipt["delivered_groups"], 0);
        assert_eq!(receipt["opened_tabs"], 0);
        assert_eq!(receipt["feed"], serde_json::json!([]));
    }
}

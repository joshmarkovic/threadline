use super::*;
use crate::{
    access::{
        AccessState, CredentialKind, DiscoveryState, RepositoryObservation,
        RepositoryObservationSource,
    },
    merges::{Gate, MergeMethod, MergePolicy, MergeSubmission, QueueRequirement},
    notifications::{DeliveryMode, PopupOutcome},
    pr::{PatchEvidence, PrFamily, PrHeader, PrInputPage, PrRecord, SourceSide},
    resources::{ActivityEvent, DeliveryPhase, Validators, WalkMode},
    reviews::{DraftInline, ReviewDraft, ReviewEvent, ReviewIntent, ReviewSubmission},
    writes::{FieldEdit, IssueChanges, Permission, WriteIntent, WriteSubmission},
};
use std::num::NonZeroU64;

pub const LIFECYCLE_REVISION: &str = "s12-native-lifecycle-v1";
pub const LIFECYCLE_ALT_USER: u64 = 42;
pub const LIFECYCLE_EVENT: &str = "lifecycle-delivered-once";
pub const LIFECYCLE_COMMENT: &str = "Threadline lifecycle submitted comment.";
pub const LIFECYCLE_DRAFT: &str = "Threadline lifecycle original inline draft.";
pub const LIFECYCLE_REVIEW: &str = "lifecycle-typed-review";
pub const LIFECYCLE_ACTIONS: [&str; 4] = [
    "00000000-0000-4000-8002-000000000001",
    "00000000-0000-4000-8002-000000000002",
    "00000000-0000-4000-8002-000000000003",
    "00000000-0000-4000-8002-000000000004",
];

/// Prepare an isolated profile with the same stable private identity as the loopback server.
pub fn populate_native_lifecycle(
    store: &Store,
    seed: u64,
    credential_key: &str,
    now: i64,
    install_credential: impl FnOnce() -> Result<()>,
) -> Result<Fixture> {
    if credential_key.len() != 32 || !credential_key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::InvalidInput("invalid lifecycle fixture key"));
    }
    let fixture = populate_profile(store, seed, Scale::Small, Profile::Lifecycle)?;
    let account = fixture.account;
    let alternate = store.ensure_account("github.com", LIFECYCLE_ALT_USER)?;
    store.write(|db| {
        db.execute(
            "UPDATE github_access_profiles SET credential_ref=?2 WHERE account_id=?1",
            params![account, credential_key],
        )?;
        db.execute(
            "INSERT INTO github_access_profiles(account_id,owner_scope,login,credential_ref,credential_kind,access)
             VALUES(?1,?2,'fixture-other',lower(hex(randomblob(16))),0,?3)",
            params![alternate, BACKLOG_OWNER, AccessState::SignedOut as i64],
        )?;
        Ok(())
    })?;
    let user = NonZeroU64::new(BACKLOG_USER).expect("nonzero fixture user");
    let previous = store
        .github_access_profile(account, BACKLOG_OWNER)?
        .ok_or(Error::NotFound)?;
    let profile = store.reserve_access_profile_credential(
        account,
        user,
        BACKLOG_LOGIN,
        BACKLOG_OWNER,
        CredentialKind::Classic,
        Some(&previous),
    )?;
    store.install_current_credential(&profile, install_credential)??;
    for (provider, name) in [
        (BACKLOG_REPOSITORY, BACKLOG_REPO_NAME),
        (BACKLOG_ARCHIVE, "archive"),
    ] {
        store.observe_github_repository(
            &profile,
            &RepositoryObservation {
                provider_id: NonZeroU64::new(provider).expect("nonzero fixture repository"),
                owner: BACKLOG_OWNER,
                name,
                private: true,
                source: RepositoryObservationSource::Direct,
            },
            now,
        )?;
    }
    if !store.record_access_result(
        &profile,
        CredentialKind::Classic,
        AccessState::Readable,
        DiscoveryState::Complete,
        now,
    )? {
        return Err(Error::Conflict(
            "could not establish the lifecycle access profile",
        ));
    }
    store.update_sync_settings(account, |settings| {
        settings.controls.work_offline = false;
        settings.controls.pause_background = true;
        settings.controls.pause_queued_actions = true;
    })?;
    let base = &fixture.comparison.base;
    let rev = store.record_pr_revision(
        account,
        fixture.reviewed_pr,
        base,
        &fixture.head,
        Some(base),
        &PrHeader {
            base_ref: Some("main".into()),
            head_ref: Some("fixture-review".into()),
            draft: Some(false),
            ..PrHeader::default()
        },
        FIXTURE_TIME_MS,
    )?;
    let progress = store.start_pr_fetch(&rev, PrFamily::Files, WalkMode::Full)?;
    store.commit_pr_page(
        &rev,
        PrFamily::Files,
        &progress,
        PrInputPage {
            records: vec![PrRecord::File {
                path: "src/relay_00.rs".into(),
                previous_path: None,
                status: "modified".into(),
                additions: Some(1),
                deletions: Some(1),
                patch: PatchEvidence::Text("@@ -1 +1 @@\n-previous\n+changed\n".into()),
            }],
            next_cursor: None,
            validators: Some(&Validators::default()),
            fetched_at_ms: FIXTURE_TIME_MS,
            partial: None,
        },
    )?;
    let anchor =
        store.capture_inline_source_anchor(&rev, "src/relay_00.rs", SourceSide::Right, 1, 1)?;
    let draft = ReviewDraft {
        event: ReviewEvent::Comment,
        summary: String::new(),
        comments: vec![DraftInline {
            key: "lifecycle-original-line".into(),
            body: LIFECYCLE_DRAFT.into(),
            anchor,
        }],
    };
    store.create_review_draft(&rev, LIFECYCLE_REVIEW, &draft)?;
    let mut workspace = store.workspace(account)?.ok_or(Error::NotFound)?;
    workspace.proportions = vec![0.43, 0.57];
    workspace.panes[1].tabs[0].view.draft = Some(LIFECYCLE_REVIEW.into());
    store.save_reader_layout(account, &workspace, None)?;

    for (index, intent) in [
        WriteIntent::CreateComment {
            body: LIFECYCLE_COMMENT.into(),
        },
        WriteIntent::EditIssue(IssueChanges {
            title: Some(FieldEdit {
                base: "Investigate relay behavior 1".into(),
                wanted: "Lifecycle fixture title edit".into(),
            }),
            ..IssueChanges::default()
        }),
    ]
    .into_iter()
    .enumerate()
    {
        store.submit_write(
            account,
            WriteSubmission {
                id: LIFECYCLE_ACTIONS[index],
                item: fixture.long_discussion,
                intent: &intent,
                draft: None,
                dependencies: if index == 0 {
                    &[]
                } else {
                    &LIFECYCLE_ACTIONS[0..1]
                },
                submitted_at_ms: FIXTURE_TIME_MS,
            },
        )?;
    }
    store.submit_review(
        &rev,
        ReviewSubmission {
            id: LIFECYCLE_ACTIONS[2],
            intent: &ReviewIntent::Review(ReviewDraft {
                event: ReviewEvent::RequestChanges,
                summary: "Threadline lifecycle held review.".into(),
                comments: Vec::new(),
            }),
            draft: None,
            dependencies: &LIFECYCLE_ACTIONS[0..1],
            submitted_at_ms: FIXTURE_TIME_MS,
        },
    )?;
    let current = store
        .github_access_profile(account, BACKLOG_OWNER)?
        .ok_or(Error::NotFound)?;
    store.record_merge_policy(
        &current,
        &rev,
        &MergePolicy {
            allowed_methods: Some([MergeMethod::Squash].into()),
            viewer_permission: Permission::Allowed,
            open: Some(true),
            draft: Some(false),
            merged: Some(false),
            mergeable: Some(true),
            queue: QueueRequirement::NotRequired,
            checks: Gate::Satisfied,
            reviews: Gate::Satisfied,
            branch_rules: Gate::Satisfied,
        },
        FIXTURE_TIME_MS,
    )?;
    store.submit_merge(
        &rev,
        MergeSubmission {
            id: LIFECYCLE_ACTIONS[3],
            method: MergeMethod::Squash,
            dependencies: &LIFECYCLE_ACTIONS[0..1],
            submitted_at_ms: FIXTURE_TIME_MS,
        },
    )?;

    store.update_notification_settings(account, |settings| {
        settings.fallback = DeliveryMode::Popup
    })?;
    store.record_activity_event(
        account,
        LIFECYCLE_EVENT,
        &ActivityEvent {
            repository: fixture.repository,
            item: Some(fixture.long_discussion),
            kind: "comment_created".into(),
            revision: None,
            occurred_at_ms: Some(now),
            observed_at_ms: now,
        },
    )?;
    store.process_notification_events(account, now + 1, "UTC", true, 10)?;
    let popup = store
        .claim_notification_popup(account, now + 30_000, "UTC", true)?
        .ok_or(Error::NotFound)?;
    store.record_notification_outcome(
        account,
        &popup.native_id,
        PopupOutcome::Delivered,
        now + 30_000,
    )?;
    store.update_sync_settings(account, |settings| settings.controls.work_offline = true)?;
    let mut seeded = fixture;
    seeded.outbox_count = LIFECYCLE_ACTIONS.len();
    seeded.typed_outbox_count = LIFECYCLE_ACTIONS.len();
    Ok(seeded)
}

/// Read only durable identifiers and states from the closed isolated fixture.
pub fn lifecycle_checkpoint(store: &Store, stage: &str) -> Result<serde_json::Value> {
    let profiles = store.github_access_profiles()?;
    let original = profiles
        .iter()
        .find(|row| row.user_id.get() == BACKLOG_USER)
        .ok_or(Error::NotFound)?;
    let alternate = profiles
        .iter()
        .find(|row| row.user_id.get() == LIFECYCLE_ALT_USER)
        .ok_or(Error::NotFound)?;
    let account = original.account;
    let layout = store.workspace(account)?.ok_or(Error::NotFound)?;
    let controls = store.sync_settings(account)?.controls;
    let draft = store
        .review_draft(account, LIFECYCLE_REVIEW)?
        .ok_or(Error::NotFound)?;
    let delivery = store
        .load_delivery_state(account, LIFECYCLE_EVENT)?
        .ok_or(Error::NotFound)?;
    let delivery_groups = store.read(|db| {
        Ok(db.query_row(
            "SELECT COUNT(*) FROM native_notification_groups WHERE account_id=?",
            [account],
            |row| row.get::<_, i64>(0),
        )?)
    })?;
    let original_rows = store.queue_page(account, 0, 10)?;
    let alternate_rows = store.queue_page(alternate.account, 0, 10)?;
    if layout.panes.len() != 2
        || layout.panes.iter().any(|pane| pane.tabs.len() != 10)
        || draft.draft.comments.len() != 1
        || draft.draft.comments[0].body != LIFECYCLE_DRAFT
        || draft.draft.comments[0].anchor.first_line != 1
        || delivery.phase != DeliveryPhase::Delivered
        || delivery_groups != 1
        || !alternate_rows.is_empty()
        || original_rows
            .iter()
            .map(|op| op.id.as_str())
            .ne(LIFECYCLE_ACTIONS)
        || store.dependents(account, LIFECYCLE_ACTIONS[0])?.len() != 3
    {
        return Err(Error::Conflict("lifecycle fixture data was not preserved"));
    }
    let attempts = store.attempts(account, LIFECYCLE_ACTIONS[0])?;
    Ok(serde_json::json!({
        "fixture_revision": LIFECYCLE_REVISION,
        "stage": stage,
        "account": account.0,
        "alternate": alternate.account.0,
        "alternate_queue": alternate_rows.len(),
        "active_account": store.active_github_account()?.map(|id| id.0),
        "access": format!("{:?}", original.access),
        "tab_counts": layout.panes.iter().map(|pane| pane.tabs.len()).collect::<Vec<_>>(),
        "pane_proportions": layout.proportions,
        "focused_pane": layout.focused_pane,
        "review_item": draft.item.0,
        "review_anchor_preserved": true,
        "prior_delivery": "Delivered",
        "delivery_groups": delivery_groups,
        "delivery_unchanged_at_ms": delivery.updated_at_ms,
        "feed_count": store.notification_feed(account, None, 10)?.len(),
        "work_offline": controls.work_offline,
        "pause_queued_actions": controls.pause_queued_actions,
        "actions": original_rows.iter().map(|row| format!("{:?}", row.state)).collect::<Vec<_>>(),
        "first_attempts": attempts.len(),
        "first_send_starts": attempts.iter().filter(|attempt| attempt.sending_at.is_some()).count(),
        "next_recovery_at_ms": store.next_operation_recovery(account)?,
        "dependents": 3,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_lifecycle_fixture_has_mixed_durable_work_and_one_delivered_event() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open_profile(directory.path()).unwrap();
        let key = "0123456789abcdef0123456789abcdef";
        let fixture =
            populate_native_lifecycle(&store, 12345, key, FIXTURE_TIME_MS + 50_000, || Ok(()))
                .unwrap_or_else(|error| match error {
                    Error::Sql(error) => panic!("{error:?}"),
                    error => panic!("{error}"),
                });
        assert_eq!(fixture.typed_outbox_count, 4);
        drop(store);
        let store = Store::open_profile(directory.path()).unwrap();
        let checkpoint = lifecycle_checkpoint(&store, "restart").unwrap();
        assert_eq!(checkpoint["access"], "Readable");
        assert_eq!(checkpoint["work_offline"], true);
        assert_eq!(checkpoint["pause_queued_actions"], true);
        assert_eq!(checkpoint["feed_count"], 1);
        assert_eq!(
            checkpoint["actions"],
            serde_json::json!(["Queued", "Queued", "Queued", "Queued"])
        );
        assert!(populate_native_lifecycle(&store, 12345, key, FIXTURE_TIME_MS, || Ok(())).is_err());
    }
}

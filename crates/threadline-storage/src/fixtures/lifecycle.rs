use super::*;
use crate::{
    access::{
        AccessProfile, AccessState, CredentialKind, DiscoveryState, RepositoryObservation,
        RepositoryObservationSource,
    },
    history::{HistoryEntry, HistoryRef},
    merges::{Gate, MergeMethod, MergePolicy, MergeSubmission, QueueRequirement},
    notifications::{DeliveryMode, PopupOutcome},
    pr::{
        InlineComment, PatchEvidence, PrDiffMode, PrFamily, PrHeader, PrInputPage, PrRecord,
        PrTabView, SourceSide,
    },
    resources::{ActivityEvent, DeliveryPhase, Validators, WalkMode},
    reviews::{DraftInline, ReviewDraft, ReviewEvent, ReviewIntent, ReviewSubmission},
    sync::{ObservationPage, ReadPriority, ReadTarget},
    writes::{FieldEdit, IssueChanges, Permission, WriteIntent, WriteSubmission},
};
use std::num::NonZeroU64;

pub const LIFECYCLE_REVISION: &str = "s12-native-lifecycle-v2";
pub const OFFLINE_OUTBOX_REVISION: &str = "s12-offline-mixed-outbox-v1";
pub const LIFECYCLE_ALT_USER: u64 = 42;
pub const LIFECYCLE_EVENT: &str = "lifecycle-delivered-once";
pub const LIFECYCLE_COMMENT: &str = "Threadline lifecycle submitted comment.";
pub const LIFECYCLE_DRAFT: &str = "Threadline lifecycle original inline draft.";
pub const LIFECYCLE_THREAD_ROOT: u64 = 771;
pub const LIFECYCLE_THREAD: &str = "TL_LIFECYCLE_THREAD";
pub const LIFECYCLE_ACTIONS: [&str; 4] = [
    "00000000-0000-4000-8002-000000000001",
    "00000000-0000-4000-8002-000000000002",
    "00000000-0000-4000-8002-000000000003",
    "00000000-0000-4000-8002-000000000004",
];

fn review_draft_id(item: ItemId) -> String {
    format!("review-{}", item.0)
}

fn seed_history_and_sync(
    store: &Store,
    fixture: &Fixture,
    profile: &AccessProfile,
    now: i64,
) -> Result<()> {
    let account = fixture.account;
    let repository = fixture.repository;
    let history_head = CommitOid::parse(&"a".repeat(40))?;
    let history_base = CommitOid::parse(&"b".repeat(40))?;
    let reference = HistoryRef::Branch("main".into());
    store.observe_history_ref(profile, repository, &reference, Some(&history_head), now)?;
    let walk = store
        .begin_history_page(profile, repository, &reference)?
        .ok_or(Error::NotFound)?;
    let entry = |oid, title: &str, parents| HistoryEntry {
        commit: GraphCommit {
            oid,
            title: title.into(),
            committed_at_ms: FIXTURE_TIME_MS,
            parents,
        },
        message: title.into(),
        author: Some("Fixture author".into()),
        authored_at_ms: Some(FIXTURE_TIME_MS),
    };
    store.commit_history_page(
        &walk,
        &[
            entry(
                history_head,
                "Offline history tip",
                vec![history_base.clone()],
            ),
            entry(history_base, "Offline history parent", vec![]),
        ],
        None,
        true,
        now,
    )?;

    let target = ReadTarget {
        repository,
        item: None,
        family: ResourceFamily::RepositoryItems,
        revision: String::new(),
    };
    store.subscribe_read(
        account,
        &target,
        "lifecycle-checkpoint",
        ReadPriority::Activity,
        now,
    )?;
    let claim = store
        .claim_sync_read(account, now)?
        .ok_or(Error::NotFound)?;
    if claim.target != target || claim.cursor.is_some() {
        return Err(Error::Conflict("Unexpected lifecycle read claim"));
    }
    store.commit_observed_page(
        &claim,
        ObservationPage {
            items: vec![],
            comments: vec![],
            next_cursor: Some("2"),
            validators: Validators::default(),
        },
        now,
        "UTC",
    )
}

/// Prepare an isolated profile with the same stable private identity as the loopback server.
pub fn populate_native_lifecycle(
    store: &Store,
    seed: u64,
    credential_key: &str,
    now: i64,
    install_credential: impl FnOnce() -> Result<()>,
) -> Result<Fixture> {
    populate_lifecycle(store, seed, credential_key, now, false, install_credential)
}

pub fn populate_native_offline_outbox(
    store: &Store,
    seed: u64,
    credential_key: &str,
    now: i64,
    install_credential: impl FnOnce() -> Result<()>,
) -> Result<Fixture> {
    populate_lifecycle(store, seed, credential_key, now, true, install_credential)
}

fn populate_lifecycle(
    store: &Store,
    seed: u64,
    credential_key: &str,
    now: i64,
    inspect_inline_submission: bool,
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
        settings.controls.pause_background = false;
        settings.controls.pause_queued_actions = true;
        settings.schedule.timezone = Some("UTC".into());
        settings.schedule.work_interval_secs = 180;
    })?;
    let current = store
        .github_access_profile(account, BACKLOG_OWNER)?
        .ok_or(Error::NotFound)?;
    seed_history_and_sync(store, &fixture, &current, now)?;
    store.update_sync_settings(account, |settings| {
        settings.controls.pause_background = true
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
                patch: PatchEvidence::Text(
                    "@@ -1,3 +1,3 @@\n-previous\n+changed\n unchanged\n another line\n".into(),
                ),
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
            anchor: anchor.clone(),
        }],
    };
    let review_id = review_draft_id(fixture.reviewed_pr);
    store.create_review_draft(&rev, &review_id, &draft)?;
    for (family, record) in [
        (
            PrFamily::Reviews,
            PrRecord::Review {
                id: 770,
                node_id: Some("TL_LIFECYCLE_REVIEW".into()),
                author: Some("Fixture author".into()),
                actor: Default::default(),
                body: "Offline formal review.".into(),
                state: "COMMENTED".into(),
                submitted_at_ms: Some(FIXTURE_TIME_MS),
                commit_oid: Some(fixture.head.as_str().into()),
            },
        ),
        (
            PrFamily::Threads,
            PrRecord::Thread {
                node_id: LIFECYCLE_THREAD.into(),
                resolved: Some(false),
                outdated: Some(false),
                comments: vec![InlineComment {
                    id: LIFECYCLE_THREAD_ROOT,
                    node_id: Some("TL_LIFECYCLE_ROOT".into()),
                    in_reply_to: None,
                    author: Some("Fixture author".into()),
                    actor: Default::default(),
                    body: "Offline inline review thread.".into(),
                    created_at_ms: Some(FIXTURE_TIME_MS),
                    updated_at_ms: Some(FIXTURE_TIME_MS),
                    anchor: Some(crate::pr::ReviewAnchor {
                        path: anchor.path.clone(),
                        side: anchor.side,
                        line: anchor.first_line,
                        original_oid: anchor.source_oid.clone(),
                    }),
                }],
                replies_complete: true,
                reply_cursor: None,
            },
        ),
    ] {
        let progress = store.start_pr_fetch(&rev, family, WalkMode::Full)?;
        store.commit_pr_page(
            &rev,
            family,
            &progress,
            PrInputPage {
                records: vec![record],
                next_cursor: None,
                validators: None,
                fetched_at_ms: FIXTURE_TIME_MS,
                partial: None,
            },
        )?;
    }
    let mut workspace = store.workspace(account)?.ok_or(Error::NotFound)?;
    workspace.proportions = vec![0.43, 0.57];
    let view = &mut workspace.panes[1].tabs[0].view;
    view.draft = Some(review_id);
    view.pr_reader = Some(PrTabView {
        diff_mode: PrDiffMode::Split,
        selected_path: Some(anchor.path.clone()),
        code_anchor: Some(crate::pr::ReviewAnchor {
            path: anchor.path.clone(),
            side: anchor.side,
            line: 3,
            original_oid: anchor.source_oid.clone(),
        }),
        ..PrTabView::default()
    });
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
    let comments = if inspect_inline_submission {
        vec![DraftInline {
            key: "lifecycle-submitted-line".into(),
            body: "Threadline lifecycle held inline note.".into(),
            anchor,
        }]
    } else {
        Vec::new()
    };
    store.submit_review(
        &rev,
        ReviewSubmission {
            id: LIFECYCLE_ACTIONS[2],
            intent: &ReviewIntent::Review(ReviewDraft {
                event: ReviewEvent::RequestChanges,
                summary: "Threadline lifecycle held review.".into(),
                comments,
            }),
            draft: None,
            dependencies: &LIFECYCLE_ACTIONS[0..1],
            submitted_at_ms: FIXTURE_TIME_MS,
        },
    )?;
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
    if inspect_inline_submission {
        seeded.revision = OFFLINE_OUTBOX_REVISION;
    }
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
    let settings = store.sync_settings(account)?;
    let controls = settings.controls;
    let primary_review = layout
        .panes
        .get(1)
        .and_then(|pane| pane.tabs.first())
        .ok_or(Error::NotFound)?;
    let primary_issue = layout
        .panes
        .first()
        .and_then(|pane| pane.tabs.first())
        .ok_or(Error::NotFound)?
        .item;
    let draft = store
        .review_draft(account, &review_draft_id(primary_review.item))?
        .ok_or(Error::NotFound)?;
    let (revision, _) = store
        .pr_revision(account, draft.item, draft.revision_id)?
        .ok_or(Error::NotFound)?;
    let history_head = CommitOid::parse(&"a".repeat(40))?;
    let history = store.history_page(account, revision.repository, &history_head, None, 10)?;
    let comments = store.discussion_page(account, primary_issue, None, 20)?;
    let reviews = store.pr_page(&revision, PrFamily::Reviews, None, 10)?;
    let threads = store.pr_page(&revision, PrFamily::Threads, None, 10)?;
    let checks = store.pr_page(&revision, PrFamily::Checks, None, 1)?;
    let search = store.search_downloaded(
        account,
        crate::reader::ReaderScope::Repository(revision.repository),
        "Investigate relay behavior 1",
        10,
    )?;
    let sync_checkpoint = store.read(|db| {
        Ok(db.query_row(
            "SELECT cursor,walk_started FROM scheduled_reads
             WHERE account_id=? AND repository_id=? AND item_id IS NULL AND family=? AND revision=''",
            params![account, revision.repository, ResourceFamily::RepositoryItems.name()],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, Option<i64>>(1)?)),
        )?)
    })?;
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
        || history.rows.len() != 2
        || comments.records.is_empty()
        || reviews.rows.len() != 1
        || threads.rows.len() != 1
        || checks.state.coverage != crate::resources::Coverage::Missing
        || search.len() != 1
        || sync_checkpoint.0.as_deref() != Some("2")
        || sync_checkpoint.1.is_none()
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
    let feed = store.notification_feed(account, None, 10)?;
    let dependent_holds = LIFECYCLE_ACTIONS[1..]
        .iter()
        .map(|id| store.dispatch_hold(account, id))
        .collect::<Result<Vec<_>>>()?;
    let credential_generation: i64 = store.read(|db| Ok(db.query_row(
        "SELECT credential_generation FROM github_access_profiles WHERE account_id=? AND owner_scope=?",
        params![account, BACKLOG_OWNER],
        |row| row.get(0),
    )?))?;
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
        "content": {
            "repository": revision.repository.0,
            "issue": primary_issue.0,
            "issue_comments": comments.records.len(),
            "pr": draft.item.0,
            "pr_revision": revision.id,
            "pr_head": revision.head.as_str(),
            "file": "src/relay_00.rs",
            "formal_reviews": reviews.rows.len(),
            "review_threads": threads.rows.len(),
            "check_coverage": format!("{:?}", checks.state.coverage),
            "history_commits": history.rows.len(),
            "history_head": history_head.as_str(),
            "search_matches": search.len(),
        },
        "sync_checkpoint": {"cursor": sync_checkpoint.0, "started_at_ms": sync_checkpoint.1},
        "schedule": {"timezone": settings.schedule.timezone, "work_interval_secs": settings.schedule.work_interval_secs},
        "review_anchor_preserved": true,
        "prior_delivery": "Delivered",
        "delivery_groups": delivery_groups,
        "delivery_unchanged_at_ms": delivery.updated_at_ms,
        "feed_count": feed.len(),
        "feed_event": feed.first().ok_or(Error::NotFound)?.id,
        "work_offline": controls.work_offline,
        "pause_queued_actions": controls.pause_queued_actions,
        "actions": original_rows.iter().map(|row| format!("{:?}", row.state)).collect::<Vec<_>>(),
        "action_ids": original_rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        "first_attempts": attempts.len(),
        "first_send_starts": attempts.iter().filter(|attempt| attempt.sending_at.is_some()).count(),
        "next_recovery_at_ms": store.next_operation_recovery(account)?,
        "dependents": 3,
        "dependent_holds": dependent_holds,
        "credential_generation": credential_generation,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue;

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

    #[test]
    fn offline_cache_clear_preserves_inspectable_intents_and_the_cancelled_prerequisite() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open_profile(directory.path()).unwrap();
        let fixture = populate_native_offline_outbox(
            &store,
            12345,
            "0123456789abcdef0123456789abcdef",
            FIXTURE_TIME_MS + 50_000,
            || Ok(()),
        )
        .unwrap();
        let account = fixture.account;
        let [parent, issue, review, merge] = LIFECYCLE_ACTIONS;
        let saved_review = store.submitted_review(account, review).unwrap().unwrap();
        let ReviewIntent::Review(payload) = &saved_review.intent else {
            panic!("Expected the exact anchored review");
        };
        assert_eq!(payload.event, ReviewEvent::RequestChanges);
        assert_eq!(payload.comments[0].anchor.source_path, "src/relay_00.rs");
        assert_eq!(payload.comments[0].anchor.first_line, 1);
        assert_eq!(saved_review.head, fixture.head);
        let saved_merge = store.submitted_merge(account, merge).unwrap().unwrap();
        assert_eq!(saved_merge.method, MergeMethod::Squash);
        assert_eq!(saved_merge.head, fixture.head);
        let draft_id = review_draft_id(fixture.reviewed_pr);
        let unsent_review = store.review_draft(account, &draft_id).unwrap().unwrap();
        assert_ne!(
            unsent_review.draft.comments[0].body,
            payload.comments[0].body
        );
        assert_eq!(store.pending_dependents_count(account, parent).unwrap(), 3);
        let alternate = store
            .ensure_account("github.com", LIFECYCLE_ALT_USER)
            .unwrap();
        assert!(store.submitted_review(alternate, review).unwrap().is_none());
        assert!(store.submitted_merge(alternate, merge).unwrap().is_none());
        assert_eq!(
            store.pending_dependents_count(alternate, parent).unwrap(),
            0
        );

        store.clear_cache(account).unwrap();
        assert!(
            store
                .discussion_page(account, fixture.long_discussion, None, 10)
                .unwrap()
                .records
                .is_empty()
        );
        assert_eq!(
            store.cancel_operation(account, parent).unwrap(),
            queue::CancelResult::Cancelled
        );
        drop(store);

        let reopened = Store::open_profile(directory.path()).unwrap();
        assert_eq!(
            reopened.pending_dependents_count(account, parent).unwrap(),
            3
        );
        assert_eq!(
            reopened.review_draft(account, &draft_id).unwrap(),
            Some(unsent_review)
        );
        assert_eq!(
            reopened.submitted_review(account, review).unwrap(),
            Some(saved_review)
        );
        assert_eq!(
            reopened.submitted_merge(account, merge).unwrap(),
            Some(saved_merge)
        );
        assert!(matches!(
            reopened.write_intent(account, issue).unwrap(),
            Some(WriteIntent::EditIssue(_))
        ));
        assert_eq!(
            reopened.write_intent(account, parent).unwrap(),
            Some(WriteIntent::CreateComment {
                body: LIFECYCLE_COMMENT.into()
            })
        );
        for id in [issue, review, merge] {
            let child = reopened.operation(account, id).unwrap().unwrap();
            assert_eq!(child.state, queue::State::Queued);
            assert_eq!(child.dependencies, [parent]);
        }
    }
}

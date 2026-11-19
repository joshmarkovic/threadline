use sha2::{Digest, Sha256};
use threadline_storage::{
    AccountId, Error, ItemId, Result, Store,
    fixtures::{
        LIFECYCLE_ACTIONS, LIFECYCLE_DRAFT, LIFECYCLE_THREAD, LIFECYCLE_THREAD_ROOT,
        OFFLINE_OUTBOX_REVISION,
    },
    pr::{PrSection, PrTabView},
    queue::{DraftContent, DraftKind, Workspace},
    reviews::ReviewIntent,
};

use super::{drafts::draft_base, write_composer::WriteBase};

fn targets(layout: &Workspace) -> Result<(ItemId, ItemId, ItemId)> {
    match layout.panes.as_slice() {
        [left, right] if left.tabs.len() == 10 && right.tabs.len() == 10 => {
            Ok((left.tabs[0].item, left.tabs[1].item, right.tabs[0].item))
        }
        _ => Err(Error::Conflict("Unexpected fixture pane layout")),
    }
}

fn names(issue: ItemId, comment: ItemId, review: ItemId) -> [String; 4] {
    [
        format!("issue-{}", issue.0),
        format!("comment-{}", comment.0),
        format!("review-{}", review.0),
        format!("review-reply-{}-{LIFECYCLE_THREAD_ROOT}", review.0),
    ]
}

const DRAFT_BODIES: [&str; 4] = [
    "Keep this offline issue edit.",
    "Keep this ordinary unsent comment.",
    LIFECYCLE_DRAFT,
    "Keep this unsent inline reply.",
];

pub(crate) fn seed_drafts(store: &Store, account: AccountId) -> Result<()> {
    let mut layout = store.workspace(account)?.ok_or(Error::NotFound)?;
    let (issue, comment, review) = targets(&layout)?;
    let [issue_key, comment_key, _, reply_key] = names(issue, comment, review);
    let (revision, _) = store
        .current_pr_revision(account, review)?
        .ok_or(Error::NotFound)?;
    let mut issue_draft = draft_base(store, account, issue, DraftKind::IssueEdit)?;
    issue_draft.body = DRAFT_BODIES[0].into();
    store.create_draft(
        account,
        &issue_key,
        issue,
        DraftKind::IssueEdit,
        &issue_draft,
    )?;
    let mut comment_draft = draft_base(store, account, comment, DraftKind::Comment)?;
    comment_draft.body = DRAFT_BODIES[1].into();
    store.create_draft(
        account,
        &comment_key,
        comment,
        DraftKind::Comment,
        &comment_draft,
    )?;
    store.create_draft(
        account,
        &reply_key,
        review,
        DraftKind::Reply,
        &DraftContent {
            body: DRAFT_BODIES[3].into(),
            base: WriteBase::ReviewReply {
                revision_id: revision.id,
                thread_id: LIFECYCLE_THREAD.into(),
                root_comment_id: LIFECYCLE_THREAD_ROOT
                    .try_into()
                    .expect("nonzero thread root"),
            }
            .encode()?,
            anchor: String::new(),
        },
    )?;

    layout.panes[0].tabs[0].view.draft = Some(issue_key);
    layout.panes[0].tabs[1].view.draft = Some(comment_key);
    let reply = &mut layout.panes[1].tabs[1];
    reply.item = review;
    reply.view.draft = Some(reply_key);
    reply.view.context = None;
    reply.view.pr_reader = Some(PrTabView {
        section: PrSection::Conversation,
        ..Default::default()
    });
    store.save_reader_layout(account, &layout, None)
}

pub(crate) fn draft_checkpoint(store: &Store, account: AccountId) -> Result<serde_json::Value> {
    let layout = store.workspace(account)?.ok_or(Error::NotFound)?;
    let (issue, comment, review) = targets(&layout)?;
    let names = names(issue, comment, review);
    let submitted: Vec<_> = store
        .queue_page(account, 0, 10)?
        .into_iter()
        .map(|row| store.operation(account, &row.id)?.ok_or(Error::NotFound))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter_map(|action| action.draft)
        .collect();
    let mut drafts = Vec::new();
    for (((id, (item, kind)), expected_body), position) in names
        .into_iter()
        .zip([
            (issue, DraftKind::IssueEdit),
            (comment, DraftKind::Comment),
            (review, DraftKind::Review),
            (review, DraftKind::Reply),
        ])
        .zip(DRAFT_BODIES)
        .zip([(0, 0), (0, 1), (1, 0), (1, 1)])
    {
        let draft = store.draft(account, &id)?.ok_or(Error::NotFound)?;
        let body = if kind == DraftKind::Review {
            let review = store.review_draft(account, &id)?.ok_or(Error::NotFound)?;
            review
                .draft
                .comments
                .first()
                .ok_or(Error::NotFound)?
                .body
                .clone()
        } else {
            draft.content.body.clone()
        };
        let tab = &layout.panes[position.0].tabs[position.1];
        if body != expected_body
            || draft.kind != kind
            || draft.item != item
            || tab.item != item
            || tab.view.draft.as_deref() != Some(&id)
            || submitted.iter().any(|(draft, _)| draft == &id)
        {
            return Err(Error::Conflict("The unsent fixture draft changed"));
        }
        let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
        drafts.push(serde_json::json!({
            "id": id, "kind": format!("{kind:?}"), "item": item.0, "tab": tab.id,
            "version": draft.revision, "body_sha256": digest,
        }));
    }
    Ok(drafts.into())
}

pub(crate) fn offline_outbox_checkpoint(
    store: &Store,
    account: AccountId,
) -> Result<serde_json::Value> {
    let review = store
        .submitted_review(account, LIFECYCLE_ACTIONS[2])?
        .ok_or(Error::NotFound)?;
    if !matches!(review.intent, ReviewIntent::Review(payload) if payload.comments.len() == 1) {
        return Err(Error::Conflict(
            "This is not the isolated offline Outbox fixture",
        ));
    }
    let usage = store.cache_usage(account)?;
    let layout = store.workspace(account)?.ok_or(Error::NotFound)?;
    Ok(serde_json::json!({
        "fixture_revision": OFFLINE_OUTBOX_REVISION,
        "account": account.0,
        "offline": store.sync_settings(account)?.controls.work_offline,
        "summaries": usage.downloaded_summaries,
        "discussion_records": usage.discussion_records,
        "content_bytes": usage.content_bytes,
        "offline_pins": usage.offline_pins,
        "tabs": layout.panes.iter().map(|pane| pane.tabs.len()).collect::<Vec<_>>(),
        "drafts": draft_checkpoint(store, account)?,
        "pending_dependents": store.pending_dependents_count(account, LIFECYCLE_ACTIONS[0])?,
        "actions": action_checkpoints(store, account)?,
    }))
}

fn action_checkpoints(store: &Store, account: AccountId) -> Result<Vec<serde_json::Value>> {
    let mut actions = Vec::new();
    for id in LIFECYCLE_ACTIONS {
        let operation = store.operation(account, id)?.ok_or(Error::NotFound)?;
        let attempts = store.attempts(account, id)?;
        let snapshot = serde_json::to_vec(&operation.snapshot).map_err(|_| Error::Corrupt)?;
        actions.push(serde_json::json!({
            "id": id,
            "kind": format!("{:?}", operation.kind),
            "target": (operation.original_repository.0, operation.original_number),
            "item": operation.item.0,
            "snapshot_sha256": format!("{:x}", Sha256::digest(snapshot)),
            "dependencies": operation.dependencies,
            "state": format!("{:?}", operation.state),
            "reason": operation.reason,
            "cancel_requested": operation.cancel_requested,
            "attempts": attempts.len(),
            "send_starts": attempts.iter().filter(|attempt| attempt.sending_at.is_some()).count(),
            "dispatch_hold": store.dispatch_hold(account, id)?,
            "scope_hold": store.github_dispatch_hold(account, id)?,
        }));
    }
    Ok(actions)
}

pub(crate) fn scope_hold_checkpoint(
    store: &Store,
    account: AccountId,
) -> Result<serde_json::Value> {
    use threadline_storage::fixtures::{BACKLOG_OWNER, BACKLOG_USER, SCOPE_HOLD_REVISION};
    let profile = store
        .github_access_profile(account, BACKLOG_OWNER)?
        .ok_or(Error::NotFound)?;
    if profile.user_id.get() != BACKLOG_USER || store.active_github_account()? != Some(account) {
        return Err(Error::Conflict(
            "The original fixture account is not active",
        ));
    }
    let actions = action_checkpoints(store, account)?;
    if actions.iter().any(|row| {
        row["dependencies"]
            .as_array()
            .is_none_or(|deps| !deps.is_empty())
    }) {
        return Err(Error::Conflict(
            "The isolated scope actions must be independent",
        ));
    }
    let repositories = store.github_repository_views(account)?;
    let controls = store.sync_settings(account)?.controls;
    Ok(serde_json::json!({
        "fixture_revision": SCOPE_HOLD_REVISION,
        "account": account.0,
        "user": profile.user_id.get(),
        "owner": profile.owner_scope,
        "offline": controls.work_offline,
        "outbox_paused": controls.pause_queued_actions,
        "selected": repositories.iter().filter(|repo| repo.selected)
            .map(|repo| repo.provider_id.get()).collect::<Vec<_>>(),
        "repositories": repositories.iter().map(|repo| (repo.provider_id.get(), repo.id.0)).collect::<Vec<_>>(),
        "drafts": draft_checkpoint(store, account)?,
        "actions": actions,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use threadline_storage::fixtures::{
        lifecycle_checkpoint, populate_native_offline_outbox, populate_native_scope_hold,
    };

    #[test]
    fn mixed_scope_hold_fixture_keeps_every_independent_target_held_after_restart() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open_profile(directory.path()).unwrap();
        let now = 1_900_000_000_000;
        let fixture = populate_native_scope_hold(
            &store,
            12345,
            "00112233445566778899aabbccddeeff",
            now,
            || Ok(()),
        )
        .unwrap();
        let account = fixture.account;
        seed_drafts(&store, account).unwrap();
        let before = scope_hold_checkpoint(&store, account).unwrap();
        assert_eq!(before["selected"], serde_json::json!([72, 71]));
        assert_eq!(before["actions"][0]["state"], "NeedsAttention");
        assert_eq!(before["actions"][0]["attempts"], 1);
        assert_eq!(
            before["actions"][0]["reason"],
            "write_permission_unavailable"
        );
        store
            .update_sync_settings(account, |settings| {
                settings.controls.work_offline = false;
                settings.controls.pause_queued_actions = false;
            })
            .unwrap();
        assert_eq!(store.eligible_queued_actions(account, now + 1).unwrap(), 3);
        store
            .update_sync_settings(account, |settings| settings.controls.work_offline = true)
            .unwrap();
        store
            .select_github_repository(account, fixture.repository, false)
            .unwrap();
        drop(store);

        let reopened = Store::open_profile(directory.path()).unwrap();
        reopened
            .update_sync_settings(account, |settings| settings.controls.work_offline = false)
            .unwrap();
        assert_eq!(
            reopened.eligible_queued_actions(account, now + 2).unwrap(),
            0
        );
        assert!(
            reopened
                .claim_write(account, now + 2, now + 1000)
                .unwrap()
                .is_none()
        );
        assert!(
            reopened
                .claim_review(account, now + 2, now + 1000)
                .unwrap()
                .is_none()
        );
        assert!(
            reopened
                .claim_merge(account, now + 2, now + 1000)
                .unwrap()
                .is_none()
        );
        let after = scope_hold_checkpoint(&reopened, account).unwrap();
        assert_eq!(after["selected"], serde_json::json!([72]));
        assert_eq!(after["user"], before["user"]);
        assert_eq!(after["drafts"], before["drafts"]);
        for (original, saved) in before["actions"]
            .as_array()
            .unwrap()
            .iter()
            .zip(after["actions"].as_array().unwrap())
        {
            for field in [
                "id",
                "target",
                "snapshot_sha256",
                "state",
                "attempts",
                "reason",
            ] {
                assert_eq!(saved[field], original[field]);
            }
            assert_eq!(saved["scope_hold"], "repository_unselected");
            assert_eq!(saved["dispatch_hold"], "repository_unselected");
            assert_eq!(saved["send_starts"], 0);
        }
    }

    #[test]
    fn mixed_lifecycle_fixture_restores_each_unsent_draft_and_cache_after_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open_profile(directory.path()).unwrap();
        let fixture = populate_native_offline_outbox(
            &store,
            12345,
            "00112233445566778899aabbccddeeff",
            1_900_000_000_000,
            || Ok(()),
        )
        .unwrap();
        seed_drafts(&store, fixture.account).unwrap();
        let before = lifecycle_checkpoint(&store, "stable").unwrap();
        let draft_state = draft_checkpoint(&store, fixture.account).unwrap();
        assert_eq!(draft_state.as_array().unwrap().len(), 4);
        assert_eq!(before["sync_checkpoint"]["cursor"], "2");
        assert_eq!(before["content"]["history_commits"], 2);
        let outbox_before = offline_outbox_checkpoint(&store, fixture.account).unwrap();
        drop(store);
        let reopened = Store::open_profile(directory.path()).unwrap();
        assert_eq!(
            draft_checkpoint(&reopened, fixture.account).unwrap(),
            draft_state
        );
        assert_eq!(lifecycle_checkpoint(&reopened, "stable").unwrap(), before);

        reopened.clear_cache(fixture.account).unwrap();
        reopened
            .cancel_operation(fixture.account, LIFECYCLE_ACTIONS[0])
            .unwrap();
        let after = offline_outbox_checkpoint(&reopened, fixture.account).unwrap();
        assert!(
            after["discussion_records"].as_u64() < outbox_before["discussion_records"].as_u64()
        );
        assert_eq!(after["offline_pins"], outbox_before["offline_pins"]);
        assert_eq!(after["drafts"], outbox_before["drafts"]);
        assert_eq!(after["tabs"], outbox_before["tabs"]);
        assert_eq!(after["pending_dependents"], 3);
        assert_eq!(after["actions"][0]["state"], "Cancelled");
        assert_eq!(after["actions"][1], outbox_before["actions"][1]);
        assert_eq!(after["actions"][2], outbox_before["actions"][2]);
        assert_eq!(after["actions"][3], outbox_before["actions"][3]);
    }
}

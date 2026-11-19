use super::{OWNER, number};
use rusqlite::{Connection, OpenFlags, params};
use serde_json::{Value, json};
use std::{error::Error, fs, path::Path, time::Duration};
use threadline_storage::{
    AccountId, ItemLocator, RepositoryId, Store, UtcMillis,
    notifications::{Category, DeliveryMode, EventActor, EventFacts},
    pr::{InlineComment, PrFamily, PrInputPage, PrRecord, PrRevision, ReviewAnchor, SourceSide},
    resources::WalkMode,
};

pub(super) const MARKER: &str = "threadline-s11-grouped-review-v1";
const ACTOR: u64 = 70021;
const PR: u64 = 70;
const REVIEW: u64 = 812;
const COMMENT: u64 = 813;
type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn revision(store: &Store, account: AccountId, repository: RepositoryId) -> Result<PrRevision> {
    let item = store
        .resolve(
            account,
            ItemLocator {
                repository,
                number: PR,
            },
        )?
        .ok_or("Missing fixture pull request")?;
    Ok(store
        .current_pr_revision(account, item)?
        .ok_or("Missing fixture revision")?
        .0)
}

fn commit(
    store: &Store,
    rev: &PrRevision,
    family: PrFamily,
    record: PrRecord,
    now: i64,
) -> Result<()> {
    let walk = store.start_pr_fetch(rev, family, WalkMode::Full)?;
    store.commit_pr_page(
        rev,
        family,
        &walk,
        PrInputPage {
            records: vec![record],
            next_cursor: None,
            partial: None,
            validators: None,
            fetched_at_ms: now,
        },
    )?;
    Ok(())
}

fn observe_review(store: &Store, rev: &PrRevision, at: i64) -> Result<()> {
    let actor = EventActor {
        user_id: Some(number(ACTOR)),
        ..Default::default()
    };
    let review = PrRecord::Review {
        id: REVIEW,
        node_id: None,
        author: Some("fixture-reviewer".into()),
        actor,
        body: "One suggested change.".into(),
        state: "COMMENTED".into(),
        submitted_at_ms: Some(at),
        commit_oid: Some(rev.head.as_str().into()),
    };
    let thread = PrRecord::Thread {
        node_id: "fixture-review-thread".into(),
        resolved: Some(false),
        outdated: Some(false),
        location: None,
        comments: vec![InlineComment {
            id: COMMENT,
            node_id: None,
            in_reply_to: None,
            author: Some("fixture-reviewer".into()),
            actor,
            body: "The first fixture line needs an example.".into(),
            created_at_ms: Some(at),
            updated_at_ms: Some(at),
            anchor: Some(ReviewAnchor {
                path: "first.txt".into(),
                side: SourceSide::Right,
                line: 1,
                original_oid: rev.head.as_str().into(),
            }),
        }],
        replies_complete: true,
        reply_cursor: None,
    };
    commit(store, rev, PrFamily::Reviews, review, at)?;
    commit(store, rev, PrFamily::Threads, thread, at)
}

pub(super) fn seed(store: &Store, account: AccountId, repository: RepositoryId) -> Result<()> {
    store.update_notification_settings(account, |s| {
        s.categories
            .insert(Category::CommentOnlyReview, DeliveryMode::Popup);
        s.categories
            .insert(Category::InlineComment, DeliveryMode::Popup);
        s.categories
            .insert(Category::CheckStatus, DeliveryMode::FeedOnly);
    })?;
    observe_review(
        store,
        &revision(store, account, repository)?,
        UtcMillis::now()?.0 + 1,
    )
}

fn owned(root: &Path) -> Result<(Connection, AccountId, RepositoryId)> {
    if fs::read(root.join(MARKER))? != MARKER.as_bytes() {
        return Err("The grouped review fixture marker did not match.".into());
    }
    let db = Connection::open_with_flags(
        root.join("threadline.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    db.busy_timeout(Duration::from_secs(3))?;
    let (account, repository) = db.query_row(
        "SELECT r.account_id, r.id FROM repositories r
         JOIN github_access_profiles p ON p.account_id = r.account_id
         JOIN accounts a ON a.id = r.account_id JOIN dispatch_profiles d ON d.account_id = a.id
         JOIN sync_settings s ON s.account_id = a.id
         WHERE (SELECT count(*) FROM accounts) = 1 AND (SELECT count(*) FROM repositories) = 1
         AND a.provider_user_id = 817123456 AND r.provider_repository_id = 917123456
         AND p.owner_scope = ? AND r.owner = ? AND r.name = 'installed-alerts' AND d.offline = 0 AND d.outbox_paused = 1 AND s.background_paused = 1",
        [OWNER, OWNER],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((db, account, repository))
}

pub(super) fn inspect(root: &Path) -> Result<Value> {
    let (mut db, account, repo) = owned(root)?;
    let tx = db.transaction()?;
    let mut events = Vec::new();
    let mut query = tx.prepare(
        "SELECT a.event_kind, a.event_key, a.occurred_at_ms, n.facts, n.published,
                json_extract(n.decision, '$.mode'), n.group_id IS NOT NULL, EXISTS (SELECT 1 FROM json_each(n.decision,'$.reasons') WHERE value='OperatingSystem')
         FROM activity_events a JOIN notification_events n USING(account_id,event_key)
         JOIN items i ON i.id = a.item_id AND i.repository_id = a.repository_id
         WHERE a.account_id = ? AND a.repository_id = ? AND i.number = ? ORDER BY n.id",
    )?;
    let rows = query.query_map(params![account, repo, 70], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, bool>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, bool>(6)?,
            r.get::<_, bool>(7)?,
        ))
    })?;
    for row in rows {
        let (kind, key, at, facts, published, mode, grouped, os_disabled) = row?;
        let facts: EventFacts = serde_json::from_str(&facts)?;
        events.push(json!({"kind":kind,"key":key,"occurred_at":at,"categories":facts.categories,"actor_id":facts.actor.user_id,
            "automation":facts.actor.known_automation,"published":published,"mode":mode,"grouped":grouped,"os_disabled":os_disabled}));
    }
    let (group_count, native_id, outcome, changed): (u32, Option<String>, Option<String>, Option<i64>) = tx.query_row(
        "SELECT count(*), MIN(native_id), MIN(state), MIN(changed_at) FROM native_notification_groups WHERE account_id = ?",
        [account], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
    let total: u32 = tx.query_row("SELECT count(*) FROM notification_events", [], |r| r.get(0))?;
    if total as usize != events.len() {
        return Err("An unexpected notification appeared outside the fixture PR.".into());
    }
    Ok(
        json!({"fixture":MARKER,"pull_number":PR,"background_and_outbox_paused":true,"event_count":total,"events":events,
        "group_count":group_count,"native_id":native_id,"outcome":outcome,"changed_at":changed}),
    )
}

pub(super) fn replay(root: &Path) -> Result<Value> {
    let state = inspect(root)?;
    if state["outcome"] != "delivered" || state["group_count"] != 1 || state["event_count"] != 2 {
        return Err("Replay requires the original delivered review group.".into());
    }
    let at = state["events"][0]["occurred_at"]
        .as_i64()
        .ok_or("Missing original review time")?;
    let (_, account, repo) = owned(root)?;
    let store = Store::open_profile(root)?;
    let rev = revision(&store, account, repo)?;
    observe_review(&store, &rev, at)?;
    let now = UtcMillis::now()?.0.max(at + 1);
    commit(
        &store,
        &rev,
        PrFamily::Checks,
        PrRecord::Check {
            id: 814,
            head_oid: rev.head.as_str().into(),
            name: "Fixture restart barrier".into(),
            status: "completed".into(),
            conclusion: Some("success".into()),
            completed_at_ms: Some(now),
        },
        now,
    )?;
    drop(store);
    inspect(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use threadline_storage::notifications::PopupOutcome;
    #[test]
    fn repeated_review_pages_leave_one_delivery_and_an_independent_feed_only_barrier() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let before = crate::seed(root, crate::Fixture::Review).unwrap();
        assert_eq!(before["event_count"], 2);
        assert_eq!(
            before["events"][0]["categories"],
            json!(["CommentOnlyReview"])
        );
        assert_eq!(before["events"][1]["categories"], json!(["InlineComment"]));
        assert_eq!(before["events"][1]["actor_id"], ACTOR);
        assert!(replay(root).is_err());
        let (_, account, _) = owned(root).unwrap();
        let store = Store::open_profile(root).unwrap();
        let now = UtcMillis::now().unwrap().0;
        assert_eq!(
            store
                .process_notification_events(account, now, "UTC", true, 20)
                .unwrap(),
            2
        );
        let group = store
            .claim_notification_popup(account, now, "UTC", true)
            .unwrap()
            .unwrap();
        assert_eq!(group.event_count, 2);
        store
            .record_notification_outcome(account, &group.native_id, PopupOutcome::Delivered, now)
            .unwrap();
        drop(store);
        let pending = replay(root).unwrap();
        assert_eq!(pending["event_count"], 3);
        assert_eq!(pending["events"][2]["published"], false);
        let store = Store::open_profile(root).unwrap();
        assert_eq!(
            store
                .process_notification_events(account, now + 1000, "UTC", true, 20)
                .unwrap(),
            1
        );
        assert!(
            store
                .claim_notification_popup(account, now + 1000, "UTC", true)
                .unwrap()
                .is_none()
        );
        let after = inspect(root).unwrap();
        assert_eq!(after["events"][2]["mode"], "FeedOnly");
        assert_eq!(after["changed_at"], pending["changed_at"]);
        assert_eq!(after["group_count"], 1);
    }
}

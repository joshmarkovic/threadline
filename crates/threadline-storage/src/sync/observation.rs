use super::*;
use crate::{
    Change, ItemLocator, ItemRepresentation, ItemUpdate, identity::ingest_in,
    reader::DiscussionComment,
};
use std::{collections::BTreeSet, num::NonZeroU64};

pub struct ObservedItem {
    pub number: u64,
    pub provider_id: u64,
    pub created_at_ms: Option<i64>,
    pub assignee_ids: Option<BTreeSet<NonZeroU64>>,
    pub update: ItemUpdate,
}

pub struct ObservedComment {
    pub number: u64,
    pub comment: DiscussionComment,
    pub actor: crate::notifications::EventActor,
}

pub struct ObservationPage<'a> {
    pub items: Vec<ObservedItem>,
    pub comments: Vec<ObservedComment>,
    pub next_cursor: Option<&'a str>,
    pub validators: Validators,
}

impl Store {
    /// Publish one page and its journal in one transaction. Only a complete walk advances its watermark.
    pub fn commit_observed_page(
        &self,
        claim: &ReadClaim,
        page: ObservationPage<'_>,
        now_ms: i64,
        system_zone: &str,
    ) -> Result<()> {
        self.write(|tx| apply_page(tx, claim, page, now_ms, Some(system_zone)))
    }

    /// Preserve visible rows from an SSO-limited page while retaining the input cursor for a verified retry.
    pub fn commit_observed_partial(
        &self,
        claim: &ReadClaim,
        page: ObservationPage<'_>,
        now_ms: i64,
    ) -> Result<()> {
        self.write(|tx| apply_page(tx, claim, page, now_ms, None))
    }
}

fn apply_page(
    tx: &Transaction<'_>,
    claim: &ReadClaim,
    page: ObservationPage<'_>,
    now: i64,
    complete_zone: Option<&str>,
) -> Result<()> {
    require_claim(tx, claim, now)?;
    let stream = match (claim.target.family, claim.target.item) {
        (ResourceFamily::RepositoryItems, None) if page.comments.is_empty() => "issues",
        (ResourceFamily::Comments, None) if page.items.is_empty() => "comments",
        _ => {
            return Err(Error::InvalidInput(
                "Page does not match this observation stream",
            ));
        }
    };
    if page.items.len() + page.comments.len() > 100 {
        return Err(Error::InvalidInput(
            "Observation page exceeds GitHub's page size",
        ));
    }
    if let Some(next) = page.next_cursor {
        let current = claim
            .cursor
            .as_deref()
            .unwrap_or("1")
            .parse::<u32>()
            .map_err(|_| Error::Corrupt)?;
        if next.parse::<u32>().ok().filter(|n| *n > 0) != current.checked_add(1)
            || current == u32::MAX
        {
            return Err(Error::InvalidInput(
                "Observation continuation skipped or repeated a page",
            ));
        }
    }
    let baseline=tx.query_row(
        "SELECT COALESCE((SELECT observed_at_ms FROM activity_baselines WHERE repository_id=?2 AND stream_key=?3), walk_started)
         FROM scheduled_reads WHERE id=?1",params![claim.id,claim.target.repository,stream],|row|row.get(0))?;
    let mut high = None;
    for row in page.items {
        high = high.max(Some(observe_issue(tx, claim, row, baseline, now)?));
    }
    for row in page.comments {
        high = high.max(Some(observe_comment(tx, claim, row, baseline, now)?));
    }
    let Some(zone) = complete_zone else {
        tx.execute(
            "UPDATE scheduled_reads SET lease_until=NULL, failure='authorization', suspended=1, failures=failures+1 WHERE id=?",
            [claim.id],
        )?;
        return Ok(());
    };
    if claim.cursor.is_none() {
        tx.execute("UPDATE scheduled_reads SET walk_etag=?2, walk_modified=?3, walk_revision=?4 WHERE id=?1",
            params![claim.id, page.validators.etag, page.validators.last_modified, page.validators.revision])?;
    }
    if let Some(high) = high {
        tx.execute("UPDATE scheduled_reads SET page_watermark=MAX(COALESCE(page_watermark,?2),?2) WHERE id=?1",
            params![claim.id,high])?;
    }
    if let Some(next) = page.next_cursor {
        tx.execute(
            "UPDATE scheduled_reads SET cursor=?2, due_at=?3, lease_until=NULL, leased_generation=NULL,
             failures=0, failure=NULL, suspended=0 WHERE id=?1",
            params![claim.id,next,now],
        )?;
        return Ok(());
    }
    let (started, high, validators) = tx.query_row(
        "SELECT walk_started, MAX(COALESCE(watermark,walk_started),
             MIN(COALESCE(page_watermark,watermark,walk_started), MAX(walk_started,?2))),
             walk_etag, walk_modified, walk_revision FROM scheduled_reads WHERE id=?1",
        params![claim.id, now],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                Validators {
                    etag: row.get(2)?,
                    last_modified: row.get(3)?,
                    revision: row.get(4)?,
                },
            ))
        },
    )?;
    tx.execute(
        "UPDATE scheduled_reads SET watermark=? WHERE id=?",
        params![high, claim.id],
    )?;
    tx.execute(
        "INSERT INTO activity_baselines(repository_id,stream_key,checkpoint,observed_at_ms) VALUES(?1,?2,?3,?4)
         ON CONFLICT(repository_id,stream_key) DO UPDATE SET checkpoint=excluded.checkpoint",
        params![claim.target.repository,stream,high.to_string(),started],
    )?;
    complete_in(tx, claim, now, zone, &validators)
}

fn observe_issue(
    tx: &Transaction<'_>,
    claim: &ReadClaim,
    row: ObservedItem,
    baseline: i64,
    now: i64,
) -> Result<i64> {
    let updated = row.update.remote_updated_at.0;
    if !matches!(&row.update.representation, ItemRepresentation::Issue(r) if r.id.get()==row.provider_id)
        || row.created_at_ms.is_some_and(|created| created > updated)
    {
        return Err(Error::InvalidInput(
            "Invalid issue feed identity or timestamp",
        ));
    }
    let state = match row.update.closed {
        Change::Value(true) => Some("closed"),
        Change::Value(false) => Some("open"),
        _ => None,
    };
    let repo = claim.target.repository;
    let is_issue = matches!(&row.update.representation, ItemRepresentation::Issue(record)
        if record.classification == crate::IssueClassification::Issue);
    let item = ingest_in(
        tx,
        claim.account,
        ItemLocator {
            repository: repo,
            number: row.number,
        },
        row.update,
    )?;
    tx.execute(
        "INSERT INTO cached_resource_records(resource_id,record_key,body)
         SELECT id,CAST(?3 AS TEXT),'' FROM cached_resources WHERE account_id=?1 AND repository_id=?2 AND family='repository-items'
         ON CONFLICT(resource_id,record_key) DO NOTHING",params![claim.account,repo,item],
    )?;
    tx.execute(
        "INSERT INTO cached_resource_pending(resource_id,record_key,body)
         SELECT id,CAST(?3 AS TEXT),'' FROM cached_resources
         WHERE account_id=?1 AND repository_id=?2 AND family='repository-items' AND fetch=1
         ON CONFLICT(resource_id,record_key) DO NOTHING",
        params![claim.account, repo, item],
    )?;
    let object = Object {
        kind: "issues",
        id: row.provider_id,
        item: Some(item),
        created: row.created_at_ms,
        updated,
        state,
    };
    observe_object(tx, claim, &object, Some(baseline), now)?;
    if is_issue && let Some(assignees) = row.assignee_ids {
        observe_assignments(tx, claim, &object, item, &assignees, baseline, now)?;
    }
    Ok(updated)
}

fn observe_assignments(
    tx: &Transaction<'_>,
    claim: &ReadClaim,
    issue: &Object<'_>,
    item: ItemId,
    current: &BTreeSet<NonZeroU64>,
    baseline: i64,
    now: i64,
) -> Result<()> {
    use crate::notifications::{self, Category, EventFacts, NotificationTarget, ReceiptMatch};

    let repository = claim.target.repository;
    let provider_id = issue.id.to_string();
    let (latest_issue, previous_time, previous_set, account_user) = tx.query_row(
        "SELECT issue.updated_at, assignments.updated_at, assignments.state, accounts.provider_user_id
         FROM feed_object_versions issue JOIN accounts ON accounts.id=issue.account_id
         LEFT JOIN feed_object_versions assignments
           ON assignments.account_id=issue.account_id AND assignments.repository_id=issue.repository_id
           AND assignments.provider_id=issue.provider_id AND assignments.category='issue_assignments'
         WHERE issue.account_id=? AND issue.repository_id=? AND issue.provider_id=? AND issue.category='issues'",
        params![claim.account, repository, provider_id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?,
                  row.get::<_, Option<String>>(2)?, row.get::<_, String>(3)?)),
    )?;
    if latest_issue > issue.updated || previous_time.is_some_and(|time| time >= issue.updated) {
        return Ok(());
    }
    let previous: Option<BTreeSet<NonZeroU64>> = previous_set
        .map(|raw| serde_json::from_str(&raw).map_err(|_| Error::Corrupt))
        .transpose()?;
    let encoded = serde_json::to_string(current).map_err(|_| Error::Corrupt)?;
    tx.execute(
        "INSERT INTO feed_object_versions(account_id,repository_id,category,provider_id,updated_at,state)
         VALUES(?,?,'issue_assignments',?,?,?)
         ON CONFLICT(account_id,repository_id,category,provider_id) DO UPDATE
         SET updated_at=excluded.updated_at,state=excluded.state",
        params![claim.account, repository, provider_id, issue.updated, encoded],
    )?;
    let changed = match &previous {
        Some(known) => known != current,
        None => !current.is_empty() && issue.created.is_some_and(|created| created > baseline),
    };
    if !changed || issue.updated <= baseline {
        return Ok(());
    }
    let event_kind = "issue_assignment_changed";
    let event_key = format!("github:issues:{provider_id}:{event_kind}:{}", issue.updated);
    let revision = issue.updated.to_string();
    let inserted = tx.execute(
        "INSERT INTO activity_events(account_id,event_key,repository_id,item_id,event_kind,revision,occurred_at_ms,observed_at_ms)
         VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(account_id,event_key) DO NOTHING",
        params![claim.account, event_key, repository, item, event_kind, revision, issue.updated, now],
    )?;
    if inserted == 1 {
        let account_user = account_user.parse().map_err(|_| Error::Corrupt)?;
        notifications::attach_in(
            tx,
            claim.account,
            &event_key,
            &EventFacts {
                target: NotificationTarget::Item { item, thread: None },
                categories: [Category::IssueAssignment].into(),
                actor: Default::default(),
                involves_me: current.contains(&account_user)
                    || previous.is_some_and(|ids| ids.contains(&account_user)),
                receipt: Some(ReceiptMatch::Ordinary {
                    action: "issue_edited".into(),
                    remote_id: provider_id,
                    revision,
                }),
            },
        )?;
    }
    Ok(())
}

fn observe_comment(
    tx: &Transaction<'_>,
    claim: &ReadClaim,
    row: ObservedComment,
    baseline: i64,
    now: i64,
) -> Result<i64> {
    let c = row.comment;
    if c.provider_id == 0 || c.updated_at_ms < c.created_at_ms {
        return Err(Error::InvalidInput(
            "Invalid comment feed identity or timestamp",
        ));
    }
    let number = i64::try_from(row.number)
        .ok()
        .filter(|n| *n > 0)
        .ok_or(Error::InvalidInput("Invalid item number"))?;
    let repo = claim.target.repository;
    let item = tx
        .query_row(
            "SELECT id FROM items WHERE repository_id=? AND account_id=? AND number=?",
            params![repo, claim.account, number],
            |row| row.get(0),
        )
        .optional()?;
    let item = match item {
        Some(id) => id,
        None => {
            let moved:bool=tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM item_aliases WHERE repository_id=? AND account_id=? AND number=?)",
                params![repo,claim.account,number],|row|row.get(0))?;
            if moved {
                return Err(Error::Conflict(
                    "Updated comment requires a verified current locator",
                ));
            }
            tx.query_row(
                "INSERT INTO items(repository_id,account_id,number) VALUES(?,?,?) RETURNING id",
                params![repo, claim.account, number],
                |row| row.get(0),
            )?
        }
    };
    apply_downloaded_comment(tx, item, &c)?;
    let object = Object {
        kind: "comments",
        id: c.provider_id,
        item: Some(item),
        created: Some(c.created_at_ms),
        updated: c.updated_at_ms,
        state: None,
    };
    if let Some((key, action)) = observe_object(tx, claim, &object, Some(baseline), now)? {
        use crate::notifications::{self, Category, EventFacts, NotificationTarget, ReceiptMatch};
        let login: String = tx.query_row(
            "SELECT p.login FROM github_access_profiles p JOIN github_repository_access r
            USING(account_id,owner_scope) WHERE r.account_id=? AND r.repository_id=?",
            params![claim.account, repo],
            |r| r.get(0),
        )?;
        let mut actor = row.actor;
        if actor.user_id.is_none() {
            let cached: Option<String> = tx.query_row(
                "SELECT author_id FROM write_comment_authors WHERE account_id=? AND item_id=? AND comment_id=?",
                params![claim.account, item, c.provider_id.to_string()], |r| r.get(0)
            ).optional()?;
            actor.user_id = cached
                .map(|id| id.parse().map_err(|_| Error::Corrupt))
                .transpose()?;
        }
        let mentioned = notifications::mentions_login(&c.body, &login);
        let mut categories = std::collections::BTreeSet::from([Category::OrdinaryComment]);
        if mentioned {
            categories.insert(Category::DirectMention);
        }
        notifications::attach_in(
            tx,
            claim.account,
            &key,
            &EventFacts {
                target: NotificationTarget::Item {
                    item,
                    thread: Some(format!("issue-comment:{}", c.provider_id)),
                },
                categories,
                actor,
                involves_me: mentioned,
                receipt: Some(ReceiptMatch::Ordinary {
                    action: action.into(),
                    remote_id: c.provider_id.to_string(),
                    revision: c.updated_at_ms.to_string(),
                }),
            },
        )?;
    }
    Ok(c.updated_at_ms)
}

struct Object<'a> {
    kind: &'a str,
    id: u64,
    item: Option<ItemId>,
    created: Option<i64>,
    updated: i64,
    state: Option<&'a str>,
}

fn observe_object(
    tx: &Transaction<'_>,
    claim: &ReadClaim,
    o: &Object<'_>,
    baseline: Option<i64>,
    now: i64,
) -> Result<Option<(String, &'static str)>> {
    let id = o.id.to_string();
    let old: Option<(i64, Option<String>)> = tx
        .query_row(
            "SELECT updated_at,state FROM feed_object_versions
        WHERE account_id=? AND repository_id=? AND category=? AND provider_id=?",
            params![claim.account, claim.target.repository, o.kind, id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if old.as_ref().is_some_and(|(time, _)| *time >= o.updated) {
        return Ok(None);
    }
    let action = baseline.and_then(|cutoff| {
        if old.is_none() && o.created.is_some_and(|t| t > cutoff) {
            return Some((
                if o.kind == "comments" {
                    "comment_created"
                } else {
                    "item_created"
                },
                o.created?,
            ));
        }
        if o.updated <= cutoff {
            return None;
        }
        match o.kind {
            "comments" if o.created.is_some_and(|t| t < o.updated) => {
                Some(("comment_edited", o.updated))
            }
            "issues"
                if old
                    .as_ref()
                    .and_then(|(_, state)| state.as_deref())
                    .zip(o.state)
                    .is_some_and(|(a, b)| a != b) =>
            {
                Some(("item_state_changed", o.updated))
            }
            _ => None,
        }
    });
    let mut observed = None;
    if let Some((action, time)) = action {
        let key = format!("github:{}:{}:{action}:{time}", o.kind, o.id);
        let inserted=tx.execute("INSERT INTO activity_events(account_id,event_key,repository_id,item_id,event_kind,revision,occurred_at_ms,observed_at_ms)
            VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(account_id,event_key) DO NOTHING",
            params![claim.account,key,claim.target.repository,o.item,action,o.updated.to_string(),time,now])?;
        if inserted == 1 {
            observed = Some((key, action));
        }
    }
    tx.execute("INSERT INTO feed_object_versions(account_id,repository_id,category,provider_id,updated_at,state)
        VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(account_id,repository_id,category,provider_id) DO UPDATE
        SET updated_at=excluded.updated_at,state=COALESCE(excluded.state,feed_object_versions.state)",
        params![claim.account,claim.target.repository,o.kind,id,o.updated,o.state])?;
    Ok(observed)
}

pub(crate) fn apply_downloaded_comment(
    tx: &Transaction<'_>,
    item: ItemId,
    c: &DiscussionComment,
) -> Result<()> {
    let resource: Option<(i64,i64,bool)> = tx
        .query_row(
            "SELECT id,generation,fetch=1 FROM cached_resources WHERE item_id=? AND family='comments' AND (coverage<>0 OR fetch=1)",
            [item],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)),
        )
        .optional()?;
    let Some((rid, generation, walking)) = resource else {
        return Ok(());
    };
    let key = format!("{:020}", c.provider_id);
    if walking {
        let changed=tx.execute(
            "INSERT INTO reader_comment_staging(resource_id,generation,record_key,node_id,author,created_at,updated_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(resource_id,generation,record_key) DO UPDATE SET
             node_id=excluded.node_id,author=excluded.author,created_at=excluded.created_at,updated_at=excluded.updated_at
             WHERE excluded.updated_at>reader_comment_staging.updated_at",
            params![rid,generation,key,c.node_id,c.author,c.created_at_ms,c.updated_at_ms],
        )?;
        if changed > 0 {
            tx.execute(
                "INSERT INTO cached_resource_pending(resource_id,record_key,body) VALUES(?1,?2,?3)
                        ON CONFLICT(resource_id,record_key) DO UPDATE SET body=excluded.body",
                params![rid, key, c.body],
            )?;
        }
    }
    let prior: Option<i64> = tx
        .query_row(
            "SELECT updated_at FROM reader_comment_metadata WHERE resource_id=? AND record_key=?",
            params![rid, key],
            |r| r.get(0),
        )
        .optional()?;
    if prior.is_some_and(|t| t >= c.updated_at_ms) {
        return Ok(());
    }
    tx.execute(
        "INSERT INTO cached_resource_records(resource_id,record_key,body) VALUES(?1,?2,?3)
        ON CONFLICT(resource_id,record_key) DO UPDATE SET body=excluded.body",
        params![rid, key, c.body],
    )?;
    tx.execute("INSERT INTO reader_comment_metadata(resource_id,record_key,node_id,author,created_at,updated_at)
        VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(resource_id,record_key) DO UPDATE SET node_id=excluded.node_id,
        author=excluded.author,created_at=excluded.created_at,updated_at=excluded.updated_at",
        params![rid,key,c.node_id,c.author,c.created_at_ms,c.updated_at_ms])?;
    Ok(())
}

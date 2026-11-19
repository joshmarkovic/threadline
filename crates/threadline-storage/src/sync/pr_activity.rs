use super::*;
use crate::{
    Change, ItemLocator, ItemRepresentation, ItemUpdate, NodeId, PullRequestRecord,
    PullRequestUpdate, UtcMillis,
    identity::ingest_in,
    pr::{PrFamily, PrHeader, PrRevision},
    resources::CommitOid,
};
use threadline_core::github::reader::{
    Page,
    activity::{OpenPull, PullActivity},
};

const INDEX: &str = "activity-open-pulls";
pub(super) const TIMELINE: &str = "activity-pull-timeline";
const SUBSCRIBER: &str = "pull-activity-monitor";
const CHECKS: &str = "pull-head-monitor";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrActivityRead {
    OpenIndex,
    Timeline,
}
impl PrActivityRead {
    pub(crate) const TIMELINE_KEY: &str = TIMELINE;

    pub fn from_target(target: &ReadTarget) -> Option<Self> {
        match (target.family, target.item, target.revision.as_str()) {
            (ResourceFamily::References, None, INDEX) => Some(Self::OpenIndex),
            (ResourceFamily::Reviews, Some(_), TIMELINE) => Some(Self::Timeline),
            _ => None,
        }
    }
}

pub(super) fn enroll_index(tx: &Transaction<'_>, account: AccountId, now: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO scheduled_reads(account_id,repository_id,family,revision,due_at)
         SELECT account_id,repository_id,'references',?2,?3 FROM github_repository_access
         WHERE account_id=?1 AND selected=1 ON CONFLICT DO NOTHING",
        params![account, INDEX, now],
    )?;
    tx.execute(
        "INSERT INTO sync_subscriptions(read_id,subscriber,priority)
         SELECT j.id,?3,2 FROM scheduled_reads j
         JOIN github_repository_access r USING(account_id,repository_id)
         WHERE j.account_id=?1 AND r.selected=1 AND j.item_id IS NULL
           AND j.family='references' AND j.revision=?2 ON CONFLICT DO NOTHING",
        params![account, INDEX, SUBSCRIBER],
    )?;
    Ok(())
}

pub(super) fn enroll_item(
    tx: &Transaction<'_>,
    account: AccountId,
    repo: RepositoryId,
    item: ItemId,
    now: i64,
    closed: bool,
    updated: i64,
) -> Result<()> {
    let monitored_since: Option<i64> = tx.query_row(
        "SELECT observed_at_ms FROM activity_baselines WHERE repository_id=? AND stream_key='pr-reviews'",
        [repo], |row| row.get(0)
    ).optional()?;
    let Some(monitored_since) = monitored_since else {
        return Ok(());
    };
    let target = ReadTarget {
        repository: repo,
        item: Some(item),
        family: ResourceFamily::Reviews,
        revision: TIMELINE.into(),
    };
    if closed && let Some(id) = target_id(tx, account, &target)? {
        let completed: bool = tx.query_row(
            "SELECT COALESCE(watermark>=?,0) FROM scheduled_reads WHERE id=?",
            params![updated, id],
            |r| r.get(0),
        )?;
        if completed {
            return Ok(());
        }
    }
    let id = subscribe_in(
        tx,
        account,
        &target,
        SUBSCRIBER,
        ReadPriority::Activity,
        now,
    )?;
    tx.execute(
        "UPDATE scheduled_reads SET watermark=?2 WHERE id=?1 AND watermark IS NULL",
        params![id, monitored_since],
    )?;
    Ok(())
}

fn identify(
    tx: &Transaction<'_>,
    claim: &ReadClaim,
    identity: &OpenPull,
    fields: PullRequestUpdate,
    title: Change<String>,
    closed: Change<bool>,
) -> Result<ItemId> {
    ingest_in(
        tx,
        claim.account,
        ItemLocator {
            repository: claim.target.repository,
            number: identity.number.get(),
        },
        ItemUpdate {
            representation: ItemRepresentation::PullRequest(PullRequestRecord {
                id: identity.id.get().try_into()?,
                node_id: Some(NodeId::new(identity.node.clone())?),
                details: fields,
            }),
            remote_updated_at: UtcMillis(identity.updated_at),
            title,
            body: Change::Missing,
            closed,
        },
    )
}

enum Incomplete {
    MissingData,
    Permission,
}

fn checkpoint(
    tx: &Transaction<'_>,
    claim: &ReadClaim,
    next: Option<&str>,
    incomplete: Option<Incomplete>,
    validated: &Validators,
    now: i64,
    zone: &str,
) -> Result<()> {
    if let Some(reason) = incomplete {
        let permission_limited = matches!(reason, Incomplete::Permission);
        tx.execute(
            "UPDATE scheduled_reads SET lease_until=NULL,
             leased_generation=CASE WHEN ?3 THEN leased_generation END,
             failure=CASE WHEN ?3 THEN 'authorization' ELSE 'partial-pull-activity' END,
             failures=failures+1,suspended=(?3 OR failures>=3),due_at=?2 WHERE id=?1",
            params![claim.id, now.saturating_add(60_000), permission_limited],
        )?;
    } else if let Some(next) = next {
        if next.is_empty() || next.len() > 512 || Some(next) == claim.cursor.as_deref() {
            return Err(Error::Conflict("Invalid activity continuation"));
        }
        tx.execute(
            "UPDATE scheduled_reads SET cursor=?2,due_at=?3,lease_until=NULL,
             leased_generation=NULL,failure=NULL,failures=0 WHERE id=?1",
            params![claim.id, next, now],
        )?;
    } else {
        tx.execute("UPDATE scheduled_reads SET watermark=MAX(COALESCE(watermark,walk_started),walk_started) WHERE id=?",[claim.id])?;
        let initial = if claim.cursor.is_none() {
            validated.clone()
        } else {
            tx.query_row(
                "SELECT walk_etag,walk_modified,walk_revision FROM scheduled_reads WHERE id=?",
                [claim.id],
                |row| {
                    Ok(Validators {
                        etag: row.get(0)?,
                        last_modified: row.get(1)?,
                        revision: row.get(2)?,
                    })
                },
            )?
        };
        complete_in(tx, claim, now, zone, &initial)?;
    }
    Ok(())
}

impl Store {
    pub fn commit_open_pull_index(
        &self,
        claim: &ReadClaim,
        page: Page<OpenPull>,
        validators: &Validators,
        now: i64,
        zone: &str,
    ) -> Result<()> {
        self.write(|tx| {
            require_claim(tx, claim, now)?;
            if PrActivityRead::from_target(&claim.target) != Some(PrActivityRead::OpenIndex)
                || page.records.len() > 20
            {
                return Err(Error::Conflict("Page does not match the repository PR index"));
            }
            let index = claim.cursor.as_deref().unwrap_or("1")
                .parse::<u32>().map_err(|_| Error::Corrupt)?;
            if page.next_page.is_some_and(|next| Some(next) != index.checked_add(1)) {
                return Err(Error::Conflict("Invalid open PR index continuation"));
            }
            for row in page.records {
                let item = identify(
                    tx, claim, &row, PullRequestUpdate::default(),
                    Change::Missing, Change::Value(false),
                )?;
                enroll_item(
                    tx, claim.account, claim.target.repository, item, now, false, row.updated_at,
                )?;
            }
            if claim.cursor.is_none() {
                tx.execute(
                    "UPDATE scheduled_reads SET walk_etag=?2,walk_modified=?3,walk_revision=?4 WHERE id=?1",
                    params![claim.id, validators.etag, validators.last_modified, validators.revision],
                )?;
            }
            let next = page.next_page.map(|number| number.to_string());
            checkpoint(
                tx, claim, next.as_deref(), page.sso_limited.then_some(Incomplete::Permission),
                validators, now, zone,
            )
        })
    }

    pub fn commit_pull_activity(
        &self,
        claim: &ReadClaim,
        page: PullActivity,
        now: i64,
        zone: &str,
    ) -> Result<()> {
        self.write(|tx| {
            require_claim(tx, claim, now)?;
            if PrActivityRead::from_target(&claim.target) != Some(PrActivityRead::Timeline)
                || page.events.len() > 20
            {
                return Err(Error::Conflict("Page does not match the PR activity claim"));
            }
            let item = claim.target.item.ok_or(Error::Corrupt)?;
            let current: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM items WHERE id=? AND account_id=? AND repository_id=?
                AND number=? AND kind='pull_request')",
                params![
                    item,
                    claim.account,
                    claim.target.repository,
                    page.identity.number.get().to_string()
                ],
                |r| r.get(0),
            )?;
            if !current {
                return Err(Error::Conflict("PR activity target moved"));
            }
            let base = CommitOid::parse(&page.base)?;
            let head = CommitOid::parse(&page.head)?;
            let fields = PullRequestUpdate {
                merged: Change::Value(page.merged),
                base_oid: Change::Value(page.base),
                head_oid: Change::Value(page.head),
                merge_oid: Change::Missing,
            };
            if identify(
                tx,
                claim,
                &page.identity,
                fields,
                Change::Value(page.title),
                Change::Value(page.closed),
            )? != item
            {
                return Err(Error::Conflict("PR provider identity changed"));
            }
            let rev = crate::pr::record_revision_in(
                tx,
                claim.account,
                item,
                &base,
                &head,
                None,
                &PrHeader::default(),
                page.identity.updated_at,
            )?;
            for event in page.events {
                crate::pr::monitor_event(tx, &rev, event, now)?;
            }
            let current_complete = !page.partial
                && page.next.is_none()
                && crate::pr::current_in(tx, claim.account, item)?.is_some_and(
                    |(current, _, stamp)| current.id == rev.id && stamp == page.identity.updated_at,
                );
            if current_complete {
                reconcile_checks(tx, &rev, page.closed, now)?;
            }
            checkpoint(
                tx,
                claim,
                page.next.as_deref(),
                page.partial.then_some(if page.permission_limited {
                    Incomplete::Permission
                } else {
                    Incomplete::MissingData
                }),
                &Validators::default(),
                now,
                zone,
            )?;
            if page.closed && current_complete {
                tx.execute(
                    "DELETE FROM sync_subscriptions WHERE read_id=? AND subscriber=?",
                    params![claim.id, SUBSCRIBER],
                )?;
            }
            Ok(())
        })
    }
}

fn reconcile_checks(tx: &Transaction<'_>, rev: &PrRevision, closed: bool, now: i64) -> Result<()> {
    let targets = [
        rev.read_target(PrFamily::Checks),
        rev.read_target(PrFamily::Statuses),
    ];
    tx.execute(
        "DELETE FROM sync_subscriptions WHERE subscriber=?1 AND read_id IN (
        SELECT id FROM scheduled_reads WHERE account_id=?2 AND repository_id=?3 AND item_id=?4
          AND (?5 OR revision NOT IN (?6,?7)))",
        params![
            CHECKS,
            rev.account,
            rev.repository,
            rev.pr,
            closed,
            targets[0].revision,
            targets[1].revision
        ],
    )?;
    if !closed {
        for target in targets {
            subscribe_in(
                tx,
                rev.account,
                &target,
                CHECKS,
                ReadPriority::Activity,
                now,
            )?;
        }
    }
    Ok(())
}

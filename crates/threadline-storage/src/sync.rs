//! Durable read scheduling. Claims perform at most one provider request.
use crate::resources::{ResourceFamily, Validators};
use crate::{AccountId, Error, ItemId, RepositoryId, Result, Store};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::time::Duration;
use threadline_core::sync::policy::retry_delay;
pub use threadline_core::sync::policy::{ReadPriority, SyncControls, WorkSchedule};

mod observation;
pub use observation::*;

const LEASE_MS: i64 = 90_000;
const SPACING_MS: i64 = 1_000;
const OVERLAP_MS: i64 = 5 * 60_000;
const SLOTS: [ReadPriority; 12] = [
    ReadPriority::Explicit,
    ReadPriority::Visible,
    ReadPriority::Explicit,
    ReadPriority::Activity,
    ReadPriority::Visible,
    ReadPriority::Recent,
    ReadPriority::Explicit,
    ReadPriority::Visible,
    ReadPriority::Activity,
    ReadPriority::Reconcile,
    ReadPriority::Explicit,
    ReadPriority::Visible,
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncSettings {
    pub schedule: WorkSchedule,
    pub controls: SyncControls,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadTarget {
    pub repository: RepositoryId,
    pub item: Option<ItemId>,
    pub family: ResourceFamily,
    pub revision: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleRead {
    pub subscriber: String,
    pub target: ReadTarget,
    pub due_ms: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrphanedRead {
    pub target: ReadTarget,
    pub generation: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadClaim {
    id: i64,
    pub account: AccountId,
    pub credential_generation: i64,
    pub target: ReadTarget,
    pub generation: i64,
    pub cursor: Option<String>,
    pub since_ms: Option<i64>,
    pub validators: Validators,
    priority: ReadPriority,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadCompletion {
    Fetched(Validators),
    NotModified(Validators),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadFailure {
    Transient,
    Throttled,
    Authorization,
    ExpiredCursor,
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncStatus {
    pub pending: u64,
    pub last_success_ms: Option<i64>,
    pub budget_not_before_ms: Option<i64>,
    pub last_failure: Option<String>,
}

fn decode_family(name: &str) -> Result<ResourceFamily> {
    use ResourceFamily::*;
    [
        RepositoryItems,
        Timeline,
        Metadata,
        Body,
        Comments,
        Files,
        Reviews,
        ReviewThreads,
        Checks,
        History,
        References,
    ]
    .into_iter()
    .find(|f| f.name() == name)
    .ok_or(Error::Corrupt)
}
fn settings(db: &Connection, account: AccountId) -> Result<SyncSettings> {
    let row = db
        .query_row(
            "SELECT s.schedule, IFNULL(s.background_paused,0), IFNULL(s.notification_quiet,0),
                IFNULL(d.offline,0), IFNULL(d.outbox_paused,0)
         FROM accounts a LEFT JOIN sync_settings s ON s.account_id=a.id
         LEFT JOIN dispatch_profiles d ON d.account_id=a.id WHERE a.id=?",
            [account],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    SyncControls {
                        pause_background: r.get(1)?,
                        notification_quiet: r.get(2)?,
                        work_offline: r.get(3)?,
                        pause_queued_actions: r.get(4)?,
                    },
                ))
            },
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    let schedule = row
        .0
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|_| Error::Corrupt)?
        .unwrap_or_default();
    Ok(SyncSettings {
        schedule,
        controls: row.1,
    })
}
pub(super) fn require_claim(tx: &Transaction<'_>, claim: &ReadClaim, now: i64) -> Result<()> {
    let valid: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM scheduled_reads j JOIN eligible_sync_repositories r USING(account_id,repository_id)
         WHERE j.id=?1 AND j.account_id=?2 AND j.generation=?3 AND j.lease_until>?4
           AND j.repository_id=?5 AND j.item_id IS ?6 AND j.family=?7 AND j.revision=?8 AND j.cursor IS ?9
           AND r.credential_generation=j.leased_generation AND r.credential_generation=?10
           AND (j.item_id IS NULL OR EXISTS(SELECT 1 FROM items i WHERE i.id=j.item_id AND i.repository_id=j.repository_id))
           AND EXISTS(SELECT 1 FROM sync_subscriptions s WHERE s.read_id=j.id
             AND (s.priority=0 OR NOT EXISTS(SELECT 1 FROM sync_settings p WHERE p.account_id=j.account_id AND p.background_paused))))",
        params![claim.id,claim.account,claim.generation,now,claim.target.repository,claim.target.item,
            claim.target.family.name(),claim.target.revision,claim.cursor,claim.credential_generation], |r|r.get(0))?;
    if !valid {
        return Err(Error::Conflict(
            "Read claim expired or lost repository authorization",
        ));
    }
    Ok(())
}

impl Store {
    /// Coalesce startup, selection and wake signals into one issue and one comment walk per selected repository.
    pub fn schedule_selected_activity(&self, account: AccountId, now: i64) -> Result<()> {
        self.write(|tx| {
            for family in [ResourceFamily::RepositoryItems, ResourceFamily::Comments] {
                tx.execute(
                    "INSERT INTO scheduled_reads(account_id,repository_id,family,revision,due_at)
                     SELECT account_id,repository_id,?2,'',?3 FROM github_repository_access
                     WHERE account_id=?1 AND selected=1 ON CONFLICT DO NOTHING",
                    params![account, family.name(), now],
                )?;
            }
            tx.execute(
                "INSERT INTO sync_subscriptions(read_id,subscriber,priority)
                 SELECT j.id,'repository-monitor',2 FROM scheduled_reads j
                 JOIN github_repository_access ra USING(account_id,repository_id)
                 WHERE j.account_id=? AND ra.selected=1 AND j.item_id IS NULL AND j.revision=''
                 AND j.family IN ('repository-items','comments') ON CONFLICT DO NOTHING",
                [account],
            )?;
            Ok(())
        })
    }

    /// The dispatcher combines this durable signal with current reachability. This query never claims or sends work.
    pub fn eligible_queued_actions(&self, account: AccountId, now: i64) -> Result<u64> {
        self.read(|db| {
            let count:i64=db.query_row(
                "SELECT COUNT(*) FROM operations q JOIN operation_gates g USING(account_id,id)
                 WHERE q.account_id=? AND q.state='queued' AND q.cancel_requested=0 AND q.retry_at<=? AND g.hold IS NULL",
                params![account,now],|row|row.get(0),
            )?;
            count.try_into().map_err(|_|Error::Corrupt)
        })
    }
    pub fn sync_settings(&self, account: AccountId) -> Result<SyncSettings> {
        self.read(|db| settings(db, account))
    }
    pub fn save_sync_settings(&self, account: AccountId, value: &SyncSettings) -> Result<()> {
        value.schedule.validate().map_err(Error::InvalidInput)?;
        let json = serde_json::to_string(&value.schedule)
            .map_err(|_| Error::InvalidInput("Invalid schedule"))?;
        self.write(|tx| {
            let controls=value.controls;
            tx.execute("INSERT INTO sync_settings(account_id,schedule,background_paused,notification_quiet)
                VALUES(?1,?2,?3,?4) ON CONFLICT(account_id) DO UPDATE SET schedule=excluded.schedule,
                background_paused=excluded.background_paused,notification_quiet=excluded.notification_quiet",
                params![account,json,controls.pause_background,controls.notification_quiet])?;
            tx.execute("INSERT INTO dispatch_profiles(account_id,offline,outbox_paused) VALUES(?1,?2,?3)
                ON CONFLICT(account_id) DO UPDATE SET offline=excluded.offline,outbox_paused=excluded.outbox_paused",
                params![account,controls.work_offline,controls.pause_queued_actions])?;
            if controls.work_offline || controls.pause_background {
                tx.execute("UPDATE scheduled_reads SET lease_until=NULL, leased_generation=NULL WHERE account_id=?1
                    AND lease_until IS NOT NULL AND (?2 OR NOT EXISTS(SELECT 1 FROM sync_subscriptions s
                         WHERE s.read_id=scheduled_reads.id AND s.priority=0))",params![account,controls.work_offline])?;
            }
            Ok(())
        })
    }

    pub fn subscribe_read(
        &self,
        account: AccountId,
        target: &ReadTarget,
        subscriber: &str,
        priority: ReadPriority,
        due_ms: i64,
    ) -> Result<()> {
        if subscriber.starts_with("reader-pane:") {
            return Err(Error::InvalidInput(
                "Reader pane subscriptions have a dedicated owner",
            ));
        }
        self.write(|tx| subscribe_in(tx, account, target, subscriber, priority, due_ms).map(|_| ()))
    }

    /// Publish the entire visible-pane demand in one transaction after startup or a layout change.
    pub fn replace_visible_read_subscriptions(
        &self,
        account: AccountId,
        panes: &[VisibleRead],
    ) -> Result<Vec<OrphanedRead>> {
        self.write(|tx| replace_panes(tx, account, panes))
    }

    /// Returns true when the last subscription was removed. The caller may cancel its transport.
    pub fn unsubscribe_read(
        &self,
        account: AccountId,
        target: &ReadTarget,
        subscriber: &str,
    ) -> Result<bool> {
        self.write(|tx| {
            let Some(id)=target_id(tx,account,target)? else {return Ok(false)};
            let removed=tx.execute("DELETE FROM sync_subscriptions WHERE read_id=? AND subscriber=?",params![id,subscriber])?;
            if removed==0 { return Ok(false); }
            let last=tx.execute("UPDATE scheduled_reads SET lease_until=NULL,leased_generation=NULL
                WHERE id=? AND NOT EXISTS(SELECT 1 FROM sync_subscriptions WHERE read_id=scheduled_reads.id)",[id])?;
            Ok(last==1)
        })
    }

    pub fn hold_sync_budget(&self, account: AccountId, not_before_ms: i64) -> Result<()> {
        self.write(|tx| budget_in(tx, account, not_before_ms))
    }
    pub fn claim_sync_read(&self, account: AccountId, now_ms: i64) -> Result<Option<ReadClaim>> {
        self.write(|tx| claim_in(tx, account, now_ms))
    }

    pub fn read_sync_repository(
        &self,
        claim: &ReadClaim,
        now_ms: i64,
    ) -> Result<(
        threadline_core::domain::RepositoryName,
        threadline_core::domain::RepositoryApiId,
        crate::access::AccessProfile,
    )> {
        use threadline_core::domain::{RepositoryApiId, RepositoryName};
        self.read(|db| {
            let snapshot=db.unchecked_transaction()?;
            require_claim(&snapshot,claim,now_ms)?;
            let (owner,name,id,scope):(String,String,String,String)=snapshot.query_row(
                "SELECT r.owner,r.name,r.provider_repository_id,a.owner_scope FROM repositories r
                 JOIN github_repository_access a ON a.repository_id=r.id AND a.account_id=r.account_id
                 WHERE r.id=? AND r.account_id=?",params![claim.target.repository,claim.account],
                |row|Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?)),
            )?;
            if !owner.eq_ignore_ascii_case(&scope) {return Err(Error::Conflict("Repository left its verified owner scope"));}
            let route=RepositoryName::new(&owner,&name).map_err(|_|Error::Corrupt)?;
            let remote=RepositoryApiId::try_from(id.parse::<u64>().map_err(|_|Error::Corrupt)?).map_err(|_|Error::Corrupt)?;
            let profile=crate::access::profile(&snapshot,claim.account,&scope)?.ok_or(Error::NotFound)?;
            Ok((route,remote,profile))
        })
    }

    /// Earliest UTC deadline for the next claim. Settings, selection and transport completions also wake the worker.
    pub fn next_sync_due(&self, account: AccountId, now_ms: i64) -> Result<Option<i64>> {
        self.read(|db| {
            let due = db.query_row(
                "WITH permitted AS (
                    SELECT j.due_at,j.lease_until FROM scheduled_reads j
                    JOIN eligible_sync_repositories r USING(account_id,repository_id)
                    WHERE j.account_id=?1
                      AND (j.suspended=0 OR (j.failure='authorization' AND j.leased_generation IS NOT r.credential_generation))
                      AND (j.item_id IS NULL OR EXISTS(SELECT 1 FROM items i WHERE i.id=j.item_id AND i.repository_id=j.repository_id))
                      AND EXISTS(SELECT 1 FROM sync_subscriptions s WHERE s.read_id=j.id
                          AND (s.priority=0 OR NOT EXISTS(SELECT 1 FROM sync_settings p WHERE p.account_id=?1 AND p.background_paused)))
                ), leases AS (
                    SELECT COUNT(*) AS active,MIN(lease_until) AS expiry FROM scheduled_reads WHERE account_id=?1 AND lease_until>?2
                )
                SELECT MAX(?2,MIN(MAX(due_at,COALESCE(lease_until,?2))),
                           COALESCE((SELECT not_before FROM sync_budgets WHERE account_id=?1),?2),
                           COALESCE((SELECT expiry FROM leases WHERE active>=2),?2)) FROM permitted",
                params![account,now_ms], |row| row.get(0),
            )?;
            Ok(due)
        })
    }

    pub fn finish_sync_read(
        &self,
        claim: &ReadClaim,
        completion: ReadCompletion,
        now_ms: i64,
        system_zone: &str,
    ) -> Result<()> {
        self.write(|tx| {
            require_claim(tx, claim, now_ms)?;
            match completion {
                ReadCompletion::NotModified(ref v) => {
                    if claim.cursor.is_some()
                        || (claim.validators.etag.is_none()
                            && claim.validators.last_modified.is_none())
                    {
                        return Err(Error::Conflict(
                            "Unconditional or partial read cannot complete with 304",
                        ));
                    }
                    let retained = Validators {
                        etag: v.etag.clone().or_else(|| claim.validators.etag.clone()),
                        last_modified: v
                            .last_modified
                            .clone()
                            .or_else(|| claim.validators.last_modified.clone()),
                        revision: v
                            .revision
                            .clone()
                            .or_else(|| claim.validators.revision.clone()),
                    };
                    complete_in(tx, claim, now_ms, system_zone, &retained)
                }
                ReadCompletion::Fetched(ref v) => complete_in(tx, claim, now_ms, system_zone, v),
            }
        })
    }

    /// Publish the requested item's metadata, presentation and successful read deadline together.
    pub fn commit_sync_item_metadata(
        &self,
        claim: &ReadClaim,
        summary: crate::reader::ListSummary,
        validators: &Validators,
        now_ms: i64,
        system_zone: &str,
    ) -> Result<ItemId> {
        self.write(|tx| {
            require_claim(tx, claim, now_ms)?;
            let target = claim
                .target
                .item
                .ok_or(Error::InvalidInput("Item metadata requires an item claim"))?;
            if claim.target.family != ResourceFamily::Metadata
                || (!claim.target.revision.is_empty()
                    && validators.revision.as_deref() != Some(&claim.target.revision))
            {
                return Err(Error::Conflict(
                    "Metadata response does not match the requested revision",
                ));
            }
            let item = crate::reader::commit_item_metadata_in(
                tx,
                claim.account,
                claim.target.repository,
                summary,
                validators,
                now_ms,
            )?;
            if item != target {
                return Err(Error::Conflict("Metadata response belongs to another item"));
            }
            complete_in(tx, claim, now_ms, system_zone, validators)?;
            Ok(item)
        })
    }

    pub fn fail_sync_read(
        &self,
        claim: &ReadClaim,
        failure: ReadFailure,
        now_ms: i64,
        server_not_before_ms: Option<i64>,
    ) -> Result<()> {
        self.write(|tx|{
            require_claim(tx,claim,now_ms)?;
            if let Some(floor)=server_not_before_ms { budget_in(tx,claim.account,floor)?; }
            let prior:u32=tx.query_row("SELECT failures FROM scheduled_reads WHERE id=?",[claim.id],|r|r.get(0))?;
            let failures=prior.saturating_add(1);
            let floor=server_not_before_ms.unwrap_or(now_ms);
            let (message,suspended,reset_cursor,attempts,due)=match failure {
                ReadFailure::Cancelled=>("cancelled",false,false,prior,now_ms),
                ReadFailure::Authorization=>("authorization",true,false,failures,now_ms),
                ReadFailure::ExpiredCursor=>("cursor_expired",prior>=4,true,failures,now_ms),
                ReadFailure::Transient|ReadFailure::Throttled=>{
                    let retry = retry_delay(failures,Duration::ZERO);
                    let delay=retry.map(|d|i64::try_from(d.as_millis()).unwrap_or(i64::MAX)).unwrap_or_default();
                    let message=if failure==ReadFailure::Throttled {"throttled"} else {"transport"};
                    (message,retry.is_none(),false,failures,now_ms.saturating_add(delay).max(floor))
                }
            };
            tx.execute("UPDATE scheduled_reads SET lease_until=NULL,
                leased_generation=CASE WHEN ?2='authorization' THEN leased_generation END, failure=?2, suspended=?3,
                failures=?4,due_at=?5,cursor=CASE WHEN ?6 THEN NULL ELSE cursor END,
                page_watermark=CASE WHEN ?6 THEN NULL ELSE page_watermark END,
                etag=CASE WHEN ?6 THEN NULL ELSE etag END,last_modified=CASE WHEN ?6 THEN NULL ELSE last_modified END WHERE id=?1",
                params![claim.id,message,suspended,attempts,due,reset_cursor])?;
            Ok(())
        })
    }

    pub fn sync_status(&self, account: AccountId) -> Result<SyncStatus> {
        self.read(|db|{
            let (count,last,reason):(i64,Option<i64>,Option<String>)=db.query_row(
                "SELECT COUNT(*) FILTER(WHERE EXISTS(SELECT 1 FROM sync_subscriptions s WHERE s.read_id=j.id)),
                    MAX(last_success), (SELECT failure FROM scheduled_reads f WHERE f.account_id=?1
                    AND f.failure IS NOT NULL ORDER BY f.last_started DESC LIMIT 1)
                 FROM scheduled_reads j WHERE j.account_id=?1",[account],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            let budget=db.query_row("SELECT not_before FROM sync_budgets WHERE account_id=?",[account],|r|r.get(0)).optional()?;
            Ok(SyncStatus{pending:count.try_into().map_err(|_|Error::Corrupt)?,last_success_ms:last,
                budget_not_before_ms:budget,last_failure:reason})
        })
    }
}

fn subscribe_in(
    tx: &Transaction<'_>,
    account: AccountId,
    target: &ReadTarget,
    subscriber: &str,
    priority: ReadPriority,
    due_ms: i64,
) -> Result<i64> {
    if subscriber.is_empty() || subscriber.len() > 200 || target.revision.len() > 512 {
        return Err(Error::InvalidInput("Invalid read subscriber or revision"));
    }
    if let Some(item) = target.item {
        let matches: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM items WHERE id=? AND repository_id=? AND account_id=?)",
            params![item, target.repository, account],
            |row| row.get(0),
        )?;
        if !matches {
            return Err(Error::Conflict(
                "Item does not belong to the requested repository",
            ));
        }
    }
    tx.execute(
        "INSERT INTO scheduled_reads(account_id,repository_id,item_id,family,revision,due_at)
         VALUES(?,?,?,?,?,?) ON CONFLICT DO NOTHING",
        params![
            account,
            target.repository,
            target.item,
            target.family.name(),
            target.revision,
            due_ms
        ],
    )?;
    let id = target_id(tx, account, target)?.ok_or(Error::Corrupt)?;
    let changed = tx.execute(
        "INSERT INTO sync_subscriptions(read_id,subscriber,priority) VALUES(?1,?2,?3)
         ON CONFLICT(read_id,subscriber) DO UPDATE SET priority=excluded.priority
         WHERE excluded.priority<>sync_subscriptions.priority",
        params![id, subscriber, priority as u8],
    )?;
    if changed > 0 {
        tx.execute(
            "UPDATE scheduled_reads SET due_at=MIN(due_at,?2) WHERE id=?1 AND failures=0",
            params![id, due_ms],
        )?;
    }
    if priority == ReadPriority::Explicit {
        tx.execute(
            "UPDATE scheduled_reads SET suspended=0,failures=0,due_at=MIN(due_at,?2) WHERE id=?1 AND suspended",
            params![id,due_ms],
        )?;
    }
    Ok(id)
}

fn replace_panes(
    tx: &Transaction<'_>,
    account: AccountId,
    panes: &[VisibleRead],
) -> Result<Vec<OrphanedRead>> {
    let mut desired = std::collections::BTreeSet::new();
    let mut cancelled = Vec::new();
    for pane in panes {
        if pane.subscriber.is_empty() {
            return Err(Error::InvalidInput(
                "Pane subscriber requires a stable identity",
            ));
        }
        let name = format!("reader-pane:{}", pane.subscriber);
        let id = subscribe_in(
            tx,
            account,
            &pane.target,
            &name,
            ReadPriority::Visible,
            pane.due_ms,
        )?;
        desired.insert((id, name));
    }
    let mut old=tx.prepare_cached(
        "SELECT s.read_id,s.subscriber FROM sync_subscriptions s JOIN scheduled_reads j ON j.id=s.read_id
         WHERE j.account_id=? AND s.subscriber GLOB 'reader-pane:*'"
    )?;
    let rows = old
        .query_map([account], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, subscriber) in rows {
        if desired.contains(&(id, subscriber.clone())) {
            continue;
        }
        tx.execute(
            "DELETE FROM sync_subscriptions WHERE read_id=? AND subscriber=?",
            params![id, subscriber],
        )?;
        let invalidated=tx.query_row(
            "UPDATE scheduled_reads SET lease_until=NULL,leased_generation=NULL WHERE id=? AND lease_until IS NOT NULL
             AND NOT EXISTS(SELECT 1 FROM sync_subscriptions s WHERE s.read_id=scheduled_reads.id)
             RETURNING repository_id,item_id,family,revision,generation",[id],
            |row|Ok((row.get(0)?,row.get(1)?,row.get::<_,String>(2)?,row.get(3)?,row.get(4)?)),
        ).optional()?;
        if let Some((repository, item, family, revision, generation)) = invalidated {
            cancelled.push(OrphanedRead {
                target: ReadTarget {
                    repository,
                    item,
                    family: decode_family(&family)?,
                    revision,
                },
                generation,
            });
        }
    }
    Ok(cancelled)
}

fn target_id(db: &Connection, account: AccountId, t: &ReadTarget) -> Result<Option<i64>> {
    Ok(db
        .query_row(
            "SELECT id FROM scheduled_reads WHERE account_id=? AND repository_id=? AND item_id IS ?
        AND family=? AND revision=?",
            params![account, t.repository, t.item, t.family.name(), t.revision],
            |r| r.get(0),
        )
        .optional()?)
}
fn budget_in(tx: &Transaction<'_>, account: AccountId, floor: i64) -> Result<()> {
    tx.execute(
        "INSERT INTO sync_budgets(account_id,not_before) VALUES(?1,?2) ON CONFLICT(account_id)
        DO UPDATE SET not_before=MAX(sync_budgets.not_before,excluded.not_before)",
        params![account, floor],
    )?;
    Ok(())
}

fn claim_in(tx: &Transaction<'_>, account: AccountId, now: i64) -> Result<Option<ReadClaim>> {
    let (floor, first_slot) = tx
        .query_row(
            "SELECT not_before,next_slot FROM sync_budgets WHERE account_id=?",
            [account],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, u8>(1)?)),
        )
        .optional()?
        .unwrap_or((i64::MIN, 0));
    if now < floor {
        return Ok(None);
    }
    let inflight: i64 = tx.query_row(
        "SELECT COUNT(*) FROM scheduled_reads WHERE account_id=? AND lease_until>?",
        params![account, now],
        |r| r.get(0),
    )?;
    if inflight >= 2 {
        return Ok(None);
    }
    let controls = settings(tx, account)?.controls;
    let mut by_priority=tx.prepare_cached(
        "SELECT j.id,r.credential_generation FROM scheduled_reads j
         JOIN eligible_sync_repositories r USING(account_id,repository_id)
         WHERE j.account_id=?1 AND j.due_at<=?2 AND (j.lease_until IS NULL OR j.lease_until<=?2)
           AND (j.suspended=0 OR (j.failure='authorization' AND j.leased_generation IS NOT r.credential_generation))
           AND (j.item_id IS NULL OR EXISTS(SELECT 1 FROM items i WHERE i.id=j.item_id AND i.repository_id=j.repository_id))
           AND (SELECT MIN(s.priority) FROM sync_subscriptions s WHERE s.read_id=j.id)=?3
         ORDER BY j.last_started IS NOT NULL,j.last_started,j.due_at,j.id LIMIT 1"
    )?;
    let mut seen = [false; 5];
    for offset in 0..SLOTS.len() {
        let slot = (usize::from(first_slot) + offset) % SLOTS.len();
        let priority = SLOTS[slot];
        if seen[priority as usize] || !controls.can_read(priority) {
            continue;
        }
        seen[priority as usize] = true;
        let Some((id, credentials)) = by_priority
            .query_row(params![account, now, priority as u8], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .optional()?
        else {
            continue;
        };
        tx.execute(
            "UPDATE scheduled_reads SET last_started=?2, generation=generation+1, lease_until=?3,
            leased_generation=?4,walk_started=COALESCE(walk_started,?2) WHERE id=?1",
            params![id, now, now.saturating_add(LEASE_MS), credentials],
        )?;
        budget_in(tx, account, now.saturating_add(SPACING_MS))?;
        tx.execute(
            "UPDATE sync_budgets SET next_slot=? WHERE account_id=?",
            params![((slot + 1) % SLOTS.len()) as u8, account],
        )?;
        let (repo,item,family,revision,generation,cursor,watermark,start,validator_since,mut validators)=tx.query_row(
            "SELECT repository_id,item_id,family,revision,generation,cursor,watermark,walk_started,validator_since,
                    etag,last_modified,validator_revision FROM scheduled_reads WHERE id=?",[id],|r|
                Ok((r.get(0)?,r.get::<_,Option<ItemId>>(1)?,r.get::<_,String>(2)?,r.get(3)?,r.get(4)?,
                    r.get::<_,Option<String>>(5)?,r.get::<_,Option<i64>>(6)?,r.get::<_,i64>(7)?,r.get::<_,Option<i64>>(8)?,
                    Validators{etag:r.get(9)?,last_modified:r.get(10)?,revision:r.get(11)?})))?;
        let family = decode_family(&family)?;
        let feed = item.is_none()
            && matches!(
                family,
                ResourceFamily::RepositoryItems | ResourceFamily::Comments
            );
        let since_ms = feed.then(|| watermark.unwrap_or(start).saturating_sub(OVERLAP_MS));
        if cursor.is_some() || validator_since != since_ms {
            validators = Validators::default();
        }
        return Ok(Some(ReadClaim {
            id,
            account,
            credential_generation: credentials,
            target: ReadTarget {
                repository: repo,
                item,
                family,
                revision,
            },
            generation,
            cursor,
            since_ms,
            validators,
            priority,
        }));
    }
    Ok(None)
}

fn complete_in(
    tx: &Transaction<'_>,
    claim: &ReadClaim,
    now: i64,
    system_zone: &str,
    validators: &Validators,
) -> Result<()> {
    tx.execute(
        "DELETE FROM sync_subscriptions WHERE read_id=? AND priority=0",
        [claim.id],
    )?;
    let recurring: Option<u8> = tx.query_row(
        "SELECT MIN(priority) FROM sync_subscriptions WHERE read_id=?",
        [claim.id],
        |r| r.get(0),
    )?;
    let priority = recurring
        .and_then(|p| SLOTS.iter().copied().find(|v| *v as u8 == p))
        .unwrap_or(claim.priority);
    let interval = settings(tx, claim.account)?
        .schedule
        .interval(now, system_zone, priority)
        .map_err(Error::InvalidInput)?;
    let delay = i64::try_from(interval.as_millis()).map_err(|_| Error::Corrupt)?;
    tx.execute("UPDATE scheduled_reads SET lease_until=NULL,leased_generation=NULL,due_at=?2,last_success=?3,
        failures=0,failure=NULL,suspended=0,cursor=NULL,walk_started=NULL,etag=?4,last_modified=?5,validator_revision=?6,
        validator_since=?7,walk_etag=NULL,walk_modified=NULL,walk_revision=NULL,page_watermark=NULL WHERE id=?1",
        params![claim.id,now.saturating_add(delay),now,validators.etag,validators.last_modified,validators.revision,claim.since_ms])?;
    Ok(())
}

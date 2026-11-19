use super::*;
use crate::{
    ItemLocator,
    access::AccessProfile,
    resources::{
        ResourceError, ResourceProgress, ResourceState, Validators, WalkMode, ensure_resource,
        require_walk_id, resume_walk_in, set_fetch_error, start_walk_in, state_in,
    },
    writes::require_observation_profile,
};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DownloadState {
    Pending,
    Active,
    Complete,
    Cancelled,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssueDownload {
    pub item: ItemId,
    pub generation: i64,
    pub state: DownloadState,
    pub requested_at_ms: i64,
    pub metadata: ResourceState,
    pub body: ResourceState,
    pub comments: ResourceState,
    pub last_error: Option<ResourceError>,
}
impl IssueDownload {
    pub fn completed_families(&self) -> u8 {
        [&self.metadata, &self.body, &self.comments]
            .into_iter()
            .filter(|s| s.coverage == Coverage::Complete)
            .count() as u8
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IssueDownloadClaim {
    pub account: AccountId,
    pub item: ItemId,
    pub locator: ItemLocator,
    pub generation: i64,
    credential_generation: i64,
    lease: i64,
}

impl IssueDownloadClaim {
    pub(crate) fn require(&self, db: &Connection, profile: &AccessProfile, now: i64) -> Result<()> {
        if profile.account != self.account {
            return Err(Error::Conflict("The issue download credential changed"));
        }
        let number = i64::try_from(self.locator.number).map_err(|_| Error::Corrupt)?;
        require_observation_profile(db, profile, self.locator.repository)?;
        let valid: bool = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM issue_downloads d JOIN items i
                ON i.id=d.item_id AND i.account_id=d.account_id
             JOIN eligible_sync_repositories e ON e.account_id=i.account_id AND e.repository_id=i.repository_id
             WHERE d.account_id=?1 AND d.item_id=?2 AND d.generation=?3 AND d.lease_token=?4
               AND d.state='active' AND d.lease_until>?5 AND i.repository_id=?6 AND i.number=?7
               AND e.credential_generation=?8)",
            params![
                self.account,
                self.item,
                self.generation,
                self.lease,
                now,
                self.locator.repository,
                number,
                self.credential_generation
            ],
            |row| row.get(0),
        )?;
        if !valid {
            return Err(Error::Conflict(
                "The issue download was cancelled, moved or reclaimed",
            ));
        }
        Ok(())
    }

    fn comments_key(&self) -> ResourceKey {
        ResourceKey {
            account: self.account,
            target: ResourceTarget::Item(self.item),
            family: ResourceFamily::Comments,
        }
    }
}

fn next_download(db: &Connection, account: AccountId) -> Result<Option<(IssueDownloadClaim, i64)>> {
    db.query_row(
        "SELECT i.id,i.repository_id,i.number,d.generation,e.credential_generation,d.lease_token,
                MAX(d.retry_at,COALESCE(d.lease_until,0)) AS due
         FROM issue_downloads d JOIN items i ON i.id=d.item_id AND i.account_id=d.account_id
         JOIN eligible_sync_repositories e ON e.account_id=i.account_id AND e.repository_id=i.repository_id
         WHERE d.account_id=?1 AND d.state IN ('pending','active') AND d.retry_at IS NOT NULL
         ORDER BY due,d.requested_at,d.item_id LIMIT 1",
        [account],
        |row| Ok((IssueDownloadClaim {
            account,
            item: row.get(0)?,
            locator: ItemLocator {
                repository: row.get(1)?,
                number: row.get::<_, i64>(2)?.try_into().map_err(|_| rusqlite::Error::InvalidQuery)?,
            },
            generation: row.get(3)?,
            credential_generation: row.get(4)?,
            lease: row.get(5)?,
        }, row.get(6)?)),
    ).optional().map_err(Into::into)
}

fn download(db: &Connection, account: AccountId, item: ItemId) -> Result<Option<IssueDownload>> {
    let record: Option<(i64, String, i64, Option<ResourceError>)> = db
        .query_row("SELECT generation,state,requested_at,last_error FROM issue_downloads WHERE account_id=? AND item_id=?", params![account, item], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .optional()?;
    let Some((generation, name, requested_at_ms, last_error)) = record else {
        return Ok(None);
    };
    let state = match name.as_str() {
        "pending" => DownloadState::Pending,
        "active" => DownloadState::Active,
        "complete" => DownloadState::Complete,
        "cancelled" => DownloadState::Cancelled,
        _ => return Err(Error::Corrupt),
    };
    let family = |family| {
        state_in(
            db,
            ResourceKey {
                account,
                target: ResourceTarget::Item(item),
                family,
            },
        )
    };
    Ok(Some(IssueDownload {
        item,
        generation,
        state,
        requested_at_ms,
        metadata: family(ResourceFamily::Metadata)?,
        body: family(ResourceFamily::Body)?,
        comments: family(ResourceFamily::Comments)?,
        last_error,
    }))
}
impl Store {
    pub fn request_issue_download(
        &self,
        account: AccountId,
        item: ItemId,
        at_ms: i64,
    ) -> Result<IssueDownload> {
        self.write(|db| {
            let issue: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM items WHERE id=? AND account_id=? AND kind='issue')", params![item, account], |r| r.get(0))?;
            if !issue {
                return Err(Error::InvalidInput("Issue download requires a verified issue in this account"));
            }
            db.execute("INSERT INTO issue_downloads(account_id,item_id,generation,state,requested_at) VALUES(?,?,1,'pending',?)
                ON CONFLICT(item_id) DO UPDATE SET generation=generation+1,state='pending',requested_at=excluded.requested_at,
                    retry_at=0,lease_until=NULL,last_error=NULL
                WHERE state='cancelled'", params![account, item, at_ms])?;
            let mut job = download(db, account, item)?.ok_or(Error::Corrupt)?;
            if job.state == DownloadState::Pending && job.completed_families() == 3 {
                db.execute("UPDATE issue_downloads SET state='complete' WHERE account_id=? AND item_id=?", params![account, item])?;
                job.state = DownloadState::Complete;
            }
            Ok(job)
        })
    }
    pub fn issue_download(
        &self,
        account: AccountId,
        item: ItemId,
    ) -> Result<Option<IssueDownload>> {
        self.read(|conn| {
            let db = conn.unchecked_transaction()?;
            download(&db, account, item)
        })
    }
    pub fn issue_downloads(
        &self,
        account: AccountId,
        after: Option<ItemId>,
        limit: u16,
    ) -> Result<Vec<IssueDownload>> {
        limit_ok(limit)?;
        self.read(|conn| {
            let db = conn.unchecked_transaction()?;
            let mut stmt = db.prepare_cached("SELECT item_id FROM issue_downloads WHERE account_id=?1 AND (?2 IS NULL OR item_id>?2) ORDER BY item_id LIMIT ?3")?;
            let ids = stmt.query_map(params![account, after, limit], |r| r.get(0))?;
            ids.map(|id| download(&db, account, id?)?.ok_or(Error::Corrupt))
                .collect()
        })
    }
    pub fn next_issue_download_due(&self, account: AccountId) -> Result<Option<i64>> {
        self.read(|db| Ok(next_download(db, account)?.map(|(_, due)| due)))
    }

    /// Only one worker can own the next page while this claim remains valid.
    pub fn claim_issue_download(
        &self,
        account: AccountId,
        now: i64,
    ) -> Result<Option<IssueDownloadClaim>> {
        self.write(|tx| {
            let Some((mut claim, _)) = next_download(tx, account)?.filter(|(_, due)| *due <= now)
            else {
                return Ok(None);
            };
            claim.lease = tx.query_row(
                "UPDATE issue_downloads SET state='active',lease_until=?3,lease_token=lease_token+1
                 WHERE account_id=?1 AND item_id=?2 RETURNING lease_token",
                params![account, claim.item, now.saturating_add(45_000)],
                |row| row.get(0),
            )?;
            Ok(Some(claim))
        })
    }

    pub fn issue_download_step(
        &self,
        claim: &IssueDownloadClaim,
        profile: &AccessProfile,
        now: i64,
    ) -> Result<IssueDownload> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            claim.require(&tx, profile, now)?;
            download(&tx, claim.account, claim.item)?.ok_or(Error::Corrupt)
        })
    }

    pub fn issue_download_discussion_progress(
        &self,
        claim: &IssueDownloadClaim,
        profile: &AccessProfile,
        now: i64,
    ) -> Result<Option<ResourceProgress>> {
        self.write(|tx| {
            claim.require(tx, profile, now)?;
            let key = claim.comments_key();
            if state_in(tx, key)?.coverage == Coverage::Complete {
                return Ok(None);
            }
            let id = ensure_resource(tx, key)?;
            if let Some(progress) = resume_walk_in(tx, id)? {
                return Ok(Some(progress));
            }
            Ok(Some(start_walk_in(tx, id, WalkMode::Full)?))
        })
    }

    pub fn commit_issue_download_metadata(
        &self,
        claim: &IssueDownloadClaim,
        profile: &AccessProfile,
        summary: ListSummary,
        now: i64,
    ) -> Result<()> {
        self.write(|tx| {
            claim.require(tx, profile, now)?;
            if summary.number != claim.locator.number
                || !matches!(&summary.update.representation, crate::ItemRepresentation::Issue(issue)
                    if issue.classification == crate::IssueClassification::Issue)
            {
                return Err(Error::Conflict(
                    "Issue reply does not match the original download target",
                ));
            }
            let item = commit_item_metadata_in(
                tx,
                claim.account,
                claim.locator.repository,
                summary,
                &Validators::default(),
                now,
            )?;
            if item != claim.item {
                return Err(Error::Conflict(
                    "Issue reply does not match the anchored issue",
                ));
            }
            Ok(())
        })
    }

    /// Keep the preceding page intact if the request fails, including when the job is cancelled.
    pub fn record_issue_download_discussion_error(
        &self,
        claim: &IssueDownloadClaim,
        profile: &AccessProfile,
        progress: &ResourceProgress,
        error: ResourceError,
        now: i64,
    ) -> Result<()> {
        self.write(|tx| {
            claim.require(tx, profile, now)?;
            let id = ensure_resource(tx, claim.comments_key())?;
            require_walk_id(tx, id, progress)?;
            set_fetch_error(tx, id, error)
        })
    }

    pub fn complete_issue_download_step(
        &self,
        claim: &IssueDownloadClaim,
        profile: &AccessProfile,
        now: i64,
    ) -> Result<()> {
        self.write(|tx| {
            claim.require(tx, profile, now)?;
            let job = download(tx, claim.account, claim.item)?.ok_or(Error::Corrupt)?;
            let state = if job.completed_families() == 3 {
                "complete"
            } else {
                "pending"
            };
            tx.execute(
                "UPDATE issue_downloads SET state=?3,retry_at=?4,lease_until=NULL,last_error=NULL
                 WHERE account_id=?1 AND item_id=?2",
                params![claim.account, claim.item, state, now],
            )?;
            Ok(())
        })
    }

    /// Deferral keeps the original generation. A missing retry time requires explicit retry.
    pub fn defer_issue_download(
        &self,
        claim: &IssueDownloadClaim,
        due: Option<i64>,
        reason: Option<ResourceError>,
    ) -> Result<()> {
        self.write(|tx| {
            tx.execute(
                "UPDATE issue_downloads SET state='pending',retry_at=?5,lease_until=NULL,last_error=coalesce(?6,last_error)
                 WHERE account_id=?1 AND item_id=?2 AND generation=?3 AND lease_token=?4
                   AND state='active' AND lease_until IS NOT NULL",
                params![claim.account, claim.item, claim.generation, claim.lease, due, reason.map(|why| why as i64)],
            )?;
            Ok(())
        })
    }

    pub fn retry_issue_download(
        &self,
        account: AccountId,
        item: ItemId,
        generation: i64,
        now: i64,
    ) -> Result<bool> {
        self.write(|tx| {
            Ok(tx.execute(
                "UPDATE issue_downloads SET retry_at=?4 WHERE account_id=?1 AND item_id=?2
                 AND generation=?3 AND state='pending'",
                params![account, item, generation, now],
            )? == 1)
        })
    }
    /// Cancel a pending download or release a completed offline pin. Cached content stays readable.
    pub fn cancel_issue_download(
        &self,
        account: AccountId,
        item: ItemId,
        generation: i64,
    ) -> Result<bool> {
        self.write(|db| {
            Ok(db.execute(
                "UPDATE issue_downloads SET state='cancelled',generation=generation+1,lease_until=NULL,last_error=NULL
            WHERE account_id=? AND item_id=? AND generation=? AND state!='cancelled'",
                params![account, item, generation],
            )? == 1)
        })
    }
}

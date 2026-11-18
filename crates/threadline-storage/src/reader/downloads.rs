use super::*;
use crate::resources::{ResourceState, state_in};
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
}
impl IssueDownload {
    pub fn completed_families(&self) -> u8 {
        [&self.metadata, &self.body, &self.comments]
            .into_iter()
            .filter(|s| s.coverage == Coverage::Complete)
            .count() as u8
    }
}
fn download(db: &Connection, account: AccountId, item: ItemId) -> Result<Option<IssueDownload>> {
    let record: Option<(i64, String, i64)> = db
        .query_row("SELECT generation,state,requested_at FROM issue_downloads WHERE account_id=? AND item_id=?", params![account, item], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?;
    let Some((generation, name, requested_at_ms)) = record else {
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
                ON CONFLICT(item_id) DO UPDATE SET generation=generation+1,state='pending',requested_at=excluded.requested_at
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
    /// A worker resumes an active generation after restart, using each resource's persisted cursor.
    pub fn start_issue_download(
        &self,
        account: AccountId,
        item: ItemId,
        generation: i64,
    ) -> Result<()> {
        self.write(|db| {
            let current = download(db, account, item)?.ok_or(Error::NotFound)?;
            let local_complete = current.completed_families() == 3;
            let changed = db.execute("UPDATE issue_downloads SET state='active' WHERE account_id=?1 AND item_id=?2
                AND generation=?3 AND state IN ('pending','active') AND (?4 OR EXISTS (
                    SELECT 1 FROM items i JOIN github_repository_access a ON a.repository_id=i.repository_id AND a.account_id=i.account_id
                    JOIN github_access_profiles p ON p.account_id=a.account_id AND p.owner_scope=a.owner_scope
                    JOIN dispatch_profiles d ON d.account_id=i.account_id JOIN active_profile active ON active.account_id=i.account_id
                    WHERE i.id=?2 AND i.account_id=?1 AND a.selected=1 AND a.access=1 AND p.access=1 AND d.authorized=1 AND d.offline=0
                    AND a.verified_generation=p.credential_generation AND p.installed_generation=p.credential_generation))", params![account, item, generation, local_complete])?;
            if changed == 0 {
                return Err(Error::Conflict("Download is no longer eligible"));
            }
            Ok(())
        })
    }
    pub fn finish_issue_download(
        &self,
        account: AccountId,
        item: ItemId,
        generation: i64,
    ) -> Result<()> {
        self.write(|db| {
            let current = download(db, account, item)?.ok_or(Error::NotFound)?;
            if current.generation != generation
                || current.state != DownloadState::Active
                || current.completed_families() != 3
            {
                return Err(Error::Conflict(
                    "Download no longer active or resources incomplete",
                ));
            }
            db.execute(
                "UPDATE issue_downloads SET state='complete' WHERE account_id=? AND item_id=?",
                params![account, item],
            )?;
            Ok(())
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
                "UPDATE issue_downloads SET state='cancelled',generation=generation+1
            WHERE account_id=? AND item_id=? AND generation=? AND state!='cancelled'",
                params![account, item, generation],
            )? == 1)
        })
    }
}

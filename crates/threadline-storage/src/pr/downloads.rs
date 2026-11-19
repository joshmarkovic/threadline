use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrDownloadState {
    Pending,
    Active,
    Complete,
    Partial,
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrDownloadAttempt {
    pub generation: i64,
    lease: Option<i64>,
}

impl PrDownloadAttempt {
    pub fn unleased(generation: i64) -> Self {
        Self {
            generation,
            lease: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PrDownloadClaim {
    pub revision: PrRevision,
    pub attempt: PrDownloadAttempt,
}

struct PendingDownload {
    revision: i64,
    item: ItemId,
    generation: i64,
    due: i64,
}

fn next_download(db: &Connection, account: AccountId) -> Result<Option<PendingDownload>> {
    db.query_row(
        "SELECT d.revision_id,r.item_id,d.generation,MAX(d.retry_at,COALESCE(d.lease_until,0)) AS due
         FROM pr_downloads d JOIN pr_revisions r ON r.id=d.revision_id
         JOIN eligible_sync_repositories e ON e.account_id=r.account_id AND e.repository_id=r.repository_id
         JOIN items i ON i.id=r.item_id AND i.repository_id=r.repository_id
         WHERE r.account_id=?1 AND d.state IN ('pending','active') AND r.id=(
             SELECT p.id FROM pr_revisions p
             WHERE p.account_id=r.account_id AND p.item_id=r.item_id AND p.repository_id=r.repository_id
             ORDER BY p.updated_at DESC,p.id ASC LIMIT 1)
         ORDER BY due,d.requested_at,d.revision_id LIMIT 1",
        [account],
        |row| Ok(PendingDownload {
            revision: row.get(0)?,
            item: row.get(1)?,
            generation: row.get(2)?,
            due: row.get(3)?,
        }),
    )
    .optional()
    .map_err(Into::into)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrDownload {
    pub generation: i64,
    pub state: PrDownloadState,
    pub requested_at_ms: i64,
    pub include_context: bool,
    pub include_media: bool,
    pub families: Vec<(PrFamily, ResourceState)>,
    pub body_downloaded: bool,
    pub conversation: ResourceState,
    pub incomplete_patches_or_threads: bool,
    pub missing_media: bool,
}

fn status(db: &Connection, rev: &PrRevision) -> Result<Option<PrDownload>> {
    validate_revision(db, rev)?;
    let row=db.query_row("SELECT generation,state,requested_at,include_context,include_media FROM pr_downloads WHERE revision_id=?",
        [rev.id],|row|Ok((row.get(0)?,row.get::<_,String>(1)?,row.get(2)?,row.get::<_,bool>(3)?,row.get::<_,bool>(4)?))).optional()?;
    let Some((generation, state, requested, include_context, include_media)) = row else {
        return Ok(None);
    };
    let names = PrFamily::REQUIRED
        .into_iter()
        .chain(include_context.then_some(PrFamily::Context))
        .chain(include_media.then_some(PrFamily::Media));
    let mut families = Vec::new();
    for family in names {
        let cached = match family_id(db, rev, family)? {
            Some(id) => state_by_id(db, id)?,
            None => missing_state(),
        };
        families.push((family, cached));
    }
    let body_downloaded = db.query_row(
        "SELECT body_at IS NOT NULL FROM pr_revisions WHERE id=?",
        [rev.id],
        |r| r.get(0),
    )?;
    let conversation = state_in(
        db,
        ResourceKey {
            account: rev.account,
            target: ResourceTarget::Item(rev.pr),
            family: ResourceFamily::Comments,
        },
    )?;
    let incomplete=db.query_row(
        "SELECT EXISTS(SELECT 1 FROM pr_family_resources f JOIN cached_resource_records r USING(resource_id)
          WHERE f.revision_id=? AND ((f.family='files' AND json_type(r.body,'$.record.File.patch.Text') IS NULL)
           OR (f.family='threads' AND json_extract(r.body,'$.record.Thread.replies_complete')=0)))",
        [rev.id],|row|row.get(0))?;
    let missing_media = include_media
        && db.query_row(
            "SELECT EXISTS(SELECT 1 FROM pr_family_resources f
         JOIN cached_resource_records row USING(resource_id)
         WHERE f.revision_id=?1 AND f.family='media' AND NOT EXISTS(
           SELECT 1 FROM cached_images c WHERE c.account_id=?2 AND c.repository_id=?3
           AND c.stable_url=json_extract(row.body,'$.record.Media.cache_key')))",
            params![rev.id, rev.account, rev.repository],
            |r| r.get::<_, bool>(0),
        )?;
    let state = match state.as_str() {
        "pending" => PrDownloadState::Pending,
        "active" => PrDownloadState::Active,
        "complete" => PrDownloadState::Complete,
        "partial" => PrDownloadState::Partial,
        "cancelled" => PrDownloadState::Cancelled,
        _ => return Err(Error::Corrupt),
    };
    let mut job = PrDownload {
        generation,
        state,
        requested_at_ms: requested,
        include_context,
        include_media,
        families,
        body_downloaded,
        conversation,
        incomplete_patches_or_threads: incomplete,
        missing_media,
    };
    if job.state == PrDownloadState::Complete && !job.fully_cached() {
        job.state = PrDownloadState::Partial;
    }
    Ok(Some(job))
}

impl PrDownload {
    pub fn unselected_optional_resources(&self) -> impl Iterator<Item = PrFamily> + '_ {
        self.families.iter().filter_map(|(family, state)| {
            (matches!(family, PrFamily::Context | PrFamily::Media)
                && state.coverage != Coverage::Complete)
                .then_some(*family)
        })
    }

    pub fn fully_cached(&self) -> bool {
        self.body_downloaded
            && self.conversation.coverage == Coverage::Complete
            && !self.incomplete_patches_or_threads
            && !self.missing_media
            && self
                .families
                .iter()
                .all(|(_, state)| state.coverage == Coverage::Complete)
    }
}

pub(crate) fn require_pr_download_attempt(
    tx: &Transaction<'_>,
    rev: &PrRevision,
    attempt: PrDownloadAttempt,
    family: Option<PrFamily>,
    now: i64,
) -> Result<()> {
    require_current_revision(tx, rev)?;
    let active: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM pr_downloads d JOIN eligible_sync_repositories e
         ON e.account_id=?3 AND e.repository_id=?4 WHERE d.revision_id=?1 AND d.generation=?2
         AND d.state='active' AND (?5<>'context' OR d.include_context) AND (?5<>'media' OR d.include_media)
         AND ((?6 IS NULL AND d.lease_until IS NULL) OR (d.lease_token=?6 AND d.lease_until>?7)))",
        params![rev.id, attempt.generation, rev.account, rev.repository,
            family.map_or("", PrFamily::name), attempt.lease, now],
        |row| row.get(0),
    )?;
    if !active {
        return Err(Error::Conflict("PR download is held or has been cancelled"));
    }
    Ok(())
}

impl Store {
    pub fn next_pr_download_due(&self, account: AccountId) -> Result<Option<i64>> {
        self.read(|db| Ok(next_download(db, account)?.map(|job| job.due)))
    }

    pub fn claim_pr_download(
        &self,
        account: AccountId,
        now: i64,
    ) -> Result<Option<PrDownloadClaim>> {
        self.write(|tx| {
            let Some(job) = next_download(tx, account)?.filter(|job| job.due <= now) else {
                return Ok(None);
            };
            let (revision, _) =
                revision_in(tx, account, job.item, job.revision)?.ok_or(Error::Corrupt)?;
            let lease = tx.query_row(
                "UPDATE pr_downloads SET state='active',lease_until=?2,lease_token=lease_token+1
                 WHERE revision_id=?1 RETURNING lease_token",
                params![job.revision, now.saturating_add(45_000)],
                |row| row.get(0),
            )?;
            Ok(Some(PrDownloadClaim {
                revision,
                attempt: PrDownloadAttempt {
                    generation: job.generation,
                    lease: Some(lease),
                },
            }))
        })
    }

    pub fn defer_pr_download(&self, claim: &PrDownloadClaim, due_at: i64) -> Result<()> {
        self.write(|tx| {
            tx.execute(
                "UPDATE pr_downloads SET state='pending',retry_at=?4,lease_until=NULL
                 WHERE revision_id=?1 AND generation=?2 AND lease_token=?3
                 AND lease_until IS NOT NULL AND state='active'",
                params![
                    claim.revision.id,
                    claim.attempt.generation,
                    claim.attempt.lease,
                    due_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn check_pr_download(
        &self,
        rev: &PrRevision,
        attempt: PrDownloadAttempt,
        now: i64,
    ) -> Result<()> {
        self.write(|tx| {
            require_pr_download_attempt(tx, rev, attempt, None, now)?;
            if attempt.lease.is_some() {
                tx.execute(
                    "UPDATE pr_downloads SET lease_until=?2
                     WHERE revision_id=?1 AND lease_until<?3",
                    params![
                        rev.id,
                        now.saturating_add(45_000),
                        now.saturating_add(30_000)
                    ],
                )?;
            }
            Ok(())
        })
    }

    pub fn pr_download(&self, rev: &PrRevision) -> Result<Option<PrDownload>> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            status(&tx, rev)
        })
    }
    pub fn request_pr_download(
        &self,
        rev: &PrRevision,
        context: bool,
        media: bool,
        now_ms: i64,
    ) -> Result<PrDownload> {
        self.write(|tx|{
            validate_revision(tx,rev)?;
            require_current_revision(tx,rev)?;
            if status(tx,rev)?.is_some_and(|job|job.state==PrDownloadState::Partial) {
                tx.execute("UPDATE pr_downloads SET state='partial' WHERE revision_id=? AND state='complete'",[rev.id])?;
            }
            tx.execute("INSERT INTO pr_downloads(revision_id,generation,state,requested_at,include_context,include_media)
                VALUES(?1,1,'pending',?2,?3,?4) ON CONFLICT(revision_id) DO UPDATE SET generation=generation+1,
                state='pending',requested_at=excluded.requested_at,include_context=excluded.include_context,
                include_media=excluded.include_media,retry_at=0,lease_until=NULL
                WHERE pr_downloads.state IN ('cancelled','partial') OR pr_downloads.include_context<>excluded.include_context
                OR pr_downloads.include_media<>excluded.include_media",params![rev.id,now_ms,context,media])?;
            let mut job=status(tx,rev)?.ok_or(Error::Corrupt)?;
            if job.fully_cached() && job.state!=PrDownloadState::Complete {
                tx.execute("UPDATE pr_downloads SET state='complete',lease_until=NULL WHERE revision_id=?",[rev.id])?;
                job.state=PrDownloadState::Complete;
            }
            Ok(job)
        })
    }
    pub fn start_pr_download(&self, rev: &PrRevision, generation: i64) -> Result<()> {
        self.write(|tx|{
            validate_revision(tx,rev)?;
            let changed=tx.execute("UPDATE pr_downloads SET state='active' WHERE revision_id=?1 AND generation=?2 AND state IN ('pending','active')
                AND lease_until IS NULL
                AND EXISTS(SELECT 1 FROM eligible_sync_repositories WHERE account_id=?3 AND repository_id=?4)",
                params![rev.id,generation,rev.account,rev.repository])?;
            if changed!=1{return Err(Error::Conflict("PR download is held or the request changed"));} Ok(())
        })
    }
    pub fn commit_pr_download_page(
        &self,
        rev: &PrRevision,
        generation: i64,
        family: PrFamily,
        expected: &ResourceProgress,
        page: PrInputPage<'_>,
    ) -> Result<Option<ResourceProgress>> {
        self.write(|tx| {
            require_pr_download_attempt(
                tx,
                rev,
                PrDownloadAttempt::unleased(generation),
                Some(family),
                page.fetched_at_ms,
            )?;
            publish_page(tx, rev, family, expected, page)
        })
    }
    pub fn finish_pr_download(&self, rev: &PrRevision, generation: i64) -> Result<PrDownloadState> {
        self.finish_pr_download_attempt(rev, PrDownloadAttempt::unleased(generation), 0)
    }

    pub fn finish_pr_download_attempt(
        &self,
        rev: &PrRevision,
        attempt: PrDownloadAttempt,
        now: i64,
    ) -> Result<PrDownloadState> {
        self.write(|tx| {
            require_pr_download_attempt(tx, rev, attempt, None, now)?;
            let job = status(tx, rev)?.ok_or(Error::NotFound)?;
            let (state, name) = if job.fully_cached() {
                (PrDownloadState::Complete, "complete")
            } else {
                (PrDownloadState::Partial, "partial")
            };
            tx.execute(
                "UPDATE pr_downloads SET state=?,lease_until=NULL WHERE revision_id=?",
                params![name, rev.id],
            )?;
            Ok(state)
        })
    }
    pub fn cancel_pr_download(&self, rev: &PrRevision, generation: i64) -> Result<bool> {
        self.write(|tx|{validate_revision(tx,rev)?; Ok(tx.execute(
            "UPDATE pr_downloads SET state='cancelled',lease_until=NULL WHERE revision_id=? AND generation=? AND state IN ('pending','active','partial','complete')",
            params![rev.id,generation])?==1)})
    }
    pub fn set_pr_file_viewed(&self, rev: &PrRevision, path: &str, viewed: bool) -> Result<()> {
        self.write(|tx|{
            let rid=family_id(tx,rev,PrFamily::Files)?.ok_or(Error::NotFound)?;
            let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM cached_resource_records WHERE resource_id=? AND record_key=?)",
                params![rid,path],|r|r.get(0))?;
            if !exists{return Err(Error::NotFound);}
            if viewed {tx.execute("INSERT INTO pr_viewed_files(revision_id,path) VALUES(?,?) ON CONFLICT DO NOTHING",params![rev.id,path])?;}
            else {tx.execute("DELETE FROM pr_viewed_files WHERE revision_id=? AND path=?",params![rev.id,path])?;}
            Ok(())
        })
    }
    pub fn pr_viewed_files(&self, rev: &PrRevision) -> Result<Vec<String>> {
        self.read(|db| {
            validate_revision(db, rev)?;
            let mut q =
                db.prepare("SELECT path FROM pr_viewed_files WHERE revision_id=? ORDER BY path")?;
            Ok(q.query_map([rev.id], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }
}

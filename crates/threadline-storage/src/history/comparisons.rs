use super::*;
use crate::pr::{PatchEvidence, PrRecord, SourceLine, SourceLocation, SourceSide};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum DiffSemantics {
    Root,
    Direct,
    MergeBase,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryDiffIdentity {
    pub repository: RepositoryId,
    pub base: Option<CommitOid>,
    pub head: CommitOid,
    pub semantics: DiffSemantics,
    pub options: ComparisonOptions,
}
impl HistoryComparison {
    pub fn diff_identity(&self) -> HistoryDiffIdentity {
        match self {
            Self::Root {
                commit, options, ..
            } => HistoryDiffIdentity {
                repository: self.repository(),
                base: None,
                head: commit.clone(),
                semantics: DiffSemantics::Root,
                options: *options,
            },
            Self::Parent { key, .. } | Self::Pair { key } => HistoryDiffIdentity {
                repository: key.repository,
                base: Some(key.base.clone()),
                head: key.head.clone(),
                semantics: match key.mode {
                    ComparisonMode::Direct => DiffSemantics::Direct,
                    ComparisonMode::MergeBase => DiffSemantics::MergeBase,
                },
                options: ComparisonOptions {
                    ignore_whitespace: key.ignore_whitespace,
                    context_lines: key.context_lines,
                },
            },
        }
    }
    fn repository(&self) -> RepositoryId {
        match self {
            Self::Root { repository, .. } => *repository,
            Self::Parent { key, .. } | Self::Pair { key } => key.repository,
        }
    }
    fn common_key(&self) -> Option<&ComparisonKey> {
        match self {
            Self::Root { .. } => None,
            Self::Parent { key, .. } | Self::Pair { key } => Some(key),
        }
    }
}
fn validate(db: &Connection, p: &HistoryComparison) -> Result<()> {
    let identity = p.diff_identity();
    let repo = p.repository();
    let commit = resources::graph_commit(db, repo, &identity.head)?.ok_or(Error::NotFound)?;
    match p {
        HistoryComparison::Root { .. } if commit.parents.is_empty() => Ok(()),
        HistoryComparison::Parent {
            key,
            parent_index,
            parent_count,
        } if key.mode == ComparisonMode::Direct
            && *parent_count == commit.parents.len()
            && commit.parents.get(*parent_index) == Some(&key.base) =>
        {
            Ok(())
        }
        HistoryComparison::Pair { key }
            if resources::graph_commit(db, repo, &key.base)?.is_some() =>
        {
            Ok(())
        }
        _ => Err(Error::Conflict(
            "Comparison does not belong to the selected root, ordered parent or pair",
        )),
    }
}

#[derive(Clone, Debug)]
pub struct HistoryDiffWalk {
    profile: AccessProfile,
    id: i64,
    generation: i64,
    pub comparison: HistoryComparison,
    pub cursor: Option<String>,
}
#[derive(Clone, Debug)]
pub struct HistoryDiffInput<'a> {
    pub identity: &'a HistoryDiffIdentity,
    /// For a merge-base comparison, the actual left source may differ from the requested base.
    pub effective_base: Option<&'a CommitOid>,
    pub files: Vec<PrRecord>,
    pub next_cursor: Option<&'a str>,
    pub complete: bool,
    pub observed_at_ms: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
struct FileRecord {
    record: PrRecord,
    mappings: Vec<SourceLine>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryFileRow {
    pub ordinal: i64,
    pub record: PrRecord,
    pub mappings: Vec<SourceLine>,
}
impl HistoryDiffPage {
    pub fn source_at(
        &self,
        ordinal: i64,
        side: SourceSide,
        patch_line: u32,
    ) -> Option<SourceLocation> {
        let file = self.files.iter().find(|f| f.ordinal == ordinal)?;
        let PrRecord::File {
            path,
            previous_path,
            ..
        } = &file.record
        else {
            return None;
        };
        let row = file.mappings.iter().find(|r| r.patch_line == patch_line)?;
        let (oid, path, line) = match side {
            SourceSide::Left => (
                self.effective_base.clone()?,
                previous_path.as_ref().unwrap_or(path),
                row.left?,
            ),
            SourceSide::Right => (self.identity.head.clone(), path, row.right?),
        };
        Some(SourceLocation {
            oid,
            path: path.clone(),
            side,
            line,
        })
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryDiffPage {
    pub identity: HistoryDiffIdentity,
    pub effective_base: Option<CommitOid>,
    pub coverage: HistoryCoverage,
    pub files: Vec<HistoryFileRow>,
    pub next_ordinal: Option<i64>,
    pub fetched_at_ms: Option<i64>,
}

fn checked_file(record: PrRecord) -> Result<FileRecord> {
    let PrRecord::File {
        path,
        previous_path,
        ..
    } = &record
    else {
        return Err(Error::InvalidInput(
            "A comparison page can contain only file records",
        ));
    };
    if path.is_empty()
        || path.contains('\0')
        || previous_path
            .as_ref()
            .is_some_and(|p| p.is_empty() || p.contains('\0'))
    {
        return Err(Error::InvalidInput("Invalid remote comparison file label"));
    }
    let mut record = record;
    let mappings = crate::pr::file_mappings(&mut record);
    Ok(FileRecord { record, mappings })
}
fn file_path(file: &PrRecord) -> Result<&str> {
    match file {
        PrRecord::File { path, .. } => Ok(path),
        _ => Err(Error::Corrupt),
    }
}

fn reconcile_field<T: Eq>(stored: &mut Option<T>, incoming: Option<T>) -> Result<()> {
    if let Some(value) = incoming {
        if stored.as_ref().is_some_and(|old| old != &value) {
            return Err(Error::Conflict(
                "Conflicting metadata for an immutable comparison file",
            ));
        }
        *stored = Some(value);
    }
    Ok(())
}

fn reconcile_patch(stored: &mut PatchEvidence, incoming: PatchEvidence) -> Result<()> {
    use PatchEvidence::*;
    if *stored == incoming {
        return Ok(());
    }
    match (&*stored, &incoming) {
        (_, Unavailable | ApiCapped) => return Ok(()),
        (Unavailable | ApiCapped, _) => (),
        (Truncated(old), Text(new) | Truncated(new)) if new.starts_with(old) => (),
        (Text(old) | Truncated(old), Truncated(new)) if old.starts_with(new) => return Ok(()),
        _ => {
            return Err(Error::Conflict(
                "Conflicting patches for an immutable comparison file",
            ));
        }
    }
    *stored = incoming;
    Ok(())
}

fn reconcile_file(mut stored: PrRecord, incoming: PrRecord) -> Result<FileRecord> {
    let (
        PrRecord::File {
            previous_path,
            status,
            additions,
            deletions,
            patch,
            ..
        },
        PrRecord::File {
            previous_path: other_path,
            status: other_status,
            additions: other_additions,
            deletions: other_deletions,
            patch: other_patch,
            ..
        },
    ) = (&mut stored, incoming)
    else {
        return Err(Error::Corrupt);
    };
    reconcile_field(previous_path, other_path)?;
    reconcile_field(additions, other_additions)?;
    reconcile_field(deletions, other_deletions)?;
    if other_status != "unknown" {
        if *status != "unknown" && *status != other_status {
            return Err(Error::Conflict("Immutable file change type changed"));
        }
        *status = other_status;
    }
    reconcile_patch(patch, other_patch)?;
    checked_file(stored)
}
impl Store {
    pub fn begin_history_diff(
        &self,
        p: &AccessProfile,
        plan: &HistoryComparison,
    ) -> Result<Option<HistoryDiffWalk>> {
        self.write(|tx| {
            let repo = plan.repository();
            require_observation_profile(tx, p, repo)?;
            validate(tx, plan)?;
            let key = encode(&plan.diff_identity())?;
            tx.execute(
                "INSERT INTO history_diffs(account_id,repository_id,key) VALUES(?,?,?)
                        ON CONFLICT(repository_id,key) DO NOTHING",
                params![p.account, repo, key],
            )?;
            let (id,generation,cursor,state)=tx.query_row("SELECT id,generation,cursor,state FROM history_diffs WHERE account_id=? AND repository_id=? AND key=?",
                        params![p.account,repo,key],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get::<_,String>(3)?)))?;
            Ok(
                matches!(state.as_str(), "new" | "partial").then(|| HistoryDiffWalk {
                    profile: p.clone(),
                    id,
                    generation,
                    comparison: plan.clone(),
                    cursor,
                }),
            )
        })
    }

    /// Restart a provider-limited immutable file list on explicit request. Retain downloaded files.
    pub fn retry_limited_history_diff(
        &self,
        p: &AccessProfile,
        plan: &HistoryComparison,
    ) -> Result<bool> {
        self.write(|tx| {
            let repo = plan.repository();
            require_observation_profile(tx, p, repo)?;
            validate(tx, plan)?;
            let id = tx
                .query_row(
                    "UPDATE history_diffs SET generation=generation+1,cursor=NULL,state='partial'
                 WHERE account_id=? AND repository_id=? AND key=? AND state='limited' RETURNING id",
                    params![p.account, repo, encode(&plan.diff_identity())?],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?;
            if let Some(id) = id {
                tx.execute("DELETE FROM history_diff_cursors WHERE diff_id=?", [id])?;
            }
            Ok(id.is_some())
        })
    }

    pub fn commit_history_diff_page(
        &self,
        walk: &HistoryDiffWalk,
        page: HistoryDiffInput<'_>,
    ) -> Result<()> {
        let original = walk.comparison.diff_identity();
        if original != *page.identity
            || page.files.len() > 300
            || page
                .next_cursor
                .is_some_and(|s| s.is_empty() || s.len() > 4096)
            || page.complete && page.next_cursor.is_some()
        {
            return Err(Error::InvalidInput(
                "Comparison result differs from its pinned identity or cursor",
            ));
        }
        if match original.semantics {
            DiffSemantics::Root => page.effective_base.is_some(),
            DiffSemantics::Direct => {
                let unavailable = page.effective_base.is_none()
                    && !page.complete
                    && page.next_cursor.is_none()
                    && page.files.is_empty();
                !unavailable && page.effective_base != original.base.as_ref()
            }
            DiffSemantics::MergeBase => false,
        } {
            return Err(Error::Conflict(
                "Incorrect source commit for this comparison",
            ));
        }
        self.write(|tx| {
            let repo = walk.comparison.repository();
            require_observation_profile(tx, &walk.profile, repo)?;
            let raw=tx.query_row("SELECT effective_base FROM history_diffs WHERE id=? AND account_id=? AND repository_id=?
                        AND key=? AND generation=? AND cursor IS ? AND state IN ('new','partial')",
                        params![walk.id,walk.profile.account,repo,encode(&original)?,walk.generation,walk.cursor],
                        |r|r.get::<_,Option<String>>(0)).optional()?.ok_or(Error::Conflict("Comparison response has a stale checkpoint"))?;
            if let (Some(old), Some(new)) = (raw.as_deref(), page.effective_base)
                && old != new.as_str()
            {
                return Err(Error::Conflict("Immutable merge base changed"));
            }
            if let Some(next) = page.next_cursor
                && tx.execute(
                    "INSERT INTO history_diff_cursors VALUES(?,?) ON CONFLICT DO NOTHING",
                    params![walk.id, next],
                )? != 1
            {
                return Err(Error::Conflict("Repeated comparison continuation"));
            }
            let mut index = tx.query_row(
                "SELECT COALESCE(MAX(ordinal),-1)+1 FROM history_diff_files WHERE diff_id=?",
                [walk.id],
                |r| r.get::<_, i64>(0),
            )?;
            for record in page.files {
                let file = checked_file(record)?;
                let path = file_path(&file.record)?.to_owned();
                let old = tx
                    .query_row(
                        "SELECT record FROM history_diff_files WHERE diff_id=? AND path=?",
                        params![walk.id, path],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?;
                if let Some(old) = old {
                    let merged = reconcile_file(decode::<FileRecord>(&old)?.record, file.record)?;
                    tx.execute(
                        "UPDATE history_diff_files SET record=? WHERE diff_id=? AND path=?",
                        params![encode(&merged)?, walk.id, path],
                    )?;
                    continue;
                }
                tx.execute(
                    "INSERT INTO history_diff_files VALUES(?,?,?,?)",
                    params![walk.id, index, path, encode(&file)?],
                )?;
                index += 1;
            }
            let state = if page.complete {
                "complete"
            } else if page.next_cursor.is_some() {
                "partial"
            } else {
                "limited"
            };
            tx.execute("UPDATE history_diffs SET cursor=?,state=?,effective_base=COALESCE(effective_base,?),generation=generation+1,fetched_at=? WHERE id=?",
                        params![page.next_cursor,state,page.effective_base.map(CommitOid::as_str),page.observed_at_ms,walk.id])?;
            if page.complete
                && let Some(key) = walk.comparison.common_key()
                && resources::comparison_id(tx, key)?.is_none()
            {
                let id = resources::insert_comparison(tx,key)?;
                tx.execute(
                    "INSERT INTO cached_comparison_files(comparison_id,file_index,path_label,previous_path_label,patch)
                     SELECT ?,ordinal,path,json_extract(record,'$.record.File.previous_path'),
                            json_extract(record,'$.record.File.patch.Text')
                     FROM history_diff_files WHERE diff_id=?",
                    params![id,walk.id],
                )?;
            }
            Ok(())
        })
    }

    pub fn history_diff_page(
        &self,
        a: AccountId,
        plan: &HistoryComparison,
        after: Option<i64>,
        limit: u16,
    ) -> Result<HistoryDiffPage> {
        self.history_diff_slice(a, plan, after, None, limit)
    }

    /// Restore a selected path using the immutable comparison's file index.
    pub fn history_diff_page_at_file(
        &self,
        account: AccountId,
        plan: &HistoryComparison,
        path: &str,
        limit: u16,
    ) -> Result<HistoryDiffPage> {
        self.history_diff_slice(account, plan, None, Some(path), limit)
    }

    fn history_diff_slice(
        &self,
        a: AccountId,
        plan: &HistoryComparison,
        after: Option<i64>,
        path: Option<&str>,
        limit: u16,
    ) -> Result<HistoryDiffPage> {
        if !(1..=256).contains(&limit) {
            return Err(Error::InvalidInput("Invalid comparison page size"));
        }
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            let repo = plan.repository();
            resources::require_repository(&tx, a, repo)?;
            validate(&tx, plan)?;
            let identity = plan.diff_identity();
            let state=tx.query_row("SELECT id,state,effective_base,fetched_at FROM history_diffs WHERE account_id=? AND repository_id=? AND key=?",
                        params![a,repo,encode(&identity)?],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<i64>>(3)?))).optional()?;
            let data = HistoryDiffPage {
                identity: identity.clone(),
                effective_base: if identity.semantics == DiffSemantics::Direct {
                    identity.base.clone()
                } else {
                    None
                },
                coverage: HistoryCoverage::Missing,
                files: vec![],
                next_ordinal: None,
                fetched_at_ms: None,
            };
            let Some((id, status, base, fetched_at_ms)) = state else {
                return legacy_page(&tx, plan, after, path, limit, data);
            };
            if status == "new" {
                return legacy_page(&tx, plan, after, path, limit, data);
            }
            let after = if let Some(path) = path {
                let ordinal:Option<i64> = tx.query_row("SELECT ordinal FROM history_diff_files WHERE diff_id=? AND path=?",params![id,path],|r|r.get(0)).optional()?;
                ordinal.map_or(i64::MAX, |n|n - 1)
            } else {after.unwrap_or(-1)};
            let mut q=tx.prepare("SELECT ordinal,record FROM history_diff_files WHERE diff_id=? AND ordinal>? ORDER BY ordinal LIMIT ?")?;
            let rows = q.query_map(
                params![id, after, i64::from(limit) + 1],
                |r| Ok((r.get(0)?, r.get::<_, String>(1)?)),
            )?;
            let mut files = Vec::new();
            for row in rows {
                let (ordinal, raw) = row?;
                let f: FileRecord = decode(&raw)?;
                files.push(HistoryFileRow {
                    ordinal,
                    record: f.record,
                    mappings: f.mappings,
                });
            }
            let more = files.len() > usize::from(limit);
            files.truncate(usize::from(limit));
            let next_ordinal = more.then(|| files.last().map(|f| f.ordinal)).flatten();
            Ok(HistoryDiffPage {
                identity,
                effective_base: base.as_deref().map(CommitOid::parse).transpose()?,
                coverage: match status.as_str() {
                    "complete" => HistoryCoverage::Complete,
                    "partial" => HistoryCoverage::Partial,
                    "limited" => HistoryCoverage::Limited,
                    _ => return Err(Error::Corrupt),
                },
                files,
                next_ordinal,
                fetched_at_ms,
            })
        })
    }
}
fn legacy_page(
    db: &Connection,
    plan: &HistoryComparison,
    after: Option<i64>,
    path: Option<&str>,
    limit: u16,
    mut page: HistoryDiffPage,
) -> Result<HistoryDiffPage> {
    let Some(key) = plan.common_key() else {
        return Ok(page);
    };
    let Some(id) = resources::comparison_id(db, key)? else {
        return Ok(page);
    };
    let after = if let Some(path) = path {
        let ordinal:Option<i64> = db.query_row("SELECT file_index FROM cached_comparison_files WHERE comparison_id=? AND path_label=? ORDER BY file_index LIMIT 1",params![id,path],|r|r.get(0)).optional()?;
        ordinal.map_or(i64::MAX, |n| n - 1)
    } else {
        after.unwrap_or(-1)
    };
    let mut q = db.prepare(
        "SELECT file_index,path_label,previous_path_label,patch FROM cached_comparison_files
        WHERE comparison_id=? AND file_index>? ORDER BY file_index LIMIT ?",
    )?;
    let rows = q.query_map(params![id, after, i64::from(limit) + 1], |r| {
        Ok((
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    })?;
    for row in rows {
        let (ordinal, path, previous_path, patch) = row?;
        let f = checked_file(PrRecord::File {
            path,
            previous_path,
            status: "unknown".into(),
            additions: None,
            deletions: None,
            patch: patch.map_or(PatchEvidence::Unavailable, PatchEvidence::Text),
        })?;
        page.files.push(HistoryFileRow {
            ordinal,
            record: f.record,
            mappings: f.mappings,
        });
    }
    if page.files.len() > usize::from(limit) {
        page.files.pop();
        page.next_ordinal = page.files.last().map(|f| f.ordinal);
    }
    page.coverage = HistoryCoverage::Partial;
    Ok(page)
}

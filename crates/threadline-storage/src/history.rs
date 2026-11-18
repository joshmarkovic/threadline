//! Bounded history walks. Mutable refs select immutable commit series; missing parents remain visible boundaries.
use crate::{
    AccountId, Error, RepositoryId, Result, Store,
    access::AccessProfile,
    resources::{self, CommitOid, ComparisonKey, ComparisonMode, GraphCommit},
    writes::{decode, encode, require_observation_profile},
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, num::NonZeroU64};
mod comparisons;
pub use comparisons::*;
mod observations;
pub use observations::*;
mod views;
pub use views::*;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
pub enum HistoryRef {
    Branch(String),
    Tag(String),
    FollowedPr(NonZeroU64),
}
impl HistoryRef {
    pub fn key(&self) -> Result<String> {
        let (prefix, name) = match self {
            Self::Branch(name) => ("refs/heads", name),
            Self::Tag(name) => ("refs/tags", name),
            Self::FollowedPr(n) => return Ok(format!("refs/pull/{n}/head")),
        };
        if name.is_empty()
            || name.starts_with('/')
            || name.ends_with('.')
            || name.contains("..")
            || name.contains("@{")
            || name
                .split('/')
                .any(|s| s.is_empty() || s.starts_with('.') || s.ends_with(".lock"))
            || name
                .bytes()
                .any(|b| b <= 32 || b == 127 || b"~^:?*[\\".contains(&b))
        {
            return Err(Error::InvalidInput("Invalid Git ref name"));
        }
        Ok(format!("{prefix}/{name}"))
    }
    pub fn from_key(key: &str) -> Option<Self> {
        if let Some(s) = key.strip_prefix("refs/heads/") {
            return Some(Self::Branch(s.into()));
        }
        if let Some(s) = key.strip_prefix("refs/tags/") {
            return Some(Self::Tag(s.into()));
        }
        key.strip_prefix("refs/pull/")?
            .strip_suffix("/head")?
            .parse()
            .ok()
            .map(Self::FollowedPr)
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub commit: GraphCommit,
    pub message: String,
    pub author: Option<String>,
    pub authored_at_ms: Option<i64>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryRefState {
    pub reference: HistoryRef,
    pub tip: Option<CommitOid>,
    pub observed_at_ms: i64,
}
#[derive(Clone, Debug)]
pub struct HistoryWalk {
    profile: AccessProfile,
    series: i64,
    generation: i64,
    pub repository: RepositoryId,
    pub reference: HistoryRef,
    pub tip: CommitOid,
    pub cursor: Option<String>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphParent {
    pub oid: CommitOid,
    pub lane: usize,
    pub loaded: bool,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryRow {
    pub ordinal: i64,
    pub lane: usize,
    pub entry: HistoryEntry,
    pub parents: Vec<GraphParent>,
    pub labels: Vec<HistoryRef>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryCoverage {
    Missing,
    Partial,
    Complete,
    Limited,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryPage {
    pub rows: Vec<HistoryRow>,
    pub next_ordinal: Option<i64>,
    pub coverage: HistoryCoverage,
    pub fetched_at_ms: Option<i64>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Edge {
    oid: CommitOid,
    lane: usize,
}

fn ref_state(
    db: &Connection,
    repo: RepositoryId,
    reference: &HistoryRef,
) -> Result<Option<HistoryRefState>> {
    let value = db
        .query_row(
            "SELECT oid,observed_at_ms FROM cached_refs WHERE repository_id=? AND name=?",
            params![repo, reference.key()?],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get(1)?)),
        )
        .optional()?;
    value
        .map(|(raw, observed_at_ms)| {
            Ok(HistoryRefState {
                reference: reference.clone(),
                tip: raw.as_deref().map(CommitOid::parse).transpose()?,
                observed_at_ms,
            })
        })
        .transpose()
}
fn slot(lanes: &mut Vec<Option<CommitOid>>, preferred: Option<usize>, oid: &CommitOid) -> usize {
    if let Some(i) = lanes.iter().position(|v| v.as_ref() == Some(oid)) {
        return i;
    }
    let i = preferred
        .filter(|i| lanes.get(*i).is_some_and(Option::is_none))
        .or_else(|| lanes.iter().position(Option::is_none))
        .unwrap_or(lanes.len());
    if i == lanes.len() {
        lanes.push(Some(oid.clone()))
    } else {
        lanes[i] = Some(oid.clone())
    }
    i
}
fn advance_lanes(
    lanes: &mut Vec<Option<CommitOid>>,
    node: usize,
    parents: &[CommitOid],
    prior: &std::collections::BTreeMap<CommitOid, usize>,
) -> Vec<Edge> {
    lanes[node] = None;
    let edges = parents
        .iter()
        .enumerate()
        .map(|(index, oid)| {
            let lane = prior
                .get(oid)
                .copied()
                .unwrap_or_else(|| slot(lanes, (index == 0).then_some(node), oid));
            Edge {
                oid: oid.clone(),
                lane,
            }
        })
        .collect();
    while lanes.last().is_some_and(Option::is_none) {
        lanes.pop();
    }
    edges
}

fn save_entries(
    tx: &Transaction<'_>,
    profile: &AccessProfile,
    repo: RepositoryId,
    entries: &[HistoryEntry],
) -> Result<()> {
    let unique: BTreeSet<_> = entries.iter().map(|e| &e.commit.oid).collect();
    if unique.len() != entries.len()
        || entries.iter().any(|e| {
            e.commit.parents.iter().collect::<BTreeSet<_>>().len() != e.commit.parents.len()
        })
    {
        return Err(Error::InvalidInput(
            "Duplicate commit or duplicate ordered parent in history page",
        ));
    }
    resources::save_commits_in(
        tx,
        profile.account,
        repo,
        &entries.iter().map(|e| e.commit.clone()).collect::<Vec<_>>(),
    )?;
    for e in entries {
        let applied=tx.execute("INSERT INTO history_details(repository_id,oid,message,author,authored_at) VALUES(?,?,?,?,?)
            ON CONFLICT(repository_id,oid) DO UPDATE SET author=COALESCE(history_details.author,excluded.author),
                authored_at=COALESCE(history_details.authored_at,excluded.authored_at)
            WHERE history_details.message=excluded.message AND (history_details.author IS NULL OR excluded.author IS NULL OR history_details.author=excluded.author)
              AND (history_details.authored_at IS NULL OR excluded.authored_at IS NULL OR history_details.authored_at=excluded.authored_at)",
            params![repo,e.commit.oid.as_str(),e.message,e.author,e.authored_at_ms])?;
        if applied != 1 {
            return Err(Error::Conflict(
                "An immutable commit returned different content",
            ));
        }
    }
    Ok(())
}
fn entry(db: &Connection, repo: RepositoryId, oid: &CommitOid) -> Result<Option<HistoryEntry>> {
    let Some(commit) = resources::graph_commit(db, repo, oid)? else {
        return Ok(None);
    };
    let raw=db.query_row("SELECT message,author,authored_at FROM history_details WHERE repository_id=? AND oid=?",
        params![repo,oid.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let (message, author, authored_at_ms) =
        raw.unwrap_or_else(|| (commit.title.clone(), None, None));
    Ok(Some(HistoryEntry {
        commit,
        message,
        author,
        authored_at_ms,
    }))
}

fn append_ready(
    tx: &Transaction<'_>,
    series: i64,
    repo: RepositoryId,
    frontier: &mut Vec<Option<CommitOid>>,
    mut ordinal: i64,
) -> Result<i64> {
    let mut ready=tx.prepare_cached("SELECT EXISTS(SELECT 1 FROM history_waiting w WHERE w.series_id=?1 AND w.oid=?2
        AND NOT EXISTS(SELECT 1 FROM history_waiting child JOIN cached_commit_parents e
          ON e.repository_id=?3 AND e.child_oid=child.oid AND e.parent_oid=w.oid WHERE child.series_id=w.series_id))")?;
    loop {
        let mut chosen = None;
        for (i, oid) in frontier.iter().enumerate() {
            let Some(oid) = oid else { continue };
            if ready.query_row(params![series, oid.as_str(), repo], |r| r.get::<_, bool>(0))? {
                chosen = Some((i, oid.clone()));
                break;
            }
        }
        let Some((lane, oid)) = chosen else { break };
        let commit = resources::graph_commit(tx, repo, &oid)?.ok_or(Error::Corrupt)?;
        let mut prior = std::collections::BTreeMap::new();
        for parent in &commit.parents {
            let previous = tx
                .query_row(
                    "SELECT lane FROM history_rows WHERE series_id=? AND oid=?",
                    params![series, parent.as_str()],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?;
            if let Some(v) = previous {
                prior.insert(
                    parent.clone(),
                    usize::try_from(v).map_err(|_| Error::Corrupt)?,
                );
            }
        }
        let edges = advance_lanes(frontier, lane, &commit.parents, &prior);
        tx.execute(
            "INSERT INTO history_rows VALUES(?,?,?,?,?)",
            params![
                series,
                ordinal,
                oid.as_str(),
                i64::try_from(lane).map_err(|_| Error::Corrupt)?,
                encode(&edges)?
            ],
        )?;
        tx.execute(
            "DELETE FROM history_waiting WHERE series_id=? AND oid=?",
            params![series, oid.as_str()],
        )?;
        ordinal += 1;
    }
    Ok(ordinal)
}

fn stage_known(
    tx: &Transaction<'_>,
    series: i64,
    repo: RepositoryId,
    frontier: &[Option<CommitOid>],
) -> Result<()> {
    tx.execute("WITH RECURSIVE upcoming(oid) AS (
            SELECT value FROM json_each(?1) WHERE type='text'
            UNION SELECT e.parent_oid FROM cached_commit_parents e JOIN upcoming u
                ON e.repository_id=?2 AND e.child_oid=u.oid LIMIT 100)
        INSERT INTO history_waiting(series_id,oid) SELECT ?3,c.oid FROM upcoming u JOIN cached_commits c
        ON c.repository_id=?2 AND c.oid=u.oid WHERE NOT EXISTS(SELECT 1 FROM history_rows h WHERE h.series_id=?3 AND h.oid=c.oid)
        ON CONFLICT DO NOTHING",params![encode(&frontier)?,repo,series])?;
    Ok(())
}

impl Store {
    pub fn history_ref(
        &self,
        account: AccountId,
        repo: RepositoryId,
        reference: &HistoryRef,
    ) -> Result<Option<HistoryRefState>> {
        self.read(|db| {
            resources::require_repository(db, account, repo)?;
            ref_state(db, repo, reference)
        })
    }
    pub fn history_entry(
        &self,
        account: AccountId,
        repo: RepositoryId,
        oid: &CommitOid,
    ) -> Result<Option<HistoryEntry>> {
        self.read(|db| {
            resources::require_repository(db, account, repo)?;
            entry(db, repo, oid)
        })
    }
    pub fn history_refs(
        &self,
        account: AccountId,
        repo: RepositoryId,
        after: Option<&str>,
        limit: u16,
    ) -> Result<Vec<HistoryRefState>> {
        if !(1..=256).contains(&limit) {
            return Err(Error::InvalidInput("Invalid history ref page size"));
        }
        self.read(|db| {
            resources::require_repository(db, account, repo)?;
            let mut q=db.prepare("SELECT name,oid,observed_at_ms FROM cached_refs WHERE repository_id=? AND name>?
                      AND (name GLOB 'refs/heads/*' OR name GLOB 'refs/tags/*' OR name GLOB 'refs/pull/*/head') ORDER BY name LIMIT ?")?;
            let rows = q.query_map(params![repo, after.unwrap_or(""), i64::from(limit)], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get(2)?,
                ))
            })?;
            rows.map(|row| {
                let (key, tip, observed_at_ms) = row?;
                Ok(HistoryRefState {
                    reference: HistoryRef::from_key(&key).ok_or(Error::Corrupt)?,
                    tip: tip.as_deref().map(CommitOid::parse).transpose()?,
                    observed_at_ms,
                })
            })
            .collect()
        })
    }
    pub fn begin_history_page(
        &self,
        p: &AccessProfile,
        repo: RepositoryId,
        reference: &HistoryRef,
    ) -> Result<Option<HistoryWalk>> {
        self.write(|tx| {
            require_observation_profile(tx, p, repo)?;
            let tip = ref_state(tx, repo, reference)?
                .and_then(|r| r.tip)
                .ok_or(Error::Conflict(
                    "The selected reference has no verified commit",
                ))?;
            let inserted=tx.execute("INSERT INTO history_series(account_id,repository_id,tip,frontier) VALUES(?,?,?,?) ON CONFLICT(repository_id,tip) DO NOTHING",
                        params![p.account,repo,tip.as_str(),encode(&[Some(&tip)])?])?;
            let (series,generation,cursor,state)=tx.query_row("SELECT id,generation,cursor,state FROM history_series WHERE account_id=? AND repository_id=? AND tip=?",
                        params![p.account,repo,tip.as_str()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get::<_,String>(3)?)))?;
            if inserted == 1 {
                let mut frontier = vec![Some(tip.clone())];
                stage_known(tx, series, repo, &frontier)?;
                let count = append_ready(tx, series, repo, &mut frontier, 0)?;
                if count > 0 {
                    let coverage = if frontier.is_empty() {
                        "complete"
                    } else {
                        "partial"
                    };
                    tx.execute(
                        "UPDATE history_series SET frontier=?,row_count=?,state=? WHERE id=?",
                        params![encode(&frontier)?, count, coverage, series],
                    )?;
                    if frontier.is_empty() {
                        return Ok(None);
                    }
                }
            }
            Ok(
                matches!(state.as_str(), "new" | "partial").then(|| HistoryWalk {
                    profile: p.clone(),
                    series,
                    generation,
                    repository: repo,
                    reference: reference.clone(),
                    tip,
                    cursor,
                }),
            )
        })
    }
    /// Revisit missing ancestors after a provider-limited walk without discarding layout or cached rows.
    pub fn retry_limited_history(
        &self,
        p: &AccessProfile,
        repo: RepositoryId,
        reference: &HistoryRef,
    ) -> Result<bool> {
        self.write(|tx| {
            require_observation_profile(tx, p, repo)?;
            let Some(tip) = ref_state(tx, repo, reference)?.and_then(|r| r.tip) else {
                return Ok(false);
            };
            let id = tx
                .query_row(
                    "UPDATE history_series SET generation=generation+1,cursor=NULL,state='partial'
                 WHERE account_id=? AND repository_id=? AND tip=? AND state='limited' RETURNING id",
                    params![p.account, repo, tip.as_str()],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?;
            if let Some(id) = id {
                tx.execute("DELETE FROM history_page_cursors WHERE series_id=?", [id])?;
            }
            Ok(id.is_some())
        })
    }

    pub fn commit_history_page(
        &self,
        walk: &HistoryWalk,
        entries: &[HistoryEntry],
        next_cursor: Option<&str>,
        complete: bool,
        at: i64,
    ) -> Result<()> {
        if entries.len() > 100
            || next_cursor.is_some_and(|s| s.is_empty() || s.len() > 4096)
            || complete && next_cursor.is_some()
        {
            return Err(Error::InvalidInput(
                "History pages require bounded records and a consistent continuation",
            ));
        }
        self.write(|tx| {
            require_observation_profile(tx, &walk.profile, walk.repository)?;
            if ref_state(tx, walk.repository, &walk.reference)?
                .and_then(|r| r.tip)
                .as_ref()
                != Some(&walk.tip)
            {
                return Err(Error::Conflict(
                    "The selected reference moved while its page was loading",
                ));
            }
            let state=tx.query_row("SELECT row_count,frontier FROM history_series WHERE id=? AND account_id=? AND repository_id=? AND tip=?
                        AND generation=? AND cursor IS ? AND state IN ('new','partial')",params![walk.series,walk.profile.account,walk.repository,
                        walk.tip.as_str(),walk.generation,walk.cursor],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?))).optional()?
                        .ok_or(Error::Conflict("History page checkpoint was superseded"))?;
            if let Some(next) = next_cursor
                && tx.execute(
                    "INSERT INTO history_page_cursors VALUES(?,?) ON CONFLICT DO NOTHING",
                    params![walk.series, next],
                )? != 1
                {
                    return Err(Error::Conflict("Repeated historical page continuation"));
            }
            let mut frontier: Vec<Option<CommitOid>> = decode(&state.1)?;
            save_entries(tx, &walk.profile, walk.repository, entries)?;
            for e in entries {
                tx.execute("INSERT INTO history_waiting SELECT ?1,?2 WHERE NOT EXISTS(
                            SELECT 1 FROM history_rows WHERE series_id=?1 AND oid=?2) ON CONFLICT DO NOTHING",params![walk.series,e.commit.oid.as_str()])?;
            }
            let row_count = append_ready(tx, walk.series, walk.repository, &mut frontier, state.0)?;
            stage_known(tx, walk.series, walk.repository, &frontier)?;
            let row_count = append_ready(tx, walk.series, walk.repository, &mut frontier, row_count)?;
            let unresolved: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM history_waiting WHERE series_id=?)",
                [walk.series],
                |r| r.get(0),
            )?;
            let coverage = if frontier.is_empty() && row_count > 0 && !unresolved {
                "complete"
            } else if next_cursor.is_some() {
                "partial"
            } else {
                "limited"
            };
            tx.execute("UPDATE history_series SET row_count=?,frontier=?,cursor=?,state=?,fetched_at=?,generation=generation+1 WHERE id=?",
                        params![row_count,encode(&frontier)?,next_cursor,coverage,at,walk.series])?;
            observations::repair_moves(tx, walk.profile.account, walk.repository, at)?;
            Ok(())
        })
    }
    pub fn history_page(
        &self,
        account: AccountId,
        repo: RepositoryId,
        tip: &CommitOid,
        after: Option<i64>,
        limit: u16,
    ) -> Result<HistoryPage> {
        if !(1..=256).contains(&limit) {
            return Err(Error::InvalidInput("Invalid history page size"));
        }
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            resources::require_repository(&tx, account, repo)?;
            let state=tx.query_row("SELECT id,state,fetched_at,row_count FROM history_series WHERE account_id=? AND repository_id=? AND tip=?",
                        params![account,repo,tip.as_str()],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<i64>>(2)?,r.get::<_,i64>(3)?))).optional()?;
            let Some((series, state, fetched_at_ms, total)) = state else {
                return Ok(HistoryPage {
                    rows: vec![],
                    coverage: HistoryCoverage::Missing,
                    next_ordinal: None,
                    fetched_at_ms: None,
                });
            };
            let mut query=tx.prepare("SELECT ordinal,oid,lane,parents FROM history_rows WHERE series_id=? AND ordinal>? ORDER BY ordinal LIMIT ?")?;
            let found = query.query_map(
                params![series, after.unwrap_or(-1), i64::from(limit)],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                    ))
                },
            )?;
            let mut rows = Vec::new();
            for row in found {
                let (ordinal, raw, lane, edges) = row?;
                let oid = CommitOid::parse(&raw)?;
                let mut parents = Vec::new();
                for edge in decode::<Vec<Edge>>(&edges)? {
                    let loaded = tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM history_rows WHERE series_id=? AND oid=?)",
                        params![series, edge.oid.as_str()],
                        |r| r.get(0),
                    )?;
                    parents.push(GraphParent {
                        oid: edge.oid,
                        lane: edge.lane,
                        loaded,
                    });
                }
                let mut labels = tx.prepare(
                    "SELECT name FROM cached_refs WHERE repository_id=? AND oid=? ORDER BY name",
                )?;
                let labels = labels
                    .query_map(params![repo, raw], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows.push(HistoryRow {
                    ordinal,
                    lane: usize::try_from(lane).map_err(|_| Error::Corrupt)?,
                    entry: entry(&tx, repo, &oid)?.ok_or(Error::Corrupt)?,
                    parents,
                    labels: labels
                        .iter()
                        .filter_map(|s| HistoryRef::from_key(s))
                        .collect(),
                });
            }
            let next_ordinal = rows.last().map(|r| r.ordinal).filter(|n| *n + 1 < total);
            Ok(HistoryPage {
                rows,
                next_ordinal,
                fetched_at_ms,
                coverage: match state.as_str() {
                    "complete" => HistoryCoverage::Complete,
                    "partial" => HistoryCoverage::Partial,
                    "limited" => HistoryCoverage::Limited,
                    "new" => HistoryCoverage::Missing,
                    _ => return Err(Error::Corrupt),
                },
            })
        })
    }

    pub fn history_ordinal(
        &self,
        account: AccountId,
        repository: RepositoryId,
        tip: &CommitOid,
        oid: &CommitOid,
    ) -> Result<Option<i64>> {
        self.read(|db| {
            resources::require_repository(db, account, repository)?;
            Ok(db
                .query_row(
                    "SELECT r.ordinal FROM history_rows r JOIN history_series s ON s.id=r.series_id
                 WHERE s.account_id=? AND s.repository_id=? AND s.tip=? AND r.oid=?",
                    params![account, repository, tip.as_str(), oid.as_str()],
                    |row| row.get(0),
                )
                .optional()?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn oid(ch: char) -> CommitOid {
        CommitOid::parse(&ch.to_string().repeat(40)).unwrap()
    }
    #[test]
    fn ut_016_merge_lanes_reuse_ordered_parents_and_keep_shared_ancestry_stable() {
        let mut lanes = vec![Some(oid('f'))];
        let prior = std::collections::BTreeMap::new();
        let merge = advance_lanes(&mut lanes, 0, &[oid('d'), oid('e')], &prior);
        assert_eq!(merge.iter().map(|e| e.lane).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(lanes, [Some(oid('d')), Some(oid('e'))]);
        assert_eq!(advance_lanes(&mut lanes, 0, &[oid('c')], &prior)[0].lane, 0);
        assert_eq!(advance_lanes(&mut lanes, 1, &[oid('c')], &prior)[0].lane, 0);
        assert_eq!(lanes, [Some(oid('c'))]);
        assert!(advance_lanes(&mut lanes, 0, &[], &prior).is_empty());
        assert!(lanes.is_empty());
    }
}

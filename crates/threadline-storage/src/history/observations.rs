use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryMonitoring {
    pub default_branch: Option<String>,
    pub branches: BTreeSet<String>,
    pub branch_patterns: BTreeSet<String>,
    pub followed_prs: BTreeSet<NonZeroU64>,
}
impl HistoryMonitoring {
    pub fn watches(&self, reference: &HistoryRef) -> bool {
        match reference {
            HistoryRef::Branch(name) => {
                self.default_branch.as_ref() == Some(name)
                    || self.branches.contains(name)
                    || self.branch_patterns.iter().any(|p| wildcard(p, name))
            }
            HistoryRef::FollowedPr(number) => self.followed_prs.contains(number),
            HistoryRef::Tag(_) => false,
        }
    }
}
/// Branch patterns use an anchored '*' wildcard. It can match zero or more characters, including '/'.
fn wildcard(pattern: &str, name: &str) -> bool {
    let mut pieces = pattern.split('*');
    let first = pieces.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let mut later = pieces.peekable();
    if later.peek().is_none() {
        return rest.is_empty();
    }
    while let Some(piece) = later.next() {
        if later.peek().is_none() {
            return rest.ends_with(piece);
        }
        let Some((_, after)) = rest.split_once(piece) else {
            return false;
        };
        rest = after;
    }
    true
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefMovement {
    Baseline,
    Unchanged,
    Deleted,
    Created,
    FastForward,
    Rewind,
    Diverged,
    Unknown,
    Stale,
}
impl RefMovement {
    fn code(self) -> &'static str {
        match self {
            Self::FastForward => "fast_forward",
            Self::Rewind => "rewind",
            Self::Diverged => "diverged",
            _ => "unknown",
        }
    }
    fn from_code(value: &str) -> Result<Self> {
        match value {
            "fast_forward" => Ok(Self::FastForward),
            "rewind" => Ok(Self::Rewind),
            "diverged" => Ok(Self::Diverged),
            "unknown" => Ok(Self::Unknown),
            _ => Err(Error::Corrupt),
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefMove {
    pub id: i64,
    pub reference: HistoryRef,
    pub previous: CommitOid,
    pub current: CommitOid,
    pub observed_at_ms: i64,
    pub movement: RefMovement,
}

fn settings(db: &Connection, a: AccountId, r: RepositoryId) -> Result<HistoryMonitoring> {
    let s = db
        .query_row(
            "SELECT settings FROM history_monitoring WHERE account_id=? AND repo=?",
            params![a, r],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    s.as_deref()
        .map(decode)
        .transpose()
        .map(Option::unwrap_or_default)
}
struct Cone {
    parents: BTreeMap<CommitOid, Vec<CommitOid>>,
    complete: bool,
}
fn cone(db: &Connection, repo: RepositoryId, tip: &CommitOid) -> Result<Cone> {
    // A bounded local walk can prove a relationship without asking GitHub to download additional ancestry.
    let mut q = db.prepare(
        "WITH RECURSIVE reachable(oid) AS (
        SELECT ?2 UNION SELECT p.parent_oid FROM cached_commit_parents p JOIN reachable x
        ON p.repository_id=?1 AND p.child_oid=x.oid LIMIT 1025)
        SELECT x.oid,c.oid IS NOT NULL,p.parent_oid FROM reachable x LEFT JOIN cached_commits c
          ON c.repository_id=?1 AND c.oid=x.oid LEFT JOIN cached_commit_parents p
          ON p.repository_id=?1 AND p.child_oid=x.oid ORDER BY x.oid,p.parent_index",
    )?;
    let rows = q.query_map(params![repo, tip.as_str()], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, bool>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut parents = BTreeMap::<CommitOid, Vec<CommitOid>>::new();
    let mut complete = true;
    for row in rows {
        let (oid, known, parent) = row?;
        if !known {
            complete = false;
            continue;
        }
        let edges = parents.entry(CommitOid::parse(&oid)?).or_default();
        if let Some(parent) = parent {
            edges.push(CommitOid::parse(&parent)?);
        }
    }
    complete &= parents.len() <= 1024;
    Ok(Cone { parents, complete })
}
fn classify(
    db: &Connection,
    repo: RepositoryId,
    old: &CommitOid,
    new: &CommitOid,
) -> Result<(RefMovement, BTreeSet<CommitOid>)> {
    let newer = cone(db, repo, new)?;
    let mut children = BTreeMap::<&CommitOid, Vec<&CommitOid>>::new();
    for (child, parents) in &newer.parents {
        for parent in parents {
            children.entry(parent).or_default().push(child)
        }
    }
    if children.contains_key(old) {
        // Every reported commit is a known descendant of the old tip and an ancestor of the new tip.
        let mut added = BTreeSet::new();
        let mut todo = vec![old];
        while let Some(id) = todo.pop() {
            for child in children.get(id).into_iter().flatten() {
                if added.insert((*child).clone()) {
                    todo.push(child)
                }
            }
        }
        if added.contains(new) {
            return Ok((RefMovement::FastForward, added));
        }
    }
    let older = cone(db, repo, old)?;
    let before = older.parents.contains_key(new) || older.parents.values().any(|p| p.contains(new));
    let change = if before {
        RefMovement::Rewind
    } else if newer.complete && older.complete {
        RefMovement::Diverged
    } else {
        RefMovement::Unknown
    };
    Ok((change, BTreeSet::new()))
}

pub(super) fn repair_moves(
    tx: &Transaction<'_>,
    a: AccountId,
    r: RepositoryId,
    at: i64,
) -> Result<()> {
    let mut q=tx.prepare("SELECT id,ref_key,old_tip,new_tip FROM history_ref_moves WHERE account_id=? AND repo=? AND outcome='unknown' ORDER BY last_checked,id LIMIT 8")?;
    let rows = q
        .query_map(params![a, r], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(q);
    for (id, key, old, new) in rows {
        let (change, added) = classify(tx, r, &CommitOid::parse(&old)?, &CommitOid::parse(&new)?)?;
        let tick:i64=tx.query_row("SELECT COALESCE(MAX(last_checked),0)+1 FROM history_ref_moves WHERE account_id=? AND repo=?",
            params![a,r],|row|row.get(0))?;
        tx.execute(
            "UPDATE history_ref_moves SET last_checked=? WHERE id=?",
            params![tick, id],
        )?;
        if change == RefMovement::Unknown {
            continue;
        }
        tx.execute(
            "UPDATE history_ref_moves SET outcome=? WHERE id=?",
            params![change.code(), id],
        )?;
        if added.is_empty() {
            continue;
        }
        let target=match HistoryRef::from_key(&key){
            Some(HistoryRef::FollowedPr(n))=>tx.query_row("SELECT id FROM items WHERE account_id=? AND repository_id=? AND number=? AND kind='pull_request'",
                params![a,r,n.to_string()],|row|row.get::<_,crate::ItemId>(0)).optional()?,
            _=>None,
        };
        for commit in added {
            let c = resources::graph_commit(tx, r, &commit)?.ok_or(Error::Corrupt)?;
            let event_key = format!("commit:{}:{}", r.0, commit.as_str());
            let inserted = tx.execute("INSERT INTO activity_events(account_id,event_key,repository_id,item_id,event_kind,revision,occurred_at_ms,observed_at_ms)
                VALUES(?,?,?,?,'branch_commit_observed',?,?,?) ON CONFLICT(account_id,event_key) DO NOTHING",
                params![a,event_key,r,target,commit.as_str(),c.committed_at_ms,at])?;
            if inserted == 1 {
                use crate::notifications::{self, Category, EventFacts, NotificationTarget};
                notifications::attach_in(
                    tx,
                    a,
                    &event_key,
                    &EventFacts {
                        target: NotificationTarget::Comparison {
                            reference: key.clone(),
                            base: CommitOid::parse(&old)?,
                            head: CommitOid::parse(&new)?,
                        },
                        categories: [Category::BranchUpdate].into(),
                        actor: Default::default(),
                        involves_me: false,
                        receipt: None,
                    },
                )?;
            }
        }
    }
    Ok(())
}

impl Store {
    pub fn history_monitoring(&self, a: AccountId, r: RepositoryId) -> Result<HistoryMonitoring> {
        self.read(|db| {
            resources::require_repository(db, a, r)?;
            settings(db, a, r)
        })
    }
    pub fn set_history_monitoring(
        &self,
        a: AccountId,
        r: RepositoryId,
        options: &HistoryMonitoring,
    ) -> Result<()> {
        for name in options.default_branch.iter().chain(options.branches.iter()) {
            HistoryRef::Branch(name.clone()).key()?;
        }
        for pattern in &options.branch_patterns {
            HistoryRef::Branch(pattern.replace('*', "x")).key()?;
        }
        self.write(|tx| {
            resources::require_repository(tx, a, r)?;
            tx.execute("INSERT INTO history_monitoring VALUES(?,?,?) ON CONFLICT(account_id,repo) DO UPDATE SET settings=excluded.settings",
                        params![a,r,encode(options)?])?;
            let mut q =
                tx.prepare("SELECT ref_key FROM history_ref_baselines WHERE account_id=? AND repo=?")?;
            let refs = q
                .query_map(params![a, r], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(q);
            for key in refs {
                let selected =
                    HistoryRef::from_key(&key).is_some_and(|reference| options.watches(&reference));
                if !selected {
                    tx.execute(
                        "DELETE FROM history_ref_baselines WHERE account_id=? AND repo=? AND ref_key=?",
                        params![a, r, key],
                    )?;
                    tx.execute("DELETE FROM history_ref_moves WHERE account_id=? AND repo=? AND ref_key=? AND outcome='unknown'",params![a,r,key])?;
                }
            }
            Ok(())
        })
    }
    /// None is authoritative deletion of this exact named ref; a failed/incomplete listing is never a deletion.
    pub fn observe_history_ref(
        &self,
        p: &AccessProfile,
        r: RepositoryId,
        reference: &HistoryRef,
        tip: Option<&CommitOid>,
        at: i64,
    ) -> Result<RefMovement> {
        let key = reference.key()?;
        self.write(|tx| {
            require_observation_profile(tx, p, r)?;
            let old = ref_state(tx, r, reference)?;
            let base=tx.query_row("SELECT tip,observed_at FROM history_ref_baselines WHERE account_id=? AND repo=? AND ref_key=?",
                        params![p.account,r,key],|row|Ok((row.get::<_,Option<String>>(0)?,row.get::<_,i64>(1)?))).optional()?;
            let latest = old
                .as_ref()
                .map(|v| v.observed_at_ms)
                .into_iter()
                .chain(base.as_ref().map(|v| v.1))
                .max();
            if latest.is_some_and(|t| t > at) {
                return Ok(RefMovement::Stale);
            }
            if old
                .as_ref()
                .is_some_and(|o| o.observed_at_ms == at && o.tip.as_ref() != tip)
                || base
                    .as_ref()
                    .is_some_and(|(oid, t)| *t == at && oid.as_deref() != tip.map(CommitOid::as_str))
            {
                return Err(Error::Conflict(
                    "A different ref tip has already been recorded for this observation",
                ));
            }
            tx.execute("INSERT INTO cached_refs(repository_id,name,oid,observed_at_ms) VALUES(?,?,?,?) ON CONFLICT(repository_id,name)
                        DO UPDATE SET oid=excluded.oid,observed_at_ms=excluded.observed_at_ms WHERE excluded.observed_at_ms>cached_refs.observed_at_ms",
                        params![r,key,tip.map(CommitOid::as_str),at])?;
            if let Some(tip) = tip {
                tx.execute("INSERT INTO history_ref_tips VALUES(?,?,?,?,?) ON CONFLICT(account_id,repo,ref_key,tip)
                            DO UPDATE SET last_observed_at=MAX(history_ref_tips.last_observed_at,excluded.last_observed_at)",
                            params![p.account,r,key,tip.as_str(),at])?;
            }
            let monitored = settings(tx, p.account, r)?.watches(reference);
            let previous = if monitored {
                tx.execute("INSERT INTO history_ref_baselines VALUES(?,?,?,?,?) ON CONFLICT(account_id,repo,ref_key)
                            DO UPDATE SET tip=excluded.tip,observed_at=excluded.observed_at",
                            params![p.account,r,key,tip.map(CommitOid::as_str),at])?;
                base.map(|(v, _)| v)
            } else {
                old.map(|o| o.tip.map(String::from))
            };
            let Some(previous) = previous else {
                return Ok(RefMovement::Baseline);
            };
            let (old, tip) = match (previous, tip) {
                (None, None) => return Ok(RefMovement::Unchanged),
                (Some(_), None) => return Ok(RefMovement::Deleted),
                (None, Some(_)) => return Ok(RefMovement::Created),
                (Some(old), Some(tip)) if old == tip.as_str() => return Ok(RefMovement::Unchanged),
                (Some(old), Some(tip)) => (old, tip),
            };
            if !monitored {
                return Ok(classify(tx, r, &CommitOid::parse(&old)?, tip)?.0);
            }
            tx.execute("INSERT INTO history_ref_moves(account_id,repo,ref_key,old_tip,new_tip,observed_at) VALUES(?,?,?,?,?,?)",
                        params![p.account,r,key,old,tip.as_str(),at])?;
            let id = tx.last_insert_rowid();
            repair_moves(tx, p.account, r, at)?;
            let outcome = tx.query_row(
                "SELECT outcome FROM history_ref_moves WHERE id=?",
                [id],
                |row| row.get::<_, String>(0),
            )?;
            RefMovement::from_code(&outcome)
        })
    }
    pub fn history_ref_moves(
        &self,
        a: AccountId,
        r: RepositoryId,
        after: i64,
        limit: u16,
    ) -> Result<Vec<RefMove>> {
        if !(1..=256).contains(&limit) {
            return Err(Error::InvalidInput("Invalid move page size"));
        }
        self.read(|db| {
            resources::require_repository(db, a, r)?;
            let mut q = db.prepare(
                "SELECT id,ref_key,old_tip,new_tip,observed_at,outcome FROM history_ref_moves
                WHERE account_id=? AND repo=? AND id>? ORDER BY id LIMIT ?",
            )?;
            let rows = q.query_map(params![a, r, after, i64::from(limit)], |row| {
                Ok((
                    row.get(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get(4)?,
                    row.get::<_, String>(5)?,
                ))
            })?;
            rows.map(|row| {
                let (id, reference, previous, current, observed_at_ms, movement) = row?;
                Ok(RefMove {
                    id,
                    reference: HistoryRef::from_key(&reference).ok_or(Error::Corrupt)?,
                    previous: CommitOid::parse(&previous)?,
                    current: CommitOid::parse(&current)?,
                    observed_at_ms,
                    movement: RefMovement::from_code(&movement)?,
                })
            })
            .collect()
        })
    }
}

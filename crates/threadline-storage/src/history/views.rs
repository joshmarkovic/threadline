use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryView {
    pub reference: HistoryRef,
    pub tip: CommitOid,
    pub selected: Option<CommitOid>,
    pub second: Option<CommitOid>,
    pub parent: Option<CommitOid>,
    pub scroll_anchor: Option<CommitOid>,
    pub scroll_offset: u32,
    #[serde(default)]
    pub graph_window: Option<HistoryWindow>,
    pub file: Option<String>,
    pub file_scroll: u32,
    pub ignore_whitespace: bool,
    pub context_lines: u16,
}

/// The explicitly expanded range within an immutable, repository-scoped history series.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HistoryWindow {
    pub after: Option<i64>,
    pub through: i64,
}

impl HistoryWindow {
    pub(super) fn valid(self) -> bool {
        self.after.is_none_or(|after| after >= 0) && self.through > self.after.unwrap_or(-1)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavedHistoryView {
    pub generation: i64,
    pub state: HistoryView,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ComparisonOptions {
    pub ignore_whitespace: bool,
    pub context_lines: u16,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistoryComparison {
    Root {
        repository: RepositoryId,
        commit: CommitOid,
        options: ComparisonOptions,
    },
    Parent {
        key: ComparisonKey,
        parent_index: usize,
        parent_count: usize,
    },
    Pair {
        key: ComparisonKey,
    },
}

impl Store {
    pub fn history_view(
        &self,
        a: AccountId,
        r: RepositoryId,
        id: &str,
    ) -> Result<Option<SavedHistoryView>> {
        self.read(|db| {
            resources::require_repository(db, a, r)?;
            let row=db.query_row("SELECT generation,state FROM history_views WHERE account_id=? AND repo=? AND view_id=?",
                        params![a,r,id],|v|Ok((v.get(0)?,v.get::<_,String>(1)?))).optional()?;
            row.map(|(generation, s)| {
                Ok(SavedHistoryView {
                    generation,
                    state: decode(&s)?,
                })
            })
            .transpose()
        })
    }
    pub fn save_history_view(
        &self,
        a: AccountId,
        r: RepositoryId,
        id: &str,
        expected_generation: Option<i64>,
        state: &HistoryView,
    ) -> Result<i64> {
        if id.is_empty()
            || state.graph_window.is_some_and(|window| !window.valid())
            || state
                .file
                .as_ref()
                .is_some_and(|path| path.is_empty() || path.contains('\0'))
        {
            return Err(Error::InvalidInput(
                "Invalid history view identity or selected file",
            ));
        }
        state.reference.key()?;
        self.write(|tx| {
            resources::require_repository(tx, a, r)?;
            let old=tx.query_row("SELECT generation,state FROM history_views WHERE account_id=? AND repo=? AND view_id=?",
                        params![a,r,id],|row|Ok((row.get::<_,i64>(0)?,row.get::<_,String>(1)?))).optional()?;
            if old.as_ref().map(|(generation, _)| *generation) != expected_generation {
                return Err(Error::Conflict(
                    "The history view changed before this update",
                ));
            }
            let prior = old
                .as_ref()
                .map(|(_, s)| decode::<HistoryView>(s))
                .transpose()?;
            let known:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM history_ref_tips WHERE account_id=?1 AND repo=?2 AND tip=?3 AND ref_key=?4
                        UNION ALL SELECT 1 FROM cached_refs WHERE repository_id=?2 AND name=?4 AND oid=?3)",
                        params![a,r,state.tip.as_str(),state.reference.key()?],|row|row.get(0))?;
            if !known
                && !prior
                    .as_ref()
                    .is_some_and(|old| old.tip == state.tip && old.reference == state.reference)
            {
                return Err(Error::Conflict(
                    "View tip has no original repository identity",
                ));
            }
            for oid in [
                state.selected.as_ref(),
                state.second.as_ref(),
                state.scroll_anchor.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                let retained = prior.as_ref().is_some_and(|v| {
                    [
                        v.selected.as_ref(),
                        v.second.as_ref(),
                        v.scroll_anchor.as_ref(),
                    ]
                    .contains(&Some(oid))
                });
                if !retained && resources::graph_commit(tx, r, oid)?.is_none() {
                    return Err(Error::Conflict("Unknown commit in this repository"));
                }
            }
            if let Some(parent) = &state.parent {
                let retained = prior
                    .as_ref()
                    .is_some_and(|v| v.selected == state.selected && v.parent.as_ref() == Some(parent));
                if !retained {
                    let selected = state
                        .selected
                        .as_ref()
                        .ok_or(Error::InvalidInput("A selected parent requires a commit"))?;
                    if !resources::graph_commit(tx, r, selected)?
                        .is_some_and(|c| c.parents.contains(parent))
                    {
                        return Err(Error::Conflict("Parent belongs to a different commit"));
                    }
                }
            }
            let next = expected_generation
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(Error::Corrupt)?;
            tx.execute(
                "INSERT INTO history_views VALUES(?,?,?,?,?) ON CONFLICT(account_id,repo,view_id)
                        DO UPDATE SET generation=excluded.generation,state=excluded.state",
                params![a, r, id, next, encode(state)?],
            )?;
            Ok(next)
        })
    }
    pub fn comparison_for_parent(
        &self,
        a: AccountId,
        r: RepositoryId,
        selected: &CommitOid,
        parent: Option<&CommitOid>,
        options: ComparisonOptions,
    ) -> Result<HistoryComparison> {
        self.read(|db| {
            resources::require_repository(db, a, r)?;
            let commit = resources::graph_commit(db, r, selected)?.ok_or(Error::NotFound)?;
            if commit.parents.is_empty() {
                if parent.is_some() {
                    return Err(Error::InvalidInput("A root commit has no parent"));
                }
                return Ok(HistoryComparison::Root {
                    repository: r,
                    commit: selected.clone(),
                    options,
                });
            }
            let index = match parent {
                Some(p) => commit
                    .parents
                    .iter()
                    .position(|v| v == p)
                    .ok_or(Error::Conflict(
                        "Not an ordered parent of the selected commit",
                    ))?,
                None => 0,
            };
            let key = ComparisonKey {
                repository: r,
                base: commit.parents[index].clone(),
                head: selected.clone(),
                mode: ComparisonMode::Direct,
                ignore_whitespace: options.ignore_whitespace,
                context_lines: options.context_lines,
            };
            Ok(HistoryComparison::Parent {
                key,
                parent_index: index,
                parent_count: commit.parents.len(),
            })
        })
    }
    pub fn comparison_between(
        &self,
        a: AccountId,
        r: RepositoryId,
        base: &CommitOid,
        head: &CommitOid,
        mode: ComparisonMode,
        options: ComparisonOptions,
    ) -> Result<HistoryComparison> {
        self.read(|db| {
            resources::require_repository(db, a, r)?;
            if resources::graph_commit(db, r, base)?.is_none()
                || resources::graph_commit(db, r, head)?.is_none()
            {
                return Err(Error::Conflict(
                    "Comparison endpoints must be established in the same repository",
                ));
            }
            Ok(HistoryComparison::Pair {
                key: ComparisonKey {
                    repository: r,
                    base: base.clone(),
                    head: head.clone(),
                    mode,
                    ignore_whitespace: options.ignore_whitespace,
                    context_lines: options.context_lines,
                },
            })
        })
    }
}

use super::*;
use crate::access::AccessProfile;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChoiceList<T> {
    pub values: Vec<T>,
    pub complete: bool,
    pub checked_at_ms: i64,
}
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MilestoneChoice {
    pub number: NonZeroU64,
    pub title: String,
}
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CachedRepositoryChoices {
    pub labels: Option<ChoiceList<String>>,
    pub assignees: Option<ChoiceList<String>>,
    pub milestones: Option<ChoiceList<MilestoneChoice>>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommentAuthorObservation {
    pub comment_id: NonZeroU64,
    pub author_id: NonZeroU64,
    pub observed_at_ms: i64,
}

fn require_profile(db: &Connection, p: &AccessProfile, r: RepositoryId) -> Result<()> {
    let authorized: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM eligible_sync_repositories e
        JOIN repositories r ON e.repository_id=r.id AND r.account_id=e.account_id
        WHERE e.repository_id=? AND e.account_id=? AND r.owner=?)",
        params![r, p.account, p.owner_scope],
        |row| row.get(0),
    )?;
    if !authorized || crate::access::profile(db, p.account, &p.owner_scope)?.as_ref() != Some(p) {
        return Err(Error::Conflict(
            "The repository or owner credential changed before the observation was stored",
        ));
    }
    Ok(())
}

pub(super) fn milestone_in(
    tx: &Transaction<'_>,
    account: AccountId,
    item: ItemId,
    number: Option<NonZeroU64>,
    stamp: i64,
) -> Result<()> {
    tx.execute("INSERT INTO write_issue_milestones(account_id,item_id,number,observed_at) VALUES(?,?,?,?)
        ON CONFLICT(account_id,item_id) DO UPDATE SET number=excluded.number,observed_at=excluded.observed_at
        WHERE excluded.observed_at>write_issue_milestones.observed_at",
        params![account,item,number.map(|id|id.to_string()),stamp])?;
    Ok(())
}

pub(super) fn comment_author_in(
    tx: &Transaction<'_>,
    account: AccountId,
    item: ItemId,
    observation: CommentAuthorObservation,
) -> Result<()> {
    let author_id = observation.author_id.to_string();
    let old=tx.query_row("SELECT author_id FROM write_comment_authors WHERE account_id=? AND item_id=? AND comment_id=?",
        params![account,item,observation.comment_id.to_string()],|r|r.get::<_,String>(0)).optional()?;
    if old.as_deref().is_some_and(|s| s != author_id) {
        return Err(Error::Conflict(
            "A stable comment already has another verified author",
        ));
    }
    tx.execute("INSERT INTO write_comment_authors(account_id,item_id,comment_id,author_id,observed_at) VALUES(?,?,?,?,?)
        ON CONFLICT(account_id,item_id,comment_id) DO UPDATE SET observed_at=MAX(write_comment_authors.observed_at,excluded.observed_at)",
        params![account,item,observation.comment_id.to_string(),author_id,observation.observed_at_ms])?;
    Ok(())
}

impl Store {
    /// Read any available option lists offline. Missing, partial and known-empty lists are distinct.
    pub fn write_choices(&self, a: AccountId, r: RepositoryId) -> Result<CachedRepositoryChoices> {
        self.read(|db| {
            let mut q = db.prepare(
                "SELECT family,value FROM write_repository_choices WHERE account_id=? AND repository_id=?",
            )?;
            let found = q.query_map(params![a, r], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?;
            let mut result = CachedRepositoryChoices::default();
            for row in found {
                let (family, value) = row?;
                match family.as_str() {
                    "labels" => result.labels = Some(decode(&value)?),
                    "assignees" => result.assignees = Some(decode(&value)?),
                    "milestones" => result.milestones = Some(decode(&value)?),
                    _ => return Err(Error::Corrupt),
                }
            }
            Ok(result)
        })
    }

    pub fn record_write_choices(
        &self,
        p: &AccessProfile,
        r: RepositoryId,
        choices: &CachedRepositoryChoices,
    ) -> Result<()> {
        fn validate(values: &[String]) -> Result<()> {
            if values
                .iter()
                .any(|s| s.trim().is_empty() || s.contains('\0'))
                || values
                    .iter()
                    .map(|s| s.to_ascii_lowercase())
                    .collect::<BTreeSet<_>>()
                    .len()
                    != values.len()
            {
                return Err(Error::InvalidInput("Duplicate or empty repository choice"));
            }
            Ok(())
        }
        self.write(|tx| {
            require_profile(tx, p, r)?;
            let mut update=tx.prepare_cached("INSERT INTO write_repository_choices(account_id,repository_id,family,value,observed_at)
                        VALUES(?,?,?,?,?) ON CONFLICT(account_id,repository_id,family) DO UPDATE SET value=excluded.value,observed_at=excluded.observed_at
                        WHERE excluded.observed_at>write_repository_choices.observed_at OR (excluded.observed_at=write_repository_choices.observed_at
                        AND json_extract(excluded.value,'$.complete') AND NOT json_extract(write_repository_choices.value,'$.complete'))")?;
            for (family, value) in [
                ("labels", &choices.labels),
                ("assignees", &choices.assignees),
            ] {
                if let Some(v) = value {
                    validate(&v.values)?;
                    update.execute(params![p.account, r, family, encode(v)?, v.checked_at_ms])?;
                }
            }
            if let Some(v) = &choices.milestones {
                let seen: BTreeSet<_> = v.values.iter().map(|v| v.number).collect();
                if seen.len() != v.values.len() || v.values.iter().any(|v| v.title.trim().is_empty()) {
                    return Err(Error::InvalidInput("Invalid repository milestone"));
                }
                update.execute(params![
                    p.account,
                    r,
                    "milestones",
                    encode(v)?,
                    v.checked_at_ms
                ])?;
            }
            Ok(())
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The provider binds the exact item and repository to the returned fields and revision"
    )]
    pub fn record_issue_write_base(
        &self,
        p: &AccessProfile,
        r: RepositoryId,
        item: ItemId,
        issue_api_id: NonZeroU64,
        number: NonZeroU64,
        fields: &IssueValues,
        milestone_title: Option<&str>,
        remote_updated_ms: i64,
    ) -> Result<()> {
        self.write(|tx| {
            require_profile(tx, p, r)?;
            let exact: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM items WHERE id=? AND account_id=?
                AND repository_id=? AND kind='issue' AND issue_api_id=? AND number=?)",
                params![
                    item,
                    p.account,
                    r,
                    issue_api_id.to_string(),
                    number.to_string()
                ],
                |rr| rr.get(0),
            )?;
            if !exact {
                return Err(Error::Conflict(
                    "Write base belongs to another repository, issue or locator",
                ));
            }
            let target = super::outcomes::CacheTarget {
                account: p.account,
                repository: r,
                item,
                issue_id: issue_api_id,
                number,
            };
            super::outcomes::publish(
                tx,
                target,
                &ConfirmedObject::Issue {
                    fields: fields.clone(),
                    milestone_title: milestone_title.map(str::to_owned),
                    updated_at_ms: remote_updated_ms,
                },
                remote_updated_ms,
            )
        })
    }

    pub fn issue_write_base(
        &self,
        account: AccountId,
        item: ItemId,
    ) -> Result<Option<IssueValues>> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            let raw=tx.query_row("SELECT title,body,body_at,closed FROM items WHERE account_id=? AND id=? AND kind='issue' AND issue_api_id IS NOT NULL",
                        params![account,item],|r|Ok((r.get::<_,Option<String>>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<i64>>(2)?,r.get::<_,Option<bool>>(3)?))).optional()?;
            let Some((title, body, body_at, closed)) = raw else {
                return Ok(None);
            };
            let mut q=tx.prepare("SELECT f.category,v.value FROM reader_facets f LEFT JOIN reader_facet_values v USING(item_id,category)
                        WHERE f.item_id=? AND f.category IN ('label','assignee') ORDER BY f.category,v.value")?;
            let mut result = IssueValues {
                title,
                body: body_at.map(|_| body),
                closed,
                ..Default::default()
            };
            let found = q.query_map([item], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?;
            for row in found {
                let (category, value) = row?;
                let entries = if category == "label" {
                    &mut result.labels
                } else {
                    &mut result.assignees
                }
                .get_or_insert_with(BTreeSet::new);
                if let Some(text) = value {
                    entries.insert(text);
                }
            }
            let proof=tx.query_row("SELECT m.number,m.observed_at>=COALESCE(f.observed_at,m.observed_at)
                        FROM write_issue_milestones m LEFT JOIN reader_issue_fields f ON f.item_id=m.item_id AND f.field='milestone'
                        WHERE m.account_id=? AND m.item_id=?",params![account,item],|r|Ok((r.get::<_,Option<String>>(0)?,r.get::<_,bool>(1)?))).optional()?;
            if let Some((num, true)) = proof {
                result.milestone = Some(num.as_deref().map(numeric).transpose()?);
            }
            if result.milestone.is_none() {
                let cleared:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM reader_issue_fields WHERE item_id=? AND field='milestone' AND value='null')",
                            [item],|r|r.get(0))?;
                if cleared {
                    result.milestone = Some(None);
                }
            }
            Ok(Some(result))
        })
    }

    pub fn record_comment_authors(
        &self,
        p: &AccessProfile,
        r: RepositoryId,
        item: ItemId,
        observations: &[CommentAuthorObservation],
    ) -> Result<()> {
        self.write(|tx| {
            require_profile(tx, p, r)?;
            for o in observations {
                let found:bool=tx.query_row(
                            "SELECT EXISTS(
                                SELECT 1 FROM items i JOIN cached_resources c ON c.item_id=i.id AND c.account_id=i.account_id
                                WHERE i.id=?1 AND i.repository_id=?2 AND i.account_id=?3 AND c.family='comments'
                                  AND (EXISTS(SELECT 1 FROM reader_comment_metadata m WHERE m.resource_id=c.id AND m.record_key=?4)
                                    OR EXISTS(SELECT 1 FROM reader_comment_staging s WHERE s.resource_id=c.id
                                              AND s.generation=c.generation AND s.record_key=?4)))",
                            params![item,r,p.account,format!("{:020}",o.comment_id.get())],|rr|rr.get(0))?;
                if !found {
                    return Err(Error::Conflict(
                        "Comment identity is not cached for this original item",
                    ));
                }
                comment_author_in(tx, p.account, item, *o)?;
            }
            Ok(())
        })
    }

    pub fn comment_author_id(
        &self,
        a: AccountId,
        item: ItemId,
        id: NonZeroU64,
    ) -> Result<Option<NonZeroU64>> {
        self.read(|db| {
            let value=db.query_row("SELECT author_id FROM write_comment_authors WHERE account_id=? AND item_id=? AND comment_id=?",
                        params![a,item,id.to_string()],|r|r.get::<_,String>(0)).optional()?;
            value.as_deref().map(numeric).transpose()
        })
    }
}

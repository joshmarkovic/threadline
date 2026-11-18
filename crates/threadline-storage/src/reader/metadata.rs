use super::*;
use crate::Change;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReaderItemMetadata {
    pub created_at_ms: Option<i64>,
    pub milestone: Change<String>,
    pub reactions: Option<ReactionCounts>,
    pub title: Change<String>,
    pub body: Change<String>,
    pub title_at_ms: Option<i64>,
    pub body_at_ms: Option<i64>,
    /// Newest observed title, body, or state revision. Per-field stamps retain mixed-age bases.
    pub remote_updated_at_ms: Option<i64>,
    pub author: Option<String>,
    pub labels: Option<Vec<String>>,
    pub assignees: Option<Vec<String>>,
    pub reviewers: Option<Vec<String>>,
}

fn content(value: Option<String>, observed_at: Option<i64>) -> Change<String> {
    match (value, observed_at) {
        (_, None) => Change::Missing,
        (None, Some(_)) => Change::Null,
        (Some(value), Some(_)) => Change::Value(value),
    }
}

impl Store {
    pub fn reader_item_metadata(
        &self,
        account: AccountId,
        item: ItemId,
    ) -> Result<Option<ReaderItemMetadata>> {
        self.read(|db| {
            let snapshot = db.unchecked_transaction()?;
            let Some(mut metadata) = snapshot.query_row(
                "SELECT title, body, title_at, body_at, closed_at FROM items WHERE id = ? AND account_id = ?",
                params![item, account], |row| {
                    let title_at_ms = row.get(2)?;
                    let body_at_ms = row.get(3)?;
                    let state_at_ms = row.get(4)?;
                    Ok(ReaderItemMetadata {
                        title: content(row.get(0)?, title_at_ms),
                        body: content(row.get(1)?, body_at_ms),
                        title_at_ms, body_at_ms,
                        remote_updated_at_ms: [title_at_ms, body_at_ms, state_at_ms].into_iter().flatten().max(),
                        ..Default::default()
                    })
                }).optional()? else { return Ok(None) };
            load_presentation(&snapshot,item,&mut metadata)?;
            let mut facets = snapshot.prepare(
                "SELECT f.category, v.value FROM reader_facets f
                 LEFT JOIN reader_facet_values v USING(item_id,category)
                 WHERE f.item_id = ? ORDER BY f.category, v.value COLLATE NOCASE"
            )?;
            let rows = facets.query_map([item], |r| Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?)))?;
            for row in rows {
                let (category, value) = row?;
                let list = match category.as_str() {
                    "author" => { metadata.author = value; continue; }
                    "label" => &mut metadata.labels,
                    "assignee" => &mut metadata.assignees,
                    "reviewer" => &mut metadata.reviewers,
                    _ => continue,
                };
                list.get_or_insert_with(Vec::new).extend(value);
            }
            Ok(Some(metadata))
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssuePresentation {
    pub created_at_ms: Option<i64>,
    pub milestone: Change<String>,
    pub reactions: Option<ReactionCounts>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReactionCounts {
    pub positive: u64,
    pub negative: u64,
    pub laugh: u64,
    pub hooray: u64,
    pub confused: u64,
    pub heart: u64,
    pub rocket: u64,
    pub eyes: u64,
    pub total: Option<u64>,
}

pub(super) fn save_presentation(
    tx: &rusqlite::Transaction<'_>,
    item: ItemId,
    at: i64,
    fields: IssuePresentation,
) -> Result<()> {
    let mut upsert=tx.prepare_cached(
        "INSERT INTO reader_issue_fields(item_id,field,value,observed_at) VALUES(?1,?2,?3,?4)
         ON CONFLICT(item_id,field) DO UPDATE SET value=excluded.value, observed_at=excluded.observed_at
         WHERE excluded.observed_at>reader_issue_fields.observed_at"
    )?;
    if let Some(created) = fields.created_at_ms {
        if created > at {
            return Err(Error::InvalidInput(
                "Issue creation follows provider update",
            ));
        }
        upsert.execute(params![item, "created", created.to_string(), at])?;
    }
    let milestone = match fields.milestone {
        Change::Missing => None,
        Change::Null => Some("null".to_owned()),
        Change::Value(value) => Some(serde_json::to_string(&value).map_err(|_| Error::Corrupt)?),
    };
    if let Some(value) = milestone {
        upsert.execute(params![item, "milestone", value, at])?;
    }
    if let Some(reactions) = fields.reactions {
        let value = serde_json::to_string(&reactions).map_err(|_| Error::Corrupt)?;
        upsert.execute(params![item, "reactions", value, at])?;
    }
    Ok(())
}

fn load_presentation(
    db: &rusqlite::Connection,
    item: ItemId,
    metadata: &mut ReaderItemMetadata,
) -> Result<()> {
    let mut fields =
        db.prepare("SELECT field,value,observed_at FROM reader_issue_fields WHERE item_id=?")?;
    let rows = fields.query_map([item], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    for row in rows {
        let (field, value, observed) = row?;
        metadata.remote_updated_at_ms = metadata.remote_updated_at_ms.max(Some(observed));
        match field.as_str() {
            "created" => {
                metadata.created_at_ms =
                    Some(serde_json::from_str(&value).map_err(|_| Error::Corrupt)?)
            }
            "milestone" => {
                metadata.milestone = serde_json::from_str::<Option<String>>(&value)
                    .map_err(|_| Error::Corrupt)?
                    .map_or(Change::Null, Change::Value)
            }
            "reactions" => {
                metadata.reactions = Some(serde_json::from_str(&value).map_err(|_| Error::Corrupt)?)
            }
            _ => return Err(Error::Corrupt),
        }
    }
    Ok(())
}

use super::*;
use crate::Change;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ReaderItemMetadata {
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

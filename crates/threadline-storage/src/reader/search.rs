use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HitSource {
    Summary,
    Comment,
    Review,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchHit {
    pub row: BrowseRow,
    pub source: HitSource,
    pub anchor: Option<String>,
    pub excerpt: String,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CacheUsage {
    pub downloaded_summaries: u64,
    pub discussion_records: u64,
    pub content_bytes: u64,
    pub offline_pins: u64,
}

impl Store {
    /// A local FTS hit names its discussion record when the match is in a comment or review.
    pub fn search_downloaded(
        &self,
        account: AccountId,
        scope: ReaderScope,
        text: &str,
        limit: u16,
    ) -> Result<Vec<SearchHit>> {
        limit_ok(limit)?;
        let text = text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        if text.chars().count() > 1024 || text.contains('\0') {
            return Err(Error::InvalidInput("Invalid search phrase"));
        }
        let phrase = format!("\"{}\"", text.replace('"', "\"\""));
        self.read(|conn| {
            let db = conn.unchecked_transaction()?;
            scope_check(&db, account, scope)?;
            let limit = usize::from(limit);
            let mut hits = Vec::with_capacity(limit);
            let mut seen = std::collections::HashSet::new();
            // Show matching titles/bodies first. Only read enough index rows to fill this page.
            let summary_sql = format!("SELECT {ROW_FIELDS},snippet(item_search,-1,'[',']','…',18) FROM item_search
                 JOIN reader_rows v ON v.id=item_search.rowid WHERE item_search MATCH ?3 AND {SCOPE}
                 ORDER BY item_search.rowid LIMIT ?4");
            let mut summaries = db.prepare_cached(&summary_sql)?;
            let mut rows = summaries.query(params![account, scope.repo(), phrase, limit as i64])?;
            while let Some(row) = rows.next()? {
                let item = row_at(row, 0)?;
                seen.insert(item.item);
                hits.push(SearchHit {
                    row: item,
                    source: HitSource::Summary,
                    anchor: None,
                    excerpt: row.get(10)?,
                });
            }
            if hits.len() == limit {
                return Ok(hits);
            }
            let discussion_sql = format!("SELECT {ROW_FIELDS},c.family,k.record_key,conversation_search.rowid FROM conversation_search
                 JOIN reader_document_keys k ON k.id=conversation_search.rowid
                 JOIN cached_resources c ON c.id=k.resource_id JOIN reader_rows v ON v.id=c.item_id
                 WHERE conversation_search MATCH ?3 AND {SCOPE} ORDER BY conversation_search.rowid");
            let mut discussion = db.prepare_cached(&discussion_sql)?;
            let mut excerpts = db.prepare_cached("SELECT snippet(conversation_search,0,'[',']','…',18)
                FROM conversation_search WHERE rowid=?1 AND conversation_search MATCH ?2")?;
            let mut matches = discussion.query(params![account, scope.repo(), phrase])?;
            while let Some(row) = matches.next()? {
                if !seen.insert(row.get(0)?) {
                    continue;
                }
                let source = if row.get::<_, String>(10)? == "comments" {
                    HitSource::Comment
                } else {
                    HitSource::Review
                };
                let doc: i64 = row.get(12)?;
                let excerpt = excerpts.query_row(params![doc, phrase], |r| r.get(0))?;
                hits.push(SearchHit {
                    row: row_at(row, 0)?,
                    source,
                    anchor: Some(row.get(11)?),
                    excerpt,
                });
                if hits.len() == limit {
                    break;
                }
            }
            Ok(hits)
        })
    }
    pub fn cache_usage(&self, account: AccountId) -> Result<CacheUsage> {
        self.read(|conn| {
            let counts: (i64, i64, i64, i64) = conn.query_row("WITH items_total AS (
              SELECT count(*) AS n,coalesce(sum(coalesce(length(cast(title AS BLOB)),0)+coalesce(length(cast(body AS BLOB)),0)),0) AS bytes
              FROM items WHERE account_id=?1 AND (title_at IS NOT NULL OR body_at IS NOT NULL)),
              records_total AS (SELECT count(*) AS n,coalesce(sum(length(cast(r.body AS BLOB))),0) AS bytes
              FROM cached_resources c JOIN cached_resource_records r ON c.id=r.resource_id WHERE c.account_id=?1 AND c.family IN ('comments','reviews','review-threads'))
              SELECT i.n,r.n,i.bytes+r.bytes,(SELECT count(*) FROM issue_downloads WHERE account_id=?1 AND state='complete')
              FROM items_total i CROSS JOIN records_total r", [account], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
            Ok(CacheUsage {
                downloaded_summaries: counts.0 as u64,
                discussion_records: counts.1 as u64,
                content_bytes: counts.2 as u64,
                offline_pins: counts.3 as u64,
            })
        })
    }
}

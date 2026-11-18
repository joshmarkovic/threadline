use super::*;
use crate::resources::ResourceError;
use crate::resources::{
    ResourcePage, ResourceProgress, ResourceRecord, ResourceState, Validators, commit_page_in,
    state_in,
};
use rusqlite::params;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiscussionComment {
    pub provider_id: u64,
    pub node_id: Option<String>,
    pub author: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub body: String,
}
pub struct DiscussionInputPage<'a> {
    pub comments: Vec<DiscussionComment>,
    pub next_cursor: Option<&'a str>,
    pub completed_validators: Option<&'a Validators>,
    pub fetched_at_ms: i64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommentMetadata {
    pub key: String,
    pub node_id: Option<String>,
    pub author: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}
pub struct DiscussionPage {
    pub records: Vec<ResourceRecord>,
    pub metadata: Vec<CommentMetadata>,
    pub next: Option<String>,
    pub state: ResourceState,
}

impl Store {
    /// Publish comment data, metadata and coverage together. Preserve readable prior data during a full refresh.
    pub fn commit_discussion_page(
        &self,
        account: AccountId,
        item: ItemId,
        expected: &ResourceProgress,
        page: DiscussionInputPage<'_>,
    ) -> Result<Option<ResourceProgress>> {
        self.commit_comments(account, item, expected, page, None)
    }
    pub fn commit_discussion_partial(
        &self,
        account: AccountId,
        item: ItemId,
        expected: &ResourceProgress,
        page: DiscussionInputPage<'_>,
        reason: ResourceError,
    ) -> Result<()> {
        self.commit_comments(account, item, expected, page, Some(reason))?;
        Ok(())
    }
    fn commit_comments(
        &self,
        account: AccountId,
        item: ItemId,
        expected: &ResourceProgress,
        page: DiscussionInputPage<'_>,
        terminal_error: Option<ResourceError>,
    ) -> Result<Option<ResourceProgress>> {
        if page.comments.len() > 100 {
            return Err(Error::InvalidInput(
                "Comment page exceeds GitHub's page size",
            ));
        }
        let key = ResourceKey {
            account,
            target: ResourceTarget::Item(item),
            family: ResourceFamily::Comments,
        };
        self.write(|tx| {
            let state = state_in(tx, key)?;
            let rid = expected.resource_id;
            tx.execute("DELETE FROM reader_comment_staging WHERE resource_id=? AND generation!=?", params![rid, expected.generation])?;
            let mut records = Vec::with_capacity(page.comments.len());
            for comment in page.comments {
                if comment.provider_id == 0 || comment.updated_at_ms < comment.created_at_ms {
                    return Err(Error::InvalidInput("Invalid comment identity or timestamp"));
                }
                let id = format!("{:020}", comment.provider_id);
                let previous: Option<DiscussionComment> = tx
                    .query_row("SELECT m.updated_at,r.body,m.node_id,m.author,m.created_at FROM reader_comment_metadata m JOIN cached_resource_records r USING(resource_id,record_key)
                     WHERE m.resource_id=?1 AND m.record_key=?2
                     UNION ALL SELECT m.updated_at,p.body,m.node_id,m.author,m.created_at FROM reader_comment_staging m JOIN cached_resource_pending p USING(resource_id,record_key)
                     WHERE m.resource_id=?1 AND m.record_key=?2 AND m.generation=?3 ORDER BY 1 DESC LIMIT 1", params![rid, id, expected.generation], |r| {
                        Ok(DiscussionComment {
                            provider_id: comment.provider_id,
                            updated_at_ms: r.get(0)?,
                            body: r.get(1)?,
                            node_id: r.get(2)?,
                            author: r.get(3)?,
                            created_at_ms: r.get(4)?,
                        })
                    })
                    .optional()?;
                let comment = previous
                    .filter(|p| p.updated_at_ms >= comment.updated_at_ms)
                    .unwrap_or(comment);
                tx.execute(
                    "INSERT INTO reader_comment_staging(resource_id,generation,record_key,node_id,author,created_at,updated_at) VALUES(?,?,?,?,?,?,?)
                ON CONFLICT(resource_id,generation,record_key) DO UPDATE SET node_id=excluded.node_id,author=excluded.author,created_at=excluded.created_at,updated_at=excluded.updated_at",
                    params![
                        rid,
                        expected.generation,
                        id,
                        comment.node_id,
                        comment.author,
                        comment.created_at_ms,
                        comment.updated_at_ms
                    ],
                )?;
                records.push(ResourceRecord {
                    key: id,
                    body: comment.body,
                });
            }
            let next = commit_page_in(
                tx,
                key,
                expected,
                ResourcePage {
                    records: &records,
                    next_cursor: page.next_cursor,
                    completed_validators: page.completed_validators,
                    fetched_at_ms: page.fetched_at_ms,
                },
                terminal_error,
            )?;
            if next.is_none() || state.coverage != Coverage::Complete {
                tx.execute("INSERT INTO reader_comment_metadata(resource_id,record_key,node_id,author,created_at,updated_at)
                SELECT s.resource_id,s.record_key,s.node_id,s.author,s.created_at,s.updated_at FROM reader_comment_staging s
                JOIN cached_resource_records r USING(resource_id,record_key) WHERE s.resource_id=? AND s.generation=?
                ON CONFLICT(resource_id,record_key) DO UPDATE SET node_id=excluded.node_id,author=excluded.author,created_at=excluded.created_at,updated_at=excluded.updated_at", params![rid, expected.generation])?;
            }
            if next.is_none() {
                tx.execute("DELETE FROM reader_comment_staging WHERE resource_id=?", [rid])?;
            }
            Ok(next)
        })
    }
    pub fn discussion_page(
        &self,
        account: AccountId,
        item: ItemId,
        after: Option<&str>,
        limit: u16,
    ) -> Result<DiscussionPage> {
        self.discussion_from(account, item, after, limit, false)
    }

    pub fn discussion_page_at(
        &self,
        account: AccountId,
        item: ItemId,
        anchor: &str,
        limit: u16,
    ) -> Result<DiscussionPage> {
        self.discussion_from(account, item, Some(anchor), limit, true)
    }
    fn discussion_from(
        &self,
        account: AccountId,
        item: ItemId,
        after: Option<&str>,
        limit: u16,
        inclusive: bool,
    ) -> Result<DiscussionPage> {
        limit_ok(limit)?;
        self.read(|conn| {
            let db = conn.unchecked_transaction()?;
            require_item(&db, account, item)?;
            let key = ResourceKey {
                account,
                target: ResourceTarget::Item(item),
                family: ResourceFamily::Comments,
            };
            let state = state_in(&db, key)?;
            if inclusive
                && !db.query_row("SELECT EXISTS(SELECT 1 FROM cached_resource_records r JOIN cached_resources c
                ON c.id=r.resource_id WHERE c.account_id=? AND c.item_id=? AND c.family='comments' AND r.record_key=?)", params![account, item, after], |r| r.get::<_, bool>(0))?
            {
                return Err(Error::NotFound);
            }

            let mut query = db.prepare_cached("SELECT r.record_key,r.body,m.node_id,m.author,m.created_at,m.updated_at FROM cached_resource_records r
                JOIN cached_resources c ON c.id=r.resource_id LEFT JOIN reader_comment_metadata m USING(resource_id,record_key)
                WHERE c.account_id=?1 AND c.item_id=?2 AND c.family='comments' AND (?3 IS NULL OR (?5=1 AND r.record_key=?3) OR
                (coalesce(m.created_at,-9223372036854775808),r.record_key)>(coalesce((SELECT created_at FROM reader_comment_metadata p
                WHERE p.resource_id=r.resource_id AND p.record_key=?3),-9223372036854775808),?3))
                ORDER BY coalesce(m.created_at,-9223372036854775808),r.record_key LIMIT ?4")?;
            let mut entries = query
                .query_map(
                    params![account, item, after, i64::from(limit) + 1, inclusive],
                    |r| {
                        let key: String = r.get(0)?;
                        let metadata = if let Some(created_at_ms) = r.get::<_, Option<i64>>(4)? {
                            Some(CommentMetadata {
                                key: key.clone(),
                                node_id: r.get(2)?,
                                author: r.get(3)?,
                                created_at_ms,
                                updated_at_ms: r.get(5)?,
                            })
                        } else {
                            None
                        };
                        Ok((
                            ResourceRecord {
                                key,
                                body: r.get(1)?,
                            },
                            metadata,
                        ))
                    },
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let next = if entries.len() > usize::from(limit) {
                entries.pop();
                entries.last().map(|(r, _)| r.key.clone())
            } else {
                None
            };
            let (records, metadata): (Vec<_>, Vec<_>) = entries.into_iter().unzip();
            Ok(DiscussionPage {
                records,
                metadata: metadata.into_iter().flatten().collect(),
                next,
                state,
            })
        })
    }
}

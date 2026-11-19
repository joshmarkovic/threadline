use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrReplyCheckpoint {
    pub thread_node: String,
    pub cursor: String,
    generation: i64,
    revision_id: i64,
}

pub struct PrRepliesInputPage<'a> {
    pub comments: Vec<InlineComment>,
    pub next: Option<&'a str>,
    pub received_at_ms: i64,
}

fn checkpoint(
    db: &Connection,
    id: i64,
    rev: &PrRevision,
    node: &str,
) -> Result<Option<PrReplyCheckpoint>> {
    let key = format!("thread:{node}");
    let saved = db
        .query_row(
            "SELECT c.generation,COALESCE(p.body,r.body)
         FROM cached_resources c
         LEFT JOIN cached_resource_pending p ON p.resource_id=c.id AND p.record_key=?2
         LEFT JOIN cached_resource_records r ON r.resource_id=c.id AND r.record_key=?2
         WHERE c.id=?1 AND COALESCE(p.body,r.body) IS NOT NULL",
            params![id, key],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((generation, raw)) = saved else {
        return Ok(None);
    };
    let saved: StoredRecord = decoded(&raw)?;
    let PrRecord::Thread {
        reply_cursor,
        replies_complete: false,
        ..
    } = saved.record
    else {
        return Ok(None);
    };
    Ok(reply_cursor.map(|cursor| PrReplyCheckpoint {
        thread_node: node.into(),
        cursor,
        generation,
        revision_id: rev.id,
    }))
}

impl Store {
    pub fn pr_reply_checkpoint(
        &self,
        rev: &PrRevision,
        node: &str,
    ) -> Result<Option<PrReplyCheckpoint>> {
        self.read(|db| {
            let tx = db.unchecked_transaction()?;
            let Some(id) = family_id(&tx, rev, PrFamily::Threads)? else {
                return Ok(None);
            };
            checkpoint(&tx, id, rev, node)
        })
    }

    /// Apply one GraphQL reply page while the outer thread revision and nested cursor still match.
    pub fn commit_pr_replies(
        &self,
        rev: &PrRevision,
        expected: &PrReplyCheckpoint,
        comments: Vec<InlineComment>,
        next: Option<&str>,
        received_at_ms: i64,
    ) -> Result<()> {
        self.commit_pr_reply_page(
            rev,
            expected,
            PrRepliesInputPage {
                comments,
                next,
                received_at_ms,
            },
            None,
        )
    }

    pub fn commit_pr_reply_page(
        &self,
        rev: &PrRevision,
        expected: &PrReplyCheckpoint,
        page: PrRepliesInputPage<'_>,
        download: Option<PrDownloadAttempt>,
    ) -> Result<()> {
        let PrRepliesInputPage {
            comments,
            next,
            received_at_ms,
        } = page;
        if comments.len() > 100 || next == Some("") || next == Some(expected.cursor.as_str()) {
            return Err(Error::InvalidInput("Invalid nested review reply page"));
        }
        self.write(|tx| {
            if let Some(attempt) = download {
                require_pr_download_attempt(
                    tx,
                    rev,
                    attempt,
                    Some(PrFamily::Threads),
                    received_at_ms,
                )?;
            }
            let id = family_id(tx, rev, PrFamily::Threads)?.ok_or(Error::NotFound)?;
            if checkpoint(tx, id, rev, &expected.thread_node)?.as_ref() != Some(expected) {
                return Err(Error::Conflict(
                    "Review thread changed before replies were received",
                ));
            }
            let record = PrRecord::Thread {
                node_id: expected.thread_node.clone(),
                resolved: None,
                outdated: None,
                location: None,
                comments,
                replies_complete: next.is_none(),
                reply_cursor: next.map(str::to_owned),
            };
            let key = record.stable_key(rev)?;
            observation::record_observations(tx, rev, &record, received_at_ms)?;
            for table in ["cached_resource_pending", "cached_resource_records"] {
                let sql = format!("SELECT body FROM {table} WHERE resource_id=? AND record_key=?");
                let original = tx
                    .query_row(&sql, params![id, key], |row| row.get::<_, String>(0))
                    .optional()?;
                let Some(raw) = original else { continue };
                let mut saved: StoredRecord = decoded(&raw)?;
                let PrRecord::Thread {
                    reply_cursor,
                    location,
                    ..
                } = &saved.record
                else {
                    return Err(Error::Corrupt);
                };
                if reply_cursor.as_deref() != Some(expected.cursor.as_str()) {
                    continue;
                }
                let mut merged = record.clone();
                if let PrRecord::Thread {
                    location: merged_location,
                    ..
                } = &mut merged
                {
                    *merged_location = location.clone();
                }
                records::merge_thread(&mut merged, &saved.record, true);
                saved.search = merged.searchable();
                saved.record = merged;
                let sql =
                    format!("UPDATE {table} SET body=?3 WHERE resource_id=?1 AND record_key=?2");
                tx.execute(&sql, params![id, key, encoded(&saved)?])?;
            }
            Ok(())
        })
    }
}

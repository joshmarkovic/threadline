//! Read-only checkpoint evidence for the mixed issue and pull-request restart fixture.
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(super) fn snapshot(
    db: &Connection,
    account: i64,
    issue: i64,
    discussion: i64,
) -> rusqlite::Result<Value> {
    let repository: i64 = db.query_row(
        "SELECT repository_id FROM items WHERE id=?1 AND account_id=?2",
        params![issue, account],
        |row| row.get(0),
    )?;
    let watermark = db.query_row(
        "SELECT watermark FROM scheduled_reads
         WHERE account_id=?1 AND repository_id=?2 AND item_id IS NULL AND family='comments' AND revision=''",
        params![account, repository],
        |row| row.get::<_, Option<i64>>(0),
    ).optional()?.flatten();
    let monitoring = db.prepare(
        "SELECT stream_key, observed_at_ms FROM activity_baselines WHERE repository_id=? ORDER BY stream_key",
    )?.query_map([repository], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<BTreeMap<_, _>>>()?;
    let timeline = db.query_row(
        "SELECT json_array(id,coverage,last_page_at_ms,last_fetch_at_ms,last_validation_at_ms,etag,continuation)
         FROM cached_resources WHERE account_id=?1 AND item_id=?2 AND family='timeline'",
        params![account, issue], |row| row.get::<_, String>(0),
    ).optional()?.map(|fields| format!("{:x}", Sha256::digest(fields.as_bytes())));
    let comments = db.prepare(
        "SELECT r.record_key, r.body, m.node_id, m.created_at, m.updated_at, m.author
         FROM cached_resource_records r JOIN reader_comment_metadata m USING(resource_id,record_key)
         WHERE r.resource_id=? ORDER BY r.record_key",
    )?.query_map([discussion], |row| {
        let body: String = row.get(1)?;
        Ok(json!({
            "id": row.get::<_, String>(0)?,
            "edited_original": body == "The original comment changed while Threadline was closed.",
            "node": row.get::<_, Option<String>>(2)?,
            "created_ms": row.get::<_, Option<i64>>(3)?,
            "updated_ms": row.get::<_, Option<i64>>(4)?,
            "author": row.get::<_, Option<String>>(5)?,
        }))
    })?.collect::<rusqlite::Result<Vec<_>>>()?;
    let pr = db
        .prepare(
            "SELECT f.family,c.coverage,c.etag,c.last_validation_at_ms,x.body FROM pr_revisions rev
         JOIN items i ON i.id=rev.item_id JOIN pr_family_resources f ON f.revision_id=rev.id
         JOIN cached_resources c ON c.id=f.resource_id
         LEFT JOIN cached_resource_records x ON x.resource_id=c.id
         WHERE rev.account_id=?1 AND rev.repository_id=?2 AND i.number=42 AND rev.head_oid=?3
           AND f.family IN ('reviews','checks','statuses') ORDER BY f.family,x.record_key",
        )?
        .query_map(params![account, repository, "b".repeat(40)], |row| {
            let payload: Option<String> = row.get(4)?;
            let value = payload
                .as_deref()
                .map(serde_json::from_str::<Value>)
                .transpose()
                .map_err(|_| rusqlite::Error::InvalidQuery)?
                .unwrap_or_default();
            Ok(json!({
                "family": row.get::<_, String>(0)?,
                "coverage": row.get::<_, i64>(1)?,
                "etag": row.get::<_, Option<String>>(2)?,
                "validated_ms": row.get::<_, Option<i64>>(3)?,
                "review_id": value.pointer("/record/Review/id"),
                "review": value.pointer("/record/Review/state"),
                "check_id": value.pointer("/record/Check/id"),
                "check": value.pointer("/record/Check/status"),
                "conclusion": value.pointer("/record/Check/conclusion"),
                "status_id": value.pointer("/record/Status/id"),
                "status": value.pointer("/record/Status/state"),
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(json!({
        "comment_feed_watermark": watermark,
        "monitoring_started": monitoring,
        "timeline_digest": timeline,
        "comments": comments,
        "pr": pr,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use threadline_storage::Store;

    #[test]
    fn mixed_catchup_inspector_reads_profile_scoped_stored_records_and_retained_validator() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("profile.sqlite3");
        let store = Store::open(&path).unwrap();
        let account = store.ensure_account("github.com", 41).unwrap();
        let repo = store
            .ensure_repository(account, 71, "fixture-org", "private")
            .unwrap();
        let issue = store.ensure_placeholder(account, repo, 41).unwrap();
        let pr = store.ensure_placeholder(account, repo, 42).unwrap();
        let db = Connection::open(&path).unwrap();
        db.execute("INSERT INTO cached_resources(id,account_id,item_id,family) VALUES (101,?1,?2,'comments'),(102,?1,?2,'timeline')",
            params![account,issue]).unwrap();
        db.execute("INSERT INTO cached_resources(id,account_id,item_id,family,coverage,etag,last_validation_at_ms)
            VALUES(103,?1,?2,'revision-statuses',2,'test-etag',24)",params![account,pr]).unwrap();
        db.execute("INSERT INTO pr_revisions(id,account_id,repository_id,item_id,base_oid,head_oid,header,updated_at)
            VALUES (7,?1,?2,?3,?4,?5,'{}',22)",params![account,repo,pr,"a".repeat(40),"b".repeat(40)]).unwrap();
        db.execute("INSERT INTO pr_family_resources(revision_id,family,resource_id) VALUES(7,'statuses',103)",[]).unwrap();
        let record = json!({"record":{"Status":{"id":7203,"state":"success"}}});
        db.execute("INSERT INTO cached_resource_records(resource_id,record_key,body) VALUES(103,'status:7203',?)",
            [record.to_string()]).unwrap();
        db.execute(
            "INSERT INTO scheduled_reads(account_id,repository_id,family,revision,due_at,watermark)
            VALUES(?1,?2,'comments','',0,18000)",
            params![account, repo],
        )
        .unwrap();
        let result = snapshot(&db, account.0, issue.0, 101).unwrap();
        assert_eq!(result["comment_feed_watermark"], 18000);
        assert_eq!(result["pr"][0]["status"], "success");
        assert_eq!(result["pr"][0]["status_id"], 7203);
        assert_eq!(result["pr"][0]["etag"], "test-etag");
        assert_eq!(result["pr"][0]["validated_ms"], 24);
        assert_eq!(result["timeline_digest"].as_str().unwrap().len(), 64);
        assert!(snapshot(&db, account.0 + 1, issue.0, 101).is_err());
    }
}

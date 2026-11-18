use super::*;
use std::collections::{BTreeMap, BTreeSet};

impl PrRecord {
    pub fn family(&self) -> PrFamily {
        match self {
            Self::Commit { .. } => PrFamily::Commits,
            Self::File { .. } => PrFamily::Files,
            Self::Review { .. } => PrFamily::Reviews,
            Self::Thread { .. } => PrFamily::Threads,
            Self::Check { .. } => PrFamily::Checks,
            Self::Status { .. } => PrFamily::Statuses,
            Self::Context { .. } => PrFamily::Context,
            Self::Media { .. } => PrFamily::Media,
        }
    }
    pub(super) fn stable_key(&self, revision: &PrRevision) -> Result<String> {
        let oid = |v: &str| CommitOid::parse(v).map(|oid| oid.as_str().to_owned());
        let numeric = |prefix: &str, id: u64| {
            if id == 0 {
                Err(Error::InvalidInput("Missing provider object identity"))
            } else {
                Ok(format!("{prefix}:{id}"))
            }
        };
        match self {
            Self::Commit { oid: id, .. } => oid(id),
            Self::File {
                path,
                previous_path,
                ..
            } => {
                label(path)?;
                if let Some(old) = previous_path {
                    label(old)?;
                }
                Ok(path.clone())
            }
            Self::Review { id, commit_oid, .. } => {
                if let Some(value) = commit_oid {
                    oid(value)?;
                }
                numeric("review", *id)
            }
            Self::Thread {
                node_id,
                comments,
                replies_complete,
                reply_cursor,
                ..
            } => {
                label(node_id)?;
                if let Some(cursor) = reply_cursor {
                    label(cursor)?;
                    if *replies_complete {
                        return Err(Error::InvalidInput(
                            "Complete inline thread has a reply cursor",
                        ));
                    }
                }
                let mut ids = BTreeSet::new();
                for c in comments {
                    if c.id == 0 || !ids.insert(c.id) || c.in_reply_to == Some(c.id) {
                        return Err(Error::InvalidInput("Invalid inline comment identity"));
                    }
                    if let Some(a) = &c.anchor {
                        label(&a.path)?;
                        oid(&a.original_oid)?;
                        if a.line == 0 {
                            return Err(Error::InvalidInput("Invalid source line"));
                        }
                    }
                }
                Ok(format!("thread:{node_id}"))
            }
            Self::Check { id, head_oid, .. } | Self::Status { id, head_oid, .. } => {
                if oid(head_oid)? != revision.head.as_str() {
                    return Err(Error::Conflict("Check or status belongs to another head"));
                }
                numeric(self.family().name(), *id)
            }
            Self::Context {
                oid: commit,
                path,
                first_line,
                ..
            } => {
                let commit = oid(commit)?;
                if commit != revision.head.as_str()
                    && revision
                        .merge_base
                        .as_ref()
                        .is_none_or(|base| base.as_str() != commit)
                {
                    return Err(Error::Conflict(
                        "Source context is outside the verified comparison",
                    ));
                }
                label(path)?;
                if *first_line == 0 {
                    return Err(Error::InvalidInput("Source context starts at line one"));
                }
                Ok(format!("{commit}:{first_line}:{path}"))
            }
            Self::Media { cache_key } => {
                label(cache_key)?;
                Ok(cache_key.clone())
            }
        }
    }
    pub(super) fn searchable(&self) -> String {
        match self {
            Self::Review { body, .. } => body.clone(),
            Self::Thread { comments, .. } => comments
                .iter()
                .map(|c| c.body.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    }
}

fn label(value: &str) -> Result<()> {
    if value.is_empty() || value.contains('\0') {
        return Err(Error::InvalidInput("Invalid provider label"));
    }
    Ok(())
}

pub(super) fn merge_thread(record: &mut PrRecord, previous: &PrRecord, same_walk: bool) {
    let PrRecord::Thread {
        comments,
        resolved,
        outdated,
        replies_complete,
        ..
    } = record
    else {
        return;
    };
    let PrRecord::Thread {
        comments: old,
        resolved: prior_resolved,
        outdated: prior_outdated,
        ..
    } = previous
    else {
        return;
    };
    *resolved = resolved.or(*prior_resolved);
    *outdated = outdated.or(*prior_outdated);
    let mut merged = BTreeMap::new();
    for comment in old {
        // An intact full reply snapshot can establish that a previously downloaded reply disappeared.
        if same_walk || !*replies_complete || comments.iter().any(|c| c.id == comment.id) {
            merged.insert(comment.id, comment.clone());
        }
    }
    for mut incoming in comments.drain(..) {
        if let Some(previous) = merged.get(&incoming.id) {
            let stored_time = previous.updated_at_ms.or(previous.created_at_ms);
            let incoming_time = incoming.updated_at_ms.or(incoming.created_at_ms);
            if stored_time > incoming_time {
                continue;
            }
            if incoming.anchor.is_none() {
                incoming.anchor = previous.anchor.clone();
            }
            if incoming.in_reply_to.is_none() {
                incoming.in_reply_to = previous.in_reply_to;
            }
        }
        merged.insert(incoming.id, incoming);
    }
    comments.extend(merged.into_values());
    comments.sort_by_key(|c| (c.created_at_ms.unwrap_or(i64::MAX), c.id));
}

pub(crate) fn publish_page(
    tx: &Transaction<'_>,
    rev: &PrRevision,
    family: PrFamily,
    expected: &ResourceProgress,
    page: PrInputPage<'_>,
) -> Result<Option<ResourceProgress>> {
    if page.records.len() > 100 {
        return Err(Error::InvalidInput("PR page exceeds GitHub's page size"));
    }
    let id = family_id(tx, rev, family)?.ok_or(Error::NotFound)?;
    require_walk_id(tx, id, expected)?;
    let mut next_ordinal: i64 = tx.query_row(
        "SELECT COALESCE(MAX(json_extract(body,'$.ordinal')),-1)+1 FROM cached_resource_pending
         WHERE resource_id=?",
        [id],
        |row| row.get(0),
    )?;
    let mut page_rows = BTreeMap::<String, StoredRecord>::new();
    for mut record in page.records {
        if record.family() != family {
            return Err(Error::Conflict("Record belongs to a different PR family"));
        }
        let key = record.stable_key(rev)?;
        if let PrRecord::Media { cache_key } = &record {
            let cached:bool=tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM cached_images WHERE account_id=? AND repository_id=? AND stable_url=?)",
                params![rev.account,rev.repository,cache_key],|row|row.get(0),
            )?;
            if !cached {
                return Err(Error::Conflict("Referenced PR media is not downloaded"));
            }
        }
        let pending = match page_rows.remove(&key) {
            Some(saved) => Some(saved),
            None => {
                let raw=tx.query_row("SELECT body FROM cached_resource_pending WHERE resource_id=? AND record_key=?",
                    params![id,key],|row|row.get::<_,String>(0)).optional()?;
                raw.as_deref().map(decoded::<StoredRecord>).transpose()?
            }
        };
        let ordinal = if let Some(saved) = pending {
            merge_thread(&mut record, &saved.record, true);
            saved.ordinal
        } else {
            if matches!(&record, PrRecord::Thread { .. }) {
                let raw=tx.query_row("SELECT body FROM cached_resource_records WHERE resource_id=? AND record_key=?",
                    params![id,key],|row|row.get::<_,String>(0)).optional()?;
                if let Some(raw) = raw {
                    merge_thread(&mut record, &decoded::<StoredRecord>(&raw)?.record, false);
                }
            }
            let ordinal = next_ordinal;
            next_ordinal += 1;
            ordinal
        };
        super::observation::record_observations(tx, rev, &record, page.fetched_at_ms)?;
        page_rows.insert(
            key,
            StoredRecord {
                mappings: super::lines::file_mappings(&mut record),
                ordinal,
                search: record.searchable(),
                record,
            },
        );
    }
    let mut records = Vec::with_capacity(page_rows.len());
    for (key, record) in page_rows {
        let body = encoded(&record)?;
        records.push(ResourceRecord { key, body });
    }
    commit_page_by_id(
        tx,
        id,
        expected,
        ResourcePage {
            records: &records,
            next_cursor: page.next_cursor,
            completed_validators: page.validators,
            fetched_at_ms: page.fetched_at_ms,
        },
        page.partial,
    )
}

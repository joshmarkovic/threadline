use super::*;

fn capture(
    db: &Connection,
    rev: &PrRevision,
    path: &str,
    side: SourceSide,
    first: u32,
    last: u32,
) -> Result<InlineAnchor> {
    if first == 0 || last < first {
        return Err(Error::InvalidInput("Invalid inclusive patch range"));
    }
    let record = stored_record(db, rev, PrFamily::Files, path)?
        .ok_or(Error::Conflict("The selected file is not downloaded"))?;
    let PrRecord::File {
        previous_path,
        patch,
        ..
    } = record.record
    else {
        return Err(Error::Corrupt);
    };
    let text = match patch {
        PatchEvidence::Text(s) | PatchEvidence::Truncated(s) => s,
        _ => return Err(Error::Conflict("This file has no verified source lines")),
    };
    let lookup = |index| {
        record
            .mappings
            .iter()
            .find(|r| r.patch_line == index)
            .and_then(|r| match side {
                SourceSide::Left => r.left,
                SourceSide::Right => r.right,
            })
            .ok_or(Error::Conflict(
                "The selected endpoint has no line on that source side",
            ))
    };
    let first_line = lookup(first)?;
    let last_line = lookup(last)?;
    let rows: Vec<_> = text.lines().collect();
    let start = usize::try_from(first - 1).map_err(|_| Error::Corrupt)?;
    let end = usize::try_from(last - 1).map_err(|_| Error::Corrupt)?;
    let selected = rows.get(start..=end).ok_or(Error::Corrupt)?;
    if selected.iter().any(|line| line.starts_with("@@")) || last_line < first_line {
        return Err(Error::InvalidInput(
            "An inline range must belong to a single source hunk",
        ));
    }
    let header = rows[..start]
        .iter()
        .rposition(|line| line.starts_with("@@"))
        .ok_or(Error::Corrupt)?;
    let after = rows[end + 1..]
        .iter()
        .position(|line| line.starts_with("@@"))
        .map_or(rows.len(), |n| end + 1 + n);
    let mut context = vec![rows[header]];
    context.extend_from_slice(&rows[start.saturating_sub(2).max(header + 1)..(end + 3).min(after)]);
    let (source_path, oid) = match side {
        SourceSide::Left => (
            previous_path.unwrap_or_else(|| path.to_owned()),
            rev.merge_base.as_ref().ok_or(Error::Conflict(
                "Missing immutable original comparison source",
            ))?,
        ),
        SourceSide::Right => (path.to_owned(), &rev.head),
    };
    Ok(InlineAnchor {
        path: path.into(),
        source_path,
        side,
        first_line,
        last_line,
        source_oid: oid.as_str().into(),
        original_hunk: context.join("\n"),
        first_patch_line: first,
        last_patch_line: last,
    })
}

pub(super) fn verify_anchors(
    tx: &Transaction<'_>,
    rev: &PrRevision,
    draft_id: Option<&str>,
    draft: &ReviewDraft,
) -> Result<()> {
    unique_keys(draft)?;
    for comment in &draft.comments {
        let anchor = &comment.anchor;
        let raw = encode(anchor)?;
        let proven: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM review_anchor_proofs WHERE account_id=? AND draft_id=?
             AND item_id=? AND revision_id=? AND anchor=?)",
            params![rev.account, draft_id, rev.pr, rev.id, raw],
            |r| r.get(0),
        )?;
        if proven {
            continue;
        }
        if capture(
            tx,
            rev,
            &anchor.path,
            anchor.side,
            anchor.first_patch_line,
            anchor.last_patch_line,
        )? != *anchor
        {
            return Err(Error::Conflict(
                "The supplied inline anchor differs from the original source",
            ));
        }
        if let Some(id) = draft_id {
            tx.execute(
                "INSERT INTO review_anchor_proofs VALUES(?,?,?,?,?)",
                params![rev.account, id, rev.pr, rev.id, raw],
            )?;
        }
    }
    Ok(())
}

fn load(db: &Connection, account: AccountId, id: &str) -> Result<Option<SavedReviewDraft>> {
    let raw=db.query_row("SELECT item_id,revision,base,anchor,body FROM drafts WHERE account_id=? AND id=? AND kind='review'",
        params![account,id],|r|Ok((r.get(0)?,r.get(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?))).optional()?;
    let Some((item, revision, base, anchor, body)) = raw else {
        return Ok(None);
    };
    let Some(id) = base.strip_prefix("pr-review-v1/") else {
        return Ok(None);
    };
    let mut draft: ReviewDraft = decode(&anchor)?;
    draft.summary = body;
    Ok(Some(SavedReviewDraft {
        item,
        revision,
        revision_id: id.parse().map_err(|_| Error::Corrupt)?,
        draft,
    }))
}

impl Store {
    pub fn capture_inline_anchor(
        &self,
        rev: &PrRevision,
        path: &str,
        side: SourceSide,
        first_patch_line: u32,
        last_patch_line: u32,
    ) -> Result<InlineAnchor> {
        self.read(|db| capture(db, rev, path, side, first_patch_line, last_patch_line))
    }
    pub fn review_draft(&self, account: AccountId, id: &str) -> Result<Option<SavedReviewDraft>> {
        self.read(|db| load(db, account, id))
    }
    pub fn create_review_draft(
        &self,
        rev: &PrRevision,
        id: &str,
        draft: &ReviewDraft,
    ) -> Result<()> {
        if id.is_empty() {
            return Err(Error::InvalidInput("The review draft ID is empty"));
        }
        self.write(|tx| {
            pr::validate_revision(tx, rev)?;
            let inserted = tx.execute(
                "INSERT INTO drafts(account_id,id,item_id,kind,body,base,anchor,revision)
                VALUES(?,?,?,'review',?,?,?,1) ON CONFLICT(account_id,id) DO NOTHING",
                params![
                    rev.account,
                    id,
                    rev.pr,
                    draft.summary,
                    format!("pr-review-v1/{}", rev.id),
                    encode(draft)?
                ],
            )?;
            if inserted == 0 {
                let existing = load(tx, rev.account, id)?
                    .ok_or(Error::Conflict("The draft ID is already used"))?;
                if existing.item != rev.pr || existing.revision_id != rev.id {
                    return Err(Error::Conflict("The draft belongs to a different review"));
                }
                return Ok(());
            }
            verify_anchors(tx, rev, Some(id), draft)
        })
    }

    pub fn save_review_draft(
        &self,
        rev: &PrRevision,
        lease: DraftSubmission<'_>,
        draft: &ReviewDraft,
    ) -> Result<i64> {
        self.change_review_draft(rev, lease, draft, false)
    }
    /// Explicit consent to replace original coordinates. Prior anchors and review text stay inspectable.
    pub fn reanchor_review_draft(
        &self,
        rev: &PrRevision,
        lease: DraftSubmission<'_>,
        draft: &ReviewDraft,
    ) -> Result<i64> {
        self.change_review_draft(rev, lease, draft, true)
    }
    fn change_review_draft(
        &self,
        rev: &PrRevision,
        lease: DraftSubmission<'_>,
        draft: &ReviewDraft,
        explicit: bool,
    ) -> Result<i64> {
        self.write(|tx| {
            pr::validate_revision(tx, rev)?;
            let old = load(tx, rev.account, lease.id)?.ok_or(Error::NotFound)?;
            if old.item != rev.pr || old.revision != lease.revision {
                return Err(Error::Conflict(
                    "The original review was changed or replaced",
                ));
            }
            if explicit {
                if pr::current_in(tx, rev.account, rev.pr)?.is_none_or(|(current, _, _)| current != *rev) {
                    return Err(Error::Conflict(
                        "Explicit re-anchoring requires the current downloaded revision",
                    ));
                }
                tx.execute("INSERT INTO review_reanchors(account_id,draft_id,revision,item_id,previous_base,previous_anchor,previous_body)
                            SELECT account_id,id,revision,item_id,base,anchor,body FROM drafts WHERE account_id=? AND id=?",
                            params![rev.account,lease.id])?;
            } else if old.revision_id != rev.id
                || draft.comments.iter().any(|c| {
                    old.draft
                        .comments
                        .iter()
                        .any(|o| o.key == c.key && o.anchor != c.anchor)
                })
            {
                return Err(Error::Conflict(
                    "A changed revision or source range requires explicit re-anchoring",
                ));
            }
            verify_anchors(tx, rev, Some(lease.id), draft)?;
            let changed = tx.execute(
                "UPDATE drafts SET body=?1,base=?2,anchor=?3,revision=revision+1
                        WHERE account_id=?4 AND id=?5 AND editor=?6 AND revision=?7 AND editor_until>?8",
                params![
                    draft.summary,
                    format!("pr-review-v1/{}", rev.id),
                    encode(draft)?,
                    rev.account,
                    lease.id,
                    lease.editor,
                    lease.revision,
                    lease.now
                ],
            )?;
            if changed != 1 {
                return Err(Error::Conflict(
                    "The editor lease or review revision changed",
                ));
            }
            Ok(lease.revision + 1)
        })
    }
    pub fn original_review_draft(
        &self,
        account: AccountId,
        id: &str,
        revision: i64,
    ) -> Result<Option<SavedReviewDraft>> {
        self.read(|db| {
            let row = db
                .query_row(
                    "SELECT item_id,previous_base,previous_anchor,previous_body FROM review_reanchors
                        WHERE account_id=? AND draft_id=? AND revision=?",
                    params![account, id, revision],
                    |r| {
                        Ok((
                            r.get(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    },
                )
                .optional()?;
            let Some((item, base, anchor, body)) = row else {
                return Ok(None);
            };
            let mut draft: ReviewDraft = decode(&anchor)?;
            draft.summary = body;
            let revision_id = base
                .strip_prefix("pr-review-v1/")
                .and_then(|s| s.parse().ok())
                .ok_or(Error::Corrupt)?;
            Ok(Some(SavedReviewDraft {
                item,
                revision,
                revision_id,
                draft,
            }))
        })
    }
}

pub(super) fn saved_matches(
    tx: &Transaction<'_>,
    rev: &PrRevision,
    lease: DraftSubmission<'_>,
    draft: &ReviewDraft,
) -> Result<bool> {
    Ok(load(tx, rev.account, lease.id)?.is_some_and(|d| {
        d.item == rev.pr
            && d.revision_id == rev.id
            && d.revision == lease.revision
            && d.draft == *draft
    }))
}

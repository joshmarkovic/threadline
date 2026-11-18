use super::*;

pub(crate) fn enroll_selected(tx: &Transaction<'_>, account: AccountId, now: i64) -> Result<()> {
    for stream in [
        "pr-reviews",
        "pr-inline-comments",
        "pr-checks",
        "pr-review-requests",
    ] {
        tx.execute(
            "INSERT INTO activity_baselines(repository_id,stream_key,checkpoint,observed_at_ms)
             SELECT repository_id,?2,'monitoring-started',?3 FROM github_repository_access
             WHERE account_id=?1 AND selected=1 ON CONFLICT(repository_id,stream_key) DO NOTHING",
            params![account, stream, now],
        )?;
    }
    Ok(())
}

fn journal(
    tx: &Transaction<'_>,
    rev: &PrRevision,
    stream: &str,
    action: &str,
    object: &str,
    occurred: Option<i64>,
    received: i64,
) -> Result<()> {
    let Some(occurred) = occurred else {
        return Ok(());
    };
    let key = format!("github:{stream}:{object}:{action}:{occurred}");
    tx.execute(
        "INSERT INTO activity_events(account_id,event_key,repository_id,item_id,event_kind,revision,occurred_at_ms,observed_at_ms)
         SELECT ?1,?2,?3,?4,?5,?6,?7,?8 FROM activity_baselines b JOIN github_repository_access r USING(repository_id)
         WHERE b.repository_id=?3 AND b.stream_key=?9 AND b.observed_at_ms<?7 AND r.account_id=?1 AND r.selected=1
         ON CONFLICT(account_id,event_key) DO NOTHING",
        params![rev.account,key,rev.repository,rev.pr,action,rev.head.as_str(),occurred,received,stream],
    )?;
    Ok(())
}
pub(super) fn record_observations(
    tx: &Transaction<'_>,
    rev: &PrRevision,
    row: &PrRecord,
    received: i64,
) -> Result<()> {
    match row {
        PrRecord::Review {
            id,
            submitted_at_ms,
            ..
        } => journal(
            tx,
            rev,
            "pr-reviews",
            "review_submitted",
            &id.to_string(),
            *submitted_at_ms,
            received,
        ),
        PrRecord::Thread { comments, .. } => {
            for c in comments {
                journal(
                    tx,
                    rev,
                    "pr-inline-comments",
                    "inline_comment_created",
                    &c.id.to_string(),
                    c.created_at_ms,
                    received,
                )?;
                if c.updated_at_ms
                    .zip(c.created_at_ms)
                    .is_some_and(|(u, b)| u > b)
                {
                    journal(
                        tx,
                        rev,
                        "pr-inline-comments",
                        "inline_comment_edited",
                        &c.id.to_string(),
                        c.updated_at_ms,
                        received,
                    )?;
                }
            }
            Ok(())
        }
        PrRecord::Check {
            id,
            head_oid,
            completed_at_ms,
            conclusion: Some(_),
            ..
        } => journal(
            tx,
            rev,
            "pr-checks",
            "check_completed",
            &format!("run:{head_oid}:{id}"),
            *completed_at_ms,
            received,
        ),
        PrRecord::Status {
            id,
            head_oid,
            updated_at_ms,
            ..
        } => journal(
            tx,
            rev,
            "pr-checks",
            "commit_status",
            &format!("status:{head_oid}:{id}"),
            *updated_at_ms,
            received,
        ),
        _ => Ok(()),
    }
}

pub(super) fn reviewer_changes(
    tx: &Transaction<'_>,
    rev: &PrRevision,
    previous: &PrHeader,
    current: &PrHeader,
    at: i64,
) -> Result<()> {
    let (Some(was), Some(now)) = (&previous.requested_reviewers, &current.requested_reviewers)
    else {
        return Ok(());
    };
    for reviewer in now
        .iter()
        .filter(|r| !was.iter().any(|old| old.eq_ignore_ascii_case(r)))
    {
        journal(
            tx,
            rev,
            "pr-review-requests",
            "review_requested",
            &format!("{}:{}", rev.pr.0, reviewer.to_ascii_lowercase()),
            Some(at),
            at,
        )?;
    }
    Ok(())
}

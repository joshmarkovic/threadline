use super::*;
use crate::notifications::{self, Category, EventFacts, NotificationTarget, ReceiptMatch};

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
    let key_action = if matches!(
        action,
        "review_approved" | "review_changes_requested" | "review_commented"
    ) {
        "review_submitted"
    } else {
        action
    };
    let key = format!("github:{stream}:{object}:{key_action}:{occurred}");
    tx.execute(
        "INSERT INTO activity_events(account_id,event_key,repository_id,item_id,event_kind,revision,occurred_at_ms,observed_at_ms)
         SELECT ?1,?2,?3,?4,?5,?6,?7,?8 FROM activity_baselines b JOIN github_repository_access r USING(repository_id)
         WHERE b.repository_id=?3 AND b.stream_key=?9 AND b.observed_at_ms<?7 AND r.account_id=?1 AND r.selected=1
         ON CONFLICT(account_id,event_key) DO NOTHING",
        params![rev.account,key,rev.repository,rev.pr,key_action,rev.head.as_str(),occurred,received,stream],
    )?;
    let category = match action {
        "review_approved" => Some(Category::Approval),
        "review_changes_requested" => Some(Category::ChangesRequested),
        "review_commented" => Some(Category::CommentOnlyReview),
        "inline_comment_created" | "inline_comment_edited" => Some(Category::InlineComment),
        "pr_ready" => Some(Category::PrReady),
        "pr_merged" => Some(Category::PrMerged),
        "review_requested" => Some(Category::ReviewRequest),
        "check_completed" | "commit_status" => Some(Category::CheckStatus),
        _ => None,
    };
    if let Some(category) = category {
        let receipt = match category {
            Category::Approval | Category::ChangesRequested | Category::CommentOnlyReview => {
                Some(ReceiptMatch::Review {
                    step: "submit_review".into(),
                    remote_id: object.into(),
                    head: rev.head.clone(),
                })
            }
            Category::InlineComment if action == "inline_comment_created" => {
                Some(ReceiptMatch::Review {
                    step: "reply_to_review_thread".into(),
                    remote_id: object.into(),
                    head: rev.head.clone(),
                })
            }
            _ => None,
        };
        let facts = EventFacts {
            target: NotificationTarget::Item {
                item: rev.pr,
                thread: None,
            },
            categories: [category].into(),
            actor: Default::default(),
            involves_me: false,
            receipt,
        };
        notifications::attach_in(tx, rev.account, &key, &facts)?;
    }
    Ok(())
}
fn enrich_text(
    tx: &Transaction<'_>,
    rev: &PrRevision,
    key: &str,
    body: &str,
    thread: Option<&str>,
) -> Result<()> {
    let pending=tx.query_row("SELECT n.facts,p.login FROM notification_events n JOIN github_repository_access r
        ON r.account_id=n.account_id AND r.repository_id=?3 JOIN github_access_profiles p USING(account_id,owner_scope)
        WHERE n.account_id=?1 AND n.event_key=?2 AND n.decision IS NULL AND n.facts IS NOT NULL",
        params![rev.account,key,rev.repository],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional()?;
    if let Some((raw, login)) = pending {
        let mut facts: EventFacts = serde_json::from_str(&raw).map_err(|_| Error::Corrupt)?;
        if notifications::mentions_login(body, &login) {
            facts.categories.insert(Category::DirectMention);
            facts.involves_me = true;
        }
        if let Some(thread) = thread {
            facts.target = NotificationTarget::Item {
                item: rev.pr,
                thread: Some(thread.into()),
            };
        }
        tx.execute(
            "UPDATE notification_events SET facts=? WHERE account_id=? AND event_key=?",
            params![
                serde_json::to_string(&facts).map_err(|_| Error::Corrupt)?,
                rev.account,
                key
            ],
        )?;
    }
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
            body,
            state,
            submitted_at_ms,
            ..
        } => {
            let action = match state.to_ascii_uppercase().as_str() {
                "APPROVED" => "review_approved",
                "CHANGES_REQUESTED" => "review_changes_requested",
                "COMMENTED" => "review_commented",
                _ => "review_submitted",
            };
            journal(
                tx,
                rev,
                "pr-reviews",
                action,
                &id.to_string(),
                *submitted_at_ms,
                received,
            )?;
            if let Some(at) = submitted_at_ms {
                enrich_text(
                    tx,
                    rev,
                    &format!("github:pr-reviews:{id}:review_submitted:{at}"),
                    body,
                    None,
                )?;
            }
            Ok(())
        }
        PrRecord::Thread {
            node_id, comments, ..
        } => {
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
                for (action, at) in [
                    ("inline_comment_created", c.created_at_ms),
                    ("inline_comment_edited", c.updated_at_ms),
                ] {
                    if let Some(at) = at {
                        enrich_text(
                            tx,
                            rev,
                            &format!("github:pr-inline-comments:{}:{action}:{at}", c.id),
                            &c.body,
                            Some(node_id),
                        )?;
                    }
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
    if previous.draft == Some(true) && current.draft == Some(false) {
        journal(
            tx,
            rev,
            "pr-review-requests",
            "pr_ready",
            &rev.pr.0.to_string(),
            Some(at),
            at,
        )?;
    }
    if previous.merged_at_ms.is_none()
        && let Some(merged) = current.merged_at_ms
    {
        journal(
            tx,
            rev,
            "pr-review-requests",
            "pr_merged",
            &rev.pr.0.to_string(),
            Some(merged),
            at,
        )?;
    }
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

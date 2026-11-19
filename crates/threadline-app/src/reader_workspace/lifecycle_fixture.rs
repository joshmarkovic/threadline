use sha2::{Digest, Sha256};
use threadline_storage::{
    AccountId, Error, ItemId, Result, Store,
    fixtures::{LIFECYCLE_DRAFT, LIFECYCLE_THREAD, LIFECYCLE_THREAD_ROOT},
    pr::{PrSection, PrTabView},
    queue::{DraftContent, DraftKind, Workspace},
};

use super::{drafts::draft_base, write_composer::WriteBase};

fn targets(layout: &Workspace) -> Result<(ItemId, ItemId, ItemId)> {
    match layout.panes.as_slice() {
        [left, right] if left.tabs.len() == 10 && right.tabs.len() == 10 => {
            Ok((left.tabs[0].item, left.tabs[1].item, right.tabs[0].item))
        }
        _ => Err(Error::Conflict("Unexpected fixture pane layout")),
    }
}

fn names(issue: ItemId, comment: ItemId, review: ItemId) -> [String; 4] {
    [
        format!("issue-{}", issue.0),
        format!("comment-{}", comment.0),
        format!("review-{}", review.0),
        format!("review-reply-{}-{LIFECYCLE_THREAD_ROOT}", review.0),
    ]
}

const DRAFT_BODIES: [&str; 4] = [
    "Keep this offline issue edit.",
    "Keep this ordinary unsent comment.",
    LIFECYCLE_DRAFT,
    "Keep this unsent inline reply.",
];

pub(crate) fn seed_drafts(store: &Store, account: AccountId) -> Result<()> {
    let mut layout = store.workspace(account)?.ok_or(Error::NotFound)?;
    let (issue, comment, review) = targets(&layout)?;
    let [issue_key, comment_key, _, reply_key] = names(issue, comment, review);
    let (revision, _) = store
        .current_pr_revision(account, review)?
        .ok_or(Error::NotFound)?;
    let mut issue_draft = draft_base(store, account, issue, DraftKind::IssueEdit)?;
    issue_draft.body = DRAFT_BODIES[0].into();
    store.create_draft(
        account,
        &issue_key,
        issue,
        DraftKind::IssueEdit,
        &issue_draft,
    )?;
    let mut comment_draft = draft_base(store, account, comment, DraftKind::Comment)?;
    comment_draft.body = DRAFT_BODIES[1].into();
    store.create_draft(
        account,
        &comment_key,
        comment,
        DraftKind::Comment,
        &comment_draft,
    )?;
    store.create_draft(
        account,
        &reply_key,
        review,
        DraftKind::Reply,
        &DraftContent {
            body: DRAFT_BODIES[3].into(),
            base: WriteBase::ReviewReply {
                revision_id: revision.id,
                thread_id: LIFECYCLE_THREAD.into(),
                root_comment_id: LIFECYCLE_THREAD_ROOT
                    .try_into()
                    .expect("nonzero thread root"),
            }
            .encode()?,
            anchor: String::new(),
        },
    )?;

    layout.panes[0].tabs[0].view.draft = Some(issue_key);
    layout.panes[0].tabs[1].view.draft = Some(comment_key);
    let reply = &mut layout.panes[1].tabs[1];
    reply.item = review;
    reply.view.draft = Some(reply_key);
    reply.view.context = None;
    reply.view.pr_reader = Some(PrTabView {
        section: PrSection::Conversation,
        ..Default::default()
    });
    store.save_reader_layout(account, &layout, None)
}

pub(crate) fn draft_checkpoint(store: &Store, account: AccountId) -> Result<serde_json::Value> {
    let layout = store.workspace(account)?.ok_or(Error::NotFound)?;
    let (issue, comment, review) = targets(&layout)?;
    let names = names(issue, comment, review);
    let submitted: Vec<_> = store
        .queue_page(account, 0, 10)?
        .into_iter()
        .map(|row| store.operation(account, &row.id)?.ok_or(Error::NotFound))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter_map(|action| action.draft)
        .collect();
    let mut drafts = Vec::new();
    for (((id, (item, kind)), expected_body), position) in names
        .into_iter()
        .zip([
            (issue, DraftKind::IssueEdit),
            (comment, DraftKind::Comment),
            (review, DraftKind::Review),
            (review, DraftKind::Reply),
        ])
        .zip(DRAFT_BODIES)
        .zip([(0, 0), (0, 1), (1, 0), (1, 1)])
    {
        let draft = store.draft(account, &id)?.ok_or(Error::NotFound)?;
        let body = if kind == DraftKind::Review {
            let review = store.review_draft(account, &id)?.ok_or(Error::NotFound)?;
            review
                .draft
                .comments
                .first()
                .ok_or(Error::NotFound)?
                .body
                .clone()
        } else {
            draft.content.body.clone()
        };
        let tab = &layout.panes[position.0].tabs[position.1];
        if body != expected_body
            || draft.kind != kind
            || draft.item != item
            || tab.item != item
            || tab.view.draft.as_deref() != Some(&id)
            || submitted.iter().any(|(draft, _)| draft == &id)
        {
            return Err(Error::Conflict("The unsent fixture draft changed"));
        }
        let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
        drafts.push(serde_json::json!({
            "id": id, "kind": format!("{kind:?}"), "item": item.0, "tab": tab.id,
            "version": draft.revision, "body_sha256": digest,
        }));
    }
    Ok(drafts.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use threadline_storage::fixtures::{lifecycle_checkpoint, populate_native_lifecycle};

    #[test]
    fn mixed_lifecycle_fixture_restores_each_unsent_draft_and_cache_after_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open_profile(directory.path()).unwrap();
        let fixture = populate_native_lifecycle(
            &store,
            12345,
            "00112233445566778899aabbccddeeff",
            1_900_000_000_000,
            || Ok(()),
        )
        .unwrap();
        seed_drafts(&store, fixture.account).unwrap();
        let before = lifecycle_checkpoint(&store, "stable").unwrap();
        let draft_state = draft_checkpoint(&store, fixture.account).unwrap();
        assert_eq!(draft_state.as_array().unwrap().len(), 4);
        assert_eq!(before["sync_checkpoint"]["cursor"], "2");
        assert_eq!(before["content"]["history_commits"], 2);
        drop(store);
        let reopened = Store::open_profile(directory.path()).unwrap();
        assert_eq!(
            draft_checkpoint(&reopened, fixture.account).unwrap(),
            draft_state
        );
        assert_eq!(lifecycle_checkpoint(&reopened, "stable").unwrap(), before);
    }
}

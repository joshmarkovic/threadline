use serde_json::{Value, json};
use threadline_storage::{
    AccountId, Change, Error, IssueApiId, IssueClassification, IssueRecord, ItemId, ItemLocator,
    ItemRepresentation, ItemUpdate, Result, Store, UtcMillis,
    fixtures::Fixture,
    pr::{
        InlineComment, PatchEvidence, PrFamily, PrHeader, PrInputPage, PrRecord, ReviewAnchor,
        ReviewThreadLocation, SourceSide,
    },
    queue::{Pane, Tab, TabView, Workspace},
    reader::{DiscussionComment, DiscussionInputPage},
    resources::{CommitOid, ResourceFamily, ResourceKey, ResourceTarget, WalkMode},
};

const STAMP: i64 = 1_900_000_000_000;
const PATH: &str = "src/comment-links.rs";
pub(super) const TERM: &str = "QuasarLedger";

fn discussion(
    store: &Store,
    account: AccountId,
    item: ItemId,
    first: u64,
    bodies: &[&str],
) -> Result<()> {
    let key = ResourceKey {
        account,
        target: ResourceTarget::Item(item),
        family: ResourceFamily::Comments,
    };
    let progress = store.start_resource_fetch(key, WalkMode::Full)?;
    let comments = bodies
        .iter()
        .enumerate()
        .map(|(index, body)| DiscussionComment {
            provider_id: first + index as u64,
            node_id: None,
            author: Some("fixture-reader".into()),
            created_at_ms: STAMP + index as i64,
            updated_at_ms: STAMP + index as i64,
            body: (*body).into(),
        })
        .collect();
    store.commit_discussion_page(
        account,
        item,
        &progress,
        DiscussionInputPage {
            comments,
            next_cursor: None,
            completed_validators: None,
            fetched_at_ms: STAMP,
        },
    )?;
    Ok(())
}

pub(super) fn seed(store: &Store, fixture: &Fixture) -> Result<Value> {
    let account = fixture.account;
    let issue = fixture.long_discussion;
    let pull = fixture.reviewed_pr;
    let other_number = 88_017;
    let other = store.ingest(
        account,
        ItemLocator {
            repository: fixture.repository,
            number: other_number,
        },
        ItemUpdate {
            representation: ItemRepresentation::Issue(IssueRecord {
                id: IssueApiId::new(other_number.try_into().map_err(|_| Error::Corrupt)?),
                node_id: None,
                classification: IssueClassification::Issue,
            }),
            remote_updated_at: UtcMillis(STAMP),
            title: Change::Value("Independent offline draft".into()),
            body: Change::Value("This pane keeps its own discussion position.".into()),
            closed: Change::Value(false),
        },
    )?;
    discussion(
        store,
        account,
        issue,
        8101,
        &[
            "QuasarLedger issue first match.",
            "QuasarLedger issue second match.",
        ],
    )?;
    discussion(
        store,
        account,
        pull,
        8201,
        &[
            "QuasarLedger pull request first match.",
            "QuasarLedger pull request second match.",
        ],
    )?;
    discussion(
        store,
        account,
        other,
        8411,
        &["Independent discussion position."],
    )?;

    let oid = |value: &str| CommitOid::parse(&value.repeat(40));
    let head = oid("b")?;
    let revision = store.record_pr_revision(
        account,
        pull,
        &oid("a")?,
        &head,
        Some(&oid("c")?),
        &PrHeader::default(),
        STAMP,
    )?;
    let review = |id, in_reply_to, body: &str| InlineComment {
        id,
        node_id: Some(format!("fixture-review-{id}")),
        in_reply_to,
        author: Some("fixture-reviewer".into()),
        actor: Default::default(),
        body: body.into(),
        created_at_ms: Some(STAMP),
        updated_at_ms: Some(STAMP),
        anchor: Some(ReviewAnchor {
            path: PATH.into(),
            side: SourceSide::Right,
            line: 9,
            original_oid: head.as_str().to_owned(),
        }),
    };
    let files = PrRecord::File {
        path: PATH.into(),
        previous_path: None,
        status: "modified".into(),
        additions: Some(1),
        deletions: Some(1),
        patch: PatchEvidence::Text("@@ -3 +9 @@\n-old text\n+new text".into()),
    };
    let thread = PrRecord::Thread {
        node_id: "PRRT_DEEP_LINK".into(),
        resolved: Some(false),
        outdated: Some(false),
        location: Some(ReviewThreadLocation::Line {
            path: PATH.into(),
            side: SourceSide::Right,
            line: 9,
        }),
        comments: vec![
            review(8301, None, "The file-bound review root."),
            review(
                8302,
                Some(8301),
                "The exact separately linked review reply.",
            ),
        ],
        replies_complete: true,
        reply_cursor: None,
    };
    for (family, record) in [(PrFamily::Files, files), (PrFamily::Threads, thread)] {
        let progress = store.start_pr_fetch(&revision, family, WalkMode::Full)?;
        store.commit_pr_page(
            &revision,
            family,
            &progress,
            PrInputPage {
                records: vec![record],
                next_cursor: None,
                validators: None,
                partial: None,
                fetched_at_ms: STAMP,
            },
        )?;
    }
    let tab = |id: &str, item, scroll_anchor| Tab {
        id: id.into(),
        item,
        view: TabView {
            scroll_anchor,
            ..Default::default()
        },
    };
    let other_anchor = format!("{:020}", 8411);
    store.save_workspace(
        account,
        &Workspace {
            focused_pane: 0,
            proportions: vec![0.59, 0.41],
            panes: vec![
                Pane {
                    active: Some(0),
                    tabs: vec![
                        tab("comment-links-issue", issue, None),
                        tab("comment-links-pull", pull, None),
                    ],
                },
                Pane {
                    active: Some(0),
                    tabs: vec![tab(
                        "comment-links-editor",
                        other,
                        Some(other_anchor.clone()),
                    )],
                },
            ],
        },
    )?;
    let summary = |id| {
        let item = store.item(account, id)?.ok_or(Error::NotFound)?;
        let Change::Value(title) = item.title else {
            return Err(Error::Corrupt);
        };
        Ok((item.locator.number, title))
    };
    let (issue_number, issue_title) = summary(issue)?;
    let (pull_number, pull_title) = summary(pull)?;
    let usage = store.cache_usage(account)?;
    Ok(json!({
        "query":TERM, "cached_items":usage.downloaded_summaries,"cached_comments":usage.discussion_records,
        "issue": {"number":issue_number,"item":issue.0,"title":issue_title,"comments":[8101,8102]},
        "pull_request": {"number":pull_number,"item":pull.0,
            "title":pull_title,"comments":[8201,8202],"review_reply":8302,
            "path":PATH,"line":9,"head":head.as_str(),"revision":revision.id},
        "editor": {"item":other.0,"number":other_number,"anchor":other_anchor,"tab":"comment-links-editor"}
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use threadline_storage::{
        fixtures::{Scale, populate_reader},
        reader::{HitSource, ReaderScope},
    };

    #[test]
    fn cached_comment_fixture_indexes_two_items_and_resolves_a_separate_review_reply() {
        let profile = tempfile::tempdir().unwrap();
        let store = Store::open_profile(profile.path()).unwrap();
        let fixture = populate_reader(&store, 12345, Scale::Small).unwrap();
        seed(&store, &fixture).unwrap();
        let results = store
            .search_downloaded(fixture.account, ReaderScope::Selected, TERM, 20)
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|hit| hit.source == HitSource::Comment));
        assert!(
            results
                .iter()
                .any(|hit| hit.row.item == fixture.long_discussion)
        );
        assert!(
            results
                .iter()
                .any(|hit| hit.row.item == fixture.reviewed_pr)
        );
        let revision = store
            .current_pr_revision(fixture.account, fixture.reviewed_pr)
            .unwrap()
            .unwrap()
            .0;
        let (thread, reply) = store
            .pr_review_comment(&revision, 8302.try_into().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(thread, "thread:PRRT_DEEP_LINK");
        assert_eq!(reply.in_reply_to, Some(8301));
        assert_eq!(reply.anchor.unwrap().path, PATH);
    }
}

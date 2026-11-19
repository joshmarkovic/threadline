use threadline_storage::{
    Error, ItemLocator, Result, Store,
    fixtures::Fixture,
    reader::{DiscussionComment, DiscussionInputPage},
    resources::{ResourceFamily, ResourceKey, ResourceTarget, WalkMode},
};

const FIRST_NOTE_ID: u64 = 50_001;
const ANCHOR_INDEX: u64 = 8;
const NOTES: u64 = 28;
const NOTE_START: i64 = 1_730_000_000_000;

#[derive(serde::Serialize)]
pub(super) struct PanePositionsFixture {
    archive: i64,
    item: i64,
    anchor: String,
    pub(super) comments: usize,
    pub(super) replaced_comments: usize,
}

pub(super) fn extend_archive_discussion(
    store: &Store,
    fixture: &Fixture,
) -> Result<PanePositionsFixture> {
    let account = fixture.account;
    let archive = store
        .github_repository_views(account)?
        .into_iter()
        .find(|repo| repo.selected && repo.name == "archive")
        .ok_or(Error::NotFound)?
        .id;
    let item = store
        .resolve(
            account,
            ItemLocator {
                repository: archive,
                number: 2,
            },
        )?
        .ok_or(Error::NotFound)?;
    let key = ResourceKey {
        account,
        target: ResourceTarget::Item(item),
        family: ResourceFamily::Comments,
    };
    let replaced_comments = store
        .discussion_page(account, item, None, 100)?
        .records
        .len();
    let progress = store.start_resource_fetch(key, WalkMode::Full)?;
    let comments = (0..NOTES)
        .map(|index| DiscussionComment {
            provider_id: FIRST_NOTE_ID + index,
            node_id: None,
            author: Some("fixture-reader".into()),
            created_at_ms: NOTE_START + index as i64,
            updated_at_ms: NOTE_START + index as i64,
            body: if index == ANCHOR_INDEX {
                "Orchid reading checkpoint".into()
            } else {
                format!("Saved archive note {}", index + 1)
            },
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
            fetched_at_ms: NOTE_START + NOTES as i64,
        },
    )?;
    Ok(PanePositionsFixture {
        archive: archive.0,
        item: item.0,
        anchor: format!("{:020}", FIRST_NOTE_ID + ANCHOR_INDEX),
        comments: usize::try_from(NOTES).expect("small fixture note count"),
        replaced_comments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use threadline_storage::{
        fixtures::{Scale, populate_reader},
        reader::ReaderScope,
    };

    #[test]
    fn pane_fixture_indexes_the_archive_comment_with_its_exact_anchor() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open_profile(directory.path()).unwrap();
        let fixture = populate_reader(&store, 12345, Scale::Small).unwrap();
        let seeded = extend_archive_discussion(&store, &fixture).unwrap();
        let hits = store
            .search_downloaded(
                fixture.account,
                ReaderScope::Selected,
                "Orchid reading checkpoint",
                10,
            )
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].row.item.0, seeded.item);
        assert_eq!(hits[0].anchor.as_deref(), Some(seeded.anchor.as_str()));
        assert_ne!(seeded.archive, fixture.repository.0);
        let workspace = store.workspace(fixture.account).unwrap().unwrap();
        assert_eq!(
            workspace
                .panes
                .iter()
                .map(|pane| pane.tabs.len())
                .collect::<Vec<_>>(),
            [10, 10]
        );
        assert_eq!(workspace.panes[1].tabs[0].item, fixture.reviewed_pr);
    }
}

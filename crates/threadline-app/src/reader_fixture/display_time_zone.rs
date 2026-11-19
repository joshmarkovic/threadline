use serde_json::{Value, json};
use threadline_storage::{
    Change, Error, IssueClassification, IssueRecord, ItemKind, ItemLocator, ItemRepresentation,
    ItemUpdate, Result, Store, UtcMillis,
    fixtures::Fixture,
    reader::{IssuePresentation, ListFacets, ListSummary},
    resources::Validators,
};

pub(super) fn seed(store: &Store, fixture: &Fixture) -> Result<Value> {
    const CREATED_AT: i64 = 1_689_163_200_000;
    const UPDATED_AT: i64 = 1_700_000_000_000;
    let locator = ItemLocator {
        repository: fixture.repository,
        number: 1,
    };
    let expected = Error::Conflict("The isolated time-zone issue is missing");
    let id = store.resolve(fixture.account, locator)?.ok_or(expected)?;
    let item = store
        .item(fixture.account, id)?
        .filter(|item| item.kind == ItemKind::Issue)
        .ok_or(Error::Conflict(
            "The time-zone target is not a cached issue",
        ))?;
    let provider_id = item.issue_id.ok_or(Error::Conflict(
        "The time-zone issue has no provider identity",
    ))?;
    store.commit_item_metadata(
        fixture.account,
        fixture.repository,
        ListSummary {
            number: locator.number,
            update: ItemUpdate {
                representation: ItemRepresentation::Issue(IssueRecord {
                    id: provider_id,
                    node_id: item.issue_node_id,
                    classification: IssueClassification::Issue,
                }),
                remote_updated_at: UtcMillis(UPDATED_AT),
                title: Change::Missing,
                body: Change::Missing,
                closed: Change::Missing,
            },
            facets: ListFacets {
                presentation: IssuePresentation {
                    created_at_ms: Some(CREATED_AT),
                    ..Default::default()
                },
                ..Default::default()
            },
        },
        &Validators::default(),
        UPDATED_AT,
    )?;
    Ok(
        json!({"issue_number": locator.number, "created_at_ms": CREATED_AT, "updated_at_ms": UPDATED_AT}),
    )
}

use super::storage::{access::*, *};
use std::num::NonZeroU64;

pub(super) fn id(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).unwrap()
}
pub(super) fn setup(db: &Store) -> (AccountId, RepositoryId, ItemId) {
    let a = db.ensure_account("github.com", 81).unwrap();
    let p = db
        .reserve_access_profile_credential(
            a,
            id(81),
            "reader",
            "work",
            CredentialKind::FineGrained,
            None,
        )
        .unwrap();
    db.install_current_credential(&p, || Ok::<_, ()>(()))
        .unwrap()
        .unwrap();
    db.activate_local_profile(a).unwrap();
    db.record_access_result(
        &p,
        p.kind,
        AccessState::Readable,
        DiscoveryState::Complete,
        1,
    )
    .unwrap();
    let r = db
        .observe_github_repository(
            &p,
            &RepositoryObservation {
                provider_id: id(200),
                owner: "work",
                name: "main",
                private: true,
                source: RepositoryObservationSource::Direct,
            },
            2,
        )
        .unwrap();
    db.select_github_repository(a, r, true).unwrap();
    let item = issue(db, a, r, 41);
    (a, r, item)
}
pub(super) fn issue(db: &Store, a: AccountId, r: RepositoryId, number: u64) -> ItemId {
    db.ingest(
        a,
        ItemLocator {
            repository: r,
            number,
        },
        ItemUpdate {
            representation: ItemRepresentation::Issue(IssueRecord {
                id: IssueApiId::new(id(1000 + number)),
                node_id: None,
                classification: IssueClassification::Issue,
            }),
            remote_updated_at: UtcMillis(10),
            title: Change::Value("old".into()),
            body: Change::Value("description".into()),
            closed: Change::Value(false),
        },
    )
    .unwrap()
}

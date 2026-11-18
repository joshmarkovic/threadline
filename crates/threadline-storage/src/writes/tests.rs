use super::*;
use crate as storage;
mod write_profile {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/write_profile.rs"
    ));
}

#[test]
fn it_009_disk_full_after_remote_success_rolls_back_receipt_cache_and_own_action_together() {
    let dir = tempfile::tempdir().unwrap();
    let db = Store::open_profile(dir.path()).unwrap();
    let (account, _, item) = write_profile::setup(&db);
    let operation = db.new_operation_id().unwrap();
    let text = "real sqlite full fixture".repeat(128 * 1024);
    let intent = WriteIntent::EditIssue(IssueChanges {
        title: Some(FieldEdit {
            base: "old".into(),
            wanted: "ours".into(),
        }),
        ..Default::default()
    });
    db.submit_write(
        account,
        WriteSubmission {
            id: &operation,
            item,
            intent: &intent,
            draft: None,
            dependencies: &[],
            submitted_at_ms: 10,
        },
    )
    .unwrap();
    let claim = db.claim_write(account, 11, 900).unwrap().unwrap();
    let current = WriteObservation {
        credential_generation: claim.route.credential_generation,
        actor: claim.route.actor,
        repository: claim.route.repository_id,
        issue: claim.route.issue_id,
        number: claim.route.number,
        pull_request: false,
        observed_at_ms: 12,
        permission: Permission::Allowed,
        fields: IssueValues {
            title: Some("old".into()),
            ..Default::default()
        },
        comment: None,
        choices: Default::default(),
    };
    let WriteAdmission::Send(ticket) = db.authorize_write(&claim, &current, 13).unwrap() else {
        panic!("expected authorization")
    };
    assert!(db.start_write_transport(&ticket, 13).unwrap());
    db.write(|tx| {
        let pages = tx.pragma_query_value(None, "page_count", |r| r.get::<_, i64>(0))?;
        tx.pragma_update(None, "max_page_count", pages)?;
        Ok(())
    })
    .unwrap();
    let result = ConfirmedWrite {
        actor: current.actor,
        repository: current.repository,
        issue: current.issue,
        number: current.number,
        object: ConfirmedObject::Issue {
            fields: IssueValues {
                title: Some("ours".into()),
                body: Some(Some(text)),
                closed: Some(false),
                ..Default::default()
            },
            updated_at_ms: 14,
            milestone_title: None,
        },
    };
    let error = db
        .confirm_write(
            account,
            &operation,
            claim.attempt(),
            ConfirmationProof::MutationResponse {
                request_id: "disk-1",
            },
            &result,
            15,
        )
        .unwrap_err();
    assert!(
        matches!(error,Error::Sql(rusqlite::Error::SqliteFailure(e,_)) if e.code==rusqlite::ErrorCode::DiskFull)
    );
    drop(db);
    let db = Store::open_profile(dir.path()).unwrap();
    assert!(db.receipts(account, &operation).unwrap().is_empty());
    assert!(db.own_write(account, &operation).unwrap().is_none());
    assert_eq!(
        db.reader_item_metadata(account, item)
            .unwrap()
            .unwrap()
            .title,
        crate::Change::Value("old".into())
    );
    assert_eq!(db.recover_expired(account, 901).unwrap(), 1);
    assert_eq!(
        db.operation(account, &operation).unwrap().unwrap().state,
        State::OutcomeUnknown
    );
    assert!(db.claim_write(account, 2000, 4000).unwrap().is_none());
    db.confirm_write(
        account,
        &operation,
        claim.attempt(),
        ConfirmationProof::MutationResponse {
            request_id: "disk-1",
        },
        &result,
        2001,
    )
    .unwrap();
    assert_eq!(
        db.own_write(account, &operation)
            .unwrap()
            .unwrap()
            .remote_id,
        claim.route.issue_id
    );
    assert_eq!(
        db.reader_item_metadata(account, item)
            .unwrap()
            .unwrap()
            .title,
        crate::Change::Value("ours".into())
    );
}

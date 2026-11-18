use std::{
    fs,
    path::Path,
    sync::{Arc, Barrier},
    thread,
};

use rusqlite::{Connection, params};
use tempfile::tempdir;
use threadline_storage::{
    AccountId, Change, DATABASE_FILENAME, Error, IssueApiId, IssueClassification, IssueRecord,
    ItemId, ItemKind, ItemLocator, ItemRepresentation, ItemUpdate, NodeId, PullRequestRecord,
    PullRequestUpdate, RepositoryId, SCHEMA_VERSION, Store, TransferEvidence, TransferOutcome,
    TransferProblem, UtcMillis,
    queue::{Pane, State, Tab, TabView, Workspace},
};

fn issue(id: u64, classification: IssueClassification, time: i64) -> ItemUpdate {
    ItemUpdate {
        representation: ItemRepresentation::Issue(IssueRecord {
            id: IssueApiId::try_from(id).unwrap(),
            node_id: Some(NodeId::new(format!("issue:{id}")).unwrap()),
            classification,
        }),
        remote_updated_at: UtcMillis(time),
        title: Change::Missing,
        body: Change::Missing,
        closed: Change::Missing,
    }
}

fn pr(id: u64, time: i64, details: PullRequestUpdate) -> ItemUpdate {
    ItemUpdate {
        representation: ItemRepresentation::PullRequest(PullRequestRecord {
            id: id.try_into().unwrap(),
            node_id: Some(NodeId::new(format!("pr:{id}")).unwrap()),
            details,
        }),
        remote_updated_at: UtcMillis(time),
        title: Change::Missing,
        body: Change::Missing,
        closed: Change::Missing,
    }
}

fn account(store: &Store, user: u64) -> AccountId {
    store.ensure_account("GitHub.COM", user).unwrap()
}

fn repo(store: &Store, account: AccountId, id: u64) -> RepositoryId {
    store
        .ensure_repository(account, id, "fixture", &format!("repo-{id}"))
        .unwrap()
}

fn locator(repository: RepositoryId, number: u64) -> ItemLocator {
    ItemLocator { repository, number }
}

fn raw(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    connection
}

#[test]
fn shared_number_namespace_and_pr_extension_are_enforced_by_sqlite() {
    let profile = tempdir().unwrap();
    let path = profile.path().join(DATABASE_FILENAME);
    let store = Store::open_profile(profile.path()).unwrap();
    let first = account(&store, 1);
    let second = account(&store, 2);
    let alpha = repo(&store, first, 6);
    let beta = repo(&store, first, 7);
    let private = repo(&store, second, 6);
    assert_eq!(store.ensure_account("github.com", 1).unwrap(), first);
    assert_ne!(store.ensure_account("example.com", 1).unwrap(), first);
    assert_eq!(
        store
            .ensure_repository(first, 6, "new-owner", "new-name")
            .unwrap(),
        alpha
    );
    assert!(matches!(
        store.ensure_repository(first, 9, "NEW-OWNER", "NEW-NAME"),
        Err(Error::Sql(_))
    ));

    let mut items = Vec::new();
    for (number, kind) in [
        (41, IssueClassification::Issue),
        (42, IssueClassification::PullRequest),
        (43, IssueClassification::Issue),
    ] {
        items.push(
            store
                .ingest(
                    first,
                    locator(alpha, number),
                    issue(100 + number, kind, 100),
                )
                .unwrap(),
        );
    }
    let change = pr(420, 110, PullRequestUpdate::default());
    assert_eq!(
        store.ingest(first, locator(alpha, 42), change).unwrap(),
        items[1]
    );
    let unrelated = store
        .ingest(
            first,
            locator(beta, 42),
            issue(7, IssueClassification::Issue, 100),
        )
        .unwrap();
    let second_account = store
        .ingest(
            second,
            locator(private, 42),
            issue(142, IssueClassification::Issue, 100),
        )
        .unwrap();
    assert_ne!(items[1], unrelated);
    assert_ne!(items[1], second_account);
    assert_eq!(store.item(first, second_account).unwrap(), None);
    assert_eq!(store.resolve(first, locator(private, 42)).unwrap(), None);
    assert!(matches!(
        store.ensure_placeholder(first, private, 42),
        Err(Error::NotFound)
    ));

    let connection = raw(&path);
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM items WHERE repository_id = ?1",
            [alpha],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 3);
    // Direct SQL changes cannot create a parallel item with a different kind.
    assert!(connection.execute(
        "INSERT INTO items (repository_id, account_id, number, kind) VALUES (?1, ?2, 42, 'issue')",
        params![alpha, first],
    ).is_err());
    assert!(connection.execute(
        "INSERT INTO pull_request_details (item_id, account_id, pull_request_api_id) VALUES (?1, ?2, '410')",
        params![items[0], first],
    ).is_err());
    assert!(
        connection
            .execute("UPDATE items SET kind = 'issue' WHERE id = ?1", [items[1]],)
            .is_err()
    );
}

#[test]
fn pr_first_and_reverse_preserve_provider_namespaces_and_fresh_fields() {
    let profile = tempdir().unwrap();
    let store = Store::open_profile(profile.path()).unwrap();
    let account = account(&store, 1);
    let repository = repo(&store, account, 6);
    for (number, details_first) in [(21, true), (22, false)] {
        let locator = locator(repository, number);
        // Equal numeric values across endpoint namespaces do not imply interchangeable IDs.
        let mut detail = pr(
            number,
            200,
            PullRequestUpdate {
                merged: Change::Value(true),
                base_oid: Change::Value("base".into()),
                head_oid: Change::Value("head".into()),
                merge_oid: Change::Null,
            },
        );
        detail.title = Change::Value("fresh title".into());
        let mut summary = issue(number, IssueClassification::PullRequest, 100);
        summary.title = Change::Value("old title".into());
        summary.body = Change::Value("available summary body".into());
        let (first, second) = if details_first {
            (detail, summary)
        } else {
            (summary, detail)
        };
        let id = store.ingest(account, locator, first).unwrap();
        let initial = store.item(account, id).unwrap().unwrap();
        if details_first {
            assert_eq!(initial.issue_id, None);
        } else {
            assert!(initial.pull_request.is_none());
        }
        assert_eq!(store.ingest(account, locator, second).unwrap(), id);

        let mut older = pr(
            number,
            150,
            PullRequestUpdate {
                merged: Change::Value(false),
                base_oid: Change::Null,
                ..Default::default()
            },
        );
        older.title = Change::Null;
        store.ingest(account, locator, older).unwrap();
        let mut tied = issue(number, IssueClassification::Unknown, 200);
        tied.title = Change::Value("ambiguous equal revision".into());
        store.ingest(account, locator, tied).unwrap();
        let mut newest = issue(number, IssueClassification::Issue, 300);
        newest.body = Change::Null;
        newest.closed = Change::Value(true);
        store.ingest(account, locator, newest).unwrap();
        let item = store.item(account, id).unwrap().unwrap();
        assert_eq!(item.kind, ItemKind::PullRequest);
        assert_eq!(item.title, Change::Value("fresh title".into()));
        assert_eq!(item.body, Change::Null);
        assert_eq!(item.closed, Change::Value(true));
        assert_eq!(item.issue_id.unwrap().get(), number);
        assert_eq!(
            item.issue_node_id.unwrap().as_str(),
            format!("issue:{number}")
        );
        let detail = item.pull_request.unwrap();
        assert_eq!(detail.id.get(), number);
        assert_eq!(detail.node_id.unwrap().as_str(), format!("pr:{number}"));
        assert_eq!(detail.fields.merged, Change::Value(true));
        assert_eq!(detail.fields.base_oid, Change::Value("base".into()));
        assert_eq!(detail.fields.head_oid, Change::Value("head".into()));
        assert_eq!(detail.fields.merge_oid, Change::Null);

        let rollback = store.ingest(
            account,
            locator,
            pr(1000 + number, 400, PullRequestUpdate::default()),
        );
        assert!(matches!(rollback, Err(Error::Conflict(_))));
        assert_eq!(
            store
                .item(account, id)
                .unwrap()
                .unwrap()
                .pull_request
                .unwrap()
                .id
                .get(),
            number
        );
    }
}

#[test]
fn separate_wal_writers_ingest_one_item_and_extension() {
    let profile = tempdir().unwrap();
    let first = Store::open_profile(profile.path()).unwrap();
    let second = Store::open_profile(profile.path()).unwrap();
    let account = account(&first, 1);
    let repository = repo(&first, account, 6);
    let locator = locator(repository, 42);
    let rendezvous = Arc::new(Barrier::new(2));

    let (issue_id, pr_id) = thread::scope(|scope| {
        let issue_writer = scope.spawn(|| {
            rendezvous.wait();
            first
                .ingest(
                    account,
                    locator,
                    issue(900, IssueClassification::PullRequest, 100),
                )
                .unwrap()
        });
        let pr_writer = scope.spawn(|| {
            rendezvous.wait();
            second
                .ingest(account, locator, pr(901, 200, PullRequestUpdate::default()))
                .unwrap()
        });
        (issue_writer.join().unwrap(), pr_writer.join().unwrap())
    });
    assert_eq!(issue_id, pr_id);

    let item = first.item(account, issue_id).unwrap().unwrap();
    assert_eq!(item.issue_id.unwrap().get(), 900);
    assert_eq!(item.pull_request.unwrap().id.get(), 901);
    let connection = raw(&profile.path().join(DATABASE_FILENAME));
    let counts: (i64, i64) = connection
        .query_row(
            "SELECT (SELECT count(*) FROM items), (SELECT count(*) FROM pull_request_details)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (1, 1));
}

#[test]
fn transfers_require_matching_identity_and_keep_previous_locator() {
    let profile = tempdir().unwrap();
    let store = Store::open_profile(profile.path()).unwrap();
    let account = account(&store, 1);
    let origin = locator(repo(&store, account, 10), 42);
    let destination = locator(repo(&store, account, 20), 77);
    let occupied_destination = locator(destination.repository, 42);
    let item = store
        .ingest(account, origin, issue(100, IssueClassification::Issue, 100))
        .unwrap();
    let blocker = store
        .ingest(
            account,
            occupied_destination,
            issue(101, IssueClassification::Issue, 100),
        )
        .unwrap();
    let proof = TransferEvidence::VerifiedIssueNode(NodeId::new("issue:100").unwrap());
    assert_eq!(
        store
            .transfer_issue(account, origin, destination, TransferEvidence::Unverified)
            .unwrap(),
        TransferOutcome::Unresolved(TransferProblem::Unverified)
    );
    assert_eq!(
        store
            .transfer_issue(
                account,
                origin,
                destination,
                TransferEvidence::VerifiedIssueId(7.try_into().unwrap())
            )
            .unwrap(),
        TransferOutcome::Unresolved(TransferProblem::IdentityMismatch)
    );
    assert_eq!(
        store
            .transfer_issue(account, origin, occupied_destination, proof.clone())
            .unwrap(),
        TransferOutcome::Unresolved(TransferProblem::DestinationOccupied)
    );
    assert_eq!(
        store.resolve(account, occupied_destination).unwrap(),
        Some(blocker)
    );
    assert_eq!(
        store
            .transfer_issue(account, origin, destination, proof.clone())
            .unwrap(),
        TransferOutcome::Moved(item)
    );
    assert_eq!(
        store
            .transfer_issue(account, origin, destination, proof.clone())
            .unwrap(),
        TransferOutcome::AlreadyCurrent(item)
    );
    assert_eq!(
        store.item(account, item).unwrap().unwrap().locator,
        destination
    );
    assert_eq!(store.resolve(account, origin).unwrap(), Some(item));
    assert!(matches!(
        store.ingest(account, origin, issue(103, IssueClassification::Issue, 200)),
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        store
            .transfer_issue(account, destination, origin, proof)
            .unwrap(),
        TransferOutcome::Moved(item)
    );
    assert_eq!(store.resolve(account, destination).unwrap(), Some(item));
    assert_eq!(store.item(account, item).unwrap().unwrap().locator, origin);
}

#[test]
fn real_wal_snapshot_search_cleanup_and_redacted_errors() {
    let profile = tempdir().unwrap();
    let store = Store::open_profile(profile.path()).unwrap();
    let first = account(&store, 1);
    let second = account(&store, 2);
    let old = locator(repo(&store, first, 2), 1);
    let private = locator(repo(&store, second, 2), 1);
    let mut update = issue(3, IssueClassification::Issue, 100);
    update.body = Change::Value("test content alpha".into());
    let visible = store.ingest(first, old, update.clone()).unwrap();
    let hidden = store.ingest(second, private, update).unwrap();
    assert_eq!(
        store.search(first, "test content", 10).unwrap(),
        vec![visible]
    );
    assert_eq!(
        store.search(second, "test content", 10).unwrap(),
        vec![hidden]
    );
    assert!(store.search(first, "\"\" OR *", 10).unwrap().is_empty());

    let path = profile.path().join(DATABASE_FILENAME);
    let mut reader = raw(&path);
    let mode: String = reader
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let snapshot = reader.transaction().unwrap();
    let read_body = |connection: &Connection| -> Option<String> {
        connection
            .query_row("SELECT body FROM items WHERE id = ?1", [visible], |row| {
                row.get(0)
            })
            .unwrap()
    };
    assert_eq!(read_body(&snapshot).as_deref(), Some("test content alpha"));
    let mut update = issue(3, IssueClassification::Issue, 200);
    update.body = Change::Value("wal update".into());
    store.ingest(first, old, update).unwrap();
    assert_eq!(read_body(&snapshot).as_deref(), Some("test content alpha"));
    snapshot.commit().unwrap();
    assert_eq!(read_body(&reader).as_deref(), Some("wal update"));

    reader
        .execute(
            "INSERT INTO item_search (item_search) VALUES ('delete-all')",
            [],
        )
        .unwrap();
    assert!(store.search(first, "wal update", 5).unwrap().is_empty());
    store.repair_search_index().unwrap();
    assert_eq!(store.search(first, "wal update", 5).unwrap(), vec![visible]);
    store.check_integrity().unwrap();

    store.clear_cache(first).unwrap();
    assert_eq!(
        store.item(first, visible).unwrap().unwrap().body,
        Change::Missing
    );
    assert_eq!(store.resolve(first, old).unwrap(), Some(visible));
    assert!(store.search(first, "wal update", 5).unwrap().is_empty());
    assert_eq!(
        store.search(second, "test content", 10).unwrap(),
        vec![hidden]
    );

    let secret = "synthetic-private-profile-path";
    let error = Error::from(std::io::Error::other(secret));
    assert!(!format!("{error} {error:?}").contains(secret));
    assert!(std::error::Error::source(&error).is_none());
}

#[test]
fn corruption_and_incompatible_or_foreign_databases_are_preserved() {
    let profile = tempdir().unwrap();
    let path = profile.path().join(DATABASE_FILENAME);
    fs::write(&path, b"not a SQLite database").unwrap();
    assert!(Store::open(&path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"not a SQLite database");

    let foreign = profile.path().join("foreign.sqlite3");
    raw(&foreign)
        .execute("CREATE TABLE important_content (value TEXT)", [])
        .unwrap();
    assert!(matches!(Store::open(&foreign), Err(Error::ForeignDatabase)));
    raw(&foreign)
        .execute("INSERT INTO important_content VALUES ('still here')", [])
        .unwrap();

    let unsupported = profile.path().join("unsupported.sqlite3");
    drop(Store::open(&unsupported).unwrap());
    raw(&unsupported)
        .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
        .unwrap();
    assert!(
        matches!(Store::open(&unsupported), Err(Error::IncompatibleSchema { found }) if found == SCHEMA_VERSION + 1)
    );
    let found: u32 = raw(&unsupported)
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(found, SCHEMA_VERSION + 1);
}

const UNKNOWN_OP: &str = "10000000-0000-4000-8000-000000000001";
const QUEUED_OP: &str = "10000000-0000-4000-8000-000000000002";

fn previous_workspace(item: ItemId) -> Workspace {
    Workspace {
        focused_pane: 0,
        proportions: vec![0.4, 0.6],
        panes: vec![
            Pane {
                active: Some(0),
                tabs: vec![Tab {
                    id: "work-91".into(),
                    item,
                    view: TabView {
                        scroll_offset: 17,
                        scroll_anchor: Some("comment-4".into()),
                        navigation: Some("conversation".into()),
                        draft: Some("edit-1".into()),
                        context: None,
                    },
                }],
            },
            Pane {
                active: None,
                tabs: Vec::new(),
            },
        ],
    }
}

// Build the prior format directly so this fixture cannot silently start at v2.
fn create_v1(path: &Path) -> (ItemId, RepositoryId, RepositoryId) {
    let mut connection = raw(path);
    let tx = connection.transaction().unwrap();
    tx.execute_batch(include_str!("../src/identity.sql"))
        .unwrap();
    tx.execute_batch(include_str!("../src/resources.sql"))
        .unwrap();
    tx.execute_batch(include_str!("../src/queue.sql")).unwrap();
    tx.pragma_update(None, "application_id", 0x5448_5244)
        .unwrap();
    tx.pragma_update(None, "user_version", 1).unwrap();
    tx.execute_batch(
        "INSERT INTO accounts (id, host, provider_user_id) VALUES (1, 'github.com', '1');
         INSERT INTO repositories (id, account_id, provider_repository_id, owner, name) VALUES
             (1, 1, '123', 'fixture', 'old'), (2, 1, '456', 'fixture', 'new');
         INSERT INTO items (id, repository_id, account_id, number, kind, issue_api_id, title, title_at)
             VALUES (91, 1, 1, 42, 'issue', '174', 'existing indexable title', 100);
         INSERT INTO drafts (account_id, id, item_id, kind, body, base, anchor, revision)
             VALUES (1, 'edit-1', 91, 'issue_edit', 'authored', 'original', 'edit-anchor', 2);
         INSERT INTO dispatch_profiles (account_id, authorized) VALUES (1, 1);
         INSERT INTO active_profile (singleton, account_id) VALUES (1, 1);
         INSERT INTO cached_resources (id, account_id, item_id, family, coverage, fetch, continuation, has_page)
             VALUES (1, 1, 91, 'comments', 1, 1, 'page-two', 1);
         INSERT INTO cached_resource_records (resource_id, record_key, body)
             VALUES (1, 'first', 'cached first page');
         INSERT INTO cached_resource_pending (resource_id, record_key, body)
             VALUES (1, 'first', 'cached first page');",
    ).unwrap();
    let item = ItemId(91);
    tx.execute(
        "INSERT INTO workspaces (account_id, layout) VALUES (1, ?1)",
        [serde_json::to_string(&previous_workspace(item)).unwrap()],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO operations
            (account_id, id, original_repository, original_number, item_id, kind, payload_version, snapshot, projection, state, cancel_requested)
         VALUES
            (1, ?1, 1, 42, ?3, 'comment', 2, '{\"payload\":\"sent text\",\"base\":\"old\",\"anchor\":\"base\",\"draft\":null}', 'pending', 'unknown', 1),
            (1, ?2, 1, 42, ?3, 'issue_edit', 1, '{\"text\":\"unsent text\",\"base_revision\":\"old\",\"anchor\":\"base\",\"draft\":null}', 'queued', 'queued', 0)",
        params![UNKNOWN_OP, QUEUED_OP, item],
    ).unwrap();
    tx.execute(
        "INSERT INTO step_receipts (account_id, operation_id, step, object_kind, remote_id, evidence)
             VALUES (1, ?1, 'create', 'comment', '9001', 'confirmed synthetic step')",
        [UNKNOWN_OP],
    ).unwrap();
    tx.commit().unwrap();
    (item, RepositoryId(1), RepositoryId(2))
}

fn assert_previous_state(connection: &Connection, item: ItemId, restored: bool) {
    let preserved: (bool, bool, bool, bool, bool) = connection
        .query_row(
            "SELECT
               EXISTS(SELECT 1 FROM drafts WHERE item_id = ?1 AND body = 'authored'
                      AND base = 'original' AND anchor = 'edit-anchor' AND revision = 2),
               EXISTS(SELECT 1 FROM workspaces WHERE account_id = 1 AND layout = ?4),
               EXISTS(SELECT 1 FROM operations WHERE item_id = ?1 AND id = ?2
                      AND state = 'unknown' AND cancel_requested = 1 AND original_repository = 1 AND original_number = 42
                      AND json_extract(snapshot, '$.payload') = 'sent text'),
               EXISTS(SELECT 1 FROM operations WHERE item_id = ?1 AND id = ?3
                      AND state = ?5 AND payload_version = 1 AND json_extract(snapshot, '$.text') = 'unsent text'),
               EXISTS(SELECT 1 FROM step_receipts WHERE operation_id = ?2 AND remote_id = '9001')",
            params![item, UNKNOWN_OP, QUEUED_OP, serde_json::to_string(&previous_workspace(item)).unwrap(),
                if restored { "unknown" } else { "queued" }],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .unwrap();
    assert_eq!(preserved, (true, true, true, true, true));
}

fn assert_typed_previous_state(store: &Store, item: ItemId, restored: bool) {
    let account = AccountId(1);
    assert_eq!(
        store.workspace(account).unwrap(),
        Some(previous_workspace(item))
    );
    let unknown = store.operation(account, UNKNOWN_OP).unwrap().unwrap();
    assert_eq!(unknown.id, UNKNOWN_OP);
    assert_eq!(unknown.item, item);
    assert_eq!(unknown.original_repository, RepositoryId(1));
    assert_eq!(unknown.original_number, 42);
    assert_eq!(unknown.state, State::OutcomeUnknown);
    assert!(unknown.cancel_requested);
    assert_eq!(unknown.snapshot.payload, "sent text");
    let pending = store.operation(account, QUEUED_OP).unwrap().unwrap();
    assert_eq!(pending.id, QUEUED_OP);
    assert_eq!(pending.item, item);
    assert_eq!(pending.original_number, 42);
    if restored {
        assert_eq!(pending.state, State::OutcomeUnknown);
        assert_eq!(pending.reason.as_deref(), Some("restored_backup"));
    } else {
        assert_eq!(pending.state, State::Queued);
    }
    assert_eq!(pending.snapshot.payload, "unsent text");
    assert_eq!(pending.snapshot.base, "old");
    assert_eq!(
        store.receipts(account, UNKNOWN_OP).unwrap()[0].remote_id,
        "9001"
    );
}

fn assert_checkpoint(connection: &Connection) {
    let progress: (String, String) = connection
        .query_row(
            "SELECT continuation, body FROM cached_resources
             JOIN cached_resource_records ON cached_resource_records.resource_id = cached_resources.id
             WHERE cached_resources.id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(progress, ("page-two".into(), "cached first page".into()));
}

#[test]
fn prior_profile_upgrade_backup_restore_transfer_and_cleanup_preserve_durable_state() {
    let profile = tempdir().unwrap();
    let path = profile.path().join(DATABASE_FILENAME);
    let (item, original_repository, next_repository) = create_v1(&path);
    let store = Store::open_profile(profile.path()).unwrap();
    let account = AccountId(1);
    assert_eq!(
        store.search(account, "indexable title", 10).unwrap(),
        vec![item]
    );
    let live = raw(&path);
    assert_previous_state(&live, item, false);
    assert_typed_previous_state(&store, item, false);
    assert_checkpoint(&live);
    let version: u32 = live
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, SCHEMA_VERSION);

    let migration_backup = store.migration_backup().unwrap();
    let before_upgrade = raw(migration_backup);
    let version: u32 = before_upgrade
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    assert_previous_state(&before_upgrade, item, false);
    assert_checkpoint(&before_upgrade);

    let original = locator(original_repository, 42);
    let destination = locator(next_repository, 77);
    assert_eq!(
        store
            .transfer_issue(
                account,
                original,
                destination,
                TransferEvidence::VerifiedIssueId(174.try_into().unwrap())
            )
            .unwrap(),
        TransferOutcome::Moved(item)
    );
    assert_previous_state(&live, item, false);
    store.clear_cache(account).unwrap();
    assert_previous_state(&live, item, false);
    assert_typed_previous_state(&store, item, false);
    assert_eq!(store.resolve(account, original).unwrap(), Some(item));
    assert_eq!(
        store.item(account, item).unwrap().unwrap().locator,
        destination
    );
    assert!(
        store
            .search(account, "indexable title", 10)
            .unwrap()
            .is_empty()
    );

    // The backup must include the live transfer committed to WAL.
    let snapshot_path = profile.path().join("live-snapshot.sqlite3");
    store.backup_to(&snapshot_path).unwrap();
    assert!(store.backup_to(&snapshot_path).is_err());
    let snapshot = raw(&snapshot_path);
    assert_previous_state(&snapshot, item, false);
    let number: i64 = snapshot
        .query_row("SELECT number FROM items WHERE id = ?1", [item], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(number, 77);

    let restored = profile.path().join("recovered.sqlite3");
    Store::recover_to(&snapshot_path, &restored).unwrap();
    let db = raw(&restored);
    assert_previous_state(&db, item, true);
    let resume_hold: String = db
        .query_row(
            "SELECT hold FROM operation_gates WHERE id = ?1",
            [QUEUED_OP],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(resume_hold, "restored_backup");
    let existing_hold: bool = live
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM restored_profiles WHERE account_id = 1)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!existing_hold);
    assert_typed_previous_state(&Store::open(&restored).unwrap(), item, true);

    // Older snapshots can also be recovered without replacing the original.
    let recovered_v1 = profile.path().join("recovered-v1.sqlite3");
    Store::recover_to(migration_backup, &recovered_v1).unwrap();
    let reopened = Store::open(&recovered_v1).unwrap();
    assert_eq!(
        reopened.search(account, "indexable title", 5).unwrap(),
        vec![item]
    );
    let recovered = raw(&recovered_v1);
    assert_previous_state(&recovered, item, true);
    assert_typed_previous_state(&reopened, item, true);
    assert_checkpoint(&recovered);
}

#[test]
fn failed_upgrade_rolls_back_changes_and_preserves_prior_profile() {
    let profile = tempdir().unwrap();
    let path = profile.path().join(DATABASE_FILENAME);
    let (item, _, _) = create_v1(&path);
    let db = raw(&path);
    db.execute("CREATE TABLE item_search (problem TEXT)", [])
        .unwrap();
    assert!(Store::open(&path).is_err());
    let version: u32 = db
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    assert_previous_state(&db, item, false);
    assert_checkpoint(&db);
    let partial_trigger_count: i64 = db.query_row(
        "SELECT count(*) FROM sqlite_schema WHERE type = 'trigger' AND name LIKE 'item_search_%'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(partial_trigger_count, 0);
    db.execute("DROP TABLE item_search", []).unwrap();
    let upgraded = Store::open(&path).unwrap();
    assert_previous_state(&db, item, false);
    assert!(upgraded.migration_backup().is_some());
}

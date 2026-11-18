//! Create an empty-profile fixture for the installed activity adapter, or inspect its durable state.
use std::{error::Error, fs, io::Write, num::NonZeroU64, path::Path};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{Value, json};
use threadline_storage::{
    AccountId, Change, IssueApiId, IssueClassification, IssueRecord, ItemId, ItemLocator,
    ItemRepresentation, ItemUpdate, RepositoryId, Store, UtcMillis,
    access::{
        AccessState, CredentialKind, DiscoveryState, RepositoryObservation,
        RepositoryObservationSource,
    },
    notifications::{Category, DeliveryMode, EventFacts, NotificationTarget, ObservedNotification},
    queue::{DraftContent, DraftKind, Pane, Tab, TabView, Workspace},
};

const MARKER: &str = "threadline-s11-installed-fixture-v1";
const RUNTIME_MARKER: &str = "threadline-s12-installed-runtime-v1";
const DRAFT: &str = "threadline-s11-draft";
const DRAFT_BODY: &str = "Unsubmitted local fixture draft";
const OWNER: &str = "threadline-activity-fixture";
const COUNT: u32 = 37;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [mode, path] = args.as_slice() else {
        return Err("Use seed or inspect and an absolute fixture directory.".into());
    };
    let root = Path::new(path);
    if !root.is_absolute() || !root.is_dir() || fs::symlink_metadata(root)?.is_symlink() {
        return Err("Supply an existing absolute fixture directory.".into());
    }
    let result = match mode.to_str() {
        Some("seed") => seed(root, false)?,
        Some("seed-runtime") => seed(root, true)?,
        Some("inspect-runtime") => inspect_runtime(root)?,
        Some("inspect") => inspect(root)?,
        _ => return Err("Unknown activity fixture command.".into()),
    };
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn number(value: u64) -> NonZeroU64 {
    NonZeroU64::new(value).expect("fixture constants are positive")
}

fn issue(
    db: &Store,
    account: AccountId,
    repository: RepositoryId,
    value: u64,
    body: String,
) -> threadline_storage::Result<ItemId> {
    db.ingest(
        account,
        ItemLocator {
            repository,
            number: value,
        },
        ItemUpdate {
            representation: ItemRepresentation::Issue(IssueRecord {
                id: IssueApiId::new(number(8_000 + value)),
                node_id: None,
                classification: IssueClassification::Issue,
            }),
            remote_updated_at: UtcMillis(10),
            title: Change::Value(format!("Activity fixture issue {value}")),
            body: Change::Value(body),
            closed: Change::Value(false),
        },
    )
}

fn seed(root: &Path, runtime: bool) -> Result<Value, Box<dyn Error>> {
    if fs::read_dir(root)?.next().is_some() {
        return Err("The activity fixture requires an empty directory.".into());
    }
    let marker_id = if runtime { RUNTIME_MARKER } else { MARKER };
    let mut marker = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join(marker_id))?;
    marker.write_all(marker_id.as_bytes())?;
    marker.sync_all()?;
    let store = Store::open_profile(root)?;
    let account = store.ensure_account("github.com", 817_123_456)?;
    let p = store.reserve_access_profile_credential(
        account,
        number(817_123_456),
        "fixture-reader",
        OWNER,
        CredentialKind::FineGrained,
        None,
    )?;
    // The fixture needs a local access snapshot. It never creates a credential or enrolls an OS account.
    store
        .install_current_credential(&p, || Ok::<_, ()>(()))?
        .map_err(|_| "fixture credential metadata failed")?;
    store.activate_local_profile(account)?;
    let now = UtcMillis::now()?.0;
    store.record_access_result(
        &p,
        p.kind,
        AccessState::Readable,
        DiscoveryState::Complete,
        now,
    )?;
    let repo = store.observe_github_repository(
        &p,
        &RepositoryObservation {
            provider_id: number(917_123_456),
            owner: OWNER,
            name: "installed-alerts",
            private: true,
            source: RepositoryObservationSource::Direct,
        },
        now,
    )?;
    store.select_github_repository(account, repo, true)?;
    let p = store
        .github_access_profile(account, OWNER)?
        .ok_or("Missing fixture access profile")?;
    store.update_sync_settings(account, |s| {
        s.controls.pause_background = true;
        s.controls.pause_queued_actions = true;
        s.controls.work_offline = runtime;
    })?;
    if runtime {
        seed_runtime(&store, account, repo)?;
        return inspect_runtime(root);
    }
    let original = issue(
        &store,
        account,
        repo,
        41,
        "Downloaded activity fixture issue 41.".into(),
    )?;
    let destination = issue(
        &store,
        account,
        repo,
        42,
        "Downloaded activity fixture issue 42.".into(),
    )?;
    store.create_draft(
        account,
        DRAFT,
        original,
        DraftKind::Comment,
        &DraftContent {
            body: DRAFT_BODY.into(),
            base: String::new(),
            anchor: String::new(),
        },
    )?;
    store.save_workspace(
        account,
        &Workspace {
            focused_pane: 0,
            proportions: vec![1.0],
            panes: vec![Pane {
                active: Some(0),
                tabs: vec![Tab {
                    id: "5b86b9f9-747b-4af3-abae-8a752f9c5af1".into(),
                    item: original,
                    view: TabView {
                        draft: Some(DRAFT.into()),
                        ..Default::default()
                    },
                }],
            }],
        },
    )?;
    store.update_notification_settings(account, |s| s.fallback = DeliveryMode::Popup)?;
    store.enroll_notification_stream(&p, repo, "installed-fixture", now - 10)?;
    let facts = EventFacts {
        target: NotificationTarget::Item {
            item: destination,
            thread: None,
        },
        categories: [Category::DirectMention].into(),
        actor: Default::default(),
        involves_me: true,
        receipt: None,
    };
    for i in 0..COUNT {
        if !store.observe_notification_change(
            &p,
            ObservedNotification {
                repository: repo,
                stream: "installed-fixture",
                key: &format!("fixture-{i}"),
                kind: "mention",
                occurred_at_ms: now - 1,
                received_at_ms: now,
                revision: None,
                facts: &facts,
            },
        )? {
            return Err("Fixture event was not recorded.".into());
        }
    }
    drop(store);
    inspect(root)
}

fn inspect(root: &Path) -> Result<Value, Box<dyn Error>> {
    if fs::read(root.join(MARKER))? != MARKER.as_bytes() {
        return Err("Unrecognized activity fixture.".into());
    }
    let mut connection = Connection::open_with_flags(
        root.join("threadline.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    connection.busy_timeout(std::time::Duration::from_secs(3))?;
    let db = connection.transaction()?;
    let owned: bool = db.query_row(
        "SELECT (SELECT count(*) FROM accounts) = 1 AND EXISTS (
        SELECT 1 FROM github_access_profiles WHERE owner_scope=? AND login='fixture-reader')",
        [OWNER],
        |r| r.get(0),
    )?;
    if !owned {
        return Err("The requested database is outside the synthetic fixture.".into());
    }
    let count = |sql: &str| db.query_row(sql, [], |r| r.get::<_, u32>(0));
    let layout_raw: String = db.query_row("SELECT layout FROM workspaces", [], |r| r.get(0))?;
    let layout: Workspace = serde_json::from_str(&layout_raw)?;
    let focus = &layout.panes[layout.focused_pane];
    let selected = focus.active.map(|index| focus.tabs[index].item);
    let number: Option<i64> = db
        .query_row("SELECT number FROM items WHERE id=?", [selected], |r| {
            r.get(0)
        })
        .optional()?;
    let preserved: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM drafts d JOIN items i ON i.id=d.item_id
        WHERE d.id=? AND d.body=? AND i.number=41)",
        [DRAFT, DRAFT_BODY],
        |r| r.get(0),
    )?;
    let group: Option<(String,String,bool)> = db.query_row("SELECT native_id,state,activated_at IS NOT NULL FROM native_notification_groups ORDER BY id LIMIT 1",
        [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    Ok(
        json!({"fixture":MARKER,"event_count":count("SELECT count(*) FROM notification_events")?,
        "feed_count":count("SELECT count(*) FROM notification_events WHERE published=1")?,
        "latest_feed_id": db.query_row("SELECT MAX(id) FROM notification_events WHERE published=1", [], |row| row.get::<_,Option<i64>>(0))?,
        "os_feed_only":count("SELECT count(*) FROM notification_events n WHERE n.published=1
            AND json_extract(n.decision,'$.mode')='FeedOnly' AND EXISTS(SELECT 1 FROM json_each(n.decision,'$.reasons') WHERE value='OperatingSystem')")?,
        "registration_feed_only":count("SELECT count(*) FROM notification_events n WHERE n.published=1
            AND json_extract(n.decision,'$.mode')='FeedOnly' AND EXISTS(SELECT 1 FROM json_each(n.decision,'$.reasons') WHERE value='RegistrationMissing')")?,
        "adapter_feed_only":count("SELECT count(*) FROM notification_events n WHERE n.published=1
            AND json_extract(n.decision,'$.mode')='FeedOnly' AND EXISTS(SELECT 1 FROM json_each(n.decision,'$.reasons') WHERE value='AdapterUnavailable')")?,
        "snoozed_targets":count("SELECT count(*) FROM notification_targets WHERE snooze_until>unixepoch('now')*1000")?,
        "read_count":count("SELECT count(*) FROM notification_events WHERE read_at IS NOT NULL")?,
        "group_count":count("SELECT count(*) FROM native_notification_groups")?,
        "native_id":group.as_ref().map(|g| &g.0),"outcome":group.as_ref().map(|g| &g.1),
        "activated":group.is_some_and(|g|g.2),"pending_opens":count("SELECT count(*) FROM pending_notification_opens")?,
        "selected_issue":number,"draft_preserved":preserved,
        "draft_tab_preserved": layout.panes.iter().flat_map(|p| &p.tabs).any(|t|t.id=="5b86b9f9-747b-4af3-abae-8a752f9c5af1"),
        "source_events":COUNT}),
    )
}

fn seed_runtime(
    store: &Store,
    account: AccountId,
    repository: RepositoryId,
) -> Result<(), Box<dyn Error>> {
    use threadline_storage::pr::{PatchEvidence, PrFamily, PrHeader, PrInputPage, PrRecord};
    use threadline_storage::resources::{CommitOid, WalkMode};

    let issue = issue(store, account, repository, 41,
        "## Offline Markdown\n\n**Native attachment**\n\n![Synthetic offline image](https://github.com/user-attachments/assets/threadline-runtime-fixture)".into())?;
    let pr = store.ingest(
        account,
        ItemLocator {
            repository,
            number: 70,
        },
        ItemUpdate {
            representation: ItemRepresentation::Issue(IssueRecord {
                id: IssueApiId::new(number(8_070)),
                node_id: None,
                classification: IssueClassification::PullRequest,
            }),
            remote_updated_at: UtcMillis(12),
            title: Change::Value("Runtime fixture pull".into()),
            body: Change::Value("## Native patch comparison".into()),
            closed: Change::Value(false),
        },
    )?;
    let base = CommitOid::parse(&"a".repeat(40))?;
    let head = CommitOid::parse(&"b".repeat(40))?;
    let revision = store.record_pr_revision(
        account,
        pr,
        &base,
        &head,
        Some(&base),
        &PrHeader::default(),
        12,
    )?;
    let walk = store.start_pr_fetch(&revision, PrFamily::Files, WalkMode::Full)?;
    let files = ["first.txt", "second.txt"]
        .into_iter()
        .map(|path| PrRecord::File {
            path: path.into(),
            previous_path: None,
            status: "modified".into(),
            additions: Some(1),
            deletions: Some(1),
            patch: PatchEvidence::Text("@@ -1 +1 @@\n-old fixture line\n+new fixture line".into()),
        })
        .collect();
    store.commit_pr_page(
        &revision,
        PrFamily::Files,
        &walk,
        PrInputPage {
            records: files,
            next_cursor: None,
            partial: None,
            validators: None,
            fetched_at_ms: 13,
        },
    )?;
    let tabs = [
        ("4136d749-6aae-4481-8cec-60e7968ed541", issue),
        ("4136d749-6aae-4481-8cec-60e7968ed570", pr),
    ]
    .into_iter()
    .map(|(id, item)| Tab {
        id: id.into(),
        item,
        view: TabView::default(),
    })
    .collect();
    store.save_workspace(
        account,
        &Workspace {
            focused_pane: 0,
            proportions: vec![1.0],
            panes: vec![Pane {
                active: Some(0),
                tabs,
            }],
        },
    )?;
    Ok(())
}

fn inspect_runtime(root: &Path) -> Result<Value, Box<dyn Error>> {
    if fs::read(root.join(RUNTIME_MARKER))? != RUNTIME_MARKER.as_bytes() {
        return Err("The runtime fixture ownership marker did not match.".into());
    }
    let connection = Connection::open_with_flags(
        root.join("threadline.sqlite3"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    connection.busy_timeout(std::time::Duration::from_secs(3))?;
    let (raw, offline): (String,bool) = connection.query_row(
        "SELECT w.layout, d.offline FROM workspaces w JOIN dispatch_profiles d ON d.account_id=w.account_id
         JOIN github_access_profiles p ON p.account_id=w.account_id WHERE p.owner_scope=?",
        [OWNER], |r| Ok((r.get(0)?,r.get(1)?)),
    )?;
    let layout: Workspace = serde_json::from_str(&raw)?;
    let pane = layout.panes.first().ok_or("Missing fixture pane")?;
    let pr = pane.tabs.get(1).and_then(|t| t.view.pr_reader.as_ref());
    Ok(
        json!({"fixture":RUNTIME_MARKER,"offline":offline,"tabs":pane.tabs.len(),"active":pane.active,
        "path":pr.and_then(|v|v.selected_path.as_deref()),"mode":pr.map(|v|v.diff_mode)}),
    )
}

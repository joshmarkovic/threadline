use rusqlite::params;

use crate::{
    AccountId, Error, ItemId, RepositoryId, Result, Store,
    queue::{
        DraftContent, DraftKind, Pane, PrContext, ReferenceCandidate, Tab, TabView, Workspace,
        load_fixture_outbox,
    },
    resources::{
        CommitOid, ComparisonFile, ComparisonKey, ComparisonMode, GraphCommit, GraphRef,
        ResourceFamily, ResourceKey, ResourceTarget, ensure_resource,
    },
};

mod backlog;
pub use backlog::*;
mod lifecycle;
pub use lifecycle::*;

const FIXTURE_TIME_MS: i64 = 1_700_000_000_000;
const BATCH: usize = 250;
pub const REVISION: &str = include_str!("../../../fixtures/storage-revision.txt").trim_ascii();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scale {
    Small,
    Section16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Fixture {
    pub revision: &'static str,
    pub account: AccountId,
    pub repository: RepositoryId,
    pub long_discussion: ItemId,
    pub reviewed_pr: ItemId,
    pub comparison: ComparisonKey,
    pub head: CommitOid,
    pub item_count: usize,
    pub comment_count: usize,
    pub open_tabs: usize,
    pub outbox_count: usize,
    pub typed_outbox_count: usize,
}

impl Scale {
    fn workload(self) -> (usize, usize, usize, usize) {
        match self {
            Self::Small => (30, 3, 6, 12),
            Self::Section16 => (50_000, 5, 40, 400),
        }
    }
}

/// Populate a fresh synthetic database. Fixture writes use bounded transactions.
/// Callers own the temporary database and can remove it if generation fails.
pub fn populate(store: &Store, seed: u64, scale: Scale) -> Result<Fixture> {
    populate_profile(store, seed, scale, Profile::Data)
}

/// Build a signed-out synthetic GitHub profile for the actual offline reader.
/// The selected repositories contain only generated data. No token is installed.
pub fn populate_reader(store: &Store, seed: u64, scale: Scale) -> Result<Fixture> {
    populate_profile(store, seed, scale, Profile::Reader)
}

pub fn populate_large_workspace(store: &Store, seed: u64) -> Result<Fixture> {
    populate_profile(store, seed, Scale::Section16, Profile::Backlog)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Profile {
    Data,
    Reader,
    Backlog,
    Lifecycle,
}

impl Profile {
    fn uses_loopback_identity(self) -> bool {
        matches!(self, Self::Backlog | Self::Lifecycle)
    }
}

fn populate_profile(store: &Store, seed: u64, scale: Scale, profile: Profile) -> Result<Fixture> {
    let reader = profile != Profile::Data;
    let backlog = profile == Profile::Backlog;
    let loopback = profile.uses_loopback_identity();
    let empty: bool = store.read(|db| {
        Ok(
            db.query_row("SELECT NOT EXISTS(SELECT 1 FROM accounts)", [], |r| {
                r.get(0)
            })?,
        )
    })?;
    if !empty {
        return Err(Error::Conflict(
            "fixture generation needs a new empty profile",
        ));
    }

    let account = store.ensure_account(
        if reader {
            "github.com"
        } else {
            "fixtures.invalid"
        },
        if loopback { BACKLOG_USER } else { seed.max(1) },
    )?;
    let (provider_repository, owner, name) = if loopback {
        (BACKLOG_REPOSITORY, BACKLOG_OWNER, BACKLOG_REPO_NAME)
    } else {
        (1, "fixture", "relay")
    };
    let repository = store.ensure_repository(account, provider_repository, owner, name)?;
    let (item_count, comments_per_item, files, history_count) = scale.workload();
    let long_extra = if scale == Scale::Section16 { 5_000 } else { 10 };
    let root_oid = fixture_oid(seed, 0)?;
    let head = fixture_oid(seed, history_count - 1)?;
    let mut tabs = Vec::with_capacity(20);
    let mut issue_targets = Vec::with_capacity(30);
    let mut reviewed_pr = None;
    let mut alternate_pr = None;
    let mut long_discussion = None;

    for first in (0..item_count).step_by(BATCH) {
        let batch = store.write(|tx| {
            let mut items = tx.prepare_cached(
                "INSERT INTO items(account_id,repository_id,number,kind,issue_api_id,title,title_at,body,body_at,closed,closed_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?7,?9,?7) RETURNING id",
            )?;
            let mut prs = tx.prepare_cached(
                "INSERT INTO pull_request_details(item_id,account_id,pull_request_api_id,merged,merged_at,base_oid,base_oid_at,head_oid,head_oid_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?5,?7,?5)",
            )?;
            let mut put_comment = tx.prepare_cached(
                "INSERT INTO cached_resource_records(resource_id,record_key,body) VALUES (?1,?2,?3)",
            )?;
            let mut initial_items = Vec::new();
            for index in first..(first + BATCH).min(item_count) {
                let number = index + 1;
                let is_pr = number % 3 == 0;
                let (kind, title, body) = if is_pr {
                    ("pull_request",format!("Improve relay workflow {number}"),"Resolves #1 and relates to #2 and #4. **Fixture review**.")
                } else {
                    ("issue",format!("Investigate relay behavior {number}"),"A cached fixture for offline **discussion** and search.")
                };
                let item = items.query_row(
                    params![account.0,repository.0,i64::try_from(number).map_err(|_| Error::InvalidInput("fixture item limit exceeded"))?,
                        kind,number.to_string(),title,FIXTURE_TIME_MS,body,number % 4 == 0],
                    |row| row.get::<_,i64>(0).map(ItemId),
                )?;
                if is_pr {
                    prs.execute(params![item.0,account.0,(60_000 + number).to_string(),number % 12 == 0,FIXTURE_TIME_MS,root_oid.as_str(),head.as_str()])?;
                }

                let key = ResourceKey { account, target: ResourceTarget::Item(item), family: ResourceFamily::Comments };
                let resource = ensure_resource(tx,key)?;
                tx.execute(
                    "UPDATE cached_resources SET coverage=2,last_page_at_ms=?2,last_fetch_at_ms=?2,last_validation_at_ms=?2,etag=?3 WHERE id=?1",
                    params![resource,FIXTURE_TIME_MS,format!("\"fixture-{seed}-{number}\"")],
                )?;
                let count = match index {
                    0 => comments_per_item + long_extra,
                    1.. if index <= long_extra => comments_per_item - 1,
                    _ => comments_per_item,
                };
                for comment in 0..count {
                    put_comment.execute(params![resource,format!("comment-{comment:06}"),
                        format!("Synthetic comment {comment} on item {number}. Seed {seed}. **Offline review**.")])?;
                }
                if number <= 30 {
                    initial_items.push((item,is_pr));
                }
            }
            Ok(initial_items)
        })?;
        for (item, is_pr) in batch {
            if is_pr {
                if reviewed_pr.is_none() {
                    reviewed_pr = Some(item);
                } else {
                    alternate_pr.get_or_insert(item);
                }
            } else {
                long_discussion.get_or_insert(item);
                issue_targets.push(item.0);
            }
            if tabs.len() < 20 {
                tabs.push(Tab {
                    id: format!("fixture-tab-{}", tabs.len()),
                    item,
                    view: TabView::default(),
                });
            }
        }
    }

    let comparison = ComparisonKey {
        repository,
        base: root_oid,
        head: head.clone(),
        mode: ComparisonMode::MergeBase,
        ignore_whitespace: false,
        context_lines: 3,
    };
    let diffs: Vec<_> = (0..files)
        .map(|file| {
            let lines = if scale == Scale::Section16 { 200 } else { 10 };
            let mut patch = format!("@@ -1,{lines} +1,{lines} @@\n-previous\n+changed\n");
            for line in 2..=lines {
                use std::fmt::Write as _;
                writeln!(patch, " shared relay line {line}").expect("write to String");
            }
            ComparisonFile {
                path: format!("src/relay_{file:02}.rs"),
                previous_path: None,
                patch: Some(patch),
            }
        })
        .collect();
    store.save_comparison(account, &comparison, &diffs)?;
    for first in (0..history_count).step_by(BATCH) {
        let commits = (first..(first + BATCH).min(history_count))
            .map(|index| {
                let parent_indices = match index {
                    0 => Vec::new(),
                    1 => vec![0],
                    n if n % 7 == 0 => vec![n - 1, n - 2],
                    n if n % 7 == 6 => vec![n - 2],
                    n => vec![n - 1],
                };
                Ok(GraphCommit {
                    oid: fixture_oid(seed, index)?,
                    title: format!("Relay commit {index}"),
                    committed_at_ms: FIXTURE_TIME_MS + i64::try_from(index).unwrap_or_default(),
                    parents: parent_indices
                        .into_iter()
                        .map(|parent| fixture_oid(seed, parent))
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        store.save_commits(account, repository, &commits)?;
    }
    for (name, index) in [
        ("refs/heads/main", history_count - 1),
        ("refs/heads/review", history_count - 2),
    ] {
        store.save_graph_ref(
            account,
            repository,
            &GraphRef {
                name: name.into(),
                target: Some(fixture_oid(seed, index)?),
                observed_at_ms: FIXTURE_TIME_MS,
            },
        )?;
    }

    let reviewed_pr = reviewed_pr.ok_or(Error::Conflict("fixture has no review target"))?;
    store.create_draft(
        account,
        "fixture-review-draft",
        reviewed_pr,
        DraftKind::Review,
        &DraftContent {
            body: "Keep the previous revision visible.".into(),
            base: head.as_str().into(),
            anchor: "src/relay_00.rs:new:19".into(),
        },
    )?;
    let discussion = long_discussion.ok_or(Error::Conflict("fixture has no discussion"))?;
    let revision = "fixture-review-body-1";
    let provenance = "fixture-pr-body";
    let candidates: Vec<_> = [1, 2, 4]
        .into_iter()
        .zip([9, 23, 30])
        .zip(&issue_targets)
        .map(|((number, position), item)| ReferenceCandidate {
            position,
            raw: format!("#{number}"),
            provenance: provenance.into(),
            verified: Some((ItemId(*item), FIXTURE_TIME_MS)),
        })
        .collect();
    store.replace_pr_references(account, reviewed_pr, None, revision, &candidates)?;
    let open_tabs = tabs.len();
    let pr_tab = tabs
        .iter()
        .position(|tab| tab.item == reviewed_pr)
        .ok_or(Error::Conflict("fixture has no review tab"))?;
    tabs[pr_tab].view = TabView {
        draft: Some("fixture-review-draft".into()),
        navigation: Some("changes".into()),
        context: Some(PrContext {
            selected_issue: Some(discussion),
            body_revision: revision.into(),
            provenance: provenance.into(),
            ratio: 0.33,
        }),
        ..TabView::default()
    };
    tabs.swap(pr_tab, open_tabs / 2);
    let alternate = alternate_pr.ok_or(Error::Conflict("fixture has no alternate PR"))?;
    let alternate_revision = "fixture-alternate-body-1";
    store.replace_pr_references(account, alternate, None, alternate_revision, &candidates)?;
    let alternate_tab = tabs
        .iter()
        .position(|tab| tab.item == alternate)
        .ok_or(Error::Conflict("fixture has no alternate PR tab"))?;
    tabs[alternate_tab].view = TabView {
        navigation: Some("discussion".into()),
        context: Some(PrContext {
            selected_issue: Some(ItemId(issue_targets[1])),
            body_revision: alternate_revision.into(),
            provenance: provenance.into(),
            ratio: 0.23,
        }),
        ..TabView::default()
    };
    tabs.swap(alternate_tab, open_tabs / 2 + 1);
    let right = tabs.split_off(open_tabs / 2);
    store.save_workspace(
        account,
        &Workspace {
            focused_pane: 1,
            proportions: vec![0.5, 0.5],
            panes: vec![
                Pane {
                    active: Some(0),
                    tabs,
                },
                Pane {
                    active: Some(0),
                    tabs: right,
                },
            ],
        },
    )?;

    let outbox_count = match (scale, profile) {
        (Scale::Section16, _) => 1000,
        (Scale::Small, Profile::Reader) => 10,
        _ => 0,
    };
    if reader {
        enroll_reader(store, account, repository, seed, item_count, profile)?;
    }
    let typed_outbox_count = if backlog {
        let [_, _, blocked, ..] = issue_targets.as_slice() else {
            return Err(Error::Conflict(
                "The backlog fixture needs independent issue targets",
            ));
        };
        let blocked_issue = ItemId(*blocked);
        backlog::seed_writes(store, account, discussion, blocked_issue)?;
        let mut settings = store.sync_settings(account)?;
        settings.controls.pause_background = true;
        store.save_sync_settings(account, &settings)?;
        LARGE_WORKSPACE_WRITES
    } else {
        0
    };
    let historical = outbox_count - typed_outbox_count;
    if historical > 0 {
        store.write(|tx| load_fixture_outbox(tx, account.0, &issue_targets, seed, historical))?;
    }

    Ok(Fixture {
        revision: match profile {
            Profile::Backlog => LARGE_WORKSPACE_REVISION,
            Profile::Lifecycle => "s12-native-lifecycle-v1",
            Profile::Reader => "s04-reader-v1",
            Profile::Data => REVISION,
        },
        account,
        repository,
        long_discussion: discussion,
        reviewed_pr,
        comparison,
        head,
        item_count,
        comment_count: item_count * comments_per_item,
        open_tabs,
        outbox_count,
        typed_outbox_count,
    })
}

fn fixture_oid(seed: u64, index: usize) -> Result<CommitOid> {
    CommitOid::parse(&format!("{seed:016x}{index:024x}"))
}

fn enroll_reader(
    store: &Store,
    account: AccountId,
    primary: RepositoryId,
    seed: u64,
    items: usize,
    fixture: Profile,
) -> Result<()> {
    use crate::{
        IssueApiId, ItemLocator, TransferEvidence, TransferOutcome, reader::repository_list_key,
    };
    let (archive, owner, login) = if fixture.uses_loopback_identity() {
        (BACKLOG_ARCHIVE, BACKLOG_OWNER, BACKLOG_LOGIN)
    } else {
        (2, "fixture", "fixture-reader")
    };
    let secondary = store.ensure_repository(account, archive, owner, "archive")?;
    let from = ItemLocator {
        repository: primary,
        number: 2,
    };
    let to = ItemLocator {
        repository: secondary,
        number: 2,
    };
    let evidence = TransferEvidence::VerifiedIssueId(IssueApiId::try_from(2).expect("fixture ID"));
    if !matches!(
        store.transfer_issue(account, from, to, evidence)?,
        TransferOutcome::Moved(_)
    ) {
        return Err(Error::Conflict(
            "Reader fixture transfer did not establish its second repository",
        ));
    }
    store.write(|db| {
        db.execute(
            "INSERT INTO github_access_profiles(account_id,owner_scope,login,credential_ref,credential_kind,access,checked_at_ms)
            VALUES(?1,?2,?3,?4,0,2,?5)",
            params![account, owner, login, format!("{seed:032x}"), FIXTURE_TIME_MS],
        )?;
        db.execute("INSERT INTO active_profile(singleton,account_id) VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET account_id=excluded.account_id", [account])?;
        db.execute("INSERT INTO dispatch_profiles(account_id,authorized,offline) VALUES(?,0,1)
            ON CONFLICT(account_id) DO UPDATE SET authorized=0,offline=1", [account])?;
        for repository in [primary, secondary] {
            db.execute("INSERT INTO github_repository_access(repository_id,account_id,owner_scope,selected,private,access,checked_at_ms,last_success_at_ms)
                VALUES(?1,?2,?3,1,1,2,?4,?4)", params![repository, account, owner, FIXTURE_TIME_MS])?;
            let id = ensure_resource(db, repository_list_key(account, repository))?;
            db.execute("UPDATE cached_resources SET coverage=2,last_page_at_ms=?2,last_fetch_at_ms=?2,last_validation_at_ms=?2 WHERE id=?1", params![id, FIXTURE_TIME_MS])?;
        }
        Ok(())
    })?;
    for first in (0..items).step_by(500) {
        let lo = first as i64 + 1;
        let hi = (first + 500).min(items) as i64;
        store.write(|db| {
            db.execute("INSERT INTO cached_resource_records(resource_id,record_key,body)
                SELECT c.id,cast(i.id AS TEXT),'' FROM items i JOIN cached_resources c ON c.repository_id=i.repository_id AND c.family='repository-items'
                WHERE i.account_id=?1 AND i.number BETWEEN ?2 AND ?3", params![account, lo, hi])?;
            db.execute("INSERT INTO cached_resources(account_id,item_id,family,coverage,last_fetch_at_ms,last_validation_at_ms)
                SELECT i.account_id,i.id,k.column1,2,?4,?4 FROM items i JOIN (VALUES('metadata'),('body'),('timeline')) k
                WHERE i.account_id=?1 AND i.number BETWEEN ?2 AND ?3", params![account, lo, hi, FIXTURE_TIME_MS])?;
            db.execute("INSERT INTO cached_resource_records(resource_id,record_key,body)
                SELECT c.id,'fixture-opened',json_object('remote_key','fixture-opened','event','opened','actor','fixture-author','occurred_at_ms',?4)
                FROM items i JOIN cached_resources c ON c.item_id=i.id AND c.family='timeline'
                WHERE i.account_id=?1 AND i.number BETWEEN ?2 AND ?3", params![account, lo, hi, FIXTURE_TIME_MS])?;
            db.execute("INSERT INTO reader_facets(item_id,category,observed_at)
                SELECT i.id,k.column1,?4 FROM items i JOIN (VALUES('assignee'),('reviewer'),('author'),('label')) k
                WHERE i.account_id=?1 AND i.number BETWEEN ?2 AND ?3", params![account, lo, hi, FIXTURE_TIME_MS])?;
            db.execute("INSERT INTO reader_facet_values(item_id,category,value)
                SELECT f.item_id,f.category,CASE f.category WHEN 'label' THEN 'offline' WHEN 'author' THEN 'fixture-author' ELSE 'fixture-reader' END
                FROM reader_facets f JOIN items i ON i.id=f.item_id WHERE i.account_id=?1 AND i.number BETWEEN ?2 AND ?3
                AND (f.category IN ('label','author') OR (f.category='assignee' AND i.number%7=0)
                OR (f.category='reviewer' AND i.kind='pull_request' AND i.number%9=0))", params![account, lo, hi])?;
            Ok(())
        })?;
    }
    Ok(())
}

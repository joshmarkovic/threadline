use super::*;
use crate::sync_worker::SyncWorker;
use threadline_core::connectivity::{ConnectionStatus, ProbeTrigger};
use threadline_storage::{
    Change, IssueClassification, IssueRecord, ItemLocator, ItemRepresentation, ItemUpdate, NodeId,
    UtcMillis, reader::DownloadState, resources::Coverage,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn issue_pin_resumes_offline_request_after_restart_without_any_open_tab() {
    let fixture = Simulator::start_with_reader(true);
    fixture.page_two_ready.store(true, Ordering::Release);
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open_profile(directory.path()).unwrap());
    let mut service = AccountService {
        store: Arc::clone(&store),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(fixture.address).unwrap(),
        fixture: true,
    };
    let cancel = Cancellation::new();
    let connected = service
        .execute(
            AccountCommand::Connect {
                repository: "fixture-org/private".into(),
                pat: Pat::from_input("threadline-test-fixture".into()),
                renew: None,
            },
            &cancel,
        )
        .await;
    assert!(connected.github_error.is_none());
    let account = connected.view.active.unwrap();
    let public = connected
        .view
        .repositories
        .iter()
        .find(|row| row.name == "public")
        .unwrap()
        .clone();
    let repository = public.id;
    let selected = service
        .execute(
            AccountCommand::Select {
                profile: connected.view.profiles[0].clone(),
                repository: public,
            },
            &cancel,
        )
        .await;
    assert!(selected.github_error.is_none());
    for private in selected
        .view
        .repositories
        .iter()
        .filter(|row| row.selected && row.id != repository)
    {
        store
            .select_github_repository(account, private.id, false)
            .unwrap();
    }

    store
        .update_sync_settings(account, |settings| {
            settings.controls.work_offline = true;
            settings.controls.pause_background = true;
        })
        .unwrap();
    let item = store
        .ingest(
            account,
            ItemLocator {
                repository,
                number: 41,
            },
            ItemUpdate {
                representation: ItemRepresentation::Issue(IssueRecord {
                    id: 84.try_into().unwrap(),
                    node_id: Some(NodeId::new("issue-84").unwrap()),
                    classification: IssueClassification::Issue,
                }),
                remote_updated_at: UtcMillis(1),
                title: Change::Missing,
                body: Change::Missing,
                closed: Change::Missing,
            },
        )
        .unwrap();
    let request = store
        .request_issue_download(account, item, UtcMillis::now().unwrap().0)
        .unwrap();
    assert_eq!(request.state, DownloadState::Pending);
    assert_eq!(request.completed_families(), 0);
    assert!(store.workspace(account).unwrap().is_none());

    let pending = fixture.requests.load(Ordering::Relaxed);
    service.store = Arc::new(Store::open_profile(directory.path()).unwrap());
    let restarted = Arc::clone(&service.store);
    let worker = SyncWorker::start_fixture(service, &tokio::runtime::Handle::current());
    let mut updates = worker.watch();
    worker.wake(Some(ProbeTrigger::Startup));
    tokio::time::timeout(
        Duration::from_secs(5),
        updates.wait_for(|update| {
            update.account == Some(account) && update.connection.status == ConnectionStatus::Offline
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(fixture.requests.load(Ordering::Relaxed), pending);
    assert!(
        restarted
            .next_issue_download_due(account)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        restarted.issue_download(account, item).unwrap().unwrap(),
        request
    );

    restarted
        .update_sync_settings(account, |settings| settings.controls.work_offline = false)
        .unwrap();
    worker.interrupt(Some(ProbeTrigger::CheckNow));
    let completed = tokio::time::timeout(Duration::from_secs(18), async {
        loop {
            let job = restarted.issue_download(account, item).unwrap().unwrap();
            if job.state == DownloadState::Complete {
                break job;
            }
            updates.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(completed.generation, request.generation);
    assert_eq!(completed.completed_families(), 3);
    assert!(completed.last_error.is_none());
    let page = restarted.discussion_page(account, item, None, 10).unwrap();
    assert_eq!(page.state.coverage, Coverage::Complete);
    assert_eq!(page.records.len(), 2);
    assert!(
        page.records
            .iter()
            .any(|row| row.body == "**updated digest**")
    );
    assert!(page.records.iter().any(|row| row.body == "second reply"));
    assert!(restarted.workspace(account).unwrap().is_none());
}

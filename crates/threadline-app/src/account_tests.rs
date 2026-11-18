use std::{
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use super::*;

struct Simulator {
    address: SocketAddr,
    sso: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    requests: Arc<AtomicUsize>,
    repository_id: Arc<AtomicU64>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Simulator {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let sso = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicUsize::new(0));
        let repository_id = Arc::new(AtomicU64::new(42));
        let (server_sso, server_stop, server_requests, server_repository) = (
            sso.clone(),
            stop.clone(),
            requests.clone(),
            repository_id.clone(),
        );
        let worker = thread::spawn(move || {
            while !server_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        server_requests.fetch_add(1, Ordering::Relaxed);
                        respond(
                            stream,
                            server_sso.load(Ordering::Relaxed),
                            server_repository.load(Ordering::Relaxed),
                        );
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("local simulator accept failed: {e}"),
                }
            }
        });
        Self {
            address,
            sso,
            stop,
            requests,
            repository_id,
            worker: Some(worker),
        }
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            if thread::panicking() {
                let _ = worker.join();
            } else {
                worker.join().unwrap();
            }
        }
    }
}

fn respond(mut stream: TcpStream, sso: bool, repository_id: u64) {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut line = String::new();
    let mut input = BufReader::new(&mut stream);
    input.read_line(&mut line).unwrap();
    let target = line.split_ascii_whitespace().nth(1).unwrap().to_owned();
    let mut headers = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
            break;
        }
        headers.push_str(&line);
    }
    assert!(
        headers
            .lines()
            .any(|line| line.eq_ignore_ascii_case("authorization: Bearer threadline-test-fixture"))
    );
    let repo = format!(
        r#"{{"id":{repository_id},"node_id":"synthetic-private-repo","name":"private","owner":{{"login":"fixture-org"}},"private":true,"permissions":{{"pull":true,"push":false,"admin":false}}}}"#
    );
    let (status,extra,body)=match target.as_str() {
        "/user" => ("200 OK",String::new(),r#"{"id":101,"login":"fixture-user"}"#.to_owned()),
        "/repos/fixture-org/private" if sso => ("403 Forbidden",
            "X-GitHub-SSO: required; url=https://github.com/orgs/fixture-org/sso?authorization_request=synthetic-request\r\n".into(),
            r#"{"message":"Resource protected by organization SAML enforcement"}"#.into()),
        "/repos/fixture-org/private" => ("200 OK",String::new(),repo.to_owned()),
        target if target.starts_with("/user/repos?") => ("200 OK",if sso {"X-GitHub-SSO: partial-results; organizations=9\r\n".into()} else {String::new()},format!("[{repo}]")),
        _ => panic!("unexpected local fixture route"),
    };
    write!(stream,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",body.len()).unwrap();
    stream.flush().unwrap();
}

#[tokio::test]
async fn production_pat_policy_rejects_other_credentials_before_http_or_storage() {
    let fixture = Simulator::start();
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(&dir.path().join("auth.sqlite")).unwrap());
    let service = AccountService {
        store: store.clone(),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(fixture.address).unwrap(),
        fixture: false,
    };
    for invalid in [
        "gho_synthetic",
        "ghs_synthetic",
        "ghu_synthetic",
        "ghr_synthetic",
        "threadline-test-fixture",
        "unclassified",
        "ghp_",
        "github_pat_",
    ] {
        let reply = service
            .execute(
                AccountCommand::Connect {
                    repository: "fixture-org/private".into(),
                    pat: Pat::from_input(invalid.into()),
                    renew: None,
                },
                &Cancellation::new(),
            )
            .await;
        assert_eq!(reply.message, Some("Enter a valid personal access token."));
        assert_eq!(reply.reachability, Reachability::NotAttempted);
    }
    assert_eq!(fixture.requests.load(Ordering::Relaxed), 0);
    assert!(store.github_access_profiles().unwrap().is_empty());
}

#[tokio::test]
async fn entered_private_route_cannot_replace_the_bound_repository_identity() {
    let fixture = Simulator::start();
    let dir = tempfile::tempdir().unwrap();
    let service = AccountService {
        store: Arc::new(Store::open(&dir.path().join("auth.sqlite")).unwrap()),
        credentials: Arc::new(FixtureVault::default()),
        github: GitHubClient::loopback(fixture.address).unwrap(),
        fixture: true,
    };
    let reconnect = |renew| AccountCommand::Connect {
        repository: "fixture-org/private".into(),
        pat: Pat::from_input("threadline-test-fixture".into()),
        renew,
    };
    let cancel = Cancellation::new();
    let connected = service.execute(reconnect(None), &cancel).await;
    assert!(connected.github_error.is_none());
    let original = connected.view.repositories[0].clone();
    let selected = service
        .execute(
            AccountCommand::Select {
                profile: connected.view.profiles[0].clone(),
                repository: original.clone(),
            },
            &cancel,
        )
        .await;
    assert!(selected.view.repositories[0].selected);
    fixture.repository_id.store(43, Ordering::Relaxed);
    let repaired = service
        .execute(reconnect(Some(selected.view.profiles[0].clone())), &cancel)
        .await;
    assert_eq!(
        repaired.github_error.unwrap().kind,
        AccessKind::IdentityMismatch
    );
    assert_eq!(repaired.view.repositories.len(), 1);
    let prior = &repaired.view.repositories[0];
    assert_eq!(prior.id, original.id);
    assert_eq!(prior.provider_id, original.provider_id);
    assert!(prior.selected);
    assert!(!prior.verified_current);
}

#[tokio::test]
async fn account_journey_persists_private_access_partial_sso_and_signout() {
    let fixture = Simulator::start();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("账户.sqlite");
    let store = Arc::new(Store::open(&path).unwrap());
    let memory = Arc::new(FixtureVault::default());
    let service = AccountService {
        store: store.clone(),
        credentials: memory.clone(),
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
    assert!(
        connected.github_error.is_none(),
        "{:?}",
        connected.github_error
    );
    assert_eq!(connected.reachability, Reachability::ReceivedResponse);
    let profile = connected.view.profiles[0].clone();
    let repo = connected.view.repositories[0].clone();
    assert_eq!(profile.user_id.get(), 101);
    assert_eq!(profile.access, AccessState::Readable);
    assert!(!repo.selected);

    let selected = service
        .execute(
            AccountCommand::Select {
                profile,
                repository: repo,
            },
            &cancel,
        )
        .await;
    assert!(
        selected.github_error.is_none(),
        "{:?}",
        selected.github_error
    );
    assert!(selected.view.repositories[0].selected);
    let profile = selected.view.profiles[0].clone();
    let repository = selected.view.repositories[0].clone();
    let ok_at = repository.last_success_at_ms;
    fixture.sso.store(true, Ordering::Relaxed);
    let restricted = service
        .execute(
            AccountCommand::Refresh {
                profile: profile.clone(),
                repository: Some(repository),
                requested: None,
            },
            &cancel,
        )
        .await;
    let error = restricted.github_error.expect("SSO should be shown");
    assert_eq!(error.kind, AccessKind::SsoRequired);
    assert!(error.authorization_url().is_some());
    assert_eq!(restricted.reachability, Reachability::ReceivedResponse);
    assert_eq!(
        restricted.view.profiles[0].discovery,
        DiscoveryState::RestrictedBySso
    );
    assert_eq!(restricted.view.repositories[0].last_success_at_ms, ok_at);
    assert_eq!(
        restricted.view.repositories[0].access,
        AccessState::SsoRequired
    );

    let stopped = service
        .execute(AccountCommand::SignOut(profile.account), &cancel)
        .await;
    assert_eq!(stopped.view.profiles[0].access, AccessState::SignedOut);
    assert!(stopped.view.repositories[0].selected);
    assert!(
        memory
            .read(&credential_ref(&profile).unwrap_or_else(|_| panic!("missing fixture reference")))
            .unwrap()
            .is_none()
    );
    assert!(fixture.requests.load(Ordering::Relaxed) >= 9);
    drop(service);
    drop(store);
    let reopened = Store::open(&path).unwrap();
    assert_eq!(
        reopened.active_github_account().unwrap(),
        Some(profile.account)
    );
    assert_eq!(
        reopened.github_repository_views(profile.account).unwrap()[0].last_success_at_ms,
        ok_at
    );
    assert!(
        !std::fs::read(&path)
            .unwrap()
            .windows(b"threadline-test-fixture".len())
            .any(|bytes| bytes == b"threadline-test-fixture")
    );
}

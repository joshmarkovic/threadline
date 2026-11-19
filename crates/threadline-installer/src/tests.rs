use super::*;
use serde_json::{Value, json};
use std::{
    io::BufRead,
    net::TcpListener,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

pub(super) const ASSET: &[u8] = b"synthetic installer fixture";
pub(super) fn digest(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub(super) fn platform() -> Platform {
    Platform {
        native_x64: true,
        workstation: true,
        build: 22631,
        highest_build: 3,
        profile_schema: Some(7),
    }
}
pub(super) fn body(base: &Url) -> Value {
    json!({"version":"1.2.3","channel":"stable","target":TARGET,"minimum_os":MINIMUM,
        "artifact_url":base.join("v1.2.3/ThreadlineSetup.exe").unwrap().as_str(),
        "package_build":4,"current_schema":15,"oldest_supported_schema":1,"size":ASSET.len(),"sha256":digest(ASSET),
        "publisher_certificate_sha256":digest(b"fixture publisher")})
}
fn request(raw: &[u8]) -> Selection {
    Selection::new("1.2.3", "stable", &digest(raw)).unwrap()
}

#[test]
fn ut_019_selects_exact_version_channel_architecture_schema_and_safe_asset() {
    let source = Source::https(
        "https://downloads.example.invalid/release/",
        &digest(b"fixture publisher"),
    )
    .unwrap();
    let good = body(&source.base);
    let raw = serde_json::to_vec(&good).unwrap();
    assert!(source.select(&raw, &request(&raw), &platform()).is_ok());
    for (field, bad) in [
        ("target", json!("aarch64-pc-windows-msvc")),
        ("version", json!("1.2.4")),
        ("channel", json!("preview")),
        ("minimum_os", json!("Windows 10")),
        ("package_build", json!(2)),
        ("current_schema", json!(5)),
        ("oldest_supported_schema", json!(8)),
        (
            "publisher_certificate_sha256",
            json!(digest(b"another signer")),
        ),
        ("size", json!(0)),
        ("size", json!(MAX_INSTALLER + 1)),
        ("sha256", json!("invalid")),
    ] {
        let mut data = good.clone();
        data[field] = bad;
        let bytes = serde_json::to_vec(&data).unwrap();
        assert!(
            source
                .select(&bytes, &request(&bytes), &platform())
                .is_err(),
            "{field}"
        );
    }
    for suffix in [
        "v1.2.3/../X.exe",
        "v1.2.3/%2e%2e/X.exe",
        "v1.2.3/nested/X.exe",
        "v1.2.3/CON.exe",
        "v1.2.3/LPT9.exe",
        "v1.2.3/.exe",
        "v1.2.3/X.exe?credential=no",
        "v1.2.2/X.exe",
        "v1.2.3/X.zip",
        "v1.2.3/X.exe:bad",
    ] {
        let mut data = good.clone();
        data["artifact_url"] = json!(format!("{}{suffix}", source.base));
        let bytes = serde_json::to_vec(&data).unwrap();
        assert!(
            source
                .select(&bytes, &request(&bytes), &platform())
                .is_err(),
            "{suffix}"
        );
    }
    for p in [
        Platform {
            native_x64: false,
            ..platform()
        },
        Platform {
            workstation: false,
            ..platform()
        },
        Platform {
            build: 21999,
            ..platform()
        },
    ] {
        assert!(source.select(&raw, &request(&raw), &p).is_err());
    }
    for (desktop, graphics) in [(false, true), (true, false), (true, true)] {
        let environment = LaunchEnvironment { desktop, graphics };
        assert_eq!(environment.check().is_ok(), desktop && graphics);
    }
    assert!(Selection::new("1.2.3-beta.1", "stable", &digest(b"m")).is_err());
    assert!(Source::https("http://example.invalid/release/", &digest(b"f")).is_err());
    assert!(
        source
            .select(&raw, &request(b"tampered"), &platform())
            .is_err()
    );
}

type RouteReplies = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

pub(super) struct HttpFixture {
    pub(super) base: Url,
    replies: RouteReplies,
    pub(super) requests: Arc<Mutex<Vec<String>>>,
    pub(super) headers: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl HttpFixture {
    pub(super) fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = Url::parse(&format!(
            "http://{}/release/",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        listener.set_nonblocking(true).unwrap();
        let replies = RouteReplies::default();
        let requests: Arc<Mutex<Vec<String>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let route_replies = replies.clone();
        let recorded_requests = requests.clone();
        let headers: Arc<Mutex<Vec<String>>> = Arc::default();
        let captured = headers.clone();
        let shutting_down = stop.clone();
        let worker = thread::spawn(move || {
            while !shutting_down.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket.set_nonblocking(false).unwrap();
                        socket
                            .set_read_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        let mut reader = std::io::BufReader::new(&socket);
                        let mut line = String::new();
                        reader.read_line(&mut line).unwrap();
                        let route = line.split_whitespace().nth(1).unwrap().to_owned();
                        let mut request_headers = String::new();
                        loop {
                            line.clear();
                            let n = reader.read_line(&mut line).unwrap();
                            request_headers.push_str(&line.to_ascii_lowercase());
                            if n == 0 || line == "\r\n" {
                                break;
                            }
                        }
                        captured.lock().unwrap().push(request_headers);
                        recorded_requests.lock().unwrap().push(route.clone());
                        let responses = route_replies.lock().unwrap();
                        let response = responses
                            .iter()
                            .find(|(url, _)| url == &route)
                            .map(|(_, content)| content.as_slice())
                            .unwrap_or(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        socket.write_all(response).unwrap();
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(3))
                    }
                    Err(e) => panic!("fixture listener: {e}"),
                }
            }
        });
        Self {
            base,
            replies,
            requests,
            headers,
            stop,
            worker: Some(worker),
        }
    }
    pub(super) fn serve(&self, url: &str, status: &str, advertised: usize, body: &[u8]) {
        let mut bytes = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {advertised}\r\nConnection: close\r\n\r\n"
        )
        .into_bytes();
        bytes.extend_from_slice(body);
        let mut replies = self.replies.lock().unwrap();
        replies.retain(|(route, _)| route != url);
        replies.push((url.into(), bytes));
    }
}
impl Drop for HttpFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.worker.take().unwrap().join().unwrap();
    }
}

#[tokio::test]
async fn it_021_real_http_stages_only_approved_complete_payload_and_retains_running_install() {
    const META: &str = "/release/v1.2.3/threadline-windows-x64.json";
    const PAYLOAD: &str = "/release/v1.2.3/ThreadlineSetup.exe";
    let server = HttpFixture::new();
    let source = Source::with_url(
        server.base.clone(),
        pin(&digest(b"fixture publisher")).unwrap(),
    )
    .unwrap();
    let raw = serde_json::to_vec(&body(&server.base)).unwrap();
    let select = request(&raw);
    let scope = tempfile::tempdir().unwrap();
    let tmp = scope.path().join("staging");
    fs::create_dir(&tmp).unwrap();
    let installed = scope.path().join("old-app");
    fs::write(&installed, b"previous working version").unwrap();
    server.serve(META, "200 OK", raw.len(), &raw);
    for (status, n, sent) in [
        ("200 OK", ASSET.len(), &ASSET[..4]),
        ("200 OK", ASSET.len(), &[0u8; 26][..]),
        ("404 Not Found", 0, &[][..]),
        ("200 OK", 102400, &[][..]),
    ] {
        server.serve(PAYLOAD, status, n, sent);
        assert!(
            source
                .stage_in(&select, &platform(), None, &tmp)
                .await
                .is_err()
        );
        assert_eq!(fs::read(&installed).unwrap(), b"previous working version");
        assert_eq!(fs::read_dir(&tmp).unwrap().count(), 0);
    }
    server.requests.lock().unwrap().clear();
    let mut wrong = body(&server.base);
    wrong["target"] = json!("aarch64-pc-windows-msvc");
    let wrong = serde_json::to_vec(&wrong).unwrap();
    server.serve(META, "200 OK", wrong.len(), &wrong);
    assert!(
        source
            .stage_in(&request(&wrong), &platform(), None, &tmp)
            .await
            .is_err()
    );
    assert_eq!(*server.requests.lock().unwrap(), [META]);
    server.serve(META, "200 OK", raw.len(), &raw);
    server.serve(PAYLOAD, "200 OK", ASSET.len(), ASSET);
    {
        let approved = source
            .stage_in(&select, &platform(), None, &tmp)
            .await
            .unwrap();
        assert_eq!(fs::read(&approved.path).unwrap(), ASSET);
        assert_eq!(approved.manifest.version, "1.2.3");
    }
    assert_eq!(fs::read_dir(&tmp).unwrap().count(), 0);
    let offline = scope.path().join("offline");
    fs::create_dir(&offline).unwrap();
    fs::write(offline.join("threadline-windows-x64.json"), &raw).unwrap();
    fs::write(offline.join("ThreadlineSetup.exe"), ASSET).unwrap();
    server.requests.lock().unwrap().clear();
    assert!(
        source
            .stage_in(&select, &platform(), Some(&offline), &tmp)
            .await
            .is_ok()
    );
    assert!(server.requests.lock().unwrap().is_empty());
}

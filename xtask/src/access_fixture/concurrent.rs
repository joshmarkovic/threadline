use super::*;
use std::time::Instant;

const PAGE_END: &str = "&per_page=20&page=";
const SAMPLE_COUNT: u8 = 60;
const ARCHIVE: &str = "/repos/fixture-org/archive";
const PRIMARY_ISSUE: &str = "/repositories/71/issues/";

pub(super) fn get(target: &str) -> Route {
    if let Some(page) = page(target) {
        return Route::BacklogFeed(page);
    }
    match target {
        USER => Route::User,
        REPOSITORY => Route::Repository,
        ARCHIVE => Route::BacklogArchive,
        "/repositories/72/issues/2" => Route::BacklogFeedMetadata(2),
        "/repositories/71/pulls/3" | "/repositories/71/pulls/6" => Route::BacklogCachedPr,
        "/repositories/71/pulls?state=open&sort=created&direction=asc&per_page=20&page=1"
        | "/repositories/72/pulls?state=open&sort=created&direction=asc&per_page=20&page=1" => {
            Route::BacklogAuxiliaryFeed
        }
        _ => {
            if [SYNC_COMMENTS, SYNC_INLINE]
                .into_iter()
                .any(|prefix| sync_since(target, prefix))
                || [SYNC_ITEMS, SYNC_COMMENTS, SYNC_INLINE]
                    .into_iter()
                    .any(|prefix| {
                        prefix
                            .strip_prefix("/repositories/71/")
                            .is_some_and(|suffix| {
                                sync_since(target, &format!("/repositories/72/{suffix}"))
                            })
                    })
            {
                return Route::BacklogAuxiliaryFeed;
            }
            let number = target
                .strip_prefix(PRIMARY_ISSUE)
                .and_then(|number| number.parse::<u16>().ok());
            match number {
                Some(2) => Route::Unexpected,
                Some(number) if number <= 30 && number > 0 => Route::BacklogFeedMetadata(number),
                Some(number)
                    if (1000..=1057).contains(&number) && (number - 1000).is_multiple_of(3) =>
                {
                    Route::BacklogFeedMetadata(number)
                }
                _ => Route::Unexpected,
            }
        }
    }
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if phase != Phase::BacklogConcurrent {
        return None;
    }
    if route == Route::BacklogCachedPr {
        return Some(Response {
            status: 404,
            reason: "Not Found",
            body: json!({"message":"The cached PR has no live detail in this fixture"}),
            sso: None,
        });
    }
    let body = match route {
        Route::BacklogFeed(_) | Route::BacklogAuxiliaryFeed => json!([]),
        Route::BacklogArchive => json!({
            "id":72,"node_id":"R_72","name":"archive","owner":{"login":"fixture-org"},
            "default_branch":"main","private":true,"permissions":{"pull":true,"push":false,"admin":false}
        }),
        Route::BacklogFeedMetadata(number) => {
            const UPDATED: &str = "2023-11-14T22:13:20Z";
            let repository = if number == 2 { "archive" } else { "private" };
            let reviewed = number.is_multiple_of(3);
            let (title, body) = if reviewed {
                (
                    format!("Improve relay workflow {number}"),
                    "Resolves #1 and relates to #2 and #4. **Fixture review**.",
                )
            } else {
                (
                    format!("Investigate relay behavior {number}"),
                    "A cached fixture for offline **discussion** and search.",
                )
            };
            let mut item = json!({
                "id":number,"number":number,"title":title,"body":body,
                "state":if number.is_multiple_of(4) { "closed" } else { "open" },
                "created_at":UPDATED,"updated_at":UPDATED,
                "repository_url":format!("https://api.github.com/repos/fixture-org/{repository}")
            });
            if reviewed {
                item["pull_request"] = json!({});
            }
            item
        }
        _ => return None,
    };
    Some(Response {
        status: 200,
        reason: "OK",
        body,
        sso: None,
    })
}

pub(super) fn page(target: &str) -> Option<u8> {
    let (query, page) = target.rsplit_once(PAGE_END)?;
    let page = page
        .parse::<u8>()
        .ok()
        .filter(|page| (1..=SAMPLE_COUNT).contains(page))?;
    sync_since(&format!("{query}{PAGE_END}1"), SYNC_ITEMS).then_some(page)
}

fn marker(phase_file: &Path, page: u8, state: &str) -> PathBuf {
    phase_file.with_file_name(format!("reader-feed-{page}-{state}"))
}

fn publish(phase_file: &Path, page: u8, state: &str) -> io::Result<()> {
    fs::File::create_new(marker(phase_file, page, state)).map(drop)
}

pub(super) struct HeldFeed<'a> {
    phase_file: &'a Path,
    page: u8,
    served: bool,
}

impl HeldFeed<'_> {
    pub(super) fn response_written(mut self) -> io::Result<()> {
        publish(self.phase_file, self.page, "served")?;
        self.served = true;
        Ok(())
    }
}

impl Drop for HeldFeed<'_> {
    fn drop(&mut self) {
        if !self.served {
            let _ = publish(self.phase_file, self.page, "response-failed");
        }
    }
}

pub(super) fn hold<'a>(
    phase_file: &'a Path,
    stopping: &AtomicBool,
    page: u8,
    socket: &TcpStream,
) -> io::Result<Option<HeldFeed<'a>>> {
    let ack = marker(phase_file, page, "painted");
    if ack.try_exists()? {
        return Err(io::Error::other(
            "paint acknowledged before the feed request",
        ));
    }
    publish(phase_file, page, "held")?;
    let started = Instant::now();
    let terminal = loop {
        if stopping.load(Ordering::Acquire) || Phase::read(phase_file)? != Phase::BacklogConcurrent
        {
            break "stopped";
        }
        if !still_connected(socket)? {
            break "disconnected";
        }
        if started.elapsed() >= Duration::from_secs(12) {
            break "expired";
        }
        if ack.try_exists()? {
            return Ok(Some(HeldFeed {
                phase_file,
                page,
                served: false,
            }));
        }
        thread::sleep(Duration::from_millis(20));
    };
    publish(phase_file, page, terminal)?;
    Ok(None)
}

pub(super) fn next(target: &str, page: u8) -> Option<String> {
    let (query, _) = target.rsplit_once(PAGE_END)?;
    (page < SAMPLE_COUNT).then(|| format!("{query}{PAGE_END}{}", page + 1))
}

fn still_connected(socket: &TcpStream) -> io::Result<bool> {
    socket.set_nonblocking(true)?;
    let observed = socket.peek(&mut [0]);
    socket.set_nonblocking(false)?;
    Ok(observed.is_err_and(|error| error.kind() == io::ErrorKind::WouldBlock))
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    let pages = records.iter().filter_map(|row| match row.route {
        Route::BacklogFeed(page) => Some((row.phase, row.status, page)),
        _ => None,
    });
    pages.eq((1..=SAMPLE_COUNT).map(|page| (Phase::BacklogConcurrent, 200, page)))
        && [Route::User, Route::Repository].into_iter().all(|route| {
            records.iter().any(|row| {
                row.phase == Phase::BacklogAuth && row.route == route && row.status == 200
            })
        })
        && records.iter().all(|row| {
            row.fixture_authorization
                && match row.phase {
                    Phase::BacklogAuth => {
                        matches!(row.route, Route::User | Route::Repository) && row.status == 200
                    }
                    Phase::BacklogConcurrent => match row.route {
                        Route::User
                        | Route::Repository
                        | Route::BacklogFeed(_)
                        | Route::BacklogArchive
                        | Route::BacklogAuxiliaryFeed
                        | Route::BacklogFeedMetadata(_) => row.status == 200,
                        Route::BacklogCachedPr => row.status == 404,
                        _ => false,
                    },
                    _ => false,
                }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_directed_feed_has_exact_contiguous_routes_and_bound() {
        let target = format!("{SYNC_ITEMS}2026-11-19T00%3A00%3A00Z{PAGE_END}1");
        assert_eq!(page(&target), Some(1));
        let second = next(&target, 1).unwrap();
        assert_eq!(page(&second), Some(2));
        assert!(next(&target, SAMPLE_COUNT).is_none());
        assert!(page(&target.replace("/71/", "/72/")).is_none());
        assert!(page(&target.replace("page=1", "page=61")).is_none());
        assert!(page(&format!("{target}&scope=unknown")).is_none());
    }

    #[test]
    fn concurrent_loopback_withholds_each_page_until_its_exact_acknowledgment() {
        let profile = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(profile.path()).unwrap();
        let phase = fixture.phase_file().to_owned();
        let request = |target: &str| {
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            write!(
                socket,
                "GET {target} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n\r\n"
            )
            .unwrap();
            socket
        };
        let read = |mut socket: TcpStream| {
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            reply
        };
        fs::write(&phase, "backlog-auth").unwrap();
        for endpoint in [USER, REPOSITORY] {
            assert!(read(request(endpoint)).starts_with("HTTP/1.1 200"));
        }
        fs::write(&phase, "backlog-concurrent").unwrap();
        for page in 1..=SAMPLE_COUNT {
            let target = format!("{SYNC_ITEMS}2026-11-19T00%3A00%3A00Z{PAGE_END}{page}");
            let socket = request(&target);
            let start = Instant::now();
            while !marker(&phase, page, "held").exists() {
                assert!(start.elapsed() < Duration::from_secs(2));
                thread::sleep(Duration::from_millis(5));
            }
            socket.set_nonblocking(true).unwrap();
            assert_eq!(
                socket.peek(&mut [0]).unwrap_err().kind(),
                io::ErrorKind::WouldBlock
            );
            socket.set_nonblocking(false).unwrap();
            assert!(!marker(&phase, page, "served").exists());
            if page > 1 {
                assert!(marker(&phase, page - 1, "painted").exists());
                assert!(marker(&phase, page - 1, "served").exists());
            }
            publish(&phase, page, "painted").unwrap();
            let reply = read(socket);
            assert!(reply.starts_with("HTTP/1.1 200"));
            assert!(marker(&phase, page, "served").exists());
            if let Some(next) = next(&target, page) {
                assert!(reply.contains(&format!(
                    "Link: <http://{}{next}>; rel=\"next\"",
                    fixture.address()
                )));
            } else {
                assert!(!reply.contains("Link:"));
            }
        }
        fixture
            .finish(profile.path(), TraceCheck::ConcurrentReader)
            .unwrap();
        let trace = fs::read(profile.path().join("github-access-trace.json")).unwrap();
        assert!(!contains_fixture_credential(&trace));
    }

    #[test]
    fn concurrent_disconnect_never_becomes_a_completed_paint() {
        let profile = tempfile::tempdir().unwrap();
        let phase = profile.path().join("phase");
        fs::write(&phase, "backlog-concurrent").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        client.shutdown(std::net::Shutdown::Both).unwrap();
        assert!(
            hold(&phase, &AtomicBool::new(false), 51, &server)
                .unwrap()
                .is_none()
        );
        assert!(marker(&phase, 51, "disconnected").is_file());
        assert!(!marker(&phase, 51, "served").exists());
    }
}

use super::*;
use chrono::{DateTime, Utc};

const HEAD: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BASE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub(super) const STATUS_ETAG: &str = "\"fixture-status-v1\"";

pub(super) fn active(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::MixedBefore | Phase::MixedOffline | Phase::MixedAfter
    )
}

fn cutoff(target: &str) -> Option<i64> {
    let value = target
        .strip_prefix(SYNC_COMMENTS)?
        .strip_suffix("&per_page=20&page=1")?;
    DateTime::parse_from_rfc3339(&value.replace("%3A", ":"))
        .ok()
        .map(|since| since.timestamp())
}

pub(super) fn after_feed(
    phase_file: &Path,
    target: Option<&str>,
    previous: &[RequestEvidence],
) -> io::Result<Route> {
    let saved = fs::read_to_string(phase_file.with_file_name("mixed-comment-watermark.txt"))?;
    let watermark = saved
        .trim()
        .parse::<i64>()
        .map_err(|_| io::Error::other("Invalid fixture watermark"))?;
    let expected = (watermark - 300_000).div_euclid(1000);
    let resumed = previous.iter().any(|r| {
        r.phase == Phase::MixedAfter && r.route == Route::MixedRecentComments && r.status == 200
    });
    Ok(match target.and_then(cutoff) {
        Some(seconds) if !resumed && seconds == expected => Route::MixedRecentComments,
        Some(seconds) if resumed && (expected..=Utc::now().timestamp()).contains(&seconds) => {
            Route::MixedFollowupComments
        }
        _ => Route::Unexpected,
    })
}

pub(super) fn get(phase: Phase, target: &str) -> Option<Route> {
    if !active(phase) {
        return None;
    }
    pr_pins::get(Phase::PrPinLoad, target).filter(|r| {
        matches!(
            r,
            Route::PrPinItem42 | Route::PrPinChecks | Route::PrPinStatuses
        )
    })
}

pub(super) fn graph(phase: Phase, body: &Value) -> Option<Route> {
    if !active(phase) {
        return None;
    }
    pr_pins::graphql(Phase::PrPinLoad, body)
}

fn comment(id: u64, changed: Option<&str>) -> Value {
    json!({"id":id,"node_id":format!("IC_{id}"),"user":{"id":75,"login":"fixture-author"},
    "created_at":"2018-01-02T00:00:00Z","updated_at":changed.unwrap_or("2018-01-02T00:00:00Z"),
    "issue_url":"https://api.github.com/repos/fixture-org/private/issues/41",
    "body":match (id,changed.is_some()) {
        (5101,true) => "The original comment changed while Threadline was closed.",
        (5101,false) => "The original cached discussion comment.",
        _ => "The second page retained its own comment.",
    }})
}

fn graph_review(changed: bool) -> Value {
    let state = if changed { "DISMISSED" } else { "APPROVED" };
    json!({"data":{"node":{"id":"PR_42","fullDatabaseId":762,"baseRefOid":BASE,"headRefOid":HEAD,
    "repository":{"id":"R_71","name":"private","owner":{"login":"fixture-org"}},
    "reviewDecision":if changed {"REVIEW_REQUIRED"} else {"APPROVED"},
    "reviews":{"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[{
        "id":"PRR_7201","fullDatabaseId":7201,"author":{"login":"fixture-reviewer","__typename":"User","databaseId":79},
        "body":"Retained fixture review","state":state,"submittedAt":"2021-01-01T00:00:00Z","commit":{"oid":HEAD}
    }]}}}})
}

pub(super) fn respond(
    phase: Phase,
    route: Route,
    phase_file: &Path,
) -> io::Result<Option<Response>> {
    if !active(phase) {
        return Ok(None);
    }
    let edited_at = if phase == Phase::MixedAfter {
        let recorded = fs::read_to_string(phase_file.with_file_name("mixed-edit-time.txt"))?;
        DateTime::parse_from_rfc3339(&recorded)
            .map_err(|_| io::Error::other("Invalid synthetic provider edit time"))?;
        Some(recorded)
    } else {
        None
    };
    Ok(response(route, edited_at.as_deref()))
}

fn response(route: Route, edited_at: Option<&str>) -> Option<Response> {
    let changed = edited_at.is_some();
    let issue = || {
        let mut row = super::response(Phase::ReaderResume, Route::Item, true, false).body;
        row["repository_url"] = json!("https://api.github.com/repos/fixture-org/private");
        if let Some(when) = edited_at {
            row["title"] = json!("Updated pagination fixture");
            row["updated_at"] = json!(when);
        }
        row
    };
    let body = match route {
        Route::Items => {
            let mut records = pr::response(Phase::PrLoad, Route::Items)?.body;
            records.as_array_mut()?.push(issue());
            records
        }
        Route::Item | Route::ScheduledIssue => issue(),
        Route::CommentsFirst => json!([comment(5101, None)]),
        Route::CommentsNext => json!([comment(5102, None)]),
        Route::MixedRecentComments | Route::MixedFollowupComments => {
            json!([comment(5101, edited_at)])
        }
        Route::UpdatedItems if changed => json!([issue()]),
        Route::UpdatedItems | Route::UpdatedComments | Route::Timeline => json!([]),
        Route::PrPinReviews => graph_review(changed),
        Route::PrPinThreads => return pr_pins::response(Phase::PrPinLoad, route),
        Route::PrPinChecks => json!({"total_count":1,"check_runs":[{
            "id":7202,"head_sha":HEAD,"name":"Fixture unit tests","status":if changed {"completed"} else {"in_progress"},
            "conclusion":if changed {Some("success")} else {None},
            "completed_at":edited_at
        }]}),
        Route::PrPinStatuses => {
            json!([{"id":7203,"context":"fixture-lint","state":"success","updated_at":"2021-01-01T00:00:00Z"}])
        }
        Route::MixedStatusValidated => {
            return Some(Response {
                status: 304,
                reason: "Not Modified",
                body: Value::Null,
                sso: None,
            });
        }
        _ => return pr::response(Phase::PrLoad, route),
    };
    Some(Response {
        status: 200,
        reason: "OK",
        body,
        sso: None,
    })
}

pub(super) fn trace(rows: &[RequestEvidence]) -> bool {
    use Route::*;
    let count = |phase, route| {
        rows.iter()
            .filter(|r| r.phase == phase && r.route == route)
            .count()
    };
    for row in rows {
        let common = matches!(
            row.route,
            User | Repository
                | Discovery
                | Graphql
                | Item
                | ScheduledIssue
                | UpdatedItems
                | UpdatedInlineComments
                | OpenPullIndex
                | PullA
                | CompareA
                | PrPinItem42
                | PrPinChecks
                | PrPinStatuses
                | PrPinReviews
                | PrPinThreads
                | PullActivityA
                | MixedStatusValidated
        );
        let permitted = match row.phase {
            Phase::MixedBefore => {
                common
                    || matches!(
                        row.route,
                        Items | CommentsFirst | Timeline | UpdatedComments
                    )
            }
            Phase::MixedAfter => {
                common
                    || matches!(
                        row.route,
                        MixedRecentComments | MixedFollowupComments | CommentsNext
                    )
            }
            _ => false,
        };
        let status = if row.route == MixedStatusValidated {
            304
        } else {
            200
        };
        if !permitted || !row.fixture_authorization || row.status != status {
            return false;
        }
    }
    let before = Phase::MixedBefore;
    let after = Phase::MixedAfter;
    count(before, CommentsFirst) == 1
        && count(before, Timeline) == 1
        && count(after, CommentsNext) == 1
        && count(after, MixedRecentComments) == 1
        && [
            Items,
            UpdatedComments,
            PrPinReviews,
            PrPinChecks,
            PrPinStatuses,
        ]
        .into_iter()
        .all(|r| count(before, r) > 0)
        && [
            UpdatedItems,
            MixedStatusValidated,
            PrPinReviews,
            PrPinChecks,
        ]
        .into_iter()
        .all(|r| count(after, r) > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{SecondsFormat, TimeDelta};
    use std::io::{Read, Write};

    #[test]
    fn mixed_catchup_serves_exact_validator_and_saved_watermark_over_http() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = AccessFixture::start(temp.path()).unwrap();
        let get = |target: &str, headers: &str| {
            let mut socket = TcpStream::connect(fixture.address()).unwrap();
            write!(
                socket,
                "GET {target} HTTP/1.1\r\nAuthorization: {FIXTURE_AUTHORIZATION}\r\n{headers}\r\n"
            )
            .unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            reply
        };
        fs::write(fixture.phase_file(), "mixed-before").unwrap();
        let status = format!("/repositories/71/commits/{HEAD}/statuses?per_page=20&page=1");
        let baseline_reply = get(&status, "");
        assert!(baseline_reply.contains("fixture-lint") && baseline_reply.contains(STATUS_ETAG));
        assert!(get(COMMENTS_FIRST, "").contains("rel=\"next\""));
        let watermark = Utc::now() - TimeDelta::minutes(1);
        fs::write(
            temp.path().join("mixed-comment-watermark.txt"),
            watermark.timestamp_millis().to_string(),
        )
        .unwrap();
        fs::write(
            temp.path().join("mixed-edit-time.txt"),
            Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        )
        .unwrap();
        fs::write(fixture.phase_file(), "mixed-after").unwrap();
        let exact = (watermark - TimeDelta::minutes(5))
            .to_rfc3339_opts(SecondsFormat::Secs, true)
            .replace(':', "%3A");
        let feed = format!("{SYNC_COMMENTS}{exact}&per_page=20&page=1");
        let stale = format!("{SYNC_COMMENTS}2018-01-02T00%3A00%3A00Z&per_page=20&page=1");
        assert!(get(&stale, "").starts_with("HTTP/1.1 404"));
        assert!(
            get(&feed, "").contains("The original comment changed while Threadline was closed.")
        );
        assert!(get(&feed, "").starts_with("HTTP/1.1 200"));
        assert!(get(&stale, "").starts_with("HTTP/1.1 404"));
        assert!(get(&status, "If-None-Match: \"different\"\r\n").contains("fixture-lint"));
        let unchanged = get(&status, &format!("If-None-Match: {STATUS_ETAG}\r\n"));
        assert!(unchanged.starts_with("HTTP/1.1 304") && unchanged.ends_with("\r\n\r\n"));
        assert!(get(COMMENTS_NEXT, "").contains("IC_5102"));
        assert_eq!(
            comment(5101, None)["node_id"],
            comment(5101, Some("2024-04-01T00:00:00Z"))["node_id"]
        );
        assert_ne!(graph_review(false), graph_review(true));
    }

    #[test]
    fn mixed_catchup_rejects_offline_traffic_and_replayed_history() {
        use Route::*;
        let mut before: Vec<_> = [
            Items,
            CommentsFirst,
            Timeline,
            UpdatedComments,
            PrPinReviews,
            PrPinChecks,
            PrPinStatuses,
        ]
        .map(|route| RequestEvidence {
            route,
            phase: Phase::MixedBefore,
            status: 200,
            fixture_authorization: true,
        })
        .into();
        before.extend(
            [
                MixedRecentComments,
                CommentsNext,
                UpdatedItems,
                PrPinReviews,
                PrPinChecks,
                MixedStatusValidated,
            ]
            .map(|route| RequestEvidence {
                route,
                phase: Phase::MixedAfter,
                status: if route == MixedStatusValidated {
                    304
                } else {
                    200
                },
                fixture_authorization: true,
            }),
        );
        assert!(trace(&before));
        for (phase, route) in [
            (Phase::MixedOffline, User),
            (Phase::MixedAfter, Items),
            (Phase::MixedAfter, Timeline),
            (Phase::MixedAfter, CommentsFirst),
            (Phase::MixedAfter, CommentsNext),
            (Phase::MixedAfter, UpdatedComments),
        ] {
            before.push(RequestEvidence {
                phase,
                route,
                status: 200,
                fixture_authorization: true,
            });
            assert!(!trace(&before));
            before.pop();
        }
        before.pop();
        assert!(!trace(&before));
    }
}

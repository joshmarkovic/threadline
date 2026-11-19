use super::*;
use std::time::Instant;

const COMMENT: &str = "Threadline lifecycle submitted comment.";
const COMMENTS: &str = "/repos/fixture-org/private/issues/1/comments";

fn active(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::LifecycleOffline
            | Phase::LifecycleReady
            | Phase::LifecycleDelay
            | Phase::LifecycleCrash
            | Phase::LifecycleRecovery
            | Phase::LifecycleSignedOut
    )
}

pub(super) fn read_route(phase: Phase, path: &str) -> Option<Route> {
    if !active(phase) {
        return None;
    }
    backlog::read_route(Phase::BacklogAuth, path)
}

pub(super) fn post(phase: Phase, path: &str, body: &Value) -> Option<Route> {
    if !active(phase) || path != COMMENTS {
        return None;
    }
    Some(if *body == json!({"body": COMMENT}) {
        Route::LifecycleComment
    } else {
        Route::Unexpected
    })
}

pub(super) fn hold(
    phase: Phase,
    route: Route,
    phase_file: &Path,
    stopping: &AtomicBool,
) -> io::Result<Option<u16>> {
    let marker = match (phase, route) {
        (Phase::LifecycleDelay, Route::User) => "lifecycle-probe-held",
        (Phase::LifecycleCrash, Route::LifecycleComment) => "lifecycle-comment-held",
        _ => return Ok(None),
    };
    let marker = phase_file.with_file_name(marker);
    // The record is fixture-owned and contains no provider data or credential.
    fs::write(&marker, b"held\n")?;
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline && !stopping.load(Ordering::Acquire) {
        if let Ok(next) = Phase::read(phase_file)
            && next != phase
        {
            return Ok((phase == Phase::LifecycleCrash).then_some(0));
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(Some(0))
}

pub(super) fn response(phase: Phase, route: Route) -> Option<Response> {
    if !active(phase) {
        return None;
    }
    if matches!(route, Route::UpdatedItems | Route::UpdatedComments) {
        return Some(Response {
            status: 200,
            reason: "OK",
            body: json!([]),
            sso: None,
        });
    }
    backlog::response(Phase::BacklogAuth, route)
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    records.iter().all(|entry| {
        entry.fixture_authorization
            && !matches!(
                entry.phase,
                Phase::LifecycleOffline | Phase::LifecycleSignedOut
            )
            && entry.route != Route::Unexpected
            && matches!(entry.status, 0 | 200)
    }) && records.iter().any(|entry| {
        entry.phase == Phase::LifecycleDelay && entry.route == Route::User && entry.status == 200
    }) && records
        .iter()
        .filter(|entry| entry.route == Route::LifecycleComment)
        .count()
        == 1
        && records.iter().any(|entry| {
            entry.phase == Phase::LifecycleCrash
                && entry.route == Route::LifecycleComment
                && entry.status == 0
        })
        && records.iter().any(|entry| {
            entry.phase == Phase::LifecycleRecovery
                && entry.route == Route::User
                && entry.status == 200
        })
        && !records.iter().any(|entry| {
            matches!(
                entry.route,
                Route::MergePut
                    | Route::ReviewCreated
                    | Route::ReviewSubmitted
                    | Route::CommentCreated
            )
        })
}

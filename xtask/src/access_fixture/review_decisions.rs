use super::*;

const SUMMARY: &str = "Threadline synthetic review summary.";
const FIRST: &str = "Threadline synthetic inline review note.";
const SECOND: &str = "Threadline synthetic second inline review note.";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DecisionStep {
    ApprovalList,
    ApprovalCreate,
    ApprovalRead,
    ApprovalReadInterrupted,
    ApprovalSubmit,
    ChangesList,
    ChangesCreate,
    ChangesRead,
    ChangesSubmit,
}

use DecisionStep::*;

impl DecisionStep {
    fn approval(self) -> bool {
        matches!(
            self,
            ApprovalList | ApprovalCreate | ApprovalRead | ApprovalReadInterrupted | ApprovalSubmit
        )
    }
    fn identity(self) -> (u32, u32, String) {
        if self.approval() {
            (42, 92, "b".repeat(40))
        } else {
            (43, 93, "e".repeat(40))
        }
    }
}

pub(super) fn active(phase: Phase) -> bool {
    matches!(
        phase,
        Phase::ReviewDecisions | Phase::ReviewPredecessor | Phase::ReviewResume
    )
}

pub(super) fn get(phase: Phase, target: &str) -> Option<Route> {
    if !active(phase) {
        return None;
    }
    let step = match target.strip_prefix("/repos/fixture-org/private/pulls/")? {
        "42/reviews?per_page=100&page=1" if phase != Phase::ReviewResume => ApprovalList,
        "42/reviews/92" if phase == Phase::ReviewPredecessor => ApprovalReadInterrupted,
        "42/reviews/92" => ApprovalRead,
        "43/reviews?per_page=100&page=1" if phase == Phase::ReviewDecisions => ChangesList,
        "43/reviews/93" if phase == Phase::ReviewDecisions => ChangesRead,
        _ => return None,
    };
    Some(Route::ReviewDecision(step))
}

pub(super) fn post(phase: Phase, path: &str, body: &Value) -> Option<Route> {
    if !active(phase) {
        return None;
    }
    let step = match path.strip_prefix("/repos/fixture-org/private/pulls/")? {
        "42/reviews" if phase != Phase::ReviewResume => ApprovalCreate,
        "42/reviews/92/events" if phase != Phase::ReviewPredecessor => ApprovalSubmit,
        "43/reviews" if phase == Phase::ReviewDecisions => ChangesCreate,
        "43/reviews/93/events" if phase == Phase::ReviewDecisions => ChangesSubmit,
        _ => return None,
    };
    let (_, _, head) = step.identity();
    let expected = match step {
        ApprovalCreate => json!({"commit_id":head,"body":SUMMARY,"comments":[
            {"path":"src/first.rs","line":1,"side":"RIGHT","body":FIRST},
            {"path":"src/first.rs","line":3,"side":"RIGHT","body":SECOND}
        ]}),
        ChangesCreate => json!({"commit_id":head,"body":SUMMARY,"comments":[
            {"path":"src/second.rs","line":1,"side":"RIGHT","body":FIRST}
        ]}),
        ApprovalSubmit => json!({"event":"APPROVE","body":SUMMARY}),
        ChangesSubmit => json!({"event":"REQUEST_CHANGES","body":SUMMARY}),
        _ => return None,
    };
    (*body == expected).then_some(Route::ReviewDecision(step))
}

pub(super) fn interrupt_read(phase_file: &Path, stopping: &AtomicBool) -> io::Result<()> {
    use std::time::Instant;
    let marker = phase_file.with_file_name("review-predecessor-durable.txt");
    fs::write(&marker, b"92\n")?;
    let started = Instant::now();
    while !stopping.load(Ordering::Acquire) && started.elapsed() < Duration::from_secs(12) {
        if Phase::read(phase_file).ok() != Some(Phase::ReviewPredecessor) {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    fs::remove_file(marker)
}

pub(super) fn predecessor_trace(records: &[RequestEvidence]) -> bool {
    use Phase::{ReviewPredecessor, ReviewResume};
    records.iter().all(|row| {
        row.fixture_authorization
            && matches!(row.phase, ReviewPredecessor | ReviewResume)
            && row.route != Route::Unexpected
            && (row.status == 200
                || row.route == Route::ReviewDecision(ApprovalReadInterrupted) && row.status == 0)
    }) && records
        .iter()
        .filter_map(|row| match row.route {
            Route::ReviewDecision(step) => Some((row.phase, step, row.status)),
            _ => None,
        })
        .eq([
            (ReviewPredecessor, ApprovalList, 200),
            (ReviewPredecessor, ApprovalCreate, 200),
            (ReviewPredecessor, ApprovalReadInterrupted, 0),
            (ReviewResume, ApprovalRead, 200),
            (ReviewResume, ApprovalSubmit, 200),
        ])
}

pub(super) fn response(route: Route) -> Option<Response> {
    let Route::ReviewDecision(step) = route else {
        return None;
    };
    let (number, id, head) = step.identity();
    let state = match step {
        ApprovalSubmit => "APPROVED",
        ChangesSubmit => "CHANGES_REQUESTED",
        _ => "PENDING",
    };
    let mut body = json!({
        "id":id, "state":state, "user":{"id":41,"login":"fixture-user"},
        "commit_id":head, "body":SUMMARY,
        "pull_request_url":format!("https://api.github.com/repos/fixture-org/private/pulls/{number}")
    });
    if matches!(step, ApprovalList | ChangesList) {
        body["id"] = json!(190);
        body["user"] = json!({"id":75,"login":"fixture-author"});
        body = json!([body]);
    }
    Some(Response {
        status: 200,
        reason: "OK",
        body,
        sso: None,
    })
}

pub(super) fn trace(records: &[RequestEvidence]) -> bool {
    records.iter().all(|row| {
        row.phase == Phase::ReviewDecisions
            && row.fixture_authorization
            && row.route != Route::Unexpected
            && row.status == 200
    }) && records
        .iter()
        .filter_map(|row| match row.route {
            Route::ReviewDecision(step) => Some(step),
            _ => None,
        })
        .eq([
            ApprovalList,
            ApprovalCreate,
            ApprovalRead,
            ApprovalSubmit,
            ChangesList,
            ChangesCreate,
            ChangesRead,
            ChangesSubmit,
        ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_has_an_empty_activity_snapshot_before_native_pause() {
        for phase in [
            Phase::ReviewDecisions,
            Phase::ReviewPredecessor,
            Phase::ReviewResume,
            Phase::ThreadQueue,
            Phase::ThreadOutdated,
        ] {
            for route in [Route::UpdatedItems, Route::UpdatedComments] {
                let reply = crate::access_fixture::response(phase, route, true, false);
                assert_eq!(reply.status, 200);
                assert_eq!(reply.body, json!([]));
            }
            assert_eq!(
                crate::access_fixture::response(phase, Route::Unexpected, true, false).status,
                404
            );
        }
    }

    #[test]
    fn decision_fixture_requires_distinct_anchors_and_exact_predecessors() {
        let phase = Phase::ReviewDecisions;
        let create = "/repos/fixture-org/private/pulls/42/reviews";
        let valid = json!({"commit_id":"b".repeat(40),"body":SUMMARY,"comments":[
            {"path":"src/first.rs","line":1,"side":"RIGHT","body":FIRST},
            {"path":"src/first.rs","line":3,"side":"RIGHT","body":SECOND}
        ]});
        assert_eq!(
            post(phase, create, &valid),
            Some(Route::ReviewDecision(ApprovalCreate))
        );
        let mut stale = valid;
        stale["comments"][1]["line"] = json!(1);
        assert!(post(phase, create, &stale).is_none());
        assert!(
            post(
                phase,
                &format!("{create}/93/events"),
                &json!({"event":"APPROVE","body":SUMMARY})
            )
            .is_none()
        );

        let mut records: Vec<_> = [
            ApprovalList,
            ApprovalCreate,
            ApprovalRead,
            ApprovalSubmit,
            ChangesList,
            ChangesCreate,
            ChangesRead,
            ChangesSubmit,
        ]
        .into_iter()
        .map(|step| RequestEvidence {
            phase,
            route: Route::ReviewDecision(step),
            fixture_authorization: true,
            status: 200,
        })
        .collect();
        assert!(trace(&records));
        records.swap(2, 3);
        assert!(!trace(&records));
        let result = response(Route::ReviewDecision(ChangesSubmit)).unwrap();
        assert_eq!(result.body["user"]["id"], 41);
        assert_eq!(result.body["state"], "CHANGES_REQUESTED");
    }

    #[test]
    fn predecessor_fixture_allows_only_the_unfinished_submit_after_restart() {
        let phase = Phase::ReviewPredecessor;
        let resumed = Phase::ReviewResume;
        let read = "/repos/fixture-org/private/pulls/42/reviews/92";
        assert_eq!(
            get(phase, read),
            Some(Route::ReviewDecision(ApprovalReadInterrupted))
        );
        assert_eq!(
            get(resumed, read),
            Some(Route::ReviewDecision(ApprovalRead))
        );
        let take = |phase, step, status| RequestEvidence {
            phase,
            route: Route::ReviewDecision(step),
            status,
            fixture_authorization: true,
        };
        let mut records = vec![
            take(phase, ApprovalList, 200),
            take(phase, ApprovalCreate, 200),
            take(phase, ApprovalReadInterrupted, 0),
            take(resumed, ApprovalRead, 200),
            take(resumed, ApprovalSubmit, 200),
        ];
        assert!(predecessor_trace(&records));
        records.insert(3, take(resumed, ApprovalCreate, 200));
        assert!(!predecessor_trace(&records));
    }
}

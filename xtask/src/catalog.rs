use serde::Serialize;

const SCENARIOS: &str = include_str!("../../docs/spec/test_scenarios.csv");

#[derive(Debug, Serialize)]
pub struct Associations {
    pub requirement_ids: Vec<&'static str>,
    pub acceptance_ids: Vec<&'static str>,
}

pub fn scenario_ids() -> impl Iterator<Item = &'static str> {
    SCENARIOS
        .lines()
        .skip(1)
        .filter_map(|row| row.split_once(',').map(|(id, _)| id))
}

pub fn fixture_revision(case: &str) -> Option<&'static str> {
    if case == "S06-SHARED-PR-PANES" {
        return Some("s06-shared-pr-panes-v1");
    }
    if case == "S08-COMMENT-DEPENDENCY" {
        return Some("s08-native-comment-dependency-v1");
    }
    if case == "S07-ISSUE-CONFLICT" {
        return Some("s07-native-issue-conflict-v1");
    }
    if case == "S08-REVIEW-PREDECESSOR" {
        return Some("s08-native-review-predecessor-v1");
    }
    if case == "S12-HISTORICAL-READER" {
        return Some("s12-historical-reader-v1");
    }
    if case == "S08-THREAD-QUEUE" {
        return Some("s08-native-thread-queue-v1");
    }
    if case == "S08-REVIEW-DECISIONS" {
        return Some("s08-native-review-decisions-v1");
    }
    if case == "S11-NATIVE-ACTIVITY" {
        return Some("s11-native-activity-v4");
    }
    if case == "S12-NATIVE-LIFECYCLE" {
        return Some("s12-native-lifecycle-v2");
    }
    if case == "S12-PANE-POSITIONS" {
        return Some("s12-pane-positions-v1");
    }
    if matches!(case, "E2E-014" | "S12-READER-CONCURRENT") {
        return Some("s12-large-workspace-v2");
    }
    if case == "E2E-020" {
        return Some("threadline-s12-installed-runtime-v1");
    }
    if matches!(case, "UT-019" | "IT-021") {
        return Some("s12-release-bootstrap-v1");
    }
    if matches!(case, "E2E-012" | "E2E-018") {
        return Some("threadline-s11-installed-fixture-v1");
    }
    if case == "E2E-013" {
        return Some("s11-native-notification-schedule-v1");
    }
    if matches!(case, "UT-014" | "UT-015" | "IT-018") {
        return Some("s11-activity-rules-v1");
    }
    if case == "IT-019" {
        return Some("s12-upgrade-durability-v1");
    }
    if case == crate::install::LOCAL_CASE {
        return Some("s12-windows-local-installer-v1");
    }
    if case == "S09-MERGE-QUEUE" {
        return Some("s09-required-merge-queue-v1");
    }
    if matches!(case, "UT-017" | "IT-014" | "IT-015" | "E2E-009") {
        return Some("s09-direct-merge-v1");
    }
    if case == "E2E-010" {
        return Some("s09-credential-boundary-v1");
    }
    if case == "S10-HISTORY-MONITOR" {
        return Some("s10-history-monitor-v1");
    }
    if case == "S05-WINDOWS-RATE-LIMIT" {
        return Some("s05-native-rate-limit-v1");
    }
    if case == "E2E-011" {
        return Some("s10-history-endpoints-v3");
    }
    if matches!(case, "UT-016" | "S10-HISTORY-HTTP") {
        return Some("s10-history-dag-v1");
    }
    if matches!(case, "IT-011" | "IT-013" | "UI-004" | "E2E-007" | "E2E-008") {
        return Some("s08-revision-bound-reviews-v1");
    }
    if matches!(
        case,
        "IT-008" | "IT-009" | "IT-010" | "IT-012" | "S07-WRITE-WORKER" | "E2E-006" | "E2E-019"
    ) {
        return Some("s07-write-engine-v1");
    }
    if case == "LIVE-001" {
        return Some("s03-live-read-allowlist-v1");
    }
    if case == "LIVE-002" {
        return Some("s07-live-issue-1-v1");
    }
    if case == "LIVE-004" {
        return Some("s09-live-pr-3-v1");
    }
    if case == "LIVE-003" {
        return Some("s08-live-pr-2-v1");
    }
    if matches!(
        case,
        "UT-011" | "UT-012" | "IT-006" | "IT-016" | "IT-017" | "E2E-005"
    ) {
        return Some(include_str!("../../fixtures/pr-reading-revision.txt").trim_ascii());
    }
    if matches!(
        case,
        "UT-002"
            | "UT-005"
            | "UT-006"
            | "UT-007"
            | "IT-007"
            | "S05-FEED-HTTP"
            | "S05-SYNC-WORKER"
            | "S05-WINDOWS-SYNC"
    ) {
        return Some("s05-sync-v1");
    }
    if matches!(
        case,
        "UT-008"
            | "UT-013"
            | "UT-018"
            | "IT-003"
            | "E2E-003"
            | "E2E-004"
            | "S04-READER-PERF"
            | "S12-PHYSICAL-READER"
            | "S04-READ-TRANSPORT"
            | "S04-CACHED-STORE"
            | "S04-READ-SERVICE"
    ) {
        return Some("s04-reader-v1");
    }
    if case.starts_with("S01-") || matches!(case, "UT-021" | "E2E-001" | "E2E-015") {
        return Some(include_str!("../../fixtures/revision.txt").trim_ascii());
    }
    if matches!(
        case,
        "UT-010"
            | "UT-020"
            | "IT-004"
            | "IT-005"
            | "IT-020"
            | "E2E-002"
            | "S03-ACCOUNT-PROFILES"
            | "S03-ACCESS-STORAGE"
    ) {
        return Some(include_str!("../../fixtures/github-access-revision.txt").trim_ascii());
    }
    matches!(
        case,
        "UT-001"
            | "UT-003"
            | "UT-004"
            | "UT-009"
            | "IT-001"
            | "IT-002"
            | "S02-RESOURCE-CONTRACTS"
            | "S02-DURABILITY"
            | "S02-DISK-FULL"
            | "S02-FIXTURES"
    )
    .then(|| include_str!("../../fixtures/storage-revision.txt").trim_ascii())
}

pub fn associations(case: &str) -> Associations {
    // These cases are implementation checks that do not have their own catalog row.
    let (requirements, acceptance) = match case {
        "S01-CORE" => ("FND-04;DATA-05;DATA-06;TEST-02", "AC-89"),
        "S01-DIFF-FIXTURE" => ("FND-02;TEST-02", "AC-89"),
        "S01-SIMULATOR" => ("FND-04;TEST-03;TEST-05", "AC-90"),
        "S01-PLATFORM" => ("PORT-02;TEST-03", "AC-83"),
        "S01-GPUI" => ("FND-02;TEST-04", "AC-84"),
        "S01-WINDOWS-SHELL" => ("PORT-05;NOTIFY-01;TEST-04", "AC-86"),
        "S01-WINDOWS-TOAST" => ("FND-02;PORT-05;NOTIFY-01;TEST-04", "AC-33;AC-86"),
        "S01-WINDOWS-BASELINE" => ("FND-01;TEST-04;TEST-08", "AC-35;AC-84"),
        "S02-RESOURCE-CONTRACTS" => (
            "CACHE-03;CACHE-05;CACHE-09;CACHE-10;SYNC-11;NOTIFY-15",
            "AC-07;AC-08;AC-90",
        ),
        "S02-DURABILITY" => (
            "CACHE-07;CACHE-13;OUTBOX-10;DATA-01;TEST-03;TEST-05",
            "AC-10;AC-68;AC-73;AC-74;AC-92",
        ),
        "S02-FIXTURES" => ("TEST-02;TEST-03;TEST-05", "AC-35;AC-90"),
        "S02-DISK-FULL" => ("OUTBOX-01;OUTBOX-10;TEST-03;TEST-05", "AC-58;AC-92"),
        "S03-ACCOUNT-PROFILES" => (
            "AUTH-04;AUTH-05;AUTH-06;AUTH-11;TEST-03",
            "AC-01;AC-02;AC-03;AC-90",
        ),
        "S03-ACCESS-STORAGE" => ("AUTH-06;AUTH-11;CACHE-08;OUTBOX-09;TEST-03", "AC-67;AC-90"),
        "S04-READER-PERF" | "S12-PHYSICAL-READER" => {
            ("CACHE-01;SEARCH-01;WORKSPACE-08;TEST-08", "AC-35")
        }
        "S12-HISTORICAL-READER" => ("CACHE-04;SEARCH-01;SEARCH-02;OFFLINE-03;TEST-04", "AC-04"),
        "S12-READER-CONCURRENT" => (
            "CACHE-01;SEARCH-01;SYNC-06;WORKSPACE-08;TEST-08",
            "AC-35;AC-48",
        ),
        "S08-REVIEW-DECISIONS" => (
            "REVIEW-01;REVIEW-02;REVIEW-03;REVIEW-04;TEST-04",
            "AC-16;AC-18;AC-65",
        ),
        "S08-THREAD-QUEUE" => ("REVIEW-01;REVIEW-04;OUTBOX-03;TEST-04", "AC-63;AC-65"),
        "S06-SHARED-PR-PANES" => ("WORKSPACE-07;CACHE-02;PR-02;TEST-04", "AC-44"),
        "S07-ISSUE-CONFLICT" => ("ISSUE-03;ISSUE-05;OUTBOX-03;TEST-04", "AC-13;AC-15"),
        "S08-COMMENT-DEPENDENCY" => ("OUTBOX-06;TEST-04;TEST-05", "AC-63;AC-92"),
        "S08-REVIEW-PREDECESSOR" => ("REVIEW-04;OUTBOX-03;TEST-04", "AC-63"),
        "S09-MERGE-QUEUE" => ("MERGE-01;MERGE-03;MERGE-04;OUTBOX-03;TEST-04", "AC-77"),
        "S12-NATIVE-LIFECYCLE" => (
            "AUTH-05;CACHE-13;OUTBOX-10;TEST-04;TEST-05",
            "AC-68;AC-73;AC-74;AC-91;AC-92",
        ),
        "S11-NATIVE-ACTIVITY" => (
            "NOTIFY-01;NOTIFY-17;SYNC-11;TEST-04",
            "AC-27;AC-28;AC-29;AC-31;AC-32;AC-39;AC-40",
        ),
        "S04-READ-TRANSPORT" => (
            "CACHE-05;ISSUE-01;SEARCH-02;SEARCH-03;TEST-03",
            "AC-07;AC-08;AC-09",
        ),
        "S04-CACHED-STORE" | "S04-READ-SERVICE" => (
            "CACHE-03;CACHE-05;OFFLINE-03;SEARCH-01;TEST-03",
            "AC-06;AC-07;AC-08;AC-09",
        ),
        "S07-WRITE-WORKER" => (
            "WRITE-04;OUTBOX-07;OUTBOX-08;TEST-03",
            "AC-13;AC-15;AC-62;AC-67",
        ),
        "S10-HISTORY-HTTP" => ("GRAPH-01;GRAPH-02;GRAPH-03;TEST-03", "AC-24;AC-25;AC-26"),
        "S10-HISTORY-MONITOR" => ("NOTIFY-04;SYNC-03;TEST-03", "AC-39;AC-54"),
        "S05-FEED-HTTP" => ("SYNC-05;SYNC-06;SYNC-07;SYNC-10;TEST-03", "AC-12;AC-54"),
        "S05-SYNC-WORKER" => (
            "SYNC-02;SYNC-03;SYNC-09;NOTIFY-13;TEST-03",
            "AC-11;AC-54;AC-57",
        ),
        "S05-WINDOWS-SYNC" => (
            "SYNC-01;SYNC-04;SYNC-08;CONNECT-01;CONNECT-02;TEST-04",
            "AC-11;AC-12;AC-57",
        ),
        "S05-WINDOWS-RATE-LIMIT" => ("SYNC-10;CONNECT-03;CACHE-03;TEST-04", "AC-12"),
        "S12-PANE-POSITIONS" => (
            "WORKSPACE-01;WORKSPACE-02;WORKSPACE-04;WORKSPACE-07;TEST-04",
            "AC-42",
        ),
        _ => catalog_associations(case).unwrap_or_default(),
    };
    Associations {
        requirement_ids: identifiers(requirements),
        acceptance_ids: identifiers(acceptance),
    }
}

fn identifiers(field: &'static str) -> Vec<&'static str> {
    field
        .trim_matches('"')
        .split(';')
        .filter(|id| !id.is_empty())
        .collect()
}

fn catalog_associations(case: &str) -> Option<(&'static str, &'static str)> {
    let row = SCENARIOS
        .lines()
        .find(|row| row.split_once(',').is_some_and(|(id, _)| id == case))?;
    association_columns(row)
}

fn association_columns(row: &str) -> Option<(&str, &str)> {
    // The catalog has one scenario per line. A quoted title may contain commas.
    let mut quoted = false;
    let mut fields = row.split(move |character| {
        if character == '"' {
            quoted = !quoted;
        }
        character == ',' && !quoted
    });
    Some((fields.nth(6)?, fields.next()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_catalog_and_internal_associations() {
        let runner = associations("UT-021");
        assert_eq!(runner.requirement_ids, ["TEST-01", "TEST-02", "TEST-08"]);
        assert_eq!(runner.acceptance_ids, ["AC-89"]);
        assert_eq!(
            associations("S01-WINDOWS-TOAST").acceptance_ids,
            ["AC-33", "AC-86"]
        );
        assert!(associations("UT-020").acceptance_ids.is_empty());
        assert!(associations("request").requirement_ids.is_empty());
    }

    #[test]
    fn finds_associations_after_quoted_commas() {
        assert_eq!(
            association_columns(
                "UT-000,Unit,\"A \"\"quoted\"\" title, with commas\",01,windows,Windows,TEST-02,AC-89,"
            ),
            Some(("TEST-02", "AC-89"))
        );
    }
}

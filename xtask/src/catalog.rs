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

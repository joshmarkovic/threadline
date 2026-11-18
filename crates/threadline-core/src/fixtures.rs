use crate::domain::{HostName, ItemKey, ItemKind, RepositoryKey, RepositoryName, Revision};
use crate::github::{ItemSummary, ReadSummary};

pub const FIXTURE_REVISION: &str = "s01-feasibility-v1";

pub struct FeasibilityFixture {
    pub request: ReadSummary,
    pub summary: ItemSummary,
}

impl FeasibilityFixture {
    pub fn new(seed: u64) -> Self {
        let item = ItemKey {
            repository: RepositoryKey {
                host: HostName::new("github.com").unwrap(),
                profile: 1.try_into().unwrap(),
                id: 120.try_into().unwrap(),
            },
            number: 42.try_into().unwrap(),
        };
        let route = RepositoryName::new("threadline-fixtures", "signal-lab").unwrap();
        let summary = ItemSummary::new(
            item.clone(),
            Revision::Synthetic(seed),
            4201.try_into().unwrap(),
            ItemKind::PullRequest,
            "Background preview ready".into(),
            Some(format!(
                "# Signal Lab\n\nSynthetic PR preview from fixture seed {seed}."
            )),
        );
        Self {
            request: ReadSummary::new(item, route),
            summary,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_fixture_replays_without_changing_resource_identity() {
        let seed = std::env::var("THREADLINE_TEST_SEED")
            .ok()
            .map(|value| value.parse::<u64>().unwrap())
            .unwrap_or(42);
        let first = FeasibilityFixture::new(seed);
        let replay = FeasibilityFixture::new(seed);
        let alternate = FeasibilityFixture::new(seed.wrapping_add(1));
        assert_eq!(first.request, replay.request);
        assert_eq!(first.summary, replay.summary);
        assert_eq!(first.request, alternate.request);
        assert_ne!(first.summary.revision(), alternate.summary.revision());
        assert_ne!(first.summary.body(), alternate.summary.body());
        assert_eq!(first.summary.kind, ItemKind::PullRequest);
    }
}

# Sprint 10 branch history and comparisons

Five real SQLite history tests passed on Windows 11 x64 at `f17e39c`. They covered bounded graph pagination, stable merge-parent lanes, branch baselines, force-pushes, stale responses and offline restart. Comparison tests bound selected parent, merge base, file rename, source side and whitespace options to immutable commits. An incomplete or binary patch retains its recorded availability. [Windows test identities and results](evidence/sprint-08/windows-regressions.json) · [History storage](../../crates/threadline-storage/src/history.rs)

E2E-011 native graph navigation and larger-workload release evidence remain separate. No new scroll benchmark was run for this verification. [Sprint requirements](../spec/sprints/sprint_10_history_and_comparisons.md)

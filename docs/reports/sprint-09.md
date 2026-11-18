# Sprint 09 direct merges

On Windows 11 x64 at `f17e39c`, the direct-merge provider test and six SQLite merge tests passed. The local HTTP fixture checked the exact expected head and merge method. SQLite tests checked branch policy, a changed head, required merge queues, changed base branches, concurrent review/merge claims and read-only recovery after an uncertain outcome. A changed target requires fresh user consent. [Windows test identities and results](evidence/sprint-08/windows-regressions.json) · [Merge storage](../../crates/threadline-storage/src/merges.rs)

These are development checks. Windows native merge scenarios E2E-009 and E2E-010 and the separately authorized LIVE-004 scenario have distinct evidence requirements. [Sprint requirements](../spec/sprints/sprint_09_merge_workflows.md)

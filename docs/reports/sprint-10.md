# Sprint 10 branch history and comparisons

The Windows 11 native history scenario passed. The reader loaded older commits, kept both merge parents, opened the second parent's patch, and restored the exact two-commit comparison and file after an offline restart. [Sprint requirements](../spec/sprints/sprint_10_history_and_comparisons.md) · [Verification receipts](evidence/sprint-10/verification.json)

## Native Windows history

| Source and scope | Result |
| --- | --- |
| Clean `40fe4d4`, Windows 11 x64 at 144 DPI | Fail, exit 1. The branch and tag fixture requests returned HTTP 200. The driver timed out waiting for their native observation. |
| Clean driver `dd0f6a4`, same app binary and desktop | Fail, exit 1. The older graph and second-parent file opened. The fixture returned HTTP 404 for the exact two-commit comparison. |
| Clean app and driver `43a288a`, same desktop, seed 12345 | Pass, exit 0. Native controls expanded the graph from one commit with two unloaded parent boundaries to three commits with no boundaries. The driver selected the second parent and opened its patch. It then opened the distinct endpoint comparison and confirmed the original file in a separate offline process. All 20 fixture HTTP requests returned 200. |
| Clean app and driver `6ea3e43`, Windows 11 x64 at 96 DPI, seed 12345 | Pass, exit 0. After the second-parent and ordinary pair comparisons, the driver selected two divergent commits and opened `divergent.txt`. The comparison API returned the expected 404. The two exact commit, two tree, and two blob reads returned 200. A separate offline process restored the divergent endpoints and file. |

The passing invocation was `cargo xtask test e2e --suite full --case E2E-011 --binary <binary-path> --seed 12345 --report-dir <report-dir>`. The receipt identifies the executable and both earlier runs. The large workload and exact Windows release package remain separate verification. [Windows history scenario](../../tools/native/windows-history.ps1) · [Scenario ledger](../spec/test_platform_traceability.csv)

An exact two-commit comparison uses the provider patch only if its merge base matches the selected left endpoint. For divergent endpoints the worker reads the two immutable Git trees, skips identical subtrees, and compares changed text blobs within fixed request and size limits. Binary, truncated, unavailable, and permission-limited paths retain their evidence. Loopback tests cover the two-tree result and preserve previously cached content when the provider denies a refresh. [Exact comparison implementation](../../crates/threadline-core/src/github/access/reader/history/objects.rs) · [Verification receipts](evidence/sprint-10/verification.json)

## History storage and monitoring

Five SQLite history tests passed on native Windows at `f17e39c`. They exercise durable continuation, stable merge-parent lanes, immutable comparison identities, offline restoration of the graph position, and retained data after force-pushes. An incomplete or binary patch keeps its recorded availability. [Windows regression receipts](evidence/sprint-08/windows-regressions.json) · [History storage](../../crates/threadline-storage/src/history.rs)

The background worker enrolls the selected repositories' configured default and chosen branches, branch patterns, and followed pull requests. The first observation establishes a baseline. Positive head movement schedules at most two recent ancestry pages and leaves older history available through the saved continuation. Offline and pause hold automatic reads. Incomplete or inaccessible observations keep the last known tip. The loopback worker test observed the configured default branch, matching branch and followed pull request through real HTTP, then checked their pinned commits in SQLite. The clean `3fb08ab` Linux run passed one `S10-HISTORY-MONITOR` worker test and 10 `IT-006` integration tests. The scheduler tests cover restart, incomplete pages, stale claims, and bounded ancestry. These results cover observation. Native delivery for a watched branch remains a Sprint 11 gate. [Scheduler tests](../../crates/threadline-storage/tests/history_monitor.rs) · [Worker test](../../crates/threadline-app/src/account_tests.rs) · [Verification receipts](evidence/sprint-10/verification.json)

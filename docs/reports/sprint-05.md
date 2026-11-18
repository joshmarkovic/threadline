# Sprint 05 synchronization and activity observation

The app reads selected repositories through a shared scheduler. It tracks older changed items and comments independently of issue number, shares requests between panes, and retains the last completed checkpoint after interruption. The Windows 11 native driver observed both authenticated update feeds and restored four synchronization settings after a separate-process restart. That native journey exited 1 when the driver timed out reopening Accounts. [Sprint requirements](../spec/sprints/sprint_05_sync_and_observation.md) · [Verification receipts](evidence/sprint-05/verification.json)

## Scheduling and coverage

Each account and selected repository has durable subscriptions, claim generations, checkpoints, conditional validators, overlap windows and server retry floors in SQLite. A request can publish rows only while its claim still matches the active profile, credential generation, repository and subscribers. Initial hydration establishes an activity baseline. Later changes record provider object identity and update time. Removing a subscription cancels an otherwise unused fetch. Local time zones and daylight-saving transitions control the configured polling schedule. Work offline, background pause, queued-action pause and notification quiet settings persist independently. [Scheduler and observation](../../crates/threadline-storage/src/sync.rs) · [Time and retry policy](../../crates/threadline-core/src/sync/policy.rs) · [Running app worker](../../crates/threadline-app/src/sync_worker.rs)

## Verification

| Environment and test layer | Command or scope | Result |
| --- | --- | --- |
| Linux x64, clean `8ebe3a9` | `cargo xtask test unit --seed 12345 --report-dir <report-dir>` | Pass. Eighteen cases ran 70 tests. UT-002, UT-005, UT-006 and UT-007 cover sparse numbering, DST, controls and server retry floors. |
| Same host and source | `cargo xtask test integration --seed 12345 --report-dir <report-dir>` | Pass. Twenty-two cases ran 83 tests. IT-006 ran three tests for old-item updates, overlapping comments and revision-bound PR catch-up. IT-007 ran four tests for fair scheduling, cancellation, rate limits and restoration. Feed HTTP and worker integration each passed against local endpoints. |
| Same host and source | `cargo xtask test ui --seed 12345 --report-dir <report-dir>` | Pass. Four cases ran nine tests. |
| Windows 11 x64, native Rust test executables | Shared core, SQLite synchronization and GPUI app controls | Pass. Seventeen core tests, all eight SQLite sync tests and all twenty app tests passed. The Windows core suite exercised all four Sprint 05 unit selectors. |
| Windows 11 x64, 144 DPI, clean driver `8e445d2`, application `24c21ed`, seed 12345 | `cargo xtask test e2e --suite full --case S05-WINDOWS-SYNC --binary <binary-path> --seed 12345 --report-dir <report-dir>` | Fail. Guarded input reached both credential fields. The private fixture repository was selected and both update feeds returned 200. The restarted app retained Work offline, Pause background, Pause queued actions and notification quiet. The trace recorded zero offline requests. The driver timed out at Accounts afterward. |

The earlier targeted Linux run exercised IT-006 before the PR extension. Both original older-item and overlapping-feed tests passed. The clean receipt above also includes the revision-bound PR case. The development fixture and local HTTP simulator supplied account, item and rate-limit evidence. Package smoke, sustained production monitoring and later release-platform reruns remain separate. [Scenario ledger](../spec/test_scenarios.csv) · [Windows and Linux receipts](evidence/sprint-05/verification.json)

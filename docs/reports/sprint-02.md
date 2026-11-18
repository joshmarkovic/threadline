# Sprint 02 storage and identity

Threadline 0.1.0 · Windows first release and portable storage · November 18, 2026

Sprint 02 is complete for the initial Windows and portable storage assignment. The development app opens its durable SQLite profile before creating the workspace. The storage gates passed with synthetic data. The tests cover shared issue and pull-request identity (AC-50 and AC-51), confirmed transfers (AC-55), restartable coverage and preservation of user work. Cached-reader interactions, provider authorization and installed upgrades have their own acceptance gates. [Sprint 02 requirements](../spec/sprints/sprint_02_storage_and_identity.md) · [Clean verification receipts](evidence/sprint-02/local-verification.json)

## Delivered behavior

| Tickets | Verified behavior |
| --- | --- |
| T01, T02 | SQLite enforces a shared repository/number namespace and separate issue and pull-request API IDs. Concurrent issue-first and PR-first ingestion preserves richer fields, known merge state and one stable item. Verified transfers keep the local ID and previous locator. Ambiguous transfers require resolution. [Identity and migration tests](../../crates/threadline-storage/tests/identity.rs) |
| T03 | Resource families independently retain coverage, freshness, validation, errors and page cursors. A failed second page preserves its first page. Records and cursor advance in one transaction. Comparison keys, commit parents, mutable refs, activity baselines and delivery receipts survive reopening. [Resource tests](../../crates/threadline-storage/tests/resources.rs) |
| T04, T09 | The stored account owns its pane/tab order, distinct tab-instance IDs, settings and draft editor leases. Edit drafts retain their bases. Review drafts retain source anchors. Tabs with the same target preserve separate view and PR context. Signing out holds dispatch while local work remains readable. [Workspace and durability tests](../../crates/threadline-storage/tests/durability.rs) |
| T05, T10 | The store uses a bounded reader/writer pair, WAL, FTS5, foreign keys and transactional migrations. A v1 upgrade creates a backup. Failed upgrades, unknown schemas and corrupted files preserve the source. Recovery publishes to a new file and holds uncertain work for reconciliation. Cache cleanup preserves drafts and Outbox snapshots. [Database implementation](../../crates/threadline-storage/src/database.rs) · [Migration tests](../../crates/threadline-storage/tests/identity.rs) |
| T06 | The reopened Section 16 fixture contained 50,000 summaries, 250,000 comments (5,005 in one discussion), 40 comparison files, 400 commits with merges, 1,000 mixed Outbox actions with dependencies, two panes, 20 tabs and one review draft. A second small database reproduced the same seed. These are persisted workload counts. [Fixture assertions](../../crates/threadline-storage/tests/fixtures.rs) |
| T07, T08 | Enqueue commits the immutable snapshot, original locator, ordering, dependencies and pending projection in one transaction. A full-disk rollback preserves the editable draft. Independent SQLite connections race claim against cancellation. A restored copy cannot blindly replay a potentially completed send. A transferred item requires a new preflight against its current locator. [Outbox tests](../../crates/threadline-storage/tests/durability.rs) · [Full-disk rollback test](../../crates/threadline-storage/src/queue.rs) |

## Test evidence

The clean source was `f8ea378866cd5d4e468c0c725b708a7f512ed9c0`, with Cargo.lock SHA-256 `28bee3e09919c863e231337b56b492b90c22105e2a20cfdffee740a032623844`. All three local commands used seed 12345 and an isolated report directory. The storage cases use `s02-storage-v1`; the existing native UI smoke uses `s01-feasibility-v1`. [Per-case results, source hashes and environment](evidence/sprint-02/local-verification.json)

| Layer and scope | Command | Result |
| --- | --- | --- |
| Linux x64 unit, Debian under WSL2 | `cargo xtask test unit --seed 12345 --report-dir <report-dir>` | Pass, 32 tests across seven cases. UT-001, UT-003, UT-004 and UT-009 each ran a pure storage decision test. |
| Linux x64 real SQLite and loopback integration | `cargo xtask test integration --seed 12345 --report-dir <report-dir>` | Pass, 32 tests across seven cases. IT-001 ran eight identity/migration tests, IT-002 ran the page and cursor failure test, and the storage subsets ran three resource, ten durability and two fixture tests. |
| Linux x64 process smoke, isolated Xvfb | `cargo xtask test e2e --suite smoke --case E2E-001 --binary <binary-path> --seed 12345 --report-dir <report-dir>` | Pass. Nine native actions included Quit and a new process reopening the same private profile. Both observations found a SQLite database with SHA-256 `b1c8e2512536049b66c749deb295294d6171cfa7d226d9e0fae63eb02c9ee78b`. The test executable's SHA-256 was `8853370360355943068b4b027731d74202c8395e6c85f0d27d425766171b0ce9`. |
| Windows native storage test executables | Identity, resource contracts, durability, full-disk rollback and both fixture tests on Windows | Pass for the exercised storage cases. |
| Hosted Windows Server 2025 x64 native smoke | `cargo xtask test e2e --suite smoke` | Pass at `f8ea378`. All ten native actions passed. The new process reopened the same SQLite profile, with matching pre- and post-Quit SHA-256. [Windows GUI artifact](https://github.com/joshmarkovic/threadline/actions/runs/44369144668/artifacts/14459846953) |
| Hosted build and core | `cargo xtask test unit` and `cargo xtask test integration` | The Windows Server 2025 x64, macOS 15 arm64 and Ubuntu 24.04 x64 jobs all passed at `6806291`. [Corrected CI run](https://github.com/joshmarkovic/threadline/actions/runs/44370589333) |
| Hosted macOS native probe | `xcrun swift tools/native/macos-window-probe.swift target/debug/threadline artifacts/macos-window-probe.json` | At `f8ea378`, core checks passed before duplicate declarations prevented the Swift probe from compiling. At `6806291`, macOS 15.8.2 build 24H232 arm64 observed the native window, verified the isolated SQLite header and Quit through AppKit with exit 0. [Original CI](https://github.com/joshmarkovic/threadline/actions/runs/44369144668) · [Corrected probe artifact](https://github.com/joshmarkovic/threadline/actions/runs/44370589333/artifacts/14457544560) |

The corrected Windows GUI job passed the GPUI tests and native process smoke at `6806291`. The separately dispatched Windows 11 reference job was skipped. [Corrected Windows GUI job](https://github.com/joshmarkovic/threadline/actions/runs/44370589333/job/135035972397)

The [hosted receipts](evidence/sprint-02/hosted-verification.json) retain the test executable digests, source report hashes, job IDs and profile verification. The hosted Windows smoke ran on Windows Server. The macOS probe exercised window creation and Quit. Full published-package coverage for each platform remains with its release sprint.

## Phase boundaries

FND-03 and CACHE-03 reach their initial storage verification here. AC-90 has real SQLite evidence; Sprint 03 supplies the authenticated HTTP side. IT-019 has migration, recovery, cleanup and sign-out storage coverage. Its final all-state application checks and PKG-003's installed upgrade belong to Sprint 12. Native smoke used a development executable. Published artifacts, installer authenticity, live-provider checks and release-build interaction timings were outside this run. [Acceptance ledger](../spec/acceptance_traceability.csv) · [Scenario ledger](../spec/test_platform_traceability.csv)

Sprint 03 connects verified GitHub identities and protected credentials to these durable account and repository IDs. Sprint 04 connects the reader to the cache, tabs, editor ownership, coverage and Outbox projections and measures cached opening and search on the Section 16 workload.

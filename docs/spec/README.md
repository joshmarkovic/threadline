# Native GitHub Client Sprint Pack

Requirements version 1.7 | November 17 2026

## Purpose and current status

This build contract contains 175 requirements, 99 acceptance scenarios, fourteen sprints and 85 concrete test scenarios. The Threadline repository also contains the application, test runner and sprint evidence. Windows setup has been tested locally. Public installation remains pending. [Sprint reports](../reports) · [Windows release checks](../reports/sprint-12.md)

## Changes in version 1.7

Windows remains first and highest priority; macOS and Linux are mandatory later milestones. Platform boundaries and early build/core tests prevent avoidable rewrite. Unit, real SQLite/HTTP integration, native-process E2E, installer/upgrade and guarded live tests are explicit per-sprint work. Every published target must supply a trusted one-to-three-command prebuilt install/update route. No release URL or publisher configuration has been supplied, so examples remain templates.

## Files to give the coding LLM

- github_desktop_requirements.md: full product and phased release contract.
- sprints/: fourteen scopes with tickets, full requirements, per-sprint test work, gates and evidence instructions.
- TESTING_PLAN.md and test_scenarios.csv: test layers, runner commands to implement, deterministic fixtures, scenario IDs, expected results and initial/phase closure owners.
- INSTALLATION_PLAN.md: quick-install templates, actual packaging/trust/update/uninstall deliverables and failure tests.
- CROSS_PLATFORM_PLAN.md: shared architecture, target defaults, OS adapters and parity obligations.
- requirements_traceability.csv and acceptance_traceability.csv: original/initial implementation and closure ownership. Status is scoped to the named initial phase.
- requirement_platform_traceability.csv, acceptance_platform_traceability.csv and test_platform_traceability.csv: separate applicability/status/evidence rows for Windows, macOS and Linux. These govern target release claims.
- SPRINT_REPORT_TEMPLATE.md: exact commands, build/package identity, target, test-layer evidence, measurements and handoff.

## How to execute

Place this entire pack in the application repository. Read the master, current sprint, companion plans and ledgers together. The logical module layout does not require a separate crate for every module. Implement working vertical slices and relevant tests with each feature. Preserve existing work and all prior scope decisions.

> Read github_desktop_requirements.md, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md, the traceability files and sprints/sprint_01_native_foundation.md. Implement Sprint 01 in this repository. Deliver runnable native Windows code plus the specified portable architecture, CI/test foundation and actual native smoke evidence. Update the ledgers and write a sprint report. Do not invent platform/live results or claim future features tested. Stop after the Sprint 01 report.

For a Windows-release assignment, continue through Sprints 01-12. For an all-platform assignment, continue through Sprint 14. Gates are engineering checks, not routine permission requests. A missing runner/publisher/signing credential blocks affected evidence or publication, not unrelated implementation. The real short installation commands must be published and tested before the corresponding platform is declared available.

## Test command contract

Implement the commands specified in TESTING_PLAN.md, including unit, integration, GPUI UI integration, native e2e smoke/full, installation with an artifact and separately allowlisted live tests. They are development interfaces to implement, not runnable scripts supplied here. Real native-process tests drive actual controls; GPUI snapshots and callbacks are integration/unit evidence. Package release gates additionally test the exact installed published binary and its trust metadata.

## Phase order and gates

Execute fourteen scope-based increments in three release phases. Sprints 01-12 deliver the complete Windows first release. Sprint 13 delivers macOS parity and Sprint 14 Linux parity. Windows remains the highest priority. These are work scopes, not fourteen equal calendar periods; split large scopes into ordered subtasks without dropping requirements or gates.

The first useful cached reader arrives in Sprint 04, scheduled refreshing in 05, PR review reading/diffs in 06, existing-issue/comment writes and the durable Outbox in 07, reviews/merges in 08-09, graph/history in 10, notifications in 11 and Windows release verification/simple installation in 12. Later OS phases reuse shared behavior and add actual native/package evidence. Platform interfaces, build/core-test visibility, test runners and native Windows smoke tests begin in Sprint 01.

The pack maps all 175 requirement IDs and 99 acceptance scenarios to an initial implementation/verification owner. The per-platform ledgers separately record applicability, phase gates and evidence. A Windows Verified/Pass does not verify macOS or Linux. Later-phase gaps remain visible and must be resolved before publishing that target; they do not silently block an otherwise complete Windows phase. A defect in shared correctness must still be fixed. AUTH-10 remains Inactive future scope.

Every sprint includes unit, integration and native end-to-end tests for its implemented features, with reproducible commands and evidence. Add tests with the feature and preserve prior behavior. The ledgers record native in-process UI tests, separate-process test builds, authorized live-provider tests and exact installed published-package smoke at their respective scopes.

An assigned Windows release ends at Sprint 12; an assigned all-platform release includes Sprints 13-14. Continue within the authorized scope without routine reapproval. Missing runners, publisher configuration, signing access or approved GitHub credentials blocks the affected validation/publication, not independent implementation. Never invent a pass, published URL or binary. Use TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and the scenario/phase ledgers alongside each sprint file.

| Sprint | Phase | Outcome |
| --- | --- | --- |
| [01: Native feasibility and project foundation](sprints/sprint_01_native_foundation.md) | Windows first release | A runnable native Windows shell proves the chosen stack can support the hardest interface and operating-system integrations. |
| [02: Shared data model, SQLite cache, drafts, and outbox](sprints/sprint_02_storage_and_identity.md) | Windows first release | A tested local domain and database can retain remote content and user drafts without losing identity or confusing partial data with complete data. |
| [03: GitHub authentication, SSO, provider, and repository selection](sprints/sprint_03_github_access.md) | Windows first release | The app can securely access an explicitly selected SSO-protected GitHub.com repository as the user. |
| [04: Cached reading, search, tabs, and offline workspace](sprints/sprint_04_cached_workspace.md) | Windows first release | The first useful reader opens downloaded conversations instantly, searches locally, and keeps multiple issues and PRs visible with saved local drafts. |
| [05: Synchronization, connectivity, and activity observation](sprints/sprint_05_sync_and_observation.md) | Windows first release | Relevant cached content stays fresh on configurable schedules without blocking reading or forcing an archive; trustworthy activity observation exists for later notifications. |
| [06: PR context, commits, files, checks, and native diffs](sprints/sprint_06_pr_reading_and_diffs.md) | Windows first release | A PR review tab presents prominent code beside one smaller body/verified-issue context pane, with cached discussions, reviews, commits, files, and checks. |
| [07: Existing-issue edits, comments, and the action outbox](sprints/sprint_07_issues_and_write_engine.md) | Windows first release | Users submit existing-issue edits and comment work online or offline, inspect or cancel pending actions, and have eligible work automatically delivered after reconnection. |
| [08: Inline PR reviews, approvals, and review drafts](sprints/sprint_08_pr_reviews.md) | Windows first release | Users can review exact source revisions, retain unfinished reviews, and submit comments, approvals, or requests for changes as themselves. |
| [09: Merge workflows](sprints/sprint_09_merge_workflows.md) | Windows first release | Permitted PRs can be directly merged with fresh server checks, expected-head protection, and accurate locally queued, uncertain, or merged outcomes. |
| [10: Branch graph and commit comparisons](sprints/sprint_10_history_and_comparisons.md) | Windows first release | Users can navigate remote branch ancestry and compare commits through the same native diff system without cloning the repository. |
| [11: Notification rules and native delivery](sprints/sprint_11_notifications.md) | Windows first release | Users receive only the selected activity, including unopened items, with reliable local history, bot suppression, grouping, and correct workspace activation. |
| [12: Windows testing, simple installation, and first release](sprints/sprint_12_release_verification.md) | Windows first release | A tested Windows-first release satisfies the complete Windows product contract and offers a verified one-to-three-command prebuilt installation/update path. |
| [13: macOS parity, native testing, and simple installation](sprints/sprint_13_macos_release.md) | macOS release | A published macOS release preserves the Windows product workflows while implementing and testing its native integrations and installation. |
| [14: Linux parity, native testing, and simple installation](sprints/sprint_14_linux_release.md) | Linux release | A published Linux release supports a bounded tested desktop/session matrix with full product parity, native services, and a simple verified prebuilt installation. |

## Status and scope rules

- Initial requirement and acceptance status covers the named phase. Scenario status covers its initial execution phase. Find the build, exact command and source artifact by the same case ID in the platform test ledger. [Initial requirements](requirements_traceability.csv) · [Initial acceptance](acceptance_traceability.csv) · [Scenario catalog](test_scenarios.csv) · [Platform test results](test_platform_traceability.csv)
- A narrow initial test can pass while its platform requirement remains Partial for release. Each platform row records evidence for that target. [Platform requirements](requirement_platform_traceability.csv) · [Platform acceptance](acceptance_platform_traceability.csv)
- PORT-07/AC-87 apply only to macOS and PORT-08/AC-88 only to Linux. They do not block Windows completion. Shared correctness defects still require repair; missing later runtime evidence is visible future-phase work.
- AUTH-10 remains Inactive future OAuth/device scope. GRAPH-07 is optional object-cache work; NOTIFY-20 requires an extensible boundary, not hosted infrastructure.
- Existing-issue edits, comments, reviews and direct merges remain required. New issue/PR creation, GitHub merge queues and auto-merge are excluded. Preserve tabs/context layout, positive-only sidebar badges, PAT/SSO, offline Outbox safety and incremental local-session restore.
- Trusted simple install/rerun update and exact published command tests are mandatory. Hosted automatic updater infrastructure remains optional. Never present a placeholder package-manager ID or URL as a released app.
- Tests must have meaningful assertions and reproducible evidence. No arbitrary coverage percentage replaces risk coverage; no required feature is certified by an empty/skipped suite. Redact secrets and private data.
- LIVE-001 through LIVE-004 are required shared-provider validation gates on an explicitly authorized host. Their target rows remain applicable. Later releases may reference existing evidence after checking provider, credential/SSO policy and API behavior still match; rerun changed or target-specific behavior. Missing authorization is Blocked/Not run, and no target phase authorizes an otherwise unapproved mutation.
- Performance, accessibility and installed lifecycle are measured on each target. Re-run affected regressions on already released platforms when shared code changes.

## Completion evidence

For each sprint record exact commands, app commit/build, fixture/seed, OS/version/architecture/session, result files and actual measurements. For publication include package digest/signature, the actual distribution destination and exact clean-machine install/update tests. Keep simulator/test builds, native desktop evidence, installed-package smoke and live GitHub tests separate. All applicable required gates for that target must pass; tests or packages unavailable for that target remain Blocked/Not run.

## Sources and authorization

The master and companions cite primary technical sources. Verify current behavior against pinned dependencies and chosen endpoints. Live writes require an explicitly designated disposable repository and explicit write opt-in; no test can bypass SSO or organization policy. This planning pack creates no repositories, issues, reviews or merges.

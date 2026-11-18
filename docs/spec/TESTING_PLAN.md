# Testing Plan

Version 1.7 companion to the Native GitHub Client Requirements

## Purpose and delivery order

Implement and maintain unit tests, integration tests, and end-to-end tests as part of each sprint. The product is a native Rust desktop application with GPUI and GPUI Kit, a real SQLite cache, direct GitHub APIs, and a durable Outbox. Windows is the first release target. macOS and Linux are later release milestones that reuse the same core behavior and must pass their own native checks before being advertised as supported.

This file specifies the test system that the coding LLM must build. The application, runner commands, fixtures, and tests described here are implementation deliverables. They are not already implemented or passed by this specification pack. Initial rows in `test_scenarios.csv` intentionally say `Not run`.

Read this plan with `github_desktop_requirements.md`, the assigned sprint file, and both acceptance and requirement traceability files. TEST-01 through TEST-08 govern the testing contract. The scenario catalog supplies concrete tests and does not replace the full acceptance requirements.

| Release phase | Required platform scope | Release gate |
| --- | --- | --- |
| Windows first release | Supported Windows reference OS and x64 architecture | Sprint 12 completes the Windows product, test evidence, and easy installation gates. |
| macOS release | The macOS version and architecture validated in the portability plan | Sprint 13 repeats applicable shared suites and proves native macOS behavior and distribution. |
| Linux release | The distribution, architecture, desktop, display-server, and graphics configuration validated in the portability plan | Sprint 14 repeats applicable shared suites and proves native Linux behavior and distribution. |

Later-platform tests remain explicitly deferred for the Windows release, never reported as passed. A defect in shared code discovered on another platform is still a real defect. A build for one OS or CPU architecture cannot certify another.

## Test levels

| Level | Required boundary | Suitable assertions |
| --- | --- | --- |
| Unit | Small domain functions or state machines; no real network, user profile, OS credential store, or persistent database | Identity, state transitions, reference parsing, coordinate mapping, scheduling, rule precedence, eligibility, and invariants. |
| Integration | Multiple production components; use a real temporary SQLite database and the real HTTP provider against a controllable local GitHub simulator | Migrations, transactions, pagination, validators, permission interpretation, persistence, scheduling, cancellation, replay, and reconciliation. |
| UI integration | Production GPUI views and components inside the framework's in-process test harness | Component interaction, layout state, focus, selected tabs, visible labels, draft bindings, and late-response rejection. This is a subset of integration testing. |
| Native end to end | A separate native application process with a real window, production UI code, real SQLite, and an external driver operating actual controls | Complete user journeys through navigation, editing, offline operation, restart, Outbox inspection, and recovery. |
| Package and installer end to end | The exact candidate distribution artifact on a clean supported environment | One-to-three-command installation, launching the installed application, rerun, upgrade, interrupted installation, data retention, and uninstall. |
| Live GitHub integration | Explicitly enabled access to an authorized, designated disposable repository | Confirm provider behavior that a simulator cannot establish, including permitted SSO/PAT behavior and mutation attribution. |

GPUI Kit's documented test harness renders and interacts with views in process. Element snapshots do not prove that a packaged application starts, that pixels are correct, or that the OS accessibility bridge and notification activation work. Keep those distinctions in reports. Sources are listed at the end of this file.

## Required developer commands

Implement a small repository-owned `xtask` runner and a Cargo alias so the following command interface works consistently from the repository root. The runner may invoke ordinary Cargo tests, a native automation driver, and platform packaging tools internally. These are required command names to implement, not commands that can run against this specification archive.

| Command | Required behavior |
| --- | --- |
| `cargo xtask test unit` | Run all implemented pure unit suites with deterministic inputs and a test summary. |
| `cargo xtask test integration` | Run database, provider, sync, Outbox, and platform-service integration suites with isolated fixtures. |
| `cargo xtask test ui` | Run GPUI in-process UI integration tests and report any renderer-specific limitations separately. |
| `cargo xtask test e2e --suite smoke` | Launch the native application and drive a small critical subset of user journeys implemented at that sprint. |
| `cargo xtask test e2e --suite full` | Drive the full applicable native application journey set on the current supported host. |
| `cargo xtask test install --artifact "<package-path>"` | Verify the exact package through its install, launch, update, retained-data, and uninstall scenarios. The quoted path is supplied by CI or the developer. |
| `cargo xtask test live --repo OWNER/REPO` | Run only explicitly enabled read-only live checks against the configured authorized test scope. |
| `cargo xtask test live --repo OWNER/REPO --allow-live-writes` | Enable specifically selected mutation cases only after the repository has been designated for disposable testing and the credential profile is verified. The flag alone is not authorization to use an arbitrary repository. |

Support a stable scenario filter such as `--case E2E-006`, a replay seed such as `--seed 12345`, and a report directory option. Reject unknown cases and options. A suite with zero selected tests must fail unless the caller explicitly requested a documented empty capability probe. Tests requiring a native host must reject a mismatched target and report the actual host.

Every command must produce an exit code, readable summary, and machine-readable results. Failures and missing required infrastructure produce nonzero results. A runner must never translate `Blocked`, `Not run`, or a silently skipped mandatory case into `Pass`. CI must execute the command, not merely check that the command string appears in a document.

The runner must not install new third-party tools, change system settings, retrieve real credentials, or start live GitHub writes without an explicitly configured execution profile. Document pinned tool setup separately. Never place a PAT in command-line arguments, fixture files, screenshots, or reports.

## Isolation and deterministic fixtures

Use fresh test-owned temporary directories, databases, and native credential namespaces. Never point an automated test at the user's actual application-data directory or normal PAT entry. Clean up only resources created by that test run. Keep failing-run evidence under an explicit artifact directory using synthetic or redacted data.

Inject clocks, randomness, retry timers, and simulator responses through production boundaries. Record the random seed, initial fixture revision, expected events, and fault schedule. Advance a virtual clock for unit and service integration tests rather than sleeping through work hours, daylight-saving transitions, or backoff periods. Native timing tests use real elapsed time and a documented machine profile.

| Fixture ID | Required contents |
| --- | --- |
| FX-IDENTITY | Issues and PRs sharing a repository number sequence, the same number in another repository, distinct API IDs, partial payload ordering, and a confirmed transfer. |
| FX-SMALL | A fast synthetic account with selected repositories, open/closed issues, open/closed/merged PRs, ordinary comments, reviews, threads, checks, labels, and assignees. |
| FX-COVERAGE | Missing, partial, empty-and-complete, failed, denied, binary, truncated, capped, and downloaded resource families. |
| FX-PR | At least two PR tabs, one with `Resolves #41`, multiple and cross-repository references, a reference to a PR rather than an issue, code examples to ignore, changing bodies, and delayed responses. |
| FX-DIFF | Added/deleted lines, renamed files, Unicode, CRLF/LF, missing final newlines, whitespace views, multiline comments, binary changes, and immutable old/new commit identities. |
| FX-OUTBOX | Every supported mutation and state, cancellation tombstones, dependencies, successful step receipts, original edit bases, review anchors, expected heads, and unknown outcomes. No new-issue creation. |
| FX-FAULTS | A programmable local HTTP simulator and storage fault injector that can fail at named lifecycle boundaries. |
| FX-NOTIFY | Human and bot actors, new versus edited comments, review bursts, mentions, baseline history, own actions, branch updates, merges, snooze, quiet hours, and precedence conflicts. |
| FX-HISTORY | Branches, merges, missing parents, paginated older history, moved/deleted refs, and a force-push. |
| FX-UPGRADE | Representative earlier schema versions with cached content, unsent drafts, paused and uncertain Outbox work, workspace state, credentials by reference, and sync/notification baselines. |
| FX-LARGE | The master specification's 50,000 item summaries, 250,000 comments, long discussion, large diff, branching history, two visible panes, twenty tabs, and 1,000-action Outbox. |
| FX-INSTALL | Valid, corrupt, truncated, wrong-platform, wrong-architecture, unexpected-version, and tampered release metadata/packages; interrupted download and extraction; occupied files and insufficient disk space. |

The local GitHub simulator must handle actual HTTP requests through the production provider. It must expose request counts and semantic request logs without secrets. Support REST and GraphQL pagination, conditional responses, delayed and reordered replies, malformed payloads, 401/403/404/429/5xx, secondary throttling, permission changes, and dropped responses before or after a simulated mutation is applied.

Offline tests must verify that no prohibited request reached the simulator, not merely set an `offline` label. Tests for new-issue exclusion must inspect commands and request traces as well as UI visibility.

## Native automation and shipping-package evidence

In Sprint 01 select and prove an external native automation path. Prefer stable accessibility identifiers and actual keyboard/pointer input. The Windows driver must operate a real GPUI window through UI Automation or another demonstrated native control/input path. macOS and Linux require equivalent proof against their supported accessibility/input backends during their feasibility and release work. Record the selected tool and pinned version in the architecture decision record; do not assume a framework API exists because its name resembles a browser testing API.

Native tests must launch, focus, resize, interact with, close, terminate, and restart the application process. For ordinary journeys, use user-visible navigation, controls, and text input. Fixture seeding, clock control, and deterministic server fault injection are permitted setup operations. Calling an application command directly and asserting its model state is not a substitute for driving that command through the native UI.

Use a release-optimized test build with an isolated profile and explicit loopback-provider configuration for deterministic native E2E. Test-only endpoint overrides and hooks must be disabled or absent in production configurations and cannot silently weaken production credential routing or TLS behavior. Use synthetic credentials with the simulator. Confirm the loopback boundary before writing them to any test store.

That instrumented test build does not establish that the exact signed shipping artifact installs or works. Maintain a separate package lane that installs the actual candidate artifact, launches that installed executable, exercises basic native navigation and offline restoration with a preseeded sanitized cache, upgrades from the prior supported package/schema, and verifies retained state. Record its digest and signing/notarization evidence where applicable. Live tests against GitHub are a separately authorized lane.

For the first public release of a platform, there may be no previously published package. Build a documented predecessor test package with the prior supported schema and seeded durable state, then upgrade it to the actual candidate. Record that predecessor's commit, schema, digest and test-only provenance. Do not invent a prior production release or omit the upgrade gate because this is the first public package.

Wait for observable conditions with bounded deadlines. Avoid fixed sleeps and assertions against incidental render counts, random database IDs, or exact pixel coordinates. Reacquire controls after view changes. If coordinate input is necessary for a custom diff, derive it from the current window/control bounds and verify the visible result. Maintain screenshot evidence for visual and focus failures, but do not make cross-OS pixel equality the behavior contract.

The native automation proof must include selectable text, a resizable context/code pair, a real review input, and a stable target after a tab switch. The current GPUI Kit guide identifies platform differences in offscreen rendering, so a headless image test cannot stand in for Windows native-window evidence. A missing desktop session, GPU/driver, accessibility permission, or runner capability is `Blocked`, not `Pass`.

## Failure and recovery obligations

Exercise the boundaries that can lose user work or misreport a remote action. For each supported action family, include the applicable scenarios below as its adapter becomes available and rerun the combined matrix by Sprint 12:

- Crash before durable enqueue, after enqueue but before worker claim, during validation, immediately before send, after server acceptance with no response, and after the response but before the local completion transaction.
- Cancel before claim, while validation is running, immediately before dispatch, and after a request may have been sent. Verify the durable winner and retained evidence.
- A predecessor succeeds but its dependent step has not started; restart without repeating the predecessor. Cancel, reject, or make the predecessor uncertain and keep dependent work inspectable.
- Change the remote issue fields, PR head, review anchor context, merge permission, required check, or merge-queue policy while an action is queued.
- Expire/revoke a PAT, renew it for the same identity, provide a different identity, switch accounts, remove a repository from scope, or revoke access.
- Fail an SQLite transaction, fill the disk, lock an installation file, interrupt a migration, and interrupt an application update. Preserve user-authored state and report recoverable failure.
- Alternate online/offline states, return rate limits, sleep and wake, cross a work-hours boundary or daylight-saving change, and restart with each pause mode set.

Use real durable storage and production worker code for integration cases. Assert both local state and remote simulator state. Stable local operation IDs do not imply GitHub server idempotency. Unknown remote outcomes must remain unknown when the available evidence cannot resolve them.

## Minimum test work by sprint

The identifiers below are in `test_scenarios.csv`. They are minimum scenarios to introduce or extend; each sprint also reruns relevant earlier regressions. An early foundation test proves only the subset described for that sprint. It cannot close an acceptance case for reviews, merges, notifications, or complete offline operation before those features exist.

The catalog distinguishes `first_sprint` from `closure_sprint`. The latter is the first release phase in which that entire scenario is due; `owner_sprint` names that closure owner, while the platform-specific closure columns record later native reruns. For shared cases, Windows closure does not certify macOS or Linux. Acceptance scenarios that span several test rows still close only under the master acceptance traceability rules.

| Sprint | Unit and integration work | Native end-to-end work and evidence |
| --- | --- | --- |
| 01 Native foundation | Implement runner/reporting and deterministic harness self-checks; UT-021. Prove GPUI UI harness and external driver separately. | E2E-001 launch and native input against fixture panes/diff. Record Windows evidence and initial macOS/Linux build or feasibility status. |
| 02 Storage and identity | UT-001/003/004/009; IT-001/002. Real database migrations, identity constraints, transactional page progress and Outbox state storage. | Rerun E2E-001. Establish isolated profiles and process restart orchestration. Do not claim full cached-reader recovery yet. |
| 03 GitHub access | UT-010/020; IT-004/005/020. Actual HTTP provider plus protected-store adapter, partial access and credential routing. | E2E-002 access/error flows through native controls. LIVE-001 is separately authorized. AC-90 closes only after actual provider integration exists. |
| 04 Cached workspace | UT-008/013/018; IT-003; UI-002/003/008. Coverage, search, pane/tab identity and stale responses. | E2E-003/004 demonstrate the implemented cached reader and partial data. Measure cached navigation using the declared fixture. |
| 05 Synchronization | UT-002/005/006/007; IT-006/007. Virtual-clock schedule and incremental observation, shared budgets and retries. | Extend E2E-003/004 with delayed refresh, offline transitions and restored pause modes. Retain usable cached input while the simulator stalls. |
| 06 PR reading and diffs | UT-011/012; IT-016/017; UI-001. Actual PR resources, source coordinates, verified references and stale-context rejection. | E2E-005 uses the reader context/code layout, missing coverage, and tab switching. Inline draft closure follows in Sprint 08. |
| 07 Issues and write engine | IT-008/010/012; UI-006. Enqueue, unsent-versus-unknown cancellation and existing-issue conflicts. Extend IT-009 for these action families. | E2E-006/019. Submit offline, restart, inspect/cancel, reconnect and reconcile. LIVE-002 remains explicitly scoped and opt-in. |
| 08 PR reviews | IT-011/013; UI-004. Real review/thread adapters and dependent-step receipts. Extend IT-009 for review families. | E2E-007/008 prove revision-bound review submission and context switching with an actual draft. LIVE-003 confirms attribution only in designated scope. |
| 09 Merge workflows | UT-017; IT-009/014/015; UI-005. Complete the mutation fault matrix, scope holds and shared pending counts. | E2E-009/010 prove current merge eligibility, changed-head hold, account isolation and full Outbox behavior. LIVE-004 only in designated disposable scope. |
| 10 History and comparisons | UT-016 and expanded UT-011/IT-016 fixtures. Verify ancestry, boundaries, parent selection and comparison identity. | E2E-011 expands older history and reviews individual/two-commit changes without losing current work. |
| 11 Notifications | UT-014/015; IT-018; UI-007. Shared observation/filtering and native adapter integration. | E2E-012/013/018 on Windows, including notification activation after cold launch, permissions and quiet hours. A simulator feed event alone does not prove OS delivery. |
| 12 Windows release | IT-019/021, UT-019; complete applicable shared regression suites and failure matrix. | Full Windows E2E including E2E-014/015/020, PKG-001 through PKG-006 and PKG-009, performance evidence and exact-package verification. |
| 13 macOS release | Rerun shared unit/integration/UI suites on the supported Mac; IT-022 and native credential/lifecycle checks. | E2E-016 plus every applicable common journey and PKG-007. Prove the published install commands, exact signed bundle, permissions and retained data. |
| 14 Linux release | Rerun shared unit/integration/UI suites on each promised Linux session; IT-023 and native credential/lifecycle checks. | E2E-017 plus every applicable common journey and PKG-008. Prove the published install commands and installed app under each promised Wayland/X11 configuration. |

## CI and release gates

From Sprint 01, configure build and core unit/integration jobs for Windows, macOS, and Linux. Use native target runners where platform dependencies require them. Compile-time checks and tests must use the committed lockfile. Run formatting and compiler/lint checks as separate gates; their success is not test coverage.

For every change, run the affected unit, integration and UI suites plus the Windows native smoke set available at that sprint. Changes to shared identity, storage, sync, Outbox, review coordinates, or installation recovery require the relevant fault cases. A failed shared-platform build must be addressed; an unprovisioned later-platform desktop lane is visibly deferred and must be provisioned before its release gate.

On main/release candidates, run full applicable suites, the full Windows native journey set, and the larger failure/restart matrix. Test performance on a documented, stable reference machine using a release build and the master budgets; do not enforce timing budgets on an uncontrolled shared runner and call the resulting noise an application defect. Record per-platform performance before that platform ships.

Use an interactive desktop runner for native tests. A headless CI job that cannot exercise the target does not satisfy its native gate. macOS and Linux may use recorded manual native feasibility checks before their automated desktop runners are provisioned, but their final automated core E2E journeys remain release requirements. Keep manual checks for screen-reader announcements, visual accessibility, OS permission dialogs and other boundaries the automation does not establish; record them separately.

A pull request cannot pass by silently dropping tests, widening a timeout to conceal an unresolved race, or retrying until one run turns green. Preserve the first failure. A bounded diagnostic rerun may collect evidence, but a flaky mandatory test is an open defect with an owner and expiration on any explicit waiver. Do not count waived or skipped cases as passes. Do not run secret-bearing live jobs for untrusted pull-request code.

## Evidence and completion

Each test result must include scenario ID, related requirement/acceptance IDs, suite level, build commit, application version, dependency lockfile identity, test fixture revision, seed, host OS/CPU/display environment, exact command, timestamps, exit code and outcome. Native failures include redacted application/driver logs and a screenshot or accessibility dump where useful. Fault tests include the named injection point, local durable state and simulator-side evidence.

Package results additionally identify the artifact filename, cryptographic digest, release metadata, install command, verification outcome, previous installed version and database schema, and post-upgrade retained-state checks. The artifact tested must be the artifact published. A new package digest requires its applicable package gate again.

Use `Not run`, `Pass`, `Fail`, `Blocked`, and `Not applicable` distinctly. `Not applicable` needs a documented phase/capability reason and must not hide a required behavior. A spec or generated test file being present is not evidence of execution. Store JUnit-compatible results plus a concise Markdown report and scenario-to-acceptance mapping. The sprint report must list failures, blocked checks and deferred future-platform gates.

AC-83 is an architecture and build-boundary gate, and AC-93 is a phased CI/evidence gate. Track their dependency-boundary inspection, actual target build/test jobs, required-job configuration and truthful release ledger in the acceptance traceability file. They are not closed merely by linking one behavioral scenario or passing the runner self-tests. The scenario catalog intentionally complements that governance evidence.

Do not impose an arbitrary global coverage percentage as the primary definition of done. Collect coverage where useful and review untested branches in high-risk logic. Use table-driven and property-based tests for identities, source-coordinate mappings, state-machine transitions, rule precedence and scheduling. The stronger gate is demonstrated behavior and fault coverage for every acceptance obligation. Tests that merely repeat an implementation calculation or assert a constant are insufficient.

## Live GitHub testing

Default suites use synthetic data and local services only. Live tests require an explicit opt-in profile, approved repository allowlist and verified account identity. Read-only discovery and mutation tests are separate. Existing-issue edits, comments, reviews and merges need specifically designated disposable targets. Destructive cleanup is restricted to resources created or explicitly designated for the run. Never create a replacement issue through an excluded New Issue flow.

Keep real PAT material in the configured secure CI credential facility or native protected store. Logs record only credential profile identifiers. Preserve evidence and report cleanup failures without probing or modifying unrelated repositories. External service outages or missing organization authorization produce `Blocked` with a reason, not a fabricated pass or an automatic change to the product scope.

## Primary references

- GPUI Kit Testing Guide: https://gpui-kit.com/docs/test/
- GPUI Kit Accessibility Guide: https://gpui-kit.com/docs/accessibility/
- GPUI Kit Installation Guide: https://gpui-kit.com/docs/installation/

These references establish framework facilities and limitations. The test organization, runner interface, fixtures, and release gates above are requirements for this application.

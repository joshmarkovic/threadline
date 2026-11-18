# Sprint 12: Windows testing, simple installation, and first release

Requirements version 1.7 | Initial sprint status Planned

## Outcome

A tested Windows-first release satisfies the complete Windows product contract and offers a verified one-to-three-command prebuilt installation/update path.

## Phase and prerequisites

- Release phase: Windows first release.
- Prerequisites: Sprint 01, Sprint 02, Sprint 03, Sprint 04, Sprint 05, Sprint 06, Sprint 07, Sprint 08, Sprint 09, Sprint 10, Sprint 11.
- Code areas: all modules, platform_windows/installer and lifecycle, diagnostics, docs, tests/performance and acceptance.
- Read the master, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and all relevant scenario/platform ledgers.
- Use actual prior evidence; a missing dependency remains blocked rather than passed. Later-platform release work does not silently move into the Windows phase.

## Implementation tickets

### S12-T01

Measure every Section 16 budget in a release build on the documented Windows machine, including 50,000 summaries, 250,000 comments, large diffs/history, and two visible panes with twenty tabs while syncing. Record p95 cached opening/tab switching and search, startup, RAM/GPU memory, frame times, and package size.

### S12-T02

Resolve measured regressions, excessive retained inactive-tab state, redundant polling, UI-thread blocking, and idle redraw. A failed budget stays a failure until fixed or explicitly revised by the product owner.

### S12-T03

Complete Windows keyboard/accessibility, real-editor selection/copy, focus, high-DPI, resizing, appearance, crash/draft recovery, and account-isolation checks. Verify migrations on representative previous databases and redact opt-in diagnostics.

### S12-T04

Produce a runnable Windows build and locally usable installer. Test install, first launch, credential setup, notification activation into a single instance, optional tray/startup behavior, upgrade, and clean uninstall on a clean supported Windows environment. Preserve user data unless deletion is explicitly requested.

### S12-T05

Ship the confirmed PAT login path and record AUTH-10 as inactive future scope. Distribution does not activate OAuth/device authorization. Implement the required package authenticity, simple install/rerun update and publication pipeline without a new login dependency. A hosted automatic updater remains optional; trusted manual update delivery is mandatory.

### S12-T06

Run the full applicable acceptance set, preserve prior evidence, and resolve all required open items. Supply source, lockfile, migrations, fixture/test commands without live credentials, build/install/auth instructions, performance results, architecture decisions, and the final requirement/acceptance ledger.

### S12-T07

Verify the final cold-start offline matrix for all cached views, drafts, settings, activity feed, and Outbox. Run a 1,000-action backlog while navigating/editing; inject connection flapping, rate limits, crashes before/after possible remote success, full disk, account switching, upgrade and cleanup with pending work. Verify accessible connection announcements and cancellation controls on Windows.

### S12-T08

Run AC-73 and AC-74 across the completed product: normal exit retains the protected PAT and every local content/session/progress family; explicit sign-out clears local credentials while retaining readable private data and a durable remote-work hold. Restore the saved workspace before a delayed API response, honor offline/pause controls, reuse same-user identities after PAT repair, and verify bounded incremental request traces with no restart-triggered full archive or baseline reset.

### S12-T09

Implement the real published Windows install/update/uninstall path from INSTALLATION_PLAN.md. Deliver a signed/native per-user prebuilt package, OS/architecture/version manifest and verification metadata, one-to-three-command normal path, safe rerun/staged upgrade, data retention and actionable failures. Replace all release placeholders with the actual approved publisher/repository/URLs before publication; missing configuration blocks publication, not independent local packaging/tests.

### S12-T10

Run unit, integration, GPUI UI integration, full native-process E2E, installer/failure/upgrade tests and approved live checks using the implemented runner. Test exact published command text and the installed release artifact, with digest/signature/build identity recorded separately from simulator/test builds. Pass AC-91 through AC-99; later macOS/Linux gates AC-87/88 remain pending.

## Required tests and commands

- Run all applicable Windows unit, integration, GPUI UI integration, full native-process E2E and approved live tests; require exact command/build/fixture evidence.
- Installer: run exact published commands on clean standard-user Windows, verify authenticity/target checks, rerun, seeded-state update, interruption, safe failure, uninstall/reinstall and installed release smoke.
- Release: performance/accessibility/offline/backlog/crash matrices and Windows phase ledgers must pass; macOS/Linux future rows remain pending, and no plan/ZIP check is an app test.

Implement and use the canonical runner from TESTING_PLAN.md: unit, integration, ui, native e2e smoke/full, install with an artifact, and separately guarded live tests. These are development commands to build, not commands already supplied by this ZIP. Record actual OS/build/fixture evidence and mark unavailable checks Blocked/Not run. Native test-build E2E and exact published-package smoke remain separate.

Scenario IDs introduced or closing here: UT-019, IT-019, IT-021, E2E-014, E2E-015, E2E-020, PKG-001, PKG-002, PKG-003, PKG-004, PKG-005, PKG-006, PKG-009.

Live-provider cases are shared gates on an explicitly authorized host. A later platform can reference existing evidence only with a recorded applicability assessment; rerun changed provider/credential-policy or target-specific behavior. Missing authorization remains Blocked/Not run and never permits unapproved writes.

Use test_scenarios.csv for fixture IDs, expected results, first/closure ownership and evidence. Release sprints rerun all scenarios applicable to their target; earlier sprint scenario lists do not waive regression checks.

## Deliverables

Published installable Windows release; real verified one-to-three-command install/update instructions; release manifest and trust metadata; complete source/lockfile/migrations; implemented test runners/fixtures; native and installer evidence; Windows phase ledgers and performance report.

## Exit gates

- Pass AC-35 through AC-38, AC-48, AC-56, AC-68, AC-73, and AC-74, and rerun all Windows-applicable AC-01 through AC-99 across the integrated Windows build. AC-87 and AC-88 belong only to their later platform phases.
- Report pass/fail against every recorded budget, including cached open/tab p95 under 100 ms, local-search p95 under 200 ms, and warm usable startup under 2 seconds.
- Inspect the shipped dependency/process inventory for embedded browser runtimes and complete clean-Windows installation/activation/uninstallation tests.
- No required failure, unimplemented scope, or blocked Windows/live validation may be silently marked complete. Record conditional requirements as inactive only with their condition and reason.
- Pass AC-91 through AC-99 with reproducible commands, per-layer/per-platform evidence, actual native process controls and the exact installed published package. Placeholders, unsigned development artifacts, snapshots and plan/ZIP validation cannot satisfy these release gates.
- Close the Windows rows of the requirement/acceptance/test platform ledgers only when evidence is complete. Do not wait for full macOS/Linux parity to declare a valid Windows release, and do not mark those later rows Pass from Windows evidence.

## Initial acceptance scenarios owned here

- **AC-35** Release-build measurements report pass or fail against all recorded performance budgets, including cached opening, local search, startup, working set, GPU memory, scrolling, and behavior during background sync. Failed budgets remain visible acceptance failures.
- **AC-36** A clean installation of the supported platform being released can install, launch, authenticate, receive a notification, follow its activation, and uninstall cleanly. Windows closes first; macOS/Linux repeat with their own packages.
- **AC-37** The shipped dependency and process inventory contains no embedded browser or webview runtime.
- **AC-38** Platform-appropriate keyboard focus, text selection, high-DPI behavior, accessibility and draft recovery pass the documented native checks, with Windows first and separate macOS/Linux evidence at their milestones.
- **AC-48** The two-pane, twenty-tab fixture meets the recorded tab-switching and responsiveness budgets while syncing. Inactive tabs do not each create polling loops or retain full large-diff render trees, and closing one view does not cancel work needed by another.
- **AC-56** After a cold start with networking disabled, downloaded issues/PRs, discussions, review data, diffs, fetched graph history, local search, activity feed, settings, drafts, and the outbox remain usable without a blocking login or connection check. Unfetched content has an explicit placeholder and retained target.
- **AC-68** Crash recovery, upgrade, sign-out, and ordinary cache cleanup preserve unresolved operations, cancellation tombstones, dependency maps, payload versions, base snapshots, and review anchors. Inject a full/unwritable database during enqueue: no Queued success is shown and the input remains recoverable.
- **AC-73** Seed every implemented content/draft type, panes/tabs with non-default view positions, local settings and offline/pause flags, pending actions, sync checkpoints, and notification history. Quit normally and relaunch online and offline. Restore the same local workspace before a deliberately delayed API response, retain the protected PAT without asking for re-entry while valid, and preserve all local data. Verify that Work offline prevents startup probes, paused actions do not dispatch, unsent drafts remain drafts, and restarting does not recreate old notification alerts. This is the full-release lifecycle matrix, not a fixture-only storage test.
- **AC-74** Explicitly sign out with cached private content and pending issue/review/merge work. Verify local PAT material is removed or invalidated, no new requests dispatch under that profile, the hold survives restart, and its local content, drafts, workspace, settings, sync checkpoints, and outbox remain readable in that signed-out profile. A different active account cannot see or send its data. In-flight effects remain uncertain when appropriate; same-user PAT repair reconciles them before safe replay. Sign-out performs no implicit purge or claimed remote undo.
- **AC-91** Run the full native application E2E journey for the completed phase: cached startup, search, PR body/issue context switching, preserved inline draft, offline submission, Outbox inspection/cancellation, reconnect/reconciliation, changed-head guards, notification activation and restart. Evidence identifies the actual app process, controls, local database and provider requests; unit calls or GPUI snapshots cannot satisfy it.
- **AC-92** Deterministically inject response loss after server success, a crash around dispatch, disk/transaction failure, cancel-versus-reconnect races and revision changes. Native/integration evidence shows retained input, stable target/operation identities, safe dependency holds and no blind duplicate send, false cancellation or lost local data. Run applicable cases throughout the feature sprints and the complete matrix at phase release.
- **AC-93** The phase report links every applicable requirement/AC/scenario to unit, integration, native E2E, installer and approved live evidence or an explicit failure/block. Commands, versions, OS/architecture and artifacts are recorded; later-platform statuses stay separate. A skipped or unavailable required native test prevents that platform's publication, not an unsupported all-platform pass.
- **AC-94** On a clean supported target with declared system prerequisites and no Rust/compiler checkout, follow the real published install instructions in one to three commands. The per-user app installs and launches; normal OS trust prompts are explained. A placeholder URL or source-build command fails this release gate.
- **AC-95** A wrong OS/architecture/version, incompatible prerequisite, truncated artifact, altered checksum/signature or unauthenticated payload is rejected before replacing the installed app. The previous install and user data remain usable, and the explanation plus safe retry path is actionable.
- **AC-96** Seed cached content, drafts, a non-default workspace, credentials, pause/offline flags, checkpoints and unresolved Outbox work, then rerun the installer and upgrade from the previous supported version. State and operation identity survive, registrations are not duplicated, and networking respects restored controls. Test interruptions, running-app locks and safe migration recovery without replaying a restored stale Outbox snapshot.
- **AC-97** Exercise denied permission, blocked/interrupted download, full disk, failed trust verification and missing prerequisite. Report a concise reason/nonzero exit and safe repair/rerun guidance without requiring developer compilation, an app PAT, security weakening, or silent system changes.
- **AC-98** Uninstall removes the application and registrations while retaining local data by default. Explicit data/credential removal is separate and explains pending-work impact. Reinstall restores the retained local profile and incremental progress without an implicit full archive or duplicate queued actions.
- **AC-99** The released platform's binaries, version/architecture manifest, authenticity metadata, tested install/update/uninstall commands and clean-machine evidence are actually published. Documentation contains no unresolved placeholder presented as runnable; unreleased platform examples remain clearly labeled templates and do not imply that their packages exist.

## Platform release rerun

Run every applicable acceptance scenario in acceptance_platform_traceability.csv for windows. This is additional target evidence, not reassignment of its original closure owner. Other-platform-only scenarios are explicitly Not applicable. Applicable IDs: AC-01, AC-02, AC-03, AC-04, AC-05, AC-06, AC-07, AC-08, AC-09, AC-10, AC-11, AC-12, AC-13, AC-14, AC-15, AC-16, AC-17, AC-18, AC-19, AC-20, AC-21, AC-22, AC-23, AC-24, AC-25, AC-26, AC-27, AC-28, AC-29, AC-30, AC-31, AC-32, AC-33, AC-34, AC-35, AC-36, AC-37, AC-38, AC-39, AC-40, AC-41, AC-42, AC-43, AC-44, AC-45, AC-46, AC-47, AC-48, AC-49, AC-50, AC-51, AC-52, AC-53, AC-54, AC-55, AC-56, AC-57, AC-58, AC-59, AC-60, AC-61, AC-62, AC-63, AC-64, AC-65, AC-66, AC-67, AC-68, AC-69, AC-70, AC-71, AC-72, AC-73, AC-74, AC-75, AC-76, AC-77, AC-78, AC-79, AC-80, AC-81, AC-82, AC-83, AC-84, AC-85, AC-86, AC-89, AC-90, AC-91, AC-92, AC-93, AC-94, AC-95, AC-96, AC-97, AC-98, AC-99.

## Full owned requirements

### AUTH-10

OAuth or device authorization is optional future scope and is inactive for the confirmed PAT-based first release. Retain a provider boundary that permits a separately authorized future registered GitHub OAuth or GitHub App integration. If that scope is explicitly activated later, verify its SSO, organization approval, and user-identity behavior and keep login secrets out of the distributed binary. Distribution alone does not activate this requirement. [S23]

Initial verification owner: Sprint 12. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### INSTALL-05

Rerunning the same install command MUST detect an installed matching version and finish successfully without duplicating shortcuts, startup entries, instances, data directories or credentials. Support a documented stable channel and explicit version selection; upgrades preserve stable app identity and state. Reject unsafe downgrades or schema incompatibility with clear recovery guidance. Startup-on-login remains opt-in; installation must not silently activate background networking or override offline/pause settings.

Initial verification owner: Sprint 12. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### INSTALL-06

Failures MUST provide a short actionable explanation, a nonzero exit code when applicable, and a safe rerun path without asking users to compile or inspect developer logs for routine problems. Test denied permissions, blocked downloads, insufficient disk space, interrupted downloads, signature/hash failure, incompatible OS/architecture, missing prerequisite, and running-app locks. Do not request an application PAT for a public installer or expose credentials in commands/logs. If a chosen private release distribution requires separate download authorization, document that boundary without reusing the app's GitHub PAT implicitly.

Initial verification owner: Sprint 12. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### INSTALL-07

Provide a documented uninstall path using native platform conventions or an installed uninstaller. Uninstall removes app binaries and registrations while preserving local user data by default; removing data or protected credentials is an explicit separate choice with pending-work consequences shown. Reinstall must recover the retained profile, workspace and progress instead of forcing a full archive. Respect the already specified distinction between ordinary exit, explicit sign-out and cache deletion.

Initial verification owner: Sprint 12. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### INSTALL-08

Release deliverables MUST include the real published binaries/packages, version/architecture manifest, verification metadata, install/update/uninstall documentation, and tested one-to-three-command installation examples for that released platform. No release repository, URL or binary is available in this specification package: all illustrative commands are labeled templates until replaced with actual published endpoints and tested. Record clean-machine install, rerun, upgrade with seeded drafts/Outbox, failure recovery and uninstall/reinstall evidence on each claimed platform. A script file, placeholder URL or successful source build is not completed installer acceptance. [S35]

Initial verification owner: Sprint 12. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

## Integration work owned elsewhere

### FND-01

The shipped application MUST have a native interface and no runtime dependency on a browser engine. Rust with GPUI is the implementation default. A different native approach requires a documented concrete feasibility reason and must preserve the no-browser constraint. An explicit sign-in or external-link action may open the user's existing default browser; this does not embed a browser in the application.

Initial owner: Sprint 01. Initial verification: Sprint 12. Native feasibility in 01; shipped dependency/process inventory in 12.

### PORT-01

Windows is the highest-priority first release, completed through Sprints 01-12. macOS in Sprint 13 and Linux in Sprint 14 are required later product milestones, not optional replacements for Windows. Publish an explicit support matrix covering OS version, CPU architecture, packaging and, for Linux, supported distribution/desktop/session/graphics prerequisites. Use the provisional targets in Section 3 until validated. Keep Windows completion distinct from later-platform readiness; no supported-platform claim is earned by compiling alone, and no later-platform release gate silently moves into the Windows phase.

Initial owner: Sprint 01. Initial verification: Sprint 12. Define phased scope and support matrix in 01; verify Windows phase statement in 12. macOS/Linux publication closes their separate platform rows in 13/14.

### PORT-02

Share domain models, GitHub integration, cache/search, synchronization, Outbox, review/diff logic, history, notification rules, and workspace state across platforms. Isolate operating-system APIs behind small capability interfaces with conditional dependencies inside platform implementations. Common code must not depend on Windows handles, registry calls, COM types, fixed Windows paths, or platform-specific credential objects. Reuse portable GPUI facilities where suitable rather than wrapping the entire toolkit in a new framework. Test interface contracts with fakes and actual adapters; do not duplicate the product's business logic for each OS.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce platform boundaries in 01 and keep shared modules portable throughout; verify the full Windows implementation boundary in 12 and parity in later platform rows.

### PORT-03

Prove the actual native renderer and necessary GPUI/GPUI Kit components early with selectable Markdown, text input/copy, large annotated diffs, the bound PR context/code layout, resizing, focus, and a graph fixture. Windows proof is a Sprint 01 gate. Configure compilation/core-test jobs for all intended targets and attempt macOS/Linux native smoke launches early on real target desktops; record gaps honestly and resolve them before that platform's release. In-process/headless tests or a successful cross-compile are not native GUI proof. No phase may add an embedded browser engine as a portability shortcut. [S31] [S32] [S33]

Initial owner: Sprint 01. Initial verification: Sprint 12. Windows native proof begins in 01; expand actual controls as built. Early other-target smoke is tracked separately; full native release evidence closes at each platform gate.

### PORT-04

Implement protected credential and application-data adapters per published OS. Use Windows Credential Manager or equivalent on Windows, the platform keychain on macOS, and a documented supported secure credential service on Linux. Store only credential references in SQLite; never fall back silently to plaintext tokens when a store is unavailable or locked. Preserve offline cached access while affected network work is held. Use appropriate per-user data/config/cache locations, stable app identity and relative managed asset paths rather than Windows-specific absolute paths. Test filesystem permissions, file locking, SQLite WAL/FTS availability, path/Unicode handling, and stable location across upgrades. Credentials and caches are per-device; this does not add cross-device synchronization. [S36]

Initial owner: Sprint 03. Initial verification: Sprint 12. Path/storage contracts in 02 and real Windows credential tests in 03; installed path and retention behavior closes in 12, with OS adapters in 13/14.

### PORT-05

Keep notification eligibility, filtering, grouping, delivery history and item routing shared, with per-OS notification/activation implementations. Verify permissions, native delivery, actions, cold/warm launch, single-instance routing, and any supported identity/replacement semantics in the packaged target app. Unsupported OS capabilities must have truthful capability reporting and usable in-app alternatives for actions such as Snooze and Mark read; no silent loss of activity or false replacement/retraction promise. Define supported desktop behavior explicitly before publishing, preserving the persistent feed and correct Open behavior. A toolkit method or Windows test does not prove another OS's integration. [S34]

Initial owner: Sprint 11. Initial verification: Sprint 12. Windows notification feasibility in 01, actual product delivery in 11, and exact installed-package activation in 12; later OS parity closes at its release.

### PORT-06

Implement and test platform-appropriate keyboard shortcuts, text input, accessibility, clipboard, high-DPI/scaling, theme changes, timezone defaults, external-link opening, startup registration, close versus Quit, background residence, sleep/wake and network-change hints. Feed normalized lifecycle events into the shared scheduler. Restore offline/pause settings before probes or dispatch. If a supported environment lacks a tray/menu-bar capability, keep the app reachable and explain background behavior rather than hiding an inaccessible running instance. Closing/quit must preserve drafts, workspace, Outbox and credentials according to the shared lifecycle contract. [S33] [S35]

Initial owner: Sprint 01. Initial verification: Sprint 12. Keep native interaction/lifecycle boundaries explicit from 01; complete Windows accessibility, restart and installed behavior in 12, and repeat for later targets.

### AUTH-11

Explicit sign-out MUST durably mark the affected account profile as signed out, prevent new network dispatch under that profile, and remove or invalidate its locally stored PAT material. Preserve readable private cached content, drafts, saved workspace, settings, synchronization checkpoints, activity baselines, and outbox records on the PC. Users may reopen that signed-out local profile for offline work without a renewed login; show its signed-out status and hold its remote actions until a PAT for the same verified user restores access. Keep that hold across restart. Ordinary exit is not sign-out. Expired or revoked PATs likewise preserve offline use and progress while holding affected remote work. A sign-out during an in-flight mutation does not prove remote cancellation; retain its evidence for reconciliation after access is restored. Do not expose one profile's data through a different active account or assume sign-out revokes the PAT at GitHub.

Initial owner: Sprint 03. Initial verification: Sprint 12. Signed-out credential/dispatch holds begin in 03; readable local-profile UI in 04 and all-writer PAT repair in 09; complete private-data retention and in-flight lifecycle matrix in 12.

### WORKSPACE-08

Save and restore the workspace per account, including pane order and proportions, open tab order, active tabs, focused pane, per-tab view state, scroll position or anchors, navigation context, and draft references. Normal exit MUST retain this state. On restart, render cached visible content before waiting for any API response or renewed login; then start eligible background work through the shared scheduler. Load inactive tabs lazily and expose missing or inaccessible content without discarding its navigation target. Restoring a workspace MUST NOT submit unsubmitted drafts, create duplicate operations, or expose another account's private tabs. The independent outbox worker resumes already submitted eligible actions under OUTBOX-04 and the restored authentication/pause settings; view restoration is not a dispatch trigger.

Initial owner: Sprint 04. Initial verification: Sprint 12. Restore local workspace/drafts before networking in 04; integrate all views and writers as built. Final normal-exit, signed-out, expired-PAT and delayed-network restoration closes in 12. View restoration never submits drafts.

### WORKSPACE-10

Share request scheduling and deduplicate in-flight fetches across views of the same resource and revision. Opening additional panes MUST NOT create independent polling loops or duplicate alerts for that resource. Give visible tabs priority within the existing schedule and rate-limit budget. Inactive tabs must not retain every large diff or continuously redraw; evict reconstructible rendering state while preserving navigation state and drafts. Closing one view must not cancel work still needed by another view or the shared write journal.

Initial owner: Sprint 05. Initial verification: Sprint 12. Shared scheduling in 05 and large-view retention in 06/10; complete integrated twenty-tab performance gate in 12.

### CACHE-06

Retain fetched text, review data, commit metadata, and diffs across restarts. Do not impose an undocumented age cutoff. Bound memory and render state independently of disk retention. Expose per-repository usage and explicit cleanup controls.

Initial owner: Sprint 04. Initial verification: Sprint 12. Disk retention/cleanup in 04; integrate all content types and verify bounded memory on full workload in 12.

### CACHE-13

Normal window close and application exit MUST retain downloaded content and resource coverage, all draft types, saved tabs/layout/view positions, local preferences, offline pins, pending actions and receipts, synchronization checkpoints/validators, and notification baselines/delivery history on the PC. Persist settings and user changes as they occur and safely flush remaining local state on orderly exit; if a user-data write fails, keep it recoverable and report the failure. Retain the protected credential according to AUTH-07. Restart MUST reuse these records and render the saved local workspace before networking completes. It must not clear the cache, reset baselines, or redownload all history merely because the process was restarted. Sign-out preserves local content under AUTH-11. Explicit cache cleanup remains a separate action and continues to protect drafts and unresolved work under CACHE-07 and OUTBOX-10.

Initial owner: Sprint 02. Initial verification: Sprint 12. Complete persistence schema in 02, PAT lifecycle in 03, local-first restoration in 04, sync in 05/06, all writer/history/notification types through 11, and the complete quit/sign-out/reopen matrix in 12.

### OFFLINE-01

Once an account and content have been established, the application MUST start and remain usable without a network check or renewed login blocking local access. Users can browse/search downloaded issues, PRs, discussions, reviews, commits, diffs, graph history, and the activity feed; use tabs and panes; edit local settings and drafts; and inspect or cancel pending work. State the cached coverage and last fetch honestly. Unseen remote content and new server events cannot be available until fetched. First-time authentication and initial acquisition of private content still require access; do not require a full repository download.

Initial owner: Sprint 04. Initial verification: Sprint 12. Offline startup/workspace in 04; each new view and action integrates as built; complete all-feature cold-start offline matrix in 12.

### SYNC-09

Run synchronization, connectivity checks, and the outbox worker in the application or optional tray process. A fully exited application or sleeping machine cannot perform local polling or send actions. Normal exit preserves local state and the protected PAT; it does not sign the user out. On launch or wake, restore the local workspace and persisted control settings first, then resume eligible actions and bounded incremental read catch-up under SYNC-11 when connectivity and authorization permit. No hosted service is required for the initial release.

Initial owner: Sprint 05. Initial verification: Sprint 12. Lifecycle scheduling begins in 05, all writers integrate by 09, graph/feed families by 11, and retained-credential plus local-first incremental restart acceptance closes in 12.

### SYNC-11

Resume from persisted per-account, per-repository, per-resource-family synchronization state on restart, reconnection, and same-user PAT renewal. Retain the last successful checkpoint, overlapping update window or supported cursor, pagination progress, conditional-request validators, resource completeness/freshness, and notification baselines. Advance content and its successful progress transactionally after each completed unit; failures or partial pages must not skip unseen changes. Prioritize visible resources and run bounded, rate-aware incremental catch-up, using supported changed-since/update feeds, conditional reads, and targeted reconciliation. Do not blindly redownload the entire cache, re-enumerate every historical discussion, reset notification baselines, or use the largest issue/PR number as the only cursor. If a cursor expires or an endpoint has no reliable delta interface, re-enumerate the necessary resource/range with stable-ID deduplication and preserved coverage while keeping other completed families intact. An update-feed page one, overlap, unchanged revalidation, or targeted repair may be necessary; no universal exact-delta or zero-unchanged-bytes guarantee is made. Preserve old-item update discovery and honor restored sign-out, offline, and pause state. [S06] [S07] [S11] [S13]

Initial owner: Sprint 05. Initial verification: Sprint 12. Persist checkpoints and stable account identity in 02/03; issue/comment restart catch-up in 05 and PR-family AC-75/76 in 06; all graph/notification families and full lifecycle request traces complete in 12.

### OUTBOX-10

Treat unresolved outbox operations, payloads, cancellation flags, dependency mappings, base snapshots, anchors, and remote reconciliation evidence as durable user data. Protect them from cache eviction, repair resets, account sign-out, and migration. Recover interrupted claims conservatively after a crash; an interrupted Sending state becomes Outcome unknown unless evidence proves it was never dispatched. Reconcile a successful remote write whose local completion transaction failed. Pending actions have no silent age expiry. Bound and virtualize the outbox UI, redact routine diagnostics, and preserve essential local content until completion/cancellation and any dependent work are settled.

Initial owner: Sprint 02. Initial verification: Sprint 12. Persist/protect queue data in 02, integrate all writer snapshots/receipts in 07-09, then verify full upgrade/cleanup/crash/backlog matrix in 12.

### DATA-01

Apply migrations transactionally where supported and test upgrading representative databases. Back up user-authored drafts and unresolved outbox work before risky transformations. Detect corruption or incompatible schema versions without silently resetting user work. Preserve operation identity, cancellation decisions, and reconciliation evidence across migration and recovery.

Initial owner: Sprint 02. Initial verification: Sprint 12. Migration and recovery mechanisms in 02; validate representative upgrade databases after all draft types and release schema exist.

### DATA-03

Keep secrets and private content out of routine diagnostic output. Export diagnostics only through an explicit user action with appropriate redaction. Treat remote Markdown, links, and filenames as data rather than instructions or executable paths.

Initial owner: Sprint 02. Initial verification: Sprint 12. Establish redaction/content safety boundaries early; verify all parsers and final explicit diagnostic export in 12.

### UX-01

Support platform-appropriate keyboard navigation, focus indication, copy and paste, high-DPI displays, light and dark appearance, and sensible resizing. Test real text editors and review controls with the accessibility features used by the target audience on each published OS. Windows is verified first; macOS/Linux parity closes in their release phases.

Initial owner: Sprint 04. Initial verification: Sprint 12. Keyboard, accessibility, DPI and appearance checked as native controls arrive; integrated release verification in 12.

### INSTALL-01

Each published platform MUST offer an end-user installation and rerunnable update path requiring one to three documented commands in its normal shell. Install prebuilt release artifacts; do not require Rust, Cargo, a compiler, Git checkout, or local source compilation. Use a per-user install by default without administrator privileges where the supported platform permits. Normal OS trust/permission prompts and the app's later PAT setup are allowed and must be explained; one to three commands does not promise zero interaction. The flow assumes the explicitly documented supported OS/system prerequisites, not hidden manual toolchain setup.

Initial owner: Sprint 01. Initial verification: Sprint 12. Define short-command/prebuilt/compatibility/trust contracts in 01; deliver actual Windows artifacts, exact published command tests and release verification in 12. macOS/Linux counterparts close in their own rows.

### INSTALL-02

Before changing the installed application, detect and validate OS, architecture, minimum supported version and required runtime/graphics/desktop prerequisites. Select the exact compatible artifact from a versioned release manifest; reject unsupported combinations with an actionable explanation. Avoid silent architecture substitution, undocumented compatibility assumptions, or downloading a different channel/version than requested. App dependencies should be bundled where practical; unavailable system prerequisites produce precise repair guidance instead of silent system modifications.

Initial owner: Sprint 01. Initial verification: Sprint 12. Define short-command/prebuilt/compatibility/trust contracts in 01; deliver actual Windows artifacts, exact published command tests and release verification in 12. macOS/Linux counterparts close in their own rows.

### INSTALL-03

Verify artifact integrity and publisher authenticity before execution or installation using the platform's appropriate signing/trust mechanism or a verified signed release manifest with an established trust root. A checksum alone establishes integrity, not publisher authenticity. Download over authenticated transport, validate the selected version/manifest/artifact relationship, fail closed on mismatch or failed verification, and clean up failed staging files. Do not require weakening OS security, disabling certificate checks, execution-policy bypass, or unauthenticated remote-script execution. The documented short command flow must preserve this trust chain.

Initial owner: Sprint 01. Initial verification: Sprint 12. Define short-command/prebuilt/compatibility/trust contracts in 01; deliver actual Windows artifacts, exact published command tests and release verification in 12. macOS/Linux counterparts close in their own rows.

### INSTALL-04

Stage and verify an update before replacing the installed binary. Handle a running instance and file locks safely, prevent simultaneous migration/Outbox dispatch during replacement, and preserve SQLite data, drafts, tabs/layout, settings, offline pins, credential references/protected tokens, sync checkpoints, baselines, and pending-action identities/receipts. Back up user data before risky migrations, and recover from interrupted install/update without falsely confirming actions or losing local work. Roll back binaries only when schema compatibility is safe; do not blindly restore an old Outbox snapshot that could replay already applied mutations. Explain recovery if migration cannot safely continue.

Initial owner: Sprint 02. Initial verification: Sprint 12. Storage backup/migration and durable operation invariants begin in 02; real all-state installed upgrades close in 12, then repeat per later platform.

### TEST-01

Implement a documented development test runner and relevant tests in every sprint, not only at release. The required command contract is cargo xtask test unit; cargo xtask test integration; cargo xtask test ui; cargo xtask test e2e --suite smoke or --suite full; cargo xtask test install --artifact <path>; and cargo xtask test live --repo OWNER/REPO with an explicit --allow-live-writes opt-in for approved mutations. Implement these commands or a documented equivalent wrapper before claiming their gates passed. They are planned commands, not tools already provided by this requirements ZIP. Each command returns a meaningful exit status, reports selected/skipped/blocked tests and environment, and writes machine-readable results plus concise logs. End users install prebuilt binaries and need none of this development tooling.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-02

Unit tests MUST cover nontrivial domain and state-machine decisions introduced by each sprint: shared item identities, absent-versus-null updates, reference parsing/type guards, completeness, schedule/DST rules, notification precedence, draft ownership, dependencies, cancellation, retry eligibility, conflict/revision checks and diff source anchors. Use controlled clocks, seeded fixtures and small independently checkable expectations. Test meaningful boundaries and regressions rather than mirroring implementation lines or pursuing an arbitrary coverage percentage. Add the relevant tests with the feature; future feature tests remain pending.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-03

Integration tests MUST exercise real SQLite databases/migrations/FTS/transactions and actual provider serialization/HTTP paths against a controllable local API server, not only mocked repository methods. Cover pagination/resumption, SSO/permissions/partial data, rate limits, cache publication, persisted checkpoints, signed-out identity holds, Outbox claiming/reconciliation, and upgrade cleanup protection. Verify production interfaces with deterministic simulated remote responses and OS adapter contract tests; use real protected-store/notification integration checks on the applicable native host. Keep live GitHub tests separate from deterministic integration suites.

Initial owner: Sprint 02. Initial verification: Sprint 12. Real SQLite integration starts in 02 and actual HTTP adapter integration in 03; later modules add cases, with full Windows integration release evidence in 12.

### TEST-04

End-to-end tests MUST launch and drive the actual native application process through its user-visible controls using supported native automation/accessibility/input, with real local persistence and the implemented HTTP adapter connected to a deterministic test server where appropriate. Start with launch/focus/quit smoke coverage in Sprint 01 and expand to cache-first reading, tab/context switching, offline drafts, queued submissions/cancellation/reconnect, review/merge revision guards, notifications and restoration as those features arrive. Screenshots, GPUI in-process UI tests, callbacks, domain function calls, or a web browser test are not substitutes for native-process E2E. Keep GPUI UI integration tests as an additional layer. Distinguish simulator/test-build native E2E from smoke and release checks of the exact installed published package; the former does not prove packaging, release feature flags or publisher trust. A native automation gap is a visible blocked gate, not a reason to relabel a unit test E2E; record reproducible manual evidence for any inherently manual OS prompt and automate the remaining journey. [S32]

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-05

Provide isolated reproducible fixtures and fault injection for delayed/out-of-order responses, partial pagination, 401/403/404/429/5xx, offline/online changes, sleep/wake, interrupted writes/migrations, full or unwritable storage, process crash before/after dispatch, timeout after remote success, cancelled predecessor chains, changed PR heads and removed links. Exercise exact data/state invariants and request traces, including no blind replay or retargeting. Use temporary per-test profiles, synthetic secrets/data and controlled test servers; reset deterministically without accessing a user's profile or emitting real notifications/writes accidentally. Preserve failure artifacts with secret redaction.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-06

Configure native-target build and portable-core unit/integration CI jobs for Windows, macOS and Linux from Sprint 01, adding actual suites as modules become available. Native Windows smoke E2E is required as part of each relevant Sprint 01-12 increment; full Windows suites gate Sprint 12. Attempt and record early native macOS/Linux smoke evidence, then require complete applicable native/E2E/install/performance coverage at Sprints 13/14. A later-platform runner or graphics limitation remains visibly Blocked/Not run for that target and must be resolved before publishing it. It does not silently prevent an otherwise complete Windows phase, while a discovered defect in shared correctness still requires repair. Headless rendering, cross-compilation and fake providers must be labeled accurately.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-07

Live-provider validation MUST use an explicitly designated disposable repository and appropriate test identities with no credentials embedded in code or results. Read-only live tests are the default; mutation tests require explicit --allow-live-writes and a configured allowlisted owner/repository, and cover only supported edits/comments/reviews/merges. Use existing issue/PR fixtures and do not add an app issue/PR-creation flow for test setup. Preserve ordinary SSO and permission enforcement. Keep destructive cases isolated, identify any effects and cleanup, and mark unavailable credentials/permissions Blocked rather than inventing successful integration evidence.

Initial owner: Sprint 03. Initial verification: Sprint 12. Guarded live read setup in 03; explicitly allowed supported mutations in 07-09; required live evidence closes at each applicable platform release without inventing credentials.

### TEST-08

A sprint report MUST list exact commands, commit/build identity, OS/version/architecture, fixture/seed, test layer and scope, results, failure artifacts, limitations, and requirement/AC/scenario links. Distinguish Pass, Fail, Blocked and Not run; skipped or mocked behavior never counts as a live/native pass. Maintain requirement, acceptance and scenario evidence per release platform, identifying simulator/test builds separately from the exact installed published package and its digest/signature. Every phase gate requires applicable unit/integration/native E2E/install tests, published-package smoke evidence, and targeted performance/accessibility evidence with no unexplained required failures. Rerun affected regression tests when shared code changes; do not claim all-platform completion from a Windows report or from validation of this planning package.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

## Verification due for this phase

FND-01, PORT-01, PORT-02, PORT-03, PORT-04, PORT-05, PORT-06, AUTH-10, AUTH-11, WORKSPACE-08, WORKSPACE-10, CACHE-06, CACHE-13, OFFLINE-01, SYNC-09, SYNC-11, OUTBOX-10, DATA-01, DATA-03, UX-01, INSTALL-01, INSTALL-02, INSTALL-03, INSTALL-04, INSTALL-05, INSTALL-06, INSTALL-07, INSTALL-08, TEST-01, TEST-02, TEST-03, TEST-04, TEST-05, TEST-06, TEST-07, TEST-08.

Use requirement_platform_traceability.csv for applicability and release-gate evidence. Initial ledger statuses refer only to their initial phase; no single Windows result closes all platforms. Keep future options and unavailable services explicit.

## Later scope and handoff

Sprint 13 delivers macOS and Sprint 14 Linux with the same shared product contract and their own native/install gates. Extra architectures/OS variants, attachment uploads, local Git working-tree operations and hosted auto-update remain separately defined scope. The Windows phase requires trusted simple install and rerun updates.

## Coding LLM instruction

Implement Sprint 12 against v1.7. Keep the app buildable and preserve the current product constraints. Complete listed implementation and test work, use actual native controls for E2E, protect original targets/drafts/Outbox state, and add only the named platform integrations. Do not add issue/PR creation, merge queues or browser engines. Keep secrets out of code/results; live writes require explicit allowlisted authorization. Implement the short verified prebuilt install/update contract for the relevant publication phase and never claim placeholder commands runnable. Update initial/per-platform/scenario ledgers with commands, environment, package identity and evidence, then use SPRINT_REPORT_TEMPLATE.md. Continue only within the assigned sprint/Windows/all-platform scope without new routine approvals.

## Technical sources

- [S06 GitHub issue and PR conversation comments](https://docs.github.com/en/rest/issues/comments)
- [S07 GitHub REST pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api)
- [S11 GitHub REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)
- [S13 GitHub inline review comments](https://docs.github.com/en/rest/pulls/comments)
- [S23 GitHub OAuth app authorization and organization access](https://docs.github.com/en/enterprise-cloud%40latest/apps/oauth-apps/using-oauth-apps/authorizing-oauth-apps)
- [S31 GPUI Kit installation and target prerequisites](https://gpui-kit.com/docs/installation/)
- [S32 GPUI Kit testing facilities and limits](https://gpui-kit.com/docs/test/)
- [S33 GPUI Kit accessibility](https://gpui-kit.com/docs/accessibility/)
- [S34 GPUI Kit system notification capabilities](https://gpui-kit.com/docs/system-notification/)
- [S35 GPUI Kit packaging](https://gpui-kit.com/docs/packaging/)
- [S36 Keyring native protected credential stores](https://docs.rs/keyring/latest/keyring/v1/index.html)

Additional current primary sources and platform/install/test constraints are in the three companion plans. Verify pinned tool/API behavior during implementation.

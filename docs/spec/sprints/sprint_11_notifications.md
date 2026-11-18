# Sprint 11: Notification rules and native delivery

Requirements version 1.7 | Initial sprint status Planned

## Outcome

Users receive only the selected activity, including unopened items, with reliable local history, bot suppression, grouping, and correct workspace activation.

## Phase and prerequisites

- Release phase: Windows first release.
- Prerequisites: Sprint 05, Sprint 06, Sprint 07, Sprint 09, Sprint 10.
- Code areas: notifications, storage/rules and delivery journal, platform_windows/toasts and activation, ui/activity feed and settings, workspace/deep links.
- Read the master, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and all relevant scenario/platform ledgers.
- Use actual prior evidence; a missing dependency remains blocked rather than passed. Later-platform release work does not silently move into the Windows phase.

## Implementation tickets

### S11-T01

Integrate all required event categories from the existing observers, including commits/branch updates, ordinary and inline comments, review requests/results, PR opening/readiness/merges, assignments/state, mentions, and supported checks. Monitor unopened items without downloading their full history.

### S11-T02

Implement global defaults, per-repository overrides, relevance/followed-item filters, feed-plus-pop-up/feed-only/off, sound off by default, mute, snooze, quiet hours, and own-action suppression.

### S11-T03

Implement actor/category/repository rules with stable identities, bot/app metadata, an explicit automated-review rule covering formal and inline reviews, separately configurable ordinary bot comments, and human-reply preservation.

### S11-T04

Implement the complete NOTIFY-10 precedence including its second paragraph. Resolve independent category paths, mention exceptions, mute, own actions, quiet hours, and snooze before grouping and deduplication; provide a human-readable rule preview.

### S11-T05

Complete the persistent feed and decision/delivery journal, baseline safety, burst grouping, current-preference catch-up, and stable Windows notification identity. Do not promise exactly-once delivery or discovery of every transient state.

### S11-T06

Implement native Open/Snooze/Mark read actions and click activation into the correct existing/new tab, item, thread, or comparison, including cold launch. Preserve unrelated panes and drafts, respect Windows notification preferences, and keep the ingestion interface extensible without a hosted relay.

### S11-T07

Restore rule settings, event-category baselines, feed read state, deduplication keys, and delivery history after normal exit, sign-out, and same-user PAT renewal. Catch up only eligible observed new activity without recreating historical alerts; complete notification contributions to AC-73/74 for Sprint 12.

### S11-T08

Complete the platform notification capability contract and real Windows native delivery/activation/lifecycle tests, including permissions, cold launch, single instance, stable target routing and in-app alternatives for unsupported OS actions. Pass AC-86 on the current packaged development slice and repeat exact published-package checks in Sprint 12.

## Required tests and commands

- Unit/integration: every notification category, precedence/mentions/actor rules, baseline/newness, grouping and durable delivery decisions.
- Native E2E/integration: real Windows notifications, permissions, cold/warm activation, single instance and native actions with an isolated profile.
- Lifecycle regression: sleep/wake, close/Quit, optional background operation and exact target/draft preservation; repeat packaged release smoke in Sprint 12.

Implement and use the canonical runner from TESTING_PLAN.md: unit, integration, ui, native e2e smoke/full, install with an artifact, and separately guarded live tests. These are development commands to build, not commands already supplied by this ZIP. Record actual OS/build/fixture evidence and mark unavailable checks Blocked/Not run. Native test-build E2E and exact published-package smoke remain separate.

Scenario IDs introduced or closing here: UT-014, UT-015, IT-018, UI-007, E2E-012, E2E-013, E2E-018.

Live-provider cases are shared gates on an explicitly authorized host. A later platform can reference existing evidence only with a recorded applicability assessment; rerun changed provider/credential-policy or target-specific behavior. Missing authorization remains Blocked/Not run and never permits unapproved writes.

Use test_scenarios.csv for fixture IDs, expected results, first/closure ownership and evidence. Release sprints rerun all scenarios applicable to their target; earlier sprint scenario lists do not waive regression checks.

## Deliverables

Persistent activity feed; complete notification settings/rule engine; native pop-ups and actions; correct activation; category-by-category baseline and delivery tests.

## Exit gates

- Pass AC-27 through AC-34, AC-39 through AC-41, and AC-47. Run real Windows activation tests plus deterministic rule and event fixtures.
- Test each supported category for baseline versus new activity, duplicate polling/restart, own actions, grouping, and capability limits. An old first-time download must never create historical banners.
- Verify bot formal/inline-review suppression preserves ordinary comments when separately enabled, human replies, and the underlying conversation. Test Comments Off with Direct mentions Pop-up and all exception/mute/snooze combinations.
- Demonstrate an unopened old issue receiving a genuinely new comment, a watched branch gaining commits, and an unmerged-to-merged transition, without an initial full archive or a classic token solely for custom pop-ups.

## Initial acceptance scenarios owned here

- **AC-27** A new comment on an unopened monitored issue can produce the configured notification without downloading all repository history.
- **AC-28** Downloading an old PR for the first time produces no historical activity banners.
- **AC-29** Muting a selected review bot suppresses its configured review alerts while preserving human replies and all PR content.
- **AC-30** Notification rule precedence is deterministic, including direct-mention exceptions, mute, snooze, and quiet hours.
- **AC-31** A grouped review produces one sensible notification, and ordinary restart does not repeat an already delivered event.
- **AC-32** Reconnection after a long offline period produces grouped catch-up activity rather than a banner flood.
- **AC-33** Clicking a Windows notification opens the correct cached item or thread, including when activation launches the app.
- **AC-34** A notification for a comment containing a mention does not produce duplicate banners for two categories by default.
- **AC-39** A watched branch gaining commits produces its configured grouped alert; an unmonitored branch does not. Commits already present at the monitoring baseline produce no historical alert.
- **AC-40** An observed unmerged-to-merged PR transition produces the configured delivery without duplicates in the normal retry/restart path. A PR already merged at the initial baseline produces no merge alert merely because it was discovered.
- **AC-41** Comments Off with Direct mentions Pop-up produces one eligible mention alert. Actor suppression blocks it unless the explicit mention exception applies; own-action suppression, repository mute, snooze, and quiet hours still apply.
- **AC-47** Opening a search result or following a notification focuses or creates the intended item tab and navigates to the correct target without replacing an unrelated tab or changing another pane's input. Account switching isolates saved tabs and private content.
- **AC-86** The packaged target app delivers a native notification, opens the exact item through cold/warm single-instance activation, honors notification preferences, and exposes unsupported actions/replacement semantics honestly with in-app alternatives. Close/Quit, optional background residence, wake and startup behavior preserve pause/offline settings and user data. Windows closes initially in Sprint 11, with clean-machine packaging repeated at release; later OS phases require their own integration evidence.

## Full owned requirements

### AUTH-09

Custom activity notifications MUST work independently of GitHub's own notifications inbox. Do not require a classic token solely to show the app's custom pop-ups. Synchronizing GitHub notification read/unread state is optional and has separate token compatibility constraints. [S14]

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-01

Provide native desktop notifications and a persistent in-app activity feed, with Windows integration first and macOS/Linux integration in their release phases under PORT-05. The native integration must support opening the correct item when clicked, single-instance behavior, and useful notification identity or grouping. Validate the chosen Rust and native API integration early, with Windows first and actual target evidence before each later release. [S24]

Initial verification owner: Sprint 11. Native notification/tray/activation feasibility in 01; persistent feed, rules and production activation in 11.

### NOTIFY-03

Provide independent controls for new commits or observed branch updates, ordinary comments, inline comments, review requests, approvals, requests for changes, comment-only reviews, PR opening or readiness, merges, issue assignments, issue state changes, and direct mentions. Check-status notifications are included where the credential and API support them.

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-05

Support per-user global defaults and per-repository overrides. Relevant-item filters include all monitored activity, items involving the user, and explicitly followed or pinned items. Store preferences locally.

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-06

Each enabled category must support pop-up plus activity feed, activity feed only, or off. Sound is independent and off by default. Provide repository mute, thread snooze, quiet hours, and suppression of the user's own actions.

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-07

Suppression rules match stable actor identity, event category, and optional repository scope. Use bot or app metadata where reliable and allow explicit account rules. Do not promise perfect automatic recognition of every automated review.

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-08

A rule for automated review activity must cover the selected actor's formal reviews and inline review comments. Ordinary PR comments from that account are separately configurable. Human replies, unrelated events, and all activity on a bot-authored PR must not be suppressed merely because the PR author is a bot.

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-09

Muting notifications MUST NOT remove or hide the underlying review or comment in its issue or PR view. Provide a convenient Mute review activity from this account action and a settings preview explaining the resulting rule.

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-10

Resolve notification categories independently. An Off setting disables that category's delivery path, not every other category matched by the same activity. Comments Off with Direct mentions Pop-up therefore permits one mention notification. Resolve per-repository overrides before global category defaults, then combine eligible paths into one notification using the strongest permitted delivery mode.

Apply global disablement and repository mute first. Apply own-action suppression and actor rules to the underlying activity across its eligible category paths. An explicitly enabled mention exception can restore only an enabled mention path suppressed by an actor rule. It cannot override global disablement, repository mute, own-action suppression, an Off mention category, snooze, or quiet hours. Apply snooze and quiet-hour delivery restrictions before grouping and deduplication. Keep enough decision information to explain the result.

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-11

Quiet hours and snooze suppress pop-ups while retaining otherwise eligible activity-feed entries. Synchronization continues according to its independent schedule. An explicit category Off setting disables that category's feed and pop-up path; a separately enabled matching category may still qualify under NOTIFY-10. Global disablement and repository mute suppress all notification paths in their scope. Underlying repository content remains available normally.

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-12

Group related bursts by repository and issue, PR, or branch. A review with many inline comments should normally produce one alert. An example is 3 new commits on feature/search, with one action opening the corresponding comparison.

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-17

After a long offline period, group eligible catch-up events into a readable summary. Do not replay a storm of old banners. Use current notification preferences when delivering queued activity.

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-18

Clicking a notification must navigate to the correct repository, item, and available thread or diff using the tab routing in WORKSPACE-11. Show cached summary content immediately and fetch missing details on demand while preserving unrelated panes. Support useful actions such as Open, Snooze, and Mark read.

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-19

Respect the published operating system's notification preferences. Local monitoring runs only while the application or tray process is active. Polling detects observed changes but cannot guarantee every transient push or state transition between checks. GitHub's general Events API has retention limits and documented latency; do not use it as the sole reliable event source. [S25]

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### NOTIFY-20

A hosted webhook relay and stronger always-on event delivery are later capabilities. Keep the event-ingestion interface extensible, but do not add required infrastructure to the first release.

Initial verification owner: Sprint 11. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### PORT-05

Keep notification eligibility, filtering, grouping, delivery history and item routing shared, with per-OS notification/activation implementations. Verify permissions, native delivery, actions, cold/warm launch, single-instance routing, and any supported identity/replacement semantics in the packaged target app. Unsupported OS capabilities must have truthful capability reporting and usable in-app alternatives for actions such as Snooze and Mark read; no silent loss of activity or false replacement/retraction promise. Define supported desktop behavior explicitly before publishing, preserving the persistent feed and correct Open behavior. A toolkit method or Windows test does not prove another OS's integration. [S34]

Initial verification owner: Sprint 12. Windows notification feasibility in 01, actual product delivery in 11, and exact installed-package activation in 12; later OS parity closes at its release.

## Integration work owned elsewhere

### PORT-02

Share domain models, GitHub integration, cache/search, synchronization, Outbox, review/diff logic, history, notification rules, and workspace state across platforms. Isolate operating-system APIs behind small capability interfaces with conditional dependencies inside platform implementations. Common code must not depend on Windows handles, registry calls, COM types, fixed Windows paths, or platform-specific credential objects. Reuse portable GPUI facilities where suitable rather than wrapping the entire toolkit in a new framework. Test interface contracts with fakes and actual adapters; do not duplicate the product's business logic for each OS.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce platform boundaries in 01 and keep shared modules portable throughout; verify the full Windows implementation boundary in 12 and parity in later platform rows.

### PORT-06

Implement and test platform-appropriate keyboard shortcuts, text input, accessibility, clipboard, high-DPI/scaling, theme changes, timezone defaults, external-link opening, startup registration, close versus Quit, background residence, sleep/wake and network-change hints. Feed normalized lifecycle events into the shared scheduler. Restore offline/pause settings before probes or dispatch. If a supported environment lacks a tray/menu-bar capability, keep the app reachable and explain background behavior rather than hiding an inaccessible running instance. Closing/quit must preserve drafts, workspace, Outbox and credentials according to the shared lifecycle contract. [S33] [S35]

Initial owner: Sprint 01. Initial verification: Sprint 12. Keep native interaction/lifecycle boundaries explicit from 01; complete Windows accessibility, restart and installed behavior in 12, and repeat for later targets.

### AUTH-11

Explicit sign-out MUST durably mark the affected account profile as signed out, prevent new network dispatch under that profile, and remove or invalidate its locally stored PAT material. Preserve readable private cached content, drafts, saved workspace, settings, synchronization checkpoints, activity baselines, and outbox records on the PC. Users may reopen that signed-out local profile for offline work without a renewed login; show its signed-out status and hold its remote actions until a PAT for the same verified user restores access. Keep that hold across restart. Ordinary exit is not sign-out. Expired or revoked PATs likewise preserve offline use and progress while holding affected remote work. A sign-out during an in-flight mutation does not prove remote cancellation; retain its evidence for reconciliation after access is restored. Do not expose one profile's data through a different active account or assume sign-out revokes the PAT at GitHub.

Initial owner: Sprint 03. Initial verification: Sprint 12. Signed-out credential/dispatch holds begin in 03; readable local-profile UI in 04 and all-writer PAT repair in 09; complete private-data retention and in-flight lifecycle matrix in 12.

### WORKSPACE-08

Save and restore the workspace per account, including pane order and proportions, open tab order, active tabs, focused pane, per-tab view state, scroll position or anchors, navigation context, and draft references. Normal exit MUST retain this state. On restart, render cached visible content before waiting for any API response or renewed login; then start eligible background work through the shared scheduler. Load inactive tabs lazily and expose missing or inaccessible content without discarding its navigation target. Restoring a workspace MUST NOT submit unsubmitted drafts, create duplicate operations, or expose another account's private tabs. The independent outbox worker resumes already submitted eligible actions under OUTBOX-04 and the restored authentication/pause settings; view restoration is not a dispatch trigger.

Initial owner: Sprint 04. Initial verification: Sprint 12. Restore local workspace/drafts before networking in 04; integrate all views and writers as built. Final normal-exit, signed-out, expired-PAT and delayed-network restoration closes in 12. View restoration never submits drafts.

### WORKSPACE-11

Repository lists, search results, in-app links, and notification activation MUST route through the workspace's open-item behavior. For notification or external URL activation, prefer a matching tab in the focused pane, otherwise the most recently active matching tab elsewhere, otherwise a new tab in the focused pane. Apply WORKSPACE-03 when choosing among matches within a pane. Navigate only the chosen tab to the requested comment or diff location. Other panes retain their positions and unsent input.

Initial owner: Sprint 04. Initial verification: Sprint 11. Open-item routing begins in 04; real notification activation completes the requirement in 11.

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

### NOTIFY-02

Activity monitoring MUST run independently of demand-loaded full content. Enabled events must be detectable on selected repositories even for issues and PRs the user has never opened. Monitoring must not require a complete historical download.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-04

Commit monitoring must support the default branch, chosen branches, branch patterns, or PRs the user follows. Identify observed new commits accurately. Do not infer who pushed a commit from its author if push-actor information is unavailable.

Initial owner: Sprint 10. Initial verification: Sprint 11. Branch-scope selection, baselines and accurate observations in 10; grouped delivery verified in 11.

### NOTIFY-13

Establish a baseline for each repository and event category when monitoring begins. Downloading old content for the first time MUST NOT itself create a new-activity notification. A genuinely new comment on an old unopened issue may still qualify based on its trustworthy event identity and timestamp.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-14

Where no trustworthy earlier state or event time exists, establish state without inventing a transition. Deduplicate events across polling endpoints and multiple notification categories. A comment that mentions the user should not produce duplicate comment and mention banners by default.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-15

Persist event keys, eligibility decisions, grouping keys, and delivery state. Reuse stable native notification identities where supported and report unsupported replacement/retraction capabilities truthfully. Prevent ordinary duplicate delivery across retries and restart; do not claim an unconditional exactly-once guarantee across application crashes and OS delivery.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-16

Distinguish comment creation from editing. Record the user's successful writes so a subsequent poll does not echo them as unsolicited new activity when own-action suppression is enabled.

Initial owner: Sprint 07. Initial verification: Sprint 11. Creation/edit distinction in 05; own successful issue/comment/review/merge writes recorded in 07-09; suppression verified in 11.

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

### TEST-08

A sprint report MUST list exact commands, commit/build identity, OS/version/architecture, fixture/seed, test layer and scope, results, failure artifacts, limitations, and requirement/AC/scenario links. Distinguish Pass, Fail, Blocked and Not run; skipped or mocked behavior never counts as a live/native pass. Maintain requirement, acceptance and scenario evidence per release platform, identifying simulator/test builds separately from the exact installed published package and its digest/signature. Every phase gate requires applicable unit/integration/native E2E/install tests, published-package smoke evidence, and targeted performance/accessibility evidence with no unexplained required failures. Rerun affected regression tests when shared code changes; do not claim all-platform completion from a Windows report or from validation of this planning package.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

## Verification due for this phase

AUTH-09, WORKSPACE-11, NOTIFY-01, NOTIFY-02, NOTIFY-03, NOTIFY-04, NOTIFY-05, NOTIFY-06, NOTIFY-07, NOTIFY-08, NOTIFY-09, NOTIFY-10, NOTIFY-11, NOTIFY-12, NOTIFY-13, NOTIFY-14, NOTIFY-15, NOTIFY-16, NOTIFY-17, NOTIFY-18, NOTIFY-19, NOTIFY-20.

Use requirement_platform_traceability.csv for applicability and release-gate evidence. Initial ledger statuses refer only to their initial phase; no single Windows result closes all platforms. Keep future options and unavailable services explicit.

## Later scope and handoff

GitHub inbox read/unread synchronization and hosted always-on delivery remain optional/later capabilities. A fully exited or sleeping computer does not perform local polling. Installer activation is repeated on a clean machine in Sprint 12.

## Coding LLM instruction

Implement Sprint 11 against v1.7. Keep the app buildable and preserve the current product constraints. Complete listed implementation and test work, use actual native controls for E2E, protect original targets/drafts/Outbox state, and add only the named platform integrations. Do not add issue/PR creation, merge queues or browser engines. Keep secrets out of code/results; live writes require explicit allowlisted authorization. Implement the short verified prebuilt install/update contract for the relevant publication phase and never claim placeholder commands runnable. Update initial/per-platform/scenario ledgers with commands, environment, package identity and evidence, then use SPRINT_REPORT_TEMPLATE.md. Continue only within the assigned sprint/Windows/all-platform scope without new routine approvals.

## Technical sources

- [S06 GitHub issue and PR conversation comments](https://docs.github.com/en/rest/issues/comments)
- [S07 GitHub REST pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api)
- [S11 GitHub REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)
- [S13 GitHub inline review comments](https://docs.github.com/en/rest/pulls/comments)
- [S14 GitHub notifications API compatibility](https://docs.github.com/en/rest/activity/notifications)
- [S24 Windows desktop notification APIs](https://learn.microsoft.com/en-us/windows/apps/develop/notifications/)
- [S25 GitHub Events API limitations](https://docs.github.com/en/rest/activity/events)
- [S32 GPUI Kit testing facilities and limits](https://gpui-kit.com/docs/test/)
- [S33 GPUI Kit accessibility](https://gpui-kit.com/docs/accessibility/)
- [S34 GPUI Kit system notification capabilities](https://gpui-kit.com/docs/system-notification/)
- [S35 GPUI Kit packaging](https://gpui-kit.com/docs/packaging/)

Additional current primary sources and platform/install/test constraints are in the three companion plans. Verify pinned tool/API behavior during implementation.

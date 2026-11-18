# Sprint 05: Synchronization, connectivity, and activity observation

Requirements version 1.7 | Initial sprint status Planned

## Outcome

Relevant cached content stays fresh on configurable schedules without blocking reading or forcing an archive; trustworthy activity observation exists for later notifications.

## Phase and prerequisites

- Release phase: Windows first release.
- Prerequisites: Sprint 02, Sprint 03, Sprint 04.
- Code areas: sync, github/conditional reads and update feeds, notifications/observation contracts, storage/jobs and baselines, settings, workspace subscriptions.
- Read the master, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and all relevant scenario/platform ledgers.
- Use actual prior evidence; a missing dependency remains blocked rather than passed. Later-platform release work does not silently move into the Windows phase.

## Implementation tickets

### S05-T01

Implement workdays, work windows, timezone, work/off-hours intervals, pause, and provisional defaults from SYNC-02. Use a controllable clock, timezone-aware DST behavior, monotonic delays, and coalesced sleep/reconnect catch-up.

### S05-T02

Complete one durable, fair scheduler with visible-pane priority, activity monitoring, recent/pinned content, and slower reconciliation under a shared budget. Deduplicate same-resource/revision reads across panes, including cancellation reference tracking.

### S05-T03

Centralize read pagination, bounded retries, conditional requests, rate-limit/polling headers, backoff, and restart resumption. Remove monitored repositories from active polling. Never apply a generic retry rule to non-idempotent writes.

### S05-T04

Track independent resource-family progress and overlapping update windows. Discover old closed/merged item changes and comment edits without relying on the largest item number or only parent timestamps. Keep full older content on demand.

### S05-T05

Implement lightweight item and ordinary-comment observations for unopened items, repository/category baselines, stable event identities, creation-versus-edit distinction, and journal persistence. Define explicit observer contracts for reviews/checks in Sprint 06 and branches in Sprint 10.

### S05-T06

Expose last success, pending jobs, throttling, auth failures, and resumable errors without replacing cached content. Confirm application/tray/exit behavior and bounded rendering retention for inactive tabs.

### S05-T07

Implement one shared reachability observer with API-based green/red status, separate auth/service/throttle reasons, neutral initial/stale state, observation generations, startup/wake/network-change probes, and bounded retry probes while work awaits connectivity. Respect server budgets and the provisional CONNECT-03 timings.

### S05-T08

Publish reconnect/startup/dependency eligibility signals for the outbox. Separate Pause background refresh, Pause queued actions, notification quiet hours, and Work offline. Restore their persisted values before startup probes or dispatch. Scheduled reads keep their work/off-hours policy; submitted actions become eligible promptly when allowed. Prove signals with fixtures now and actual dispatch in Sprint 07.

### S05-T09

Resume bounded incremental catch-up from persisted resource-family progress, validators, overlapping update windows, partial pages, and notification baselines. Advance content and successful progress atomically. Use conditional reads and targeted reconciliation where delta support is absent or cursors expire; do not reset the cache, replay old notification baselines, or use a largest-number-only cursor. Record request traces for issue/comment families now; AC-75/76 close after PR-family integration in Sprint 06, and the full lifecycle closes in Sprint 12.

### S05-T10

Add deterministic unit/integration tests for schedules, timezone/DST, rate budgets, partial pages, stale/out-of-order connectivity evidence and wake/reconnect. Drive the implemented Windows settings/connectivity controls in a real process and verify persisted pause/offline flags and bounded request traces.

## Required tests and commands

- Unit/integration: controlled DST/timezone/work hours, rate limits, coalesced wake, checkpoint recovery and notification baselines.
- Native E2E: manipulate real offline/refresh controls and observe API-based connection evidence; prove stale responses and cancellation do not overwrite current state.
- Request traces: shared polling across panes, old-item updates and bounded reconnect work; actual future mutation replay remains for its feature sprint.

Implement and use the canonical runner from TESTING_PLAN.md: unit, integration, ui, native e2e smoke/full, install with an artifact, and separately guarded live tests. These are development commands to build, not commands already supplied by this ZIP. Record actual OS/build/fixture evidence and mark unavailable checks Blocked/Not run. Native test-build E2E and exact published-package smoke remain separate.

Scenario IDs introduced or closing here: UT-002, UT-005, UT-006, UT-007, IT-006, IT-007.

Live-provider cases are shared gates on an explicitly authorized host. A later platform can reference existing evidence only with a recorded applicability assessment; rerun changed provider/credential-policy or target-specific behavior. Missing authorization remains Blocked/Not run and never permits unapproved writes.

Use test_scenarios.csv for fixture IDs, expected results, first/closure ownership and evidence. Release sprints rerun all scenarios applicable to their target; earlier sprint scenario lists do not waive regression checks.

## Deliverables

Durable shared scheduler; schedule settings; refresh status; conditional/reconciled reads; baselines and event journal; controlled-clock and failure tests.

## Exit gates

- Pass AC-11, AC-12, and AC-54, including item 900 already cached while item 12 changes later.
- Show that two views share one in-flight fetch and schedule, closing one does not cancel another's work, and a busy pane cannot starve monitoring.
- Prove initial historical hydration records a baseline without inventing activity. Store a genuinely new comment on an unopened issue as an observation without fetching the full discussion.
- Demonstrate pause, DST/timezone change, sleep/wake, reconnect, throttling, and repository removal. Pop-up delivery acceptance remains open until Sprint 11.
- Pass AC-57 including verified API errors versus transport failure, stale evidence, cancelled requests, out-of-order results, and single-observer behavior across panes. This does not yet close actual outbox replay.

## Initial acceptance scenarios owned here

- **AC-11** Work-hour changes, timezone changes, sleep, wake, and offline recovery produce the intended scheduling behavior without a burst of overdue requests.
- **AC-12** A rate-limit response pauses appropriate work while cached navigation and draft editing remain usable.
- **AC-54** With item 900 cached, a later comment or state change on item 12 is still found through the applicable update path. Gaps in repository numbers do not cause integer-by-integer probing, false deletions, or a false claim that all history is downloaded.
- **AC-57** The global indicator shows green for current verified GitHub responses and red after current transport failure or Work offline. Startup/stale evidence uses a neutral state. 401/403/429/5xx show separate auth/throttle/service status rather than falsely claiming no Internet. A late response, cancelled view request, multiple panes, sleep/wake, and network change do not produce false status or probe storms.

## Full owned requirements

### SYNC-01

Users MUST configure workdays, work windows, timezone, work-hours interval, off-hours interval, and pause behavior. Support daylight-saving changes through timezone-aware scheduling. Default to the operating system timezone through the platform adapter and allow an explicit override.

Initial verification owner: Sprint 05. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### SYNC-02

Use these provisional defaults: Monday through Friday, 09:00 to 17:00; work-hours checks every 120 seconds; off-hours checks every 30 minutes; stale items in the selected tab of each visible pane eligible for a 60-second check. Expose these values in settings. Treat them as requested intervals subject to API constraints and actual workload. Multiple views of one item share its refresh schedule.

Initial verification owner: Sprint 05. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### SYNC-03

Give priority to explicit user actions and their eligible submitted outbox work, then the selected item in each visible pane, enabled activity monitoring, recently used or pinned content, and slower reconciliation. Apply a shared budget and fair scheduling so a busy pane or outbox cannot starve other work. Fetch older history on demand. Do not launch a mandatory full historical backfill. Keep dependency order and mutation pacing even when draining the outbox.

Initial verification owner: Sprint 09. Read scheduling and lifecycle in 05; prove eligible outbox dispatch outside read schedules and on restart across issue/review/merge actions in 07-09.

### SYNC-04

A user-triggered open or refresh may run outside work hours. Explicitly submitted outbox actions also resume outside work hours when eligible; quiet hours control notification delivery and read-refresh schedules control background reads, not authorization to send queued actions. Provide a separately labeled Pause queued actions control, and distinguish it from Pause background refresh and Work offline. Persist these settings and restore them before any startup network work. Work offline blocks all networking until disabled; Pause queued actions blocks new mutation dispatch, and Pause background refresh blocks scheduled catch-up reads. After sleep or reconnection, coalesce overdue read jobs into a bounded catch-up pass and resume eligible actions without replaying every missed timer tick or sending a mutation burst. Required outbox preflight/reconciliation reads remain governed by the action and connectivity controls, not the background-read schedule.

Initial verification owner: Sprint 09. Read scheduling and lifecycle in 05; prove eligible outbox dispatch outside read schedules and on restart across issue/review/merge actions in 07-09.

### SYNC-05

Centralize pagination, cancellation, request deduplication, retry limits, exponential backoff, and server budget handling. Honor relevant `Retry-After`, rate-limit, and polling headers. Authenticated conditional requests that return 304 can preserve the primary REST quota, but do not make polling unlimited. [S11] [S12]

Initial verification owner: Sprint 06. General read scheduler and item/comment observers in 05; PR-family pagination and review/check reconciliation in 06.

### SYNC-06

Maintain independent synchronization progress for resource families. Use repository-level updated-comment feeds where available, but do not assume the parent issue or PR timestamp is a complete change feed for every child object. Use overlapping update windows and periodic reconciliation where required. [S06] [S13]

Initial verification owner: Sprint 06. General read scheduler and item/comment observers in 05; PR-family pagination and review/check reconciliation in 06.

### SYNC-07

Include all states when checking for relevant updates. A new comment on an old closed issue or merged PR can be relevant. Do not permanently freeze previously downloaded historical items merely because they are closed.

Initial verification owner: Sprint 06. General read scheduler and item/comment observers in 05; PR-family pagination and review/check reconciliation in 06.

### SYNC-08

Expose last successful synchronization, pending work, throttling, authentication problems, and resumable failures. Background failures must not replace usable cached screens with blocking error pages.

Initial verification owner: Sprint 05. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### SYNC-09

Run synchronization, connectivity checks, and the outbox worker in the application or optional tray process. A fully exited application or sleeping machine cannot perform local polling or send actions. Normal exit preserves local state and the protected PAT; it does not sign the user out. On launch or wake, restore the local workspace and persisted control settings first, then resume eligible actions and bounded incremental read catch-up under SYNC-11 when connectivity and authorization permit. No hosted service is required for the initial release.

Initial verification owner: Sprint 12. Lifecycle scheduling begins in 05, all writers integrate by 09, graph/feed families by 11, and retained-credential plus local-first incremental restart acceptance closes in 12.

### SYNC-10

The highest downloaded issue or PR number MUST NOT be the sole synchronization cursor. Updates to old items and their comments, reviews, or state must still be discovered. Use supported update timestamps or cursors, overlapping windows, pagination, and resource-family reconciliation. Enumerate through supported list APIs instead of requesting every integer in a number range. Missing numbers do not establish deletion or complete cache coverage. [S28]

Initial verification owner: Sprint 05. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### SYNC-11

Resume from persisted per-account, per-repository, per-resource-family synchronization state on restart, reconnection, and same-user PAT renewal. Retain the last successful checkpoint, overlapping update window or supported cursor, pagination progress, conditional-request validators, resource completeness/freshness, and notification baselines. Advance content and its successful progress transactionally after each completed unit; failures or partial pages must not skip unseen changes. Prioritize visible resources and run bounded, rate-aware incremental catch-up, using supported changed-since/update feeds, conditional reads, and targeted reconciliation. Do not blindly redownload the entire cache, re-enumerate every historical discussion, reset notification baselines, or use the largest issue/PR number as the only cursor. If a cursor expires or an endpoint has no reliable delta interface, re-enumerate the necessary resource/range with stable-ID deduplication and preserved coverage while keeping other completed families intact. An update-feed page one, overlap, unchanged revalidation, or targeted repair may be necessary; no universal exact-delta or zero-unchanged-bytes guarantee is made. Preserve old-item update discovery and honor restored sign-out, offline, and pause state. [S06] [S07] [S11] [S13]

Initial verification owner: Sprint 12. Persist checkpoints and stable account identity in 02/03; issue/comment restart catch-up in 05 and PR-family AC-75/76 in 06; all graph/notification families and full lifecycle request traces complete in 12.

### WORKSPACE-10

Share request scheduling and deduplicate in-flight fetches across views of the same resource and revision. Opening additional panes MUST NOT create independent polling loops or duplicate alerts for that resource. Give visible tabs priority within the existing schedule and rate-limit budget. Inactive tabs must not retain every large diff or continuously redraw; evict reconstructible rendering state while preserving navigation state and drafts. Closing one view must not cancel work still needed by another view or the shared write journal.

Initial verification owner: Sprint 12. Shared scheduling in 05 and large-view retention in 06/10; complete integrated twenty-tab performance gate in 12.

### NOTIFY-02

Activity monitoring MUST run independently of demand-loaded full content. Enabled events must be detectable on selected repositories even for issues and PRs the user has never opened. Monitoring must not require a complete historical download.

Initial verification owner: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-13

Establish a baseline for each repository and event category when monitoring begins. Downloading old content for the first time MUST NOT itself create a new-activity notification. A genuinely new comment on an old unopened issue may still qualify based on its trustworthy event identity and timestamp.

Initial verification owner: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-14

Where no trustworthy earlier state or event time exists, establish state without inventing a transition. Deduplicate events across polling endpoints and multiple notification categories. A comment that mentions the user should not produce duplicate comment and mention banners by default.

Initial verification owner: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-15

Persist event keys, eligibility decisions, grouping keys, and delivery state. Reuse stable native notification identities where supported and report unsupported replacement/retraction capabilities truthfully. Prevent ordinary duplicate delivery across retries and restart; do not claim an unconditional exactly-once guarantee across application crashes and OS delivery.

Initial verification owner: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### CONNECT-03

Maintain one shared connectivity observer. Reuse ordinary API traffic; issue a lightweight non-mutating probe at startup, wake, network-change hints, or Check now when permitted. Use a provisional 120-second freshness window, then show stale evidence instead of increasing idle background polling beyond the configured refresh schedule. While submitted work is waiting on connectivity and the app/tray is active, probe automatically with bounded backoff and jitter, provisionally 5, 15, 30, 60, then at most every 120 seconds. Respect shared request budgets and longer server Retry-After/rate-limit delays. A new reachable result wakes the dispatcher once; many panes or queued items must not each probe or drain independently. Work offline disables probes and sends until explicitly turned off.

Initial verification owner: Sprint 07. Probe/backoff and eligibility signal tests in 05; integrate actual waiting submissions and automatic dispatcher wake-up in 07.

## Integration work owned elsewhere

### FND-04

Use Tokio for asynchronous application work and reqwest for HTTP. Use typed request and response models, explicit cancellation, structured errors, and a centralized request scheduler. A different library is acceptable only with a recorded compatibility reason.

Initial owner: Sprint 01. Initial verification: Sprint 05. Background/provider contracts begin in 01; complete central scheduler in 05.

### FND-06

UI events MUST enqueue or invoke application operations without performing blocking work. Results MUST carry resource identity and revision. View-specific work must also retain a tab-instance identity and selection generation so a late response cannot replace another tab's content or a newer selection. Shared resource results may update the cache and notify all matching views without changing their selection or focus.

Initial owner: Sprint 04. Initial verification: Sprint 09. Carry view identity, resource revision, cache publication, and context preservation through all read/write workflows.

### FND-07

The cache is the normal read source. Successful reads and writes update persistent records, then notify subscribed views. Preserve scroll position, selected file, and unfinished input when background refresh completes.

Initial owner: Sprint 04. Initial verification: Sprint 09. Carry view identity, resource revision, cache publication, and context preservation through all read/write workflows.

### PORT-02

Share domain models, GitHub integration, cache/search, synchronization, Outbox, review/diff logic, history, notification rules, and workspace state across platforms. Isolate operating-system APIs behind small capability interfaces with conditional dependencies inside platform implementations. Common code must not depend on Windows handles, registry calls, COM types, fixed Windows paths, or platform-specific credential objects. Reuse portable GPUI facilities where suitable rather than wrapping the entire toolkit in a new framework. Test interface contracts with fakes and actual adapters; do not duplicate the product's business logic for each OS.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce platform boundaries in 01 and keep shared modules portable throughout; verify the full Windows implementation boundary in 12 and parity in later platform rows.

### PORT-06

Implement and test platform-appropriate keyboard shortcuts, text input, accessibility, clipboard, high-DPI/scaling, theme changes, timezone defaults, external-link opening, startup registration, close versus Quit, background residence, sleep/wake and network-change hints. Feed normalized lifecycle events into the shared scheduler. Restore offline/pause settings before probes or dispatch. If a supported environment lacks a tray/menu-bar capability, keep the app reachable and explain background behavior rather than hiding an inaccessible running instance. Closing/quit must preserve drafts, workspace, Outbox and credentials according to the shared lifecycle contract. [S33] [S35]

Initial owner: Sprint 01. Initial verification: Sprint 12. Keep native interaction/lifecycle boundaries explicit from 01; complete Windows accessibility, restart and installed behavior in 12, and repeat for later targets.

### AUTH-06

Provide a user-initiated action to open an appropriate GitHub PAT or SSO authorization page in the external default browser. Validate returned authorization URLs before opening them. Preserve the current screen, cached content, drafts, outbox, and synchronization checkpoints while authorization is repaired. After verifying that a replacement PAT belongs to the same GitHub user and restores the required access, reuse the existing account/resource identities and progress, retry eligible reads, and automatically resume eligible submitted actions under the persisted pause/offline settings. A different user must receive a separate account context; replacing a token must not create a new cache for the same user.

Initial owner: Sprint 03. Initial verification: Sprint 09. Authorization repair restores the same identity in 03; reconnect signals in 05 and automatic eligible outbox resumption across all writers in 07-09.

### AUTH-11

Explicit sign-out MUST durably mark the affected account profile as signed out, prevent new network dispatch under that profile, and remove or invalidate its locally stored PAT material. Preserve readable private cached content, drafts, saved workspace, settings, synchronization checkpoints, activity baselines, and outbox records on the PC. Users may reopen that signed-out local profile for offline work without a renewed login; show its signed-out status and hold its remote actions until a PAT for the same verified user restores access. Keep that hold across restart. Ordinary exit is not sign-out. Expired or revoked PATs likewise preserve offline use and progress while holding affected remote work. A sign-out during an in-flight mutation does not prove remote cancellation; retain its evidence for reconciliation after access is restored. Do not expose one profile's data through a different active account or assume sign-out revokes the PAT at GitHub.

Initial owner: Sprint 03. Initial verification: Sprint 12. Signed-out credential/dispatch holds begin in 03; readable local-profile UI in 04 and all-writer PAT repair in 09; complete private-data retention and in-flight lifecycle matrix in 12.

### BROWSE-01

Users MUST be able to select, remove, and revisit accessible repositories. Show organization, repository name, access state, last successful check, and cached storage usage. Removing a monitored repository stops its polling; deleting its cached data is a separate explicit action.

Initial owner: Sprint 03. Initial verification: Sprint 05. Repository selection/storage usage in 03-04; removal stops the running scheduler in 05.

### WORKSPACE-08

Save and restore the workspace per account, including pane order and proportions, open tab order, active tabs, focused pane, per-tab view state, scroll position or anchors, navigation context, and draft references. Normal exit MUST retain this state. On restart, render cached visible content before waiting for any API response or renewed login; then start eligible background work through the shared scheduler. Load inactive tabs lazily and expose missing or inaccessible content without discarding its navigation target. Restoring a workspace MUST NOT submit unsubmitted drafts, create duplicate operations, or expose another account's private tabs. The independent outbox worker resumes already submitted eligible actions under OUTBOX-04 and the restored authentication/pause settings; view restoration is not a dispatch trigger.

Initial owner: Sprint 04. Initial verification: Sprint 12. Restore local workspace/drafts before networking in 04; integrate all views and writers as built. Final normal-exit, signed-out, expired-PAT and delayed-network restoration closes in 12. View restoration never submits drafts.

### CACHE-05

Commit downloaded pages and their progress atomically. Retry and resume reads without duplicating records. Persist enough progress to resume interrupted pagination. If a cursor or page boundary becomes unreliable, safely re-enumerate the affected range using stable IDs and deduplication. [S07]

Initial owner: Sprint 04. Initial verification: Sprint 05. Atomic progress primitives in 02, resumable UI pagination in 04, scheduled read resumption in 05.

### CACHE-10

A 404, authentication failure, or temporary API error MUST NOT trigger bulk cache deletion. Record the reason when known, stop inappropriate retries, and reconcile after access is restored. Distinguish an unknown resource from a confirmed deletion.

Initial owner: Sprint 02. Initial verification: Sprint 05. Storage preservation in 02; authorization/provider handling in 03; live sync reconciliation in 05.

### CACHE-13

Normal window close and application exit MUST retain downloaded content and resource coverage, all draft types, saved tabs/layout/view positions, local preferences, offline pins, pending actions and receipts, synchronization checkpoints/validators, and notification baselines/delivery history on the PC. Persist settings and user changes as they occur and safely flush remaining local state on orderly exit; if a user-data write fails, keep it recoverable and report the failure. Retain the protected credential according to AUTH-07. Restart MUST reuse these records and render the saved local workspace before networking completes. It must not clear the cache, reset baselines, or redownload all history merely because the process was restarted. Sign-out preserves local content under AUTH-11. Explicit cache cleanup remains a separate action and continues to protect drafts and unresolved work under CACHE-07 and OUTBOX-10.

Initial owner: Sprint 02. Initial verification: Sprint 12. Complete persistence schema in 02, PAT lifecycle in 03, local-first restoration in 04, sync in 05/06, all writer/history/notification types through 11, and the complete quit/sign-out/reopen matrix in 12.

### OFFLINE-01

Once an account and content have been established, the application MUST start and remain usable without a network check or renewed login blocking local access. Users can browse/search downloaded issues, PRs, discussions, reviews, commits, diffs, graph history, and the activity feed; use tabs and panes; edit local settings and drafts; and inspect or cancel pending work. State the cached coverage and last fetch honestly. Unseen remote content and new server events cannot be available until fetched. First-time authentication and initial acquisition of private content still require access; do not require a full repository download.

Initial owner: Sprint 04. Initial verification: Sprint 12. Offline startup/workspace in 04; each new view and action integrates as built; complete all-feature cold-start offline matrix in 12.

### OFFLINE-03

Provide Download for offline use for a selected item, with the resource scope and revision shown before downloading. Include its accessible discussion and, for a selected PR revision, review/thread data, commit/file metadata and available patches; let users choose optional media or additional source context. Track progress and per-resource completion, expose caps or unavailable content, and resume interrupted downloads. An offline request to fetch missing data becomes a visible, cancellable pending download that starts when eligible. Download jobs are distinct from remote mutation actions. Never present partially fetched data as fully available or start a mandatory full-repository archive.

Initial owner: Sprint 04. Initial verification: Sprint 06. Issue downloads and coverage in 04, scheduled resumption in 05, full PR resource scope and AC-70 in 06.

### CONNECT-01

Show a persistent, prominent green/red connection indicator in the application chrome, visible across panes, with an accessible icon and text label; color alone is insufficient. Green means recently verified GitHub API reachability. Red means Offline or Cannot reach GitHub after a current transport failure or an explicit Work offline choice. Use a neutral Checking or Status stale state before an initial result and when evidence is too old, rather than asserting a connection indefinitely. Show last checked time, last successful contact, reason, and Check now in its details, plus a pending-action count linking to the Outbox.

Initial owner: Sprint 04. Initial verification: Sprint 05. Provider status contract in 03, persistent accessible chrome in 04, real shared observer and reachability classification in 05.

### CONNECT-02

Derive reachability from verified HTTPS responses from the configured GitHub API and current transport errors, not from cached data or a network adapter icon alone. Successful responses, including 304, establish contact. A genuine API 401, authorization 403/404, 429, or 5xx also demonstrates a reachable API endpoint, while separate status explains Sign-in required, SSO approval, Throttled, or Service error and blocks affected writes as appropriate. DNS, connection, TLS, and timeout failures indicate inability to reach the service securely; they do not prove that every Internet destination is down. Request cancellation caused by tab navigation is not an offline signal. Suppress older/out-of-order observations so they cannot overwrite newer connectivity evidence; scope authorization errors separately by account/repository.

Initial owner: Sprint 03. Initial verification: Sprint 05. Typed provider evidence in 03; UI consumes it in 04 and full stale/out-of-order/transport/error cases close in 05.

### NOTIFY-16

Distinguish comment creation from editing. Record the user's successful writes so a subsequent poll does not echo them as unsolicited new activity when own-action suppression is enabled.

Initial owner: Sprint 07. Initial verification: Sprint 11. Creation/edit distinction in 05; own successful issue/comment/review/merge writes recorded in 07-09; suppression verified in 11.

### OUTBOX-04

Automatically validate and dispatch eligible submitted actions after reconnection, application restart/wake, dependency completion, authorization repair under the same user, or expiry of a retry delay. Apply bounded exponential backoff with jitter, mutation pacing, shared API budgets, and relevant server headers. Off-hours read schedules and notification quiet hours do not delay already submitted actions. Respect Pause queued actions and Work offline. Definitely unapplied temporary failures remain queued; unrecoverable or content conflicts require attention; uncertain mutation outcomes follow OUTBOX-05. Only the running application/tray can dispatch. [S11] [S12]

Initial owner: Sprint 07. Initial verification: Sprint 09. Scheduler signals and pause semantics in 05; automatic eligible dispatch and pacing for each writer in 07-09.

### OUTBOX-05

Prevent duplicate local submission with stable operation IDs, transactional claims, and per-attempt evidence; do not assume this creates server idempotency. For a connection loss, timeout, crash, or ambiguous server error after dispatch, preserve Outcome unknown and reconcile before another mutation. Automatically retry only if reliable evidence shows no remote effect, or verified endpoint idempotency makes it safe. Persist known remote object/operation IDs and confirm through server state where possible. Matching body text or a brief absence from a list is not proof. If still unresolved, retain the action for attention rather than silently replaying or marking success. No unconditional exactly-once delivery guarantee is made.

Initial owner: Sprint 07. Initial verification: Sprint 09. Storage/dispatcher contracts precede actual issue/comment replay in 07; finish uncertain-outcome, cancellation and current-intent validation for reviews and merges by 09.

### OUTBOX-07

Cancellation of an unsent queued or validating action MUST durably win against dispatch through an atomic state transition checked immediately before the mutation is sent. If cancellation wins, no request for that operation may later be sent, including after restart. If dispatch has already won, show Cancellation requested/May already be sent, stop further mutation attempts, and continue permitted read-only reconciliation. Cancelling an HTTP task is not proof GitHub did nothing. Do not claim remote rollback: a confirmed comment remains a comment and a remotely completed merge remains merged. GitHub merge-queue withdrawal is outside first-release scope. Keep original text, dispatch evidence, and dependent actions recoverable even if sign-out delays reconciliation.

Initial owner: Sprint 07. Initial verification: Sprint 09. Storage/dispatcher contracts precede actual issue/comment replay in 07; finish uncertain-outcome, cancellation and current-intent validation for reviews and merges by 09.

### OUTBOX-08

Freshly validate the original target, permission, edit base values, review anchors, intended commit, merge method, and applicable policy immediately before mutation dispatch. Automatically execute unchanged valid intent without routine reconfirmation. A conflicting remote edit, changed reviewed/merge head, missing target, or changed permission moves the action to Needs attention. Never silently rebase a queued edit, re-anchor a review, approve a new revision, substitute a merge head, or change the chosen merge method. Show the current-versus-intended context so the user can correct or cancel it. Live server enforcement remains authoritative. [S09] [S10]

Initial owner: Sprint 07. Initial verification: Sprint 09. Storage/dispatcher contracts precede actual issue/comment replay in 07; finish uncertain-outcome, cancellation and current-intent validation for reviews and merges by 09.

### DATA-02

Use UTC for persisted event timestamps and timezone-aware local scheduling. Use monotonic timing for in-process delays. Do not use a local wall-clock jump as evidence that remote content changed.

Initial owner: Sprint 02. Initial verification: Sprint 05. UTC/monotonic conventions in 02; timezone/DST scheduling exercised in 05.

### UX-02

Preserve user context while refreshing. Avoid replacing a useful cached view with a global spinner, jumping scroll positions, or losing a draft when selecting another tab. Loading and errors belong to the affected resource or pane; a slow request in one pane must not block navigation or editing in another.

Initial owner: Sprint 04. Initial verification: Sprint 10. Context-preserving loading/error behavior must hold through all reader, writer, diff and history views.

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

FND-04, BROWSE-01, CACHE-05, CACHE-10, SYNC-01, SYNC-02, SYNC-08, SYNC-10, CONNECT-01, CONNECT-02, DATA-02.

Use requirement_platform_traceability.csv for applicability and release-gate evidence. Initial ledger statuses refer only to their initial phase; no single Windows result closes all platforms. Keep future options and unavailable services explicit.

## Later scope and handoff

Sprint 06 connects formal review/request/thread/check observers; Sprint 10 connects branch observers. Full event rule decisions and native delivery close NOTIFY requirements in Sprint 11. The twenty-tab integrated performance gate closes in Sprint 12.

## Coding LLM instruction

Implement Sprint 05 against v1.7. Keep the app buildable and preserve the current product constraints. Complete listed implementation and test work, use actual native controls for E2E, protect original targets/drafts/Outbox state, and add only the named platform integrations. Do not add issue/PR creation, merge queues or browser engines. Keep secrets out of code/results; live writes require explicit allowlisted authorization. Implement the short verified prebuilt install/update contract for the relevant publication phase and never claim placeholder commands runnable. Update initial/per-platform/scenario ledgers with commands, environment, package identity and evidence, then use SPRINT_REPORT_TEMPLATE.md. Continue only within the assigned sprint/Windows/all-platform scope without new routine approvals.

## Technical sources

- [S06 GitHub issue and PR conversation comments](https://docs.github.com/en/rest/issues/comments)
- [S07 GitHub REST pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api)
- [S09 GitHub pull request reviews](https://docs.github.com/en/rest/pulls/reviews)
- [S10 GitHub pull request reads and merges](https://docs.github.com/en/rest/pulls/pulls)
- [S11 GitHub REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)
- [S12 GitHub REST rate limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)
- [S13 GitHub inline review comments](https://docs.github.com/en/rest/pulls/comments)
- [S28 GitHub issue representations identifiers and updates](https://docs.github.com/en/rest/issues/issues)
- [S32 GPUI Kit testing facilities and limits](https://gpui-kit.com/docs/test/)
- [S33 GPUI Kit accessibility](https://gpui-kit.com/docs/accessibility/)
- [S35 GPUI Kit packaging](https://gpui-kit.com/docs/packaging/)

Additional current primary sources and platform/install/test constraints are in the three companion plans. Verify pinned tool/API behavior during implementation.

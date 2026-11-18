# Sprint 02: Shared data model, SQLite cache, drafts, and outbox

Requirements version 1.7 | Initial sprint status Planned

## Outcome

A tested local domain and database can retain remote content and user drafts without losing identity or confusing partial data with complete data.

## Phase and prerequisites

- Release phase: Windows first release.
- Prerequisites: Sprint 01.
- Code areas: domain, storage, queries/FTS schema, review identity types, workspace persistence, tests/fixtures.
- Read the master, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and all relevant scenario/platform ledgers.
- Use actual prior evidence; a missing dependency remains blocked rather than passed. Later-platform release work does not silently move into the Windows phase.

## Implementation tickets

### S02-T01

Implement SQLite migrations, foreign keys, indexes, WAL, short transactions, bounded connections, and FTS5 foundations. Add account-scoped repositories, common items, one-to-one PR details, and distinct typed issue/PR provider IDs.

### S02-T02

Enforce UNIQUE(repository_id, number), excluding kind. Implement transactional upserts for issue-first, PR-first, reversed, and concurrent arrival; preserve known merged state, absent-versus-null distinctions, and newer fields. Keep stable local item IDs through verified transfers and aliases.

### S02-T03

Persist resource-family completeness, freshness, validators, pagination progress, immutable comparison identities, commit parents, mutable refs, and revision-bound state. Establish storage contracts for sync jobs, activity baselines, event keys, delivery state, and write outcomes.

### S02-T04

Persist account-scoped workspace layouts and drafts independently of tabs and cache rows. Existing-issue edits and comment/review drafts retain their established target identity. Provide shared editor-ownership and operation-identity primitives; do not add a new-issue draft type. Persist per-PR context mode, selected verified issue, body-reference revision/provenance and layout fields for Sprint 06 integration.

### S02-T05

Implement draft-safe cleanup, repair and migration behavior, schema incompatibility handling, UTC timestamp conventions, diagnostic redaction boundaries, and safe handling of remote paths and content.

### S02-T06

Create small deterministic fixtures and a reproducible large-data generator for the Section 16 workload, including 1,000 mixed outbox entries. Test migration upgrades, account isolation, corruption/error handling, and draft/outbox survival during cache cleanup.

### S02-T07

Implement durable submitted-operation identities and immutable payload snapshots separate from editable drafts. Atomically persist enqueue, local pending projection, original identity/target/base revision, dependency IDs, per-target ordering, attempts, claims, retry times, cancellation decisions, and remote step receipts.

### S02-T08

Add crash-recovery and atomic cancel-versus-dispatch storage primitives, confirmed comment/review/thread step identity mapping, and payload schema migration. Protect required base snapshots and review anchors; a failed/full-disk enqueue must never return Queued success.

### S02-T09

Persist the complete normal-exit session contract: cache and coverage, workspace/view state, drafts, preferences and pause/offline settings, outbox evidence, per-family progress/validators, and notification baselines. Keep account identity independent of PAT values. Establish durable signed-out holds without deleting readable profile content; complete UI and writer integration in later sprints.

### S02-T10

Test actual temporary SQLite/WAL/FTS databases, migrations and disk/crash failures across configured core runners. Define stable per-user path and safe backup/migration boundaries for installed upgrades; do not place durable drafts or Outbox state in disposable cache. Deliver the storage subset of AC-90 now, with the real HTTP adapter portion closing in Sprint 03.

## Required tests and commands

- Unit/integration: exercise actual temporary SQLite migrations, uniqueness/upserts, partial coverage, FTS, draft/Outbox durability, claims and cancel races, including disk/transaction failure.
- Native smoke: verify the build still launches/quits with an isolated profile; storage fixtures are not user-workflow E2E. Run available storage suites on all core targets.
- Upgrade foundation: test valid SQLite backup/recovery and schema compatibility using synthetic prior databases, preserving user-authored state.

Implement and use the canonical runner from TESTING_PLAN.md: unit, integration, ui, native e2e smoke/full, install with an artifact, and separately guarded live tests. These are development commands to build, not commands already supplied by this ZIP. Record actual OS/build/fixture evidence and mark unavailable checks Blocked/Not run. Native test-build E2E and exact published-package smoke remain separate.

Scenario IDs introduced or closing here: UT-001, UT-003, UT-004, UT-009, IT-001, IT-002, IT-019, PKG-003.

Live-provider cases are shared gates on an explicitly authorized host. A later platform can reference existing evidence only with a recorded applicability assessment; rerun changed provider/credential-policy or target-specific behavior. Missing authorization remains Blocked/Not run and never permits unapproved writes.

Use test_scenarios.csv for fixture IDs, expected results, first/closure ownership and evidence. Release sprints rerun all scenarios applicable to their target; earlier sprint scenario lists do not waive regression checks.

## Deliverables

Migrations and repositories; typed domain models; transactional ingestion; FTS foundations; separate drafts and durable outbox; dependency/claim/cancellation contracts; fixture generator; storage and migration tests.

## Exit gates

- Pass AC-50, AC-51, and AC-55 with database fixtures, including concurrent ingestion and an unknown issue-endpoint ID on a PR-first load.
- Prove cleanup cannot cascade into user drafts, including existing-issue edit base snapshots, comment drafts, and review drafts with source anchors.
- Demonstrate that missing, partial, complete, stale, unavailable, and failed resources survive restart distinctly. Authorization failures preserve cached content.

## Initial acceptance scenarios owned here

- **AC-50** Issue 41, PR 42, and issue 43 in one repository occupy three common item rows. A second representation of PR 42 updates the same row. Another repository's issue 42 remains a separate item, and the uniqueness constraint cannot be bypassed by changing kind.
- **AC-51** Fetching a PR through the issue endpoint and then the pull-request endpoint, in the reverse order, or concurrently produces one item and at most one PR extension with correctly distinguished API IDs. PR-first loading works while the issue-endpoint ID is unknown. Partial or older payloads do not downgrade PR kind, overwrite newer fields, clear known PR details, or mark unrelated resources complete.
- **AC-55** A fixture representing a confirmed issue transfer preserves the local item identity, drafts, and tab references while updating its current repository locator. An unrelated destination item with the same number is not merged into it; unresolved identity remains explicit.

## Full owned requirements

### FND-03

Use SQLite with migrations, foreign keys, suitable indexes, and bounded connections. WAL allows reads during writes; FTS5 supports local full-text search. Keep transactions short and perform database work outside the UI thread. [S04] [S05]

Initial verification owner: Sprint 02. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### DATA-01

Apply migrations transactionally where supported and test upgrading representative databases. Back up user-authored drafts and unresolved outbox work before risky transformations. Detect corruption or incompatible schema versions without silently resetting user work. Preserve operation identity, cancellation decisions, and reconciliation evidence across migration and recovery.

Initial verification owner: Sprint 12. Migration and recovery mechanisms in 02; validate representative upgrade databases after all draft types and release schema exist.

### DATA-02

Use UTC for persisted event timestamps and timezone-aware local scheduling. Use monotonic timing for in-process delays. Do not use a local wall-clock jump as evidence that remote content changed.

Initial verification owner: Sprint 05. UTC/monotonic conventions in 02; timezone/DST scheduling exercised in 05.

### DATA-03

Keep secrets and private content out of routine diagnostic output. Export diagnostics only through an explicit user action with appropriate redaction. Treat remote Markdown, links, and filenames as data rather than instructions or executable paths.

Initial verification owner: Sprint 12. Establish redaction/content safety boundaries early; verify all parsers and final explicit diagnostic export in 12.

### DATA-05

Store the visible GitHub number separately from the local primary key and API object IDs. GitHub issues and PRs use the same increasing number sequence within a repository: an issue numbered 41 can be followed by PR 42 and issue 43. There cannot be two distinct items consisting of issue 42 and PR 42 in that repository. Enforce UNIQUE(repository_id, number), without kind in that constraint. Here repository_id identifies a local repository record already scoped to host and account access context. The same number in a different repository or account context must not collide. [S27]

Initial verification owner: Sprint 04. Database identity contract in 02; live provider and browsing ingestion in 03/04.

### DATA-06

Use an items table for shared fields and a pull_request_details table whose item_id is both its primary key and a foreign key to items. Only confirmed PR items may have this extension. The item kind distinguishes an ordinary issue from a PR once classified. PR summaries may exist before PR details are fetched; a navigation placeholder may remain unclassified until metadata arrives. Track missing detail separately from known false or null values. Keep PR merge state and source references in the extension, and derive the displayed merged state from confirmed PR data. A common closed-state update must not erase known merge information.

Initial verification owner: Sprint 06. Storage fixtures first; live issue-shaped PR ingestion in 04 and PR endpoint/details integration in 06.

### DATA-07

Issue endpoints can return PRs, identified by their pull_request property. For that representation, the REST id is an issue ID; the pull-request endpoint supplies the separate PR ID. Store these in distinct typed fields or identity mappings and retain node IDs as opaque provider identifiers. If PR data arrives first, its issue-endpoint ID may remain unknown until fetched. Upsert both representations to the same item using the scoped repository number and verified provider identities. Resolve concurrent ingestion transactionally. Distinguish absent fields from explicit nulls and reject stale overwrites using applicable revision or freshness evidence. Never substitute one ID for the other, create a duplicate row for the second representation, downgrade a known PR because a partial payload omits its marker, or mark unrelated resource families complete. [S28]

Initial verification owner: Sprint 06. Storage fixtures first; live issue-shaped PR ingestion in 04 and PR endpoint/details integration in 06.

### DATA-08

Store ordinary issue and PR conversation comments in a shared conversation-comments model keyed to item_id. Formal reviews and inline review comments remain distinct resources with their own provider identity namespaces and source anchors. Share common labels, assignees, search, drafts, and workspace references through the item identity. Route each remote operation by its API capability; a shared table does not make PR review or merge operations valid for ordinary issues. [S06] [S09] [S10] [S13]

Initial verification owner: Sprint 09. Shared comments and typed resources in storage/reads; review/merge guards complete in 09.

### DATA-09

Use the stable local item_id for relationships, including drafts, tabs, comments, fetch state, and search records. The repository number is the current locator. Resolve a confirmed issue transfer or redirect through verified provider identity before changing that locator; preserve local relationships and the previous locator as an alias where appropriate. Never combine unrelated items merely because their numbers match, and never treat repository names or URLs as immutable database identity. If identity cannot be confirmed, preserve the existing record and surface the unresolved relocation. [S28]

Initial verification owner: Sprint 04. Fixture transfer in 02; provider resolution and workspace/search/draft references integrated in 03/04.

### DATA-10

Store derived PR-body issue references separately from confirmed item identity and server relationship facts. Key a candidate set by source PR item_id and body revision/hash, preserve raw/reference position and provenance, and retain verification state and the verified target issue item_id when available. Recompute when the source body changes; retain unchanged verified targets and selection where still valid, but never associate a prior PR's or prior body's removed target with the current context. Cache reference evidence for offline use with its last validation, without marking current remote access or auto-closing behavior confirmed. Use the existing account/repository identity and transfer rules; do not create a new ordinary issue record or change a known PR kind merely to satisfy a parsed #number. Drafts and outbox actions keep their original stable targets even if a reference disappears. [S27] [S28] [S29] [S30]

Initial verification owner: Sprint 06. Reference/provenance and view-state schema in 02; local state foundations in 04; real PR-body parsing, verified issue identities, changed-body reconciliation and offline context in 06.

### CACHE-03

Persist completeness separately from freshness. Resource state must distinguish missing, downloading, partial, complete, unavailable, and failed. Freshness records include last successful fetch, last validation, validators, and applicable revision. A complete but stale resource remains readable.

Initial verification owner: Sprint 02. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### CACHE-07

Existing-issue edit drafts, comment drafts, review drafts, and submitted outbox payloads are user data. Cache clearing, repair, failed downloads, migrations, reconciliation, and closing tabs or panes MUST preserve them unless the user explicitly deletes the drafts or cancels the actions. Drafts and outbox entries exist independently of any viewing tab. Preserve source anchors, edit base values, dependency mappings, and the minimal referenced content needed by unresolved actions; apply OUTBOX-10 to cleanup and recovery.

Initial verification owner: Sprint 09. Draft/outbox-safe storage in 02; recheck cleanup and recovery for every draft and queued-action type through 09; repeat final migration matrix in 12.

### CACHE-08

Namespace records by host and account access context, with stable repository and resource identities. Issues and PRs share one repository-local number namespace. Map their API representations to a common local item while preserving distinct issue and pull-request API object IDs. Follow DATA-05 through DATA-09 for keys and ingestion. Never expose another account's private cached content after switching identities. [S27] [S28]

Initial verification owner: Sprint 04. Storage scoping in 02, real identity switching in 03, cached browsing isolation in 04.

### CACHE-09

Associate comparison data with immutable commit identities and comparison options. Associate mutable branch labels, status checks, and review state with their relevant revisions and timestamps. A branch name alone is not an adequate diff cache key.

Initial verification owner: Sprint 10. Typed immutable comparison identity in 02; actual PR/review and repository comparison integration later.

### CACHE-10

A 404, authentication failure, or temporary API error MUST NOT trigger bulk cache deletion. Record the reason when known, stop inappropriate retries, and reconcile after access is restored. Distinguish an unknown resource from a confirmed deletion.

Initial verification owner: Sprint 05. Storage preservation in 02; authorization/provider handling in 03; live sync reconciliation in 05.

### CACHE-13

Normal window close and application exit MUST retain downloaded content and resource coverage, all draft types, saved tabs/layout/view positions, local preferences, offline pins, pending actions and receipts, synchronization checkpoints/validators, and notification baselines/delivery history on the PC. Persist settings and user changes as they occur and safely flush remaining local state on orderly exit; if a user-data write fails, keep it recoverable and report the failure. Retain the protected credential according to AUTH-07. Restart MUST reuse these records and render the saved local workspace before networking completes. It must not clear the cache, reset baselines, or redownload all history merely because the process was restarted. Sign-out preserves local content under AUTH-11. Explicit cache cleanup remains a separate action and continues to protect drafts and unresolved work under CACHE-07 and OUTBOX-10.

Initial verification owner: Sprint 12. Complete persistence schema in 02, PAT lifecycle in 03, local-first restoration in 04, sync in 05/06, all writer/history/notification types through 11, and the complete quit/sign-out/reopen matrix in 12.

### OUTBOX-01

Route every explicit remote mutation through a durable SQLite outbox before sending, online as well as offline. Commit a stable operation ID, immutable submitted payload/version, original account and target identities, base values or commit revision, dependency references, draft snapshot, and locally pending projection atomically. Acknowledging Queued requires a successful durable commit; if storage is unavailable or full, keep the editor recoverable and clearly report that the action was not queued. Draft autosave does not authorize dispatch. The explicit action submission is authorization for its automatic later execution, without a routine second confirmation on reconnect.

Initial verification owner: Sprint 09. Durable schema and operation/comment/review step identities in 02; offline UI foundation in 04; full lifecycle, replay, dependencies and step receipts across writers in 07-09.

### OUTBOX-02

Use a recoverable operation lifecycle: Queued (including waiting for connection, dependency, pause or retry time), Validating (fresh read/preflight), Sending (a mutation may reach GitHub), Outcome unknown (automatic reconciliation), Waiting for GitHub (accepted asynchronous work), Needs attention (blocked by auth, conflict, unsupported capability or permanent validation failure), Confirmed, and Cancelled. Store a machine-readable reason independently of the state. Unsubmitted Draft is not an outbox entry. Confirmed and Cancelled are terminal only when justified by evidence; an ambiguous or remotely pending action remains inspectable. Exhausted transient retries wait in Queued and resume automatically when eligible.

Initial verification owner: Sprint 09. Durable schema and operation/comment/review step identities in 02; offline UI foundation in 04; full lifecycle, replay, dependencies and step receipts across writers in 07-09.

### OUTBOX-06

Preserve dependency order for related supported operations while allowing unrelated eligible work to progress under the shared budget. Retain stable local operation/draft identities and record confirmed server identities for comment, review, and thread steps where a later supported operation requires them. Preserve review/thread sequencing and meaningful per-target edit order, including proven predecessor results in later preflight comparisons. Persist step identities and receipts for multi-request workflows; resume eligible unfinished steps rather than repeating completed steps. Show partially applied remote effects explicitly. When a predecessor is cancelled, rejected, or uncertain, hold dependent actions with a clear reason; never guess a remote target or drop descendants silently. Cancelling a parent must show its dependent impact, and users may inspect/cancel each remaining dependent. New-issue placeholders, local issue creation IDs, and create-issue dependency chains are not part of this contract.

Initial verification owner: Sprint 09. Durable schema and operation/comment/review step identities in 02; offline UI foundation in 04; full lifecycle, replay, dependencies and step receipts across writers in 07-09.

### OUTBOX-10

Treat unresolved outbox operations, payloads, cancellation flags, dependency mappings, base snapshots, anchors, and remote reconciliation evidence as durable user data. Protect them from cache eviction, repair resets, account sign-out, and migration. Recover interrupted claims conservatively after a crash; an interrupted Sending state becomes Outcome unknown unless evidence proves it was never dispatched. Reconcile a successful remote write whose local completion transaction failed. Pending actions have no silent age expiry. Bound and virtualize the outbox UI, redact routine diagnostics, and preserve essential local content until completion/cancellation and any dependent work are settled.

Initial verification owner: Sprint 12. Persist/protect queue data in 02, integrate all writer snapshots/receipts in 07-09, then verify full upgrade/cleanup/crash/backlog matrix in 12.

### TEST-03

Integration tests MUST exercise real SQLite databases/migrations/FTS/transactions and actual provider serialization/HTTP paths against a controllable local API server, not only mocked repository methods. Cover pagination/resumption, SSO/permissions/partial data, rate limits, cache publication, persisted checkpoints, signed-out identity holds, Outbox claiming/reconciliation, and upgrade cleanup protection. Verify production interfaces with deterministic simulated remote responses and OS adapter contract tests; use real protected-store/notification integration checks on the applicable native host. Keep live GitHub tests separate from deterministic integration suites.

Initial verification owner: Sprint 12. Real SQLite integration starts in 02 and actual HTTP adapter integration in 03; later modules add cases, with full Windows integration release evidence in 12.

### INSTALL-04

Stage and verify an update before replacing the installed binary. Handle a running instance and file locks safely, prevent simultaneous migration/Outbox dispatch during replacement, and preserve SQLite data, drafts, tabs/layout, settings, offline pins, credential references/protected tokens, sync checkpoints, baselines, and pending-action identities/receipts. Back up user data before risky migrations, and recover from interrupted install/update without falsely confirming actions or losing local work. Roll back binaries only when schema compatibility is safe; do not blindly restore an old Outbox snapshot that could replay already applied mutations. Explain recovery if migration cannot safely continue.

Initial verification owner: Sprint 12. Storage backup/migration and durable operation invariants begin in 02; real all-state installed upgrades close in 12, then repeat per later platform.

## Integration work owned elsewhere

### PORT-02

Share domain models, GitHub integration, cache/search, synchronization, Outbox, review/diff logic, history, notification rules, and workspace state across platforms. Isolate operating-system APIs behind small capability interfaces with conditional dependencies inside platform implementations. Common code must not depend on Windows handles, registry calls, COM types, fixed Windows paths, or platform-specific credential objects. Reuse portable GPUI facilities where suitable rather than wrapping the entire toolkit in a new framework. Test interface contracts with fakes and actual adapters; do not duplicate the product's business logic for each OS.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce platform boundaries in 01 and keep shared modules portable throughout; verify the full Windows implementation boundary in 12 and parity in later platform rows.

### PORT-04

Implement protected credential and application-data adapters per published OS. Use Windows Credential Manager or equivalent on Windows, the platform keychain on macOS, and a documented supported secure credential service on Linux. Store only credential references in SQLite; never fall back silently to plaintext tokens when a store is unavailable or locked. Preserve offline cached access while affected network work is held. Use appropriate per-user data/config/cache locations, stable app identity and relative managed asset paths rather than Windows-specific absolute paths. Test filesystem permissions, file locking, SQLite WAL/FTS availability, path/Unicode handling, and stable location across upgrades. Credentials and caches are per-device; this does not add cross-device synchronization. [S36]

Initial owner: Sprint 03. Initial verification: Sprint 12. Path/storage contracts in 02 and real Windows credential tests in 03; installed path and retention behavior closes in 12, with OS adapters in 13/14.

### AUTH-11

Explicit sign-out MUST durably mark the affected account profile as signed out, prevent new network dispatch under that profile, and remove or invalidate its locally stored PAT material. Preserve readable private cached content, drafts, saved workspace, settings, synchronization checkpoints, activity baselines, and outbox records on the PC. Users may reopen that signed-out local profile for offline work without a renewed login; show its signed-out status and hold its remote actions until a PAT for the same verified user restores access. Keep that hold across restart. Ordinary exit is not sign-out. Expired or revoked PATs likewise preserve offline use and progress while holding affected remote work. A sign-out during an in-flight mutation does not prove remote cancellation; retain its evidence for reconciliation after access is restored. Do not expose one profile's data through a different active account or assume sign-out revokes the PAT at GitHub.

Initial owner: Sprint 03. Initial verification: Sprint 12. Signed-out credential/dispatch holds begin in 03; readable local-profile UI in 04 and all-writer PAT repair in 09; complete private-data retention and in-flight lifecycle matrix in 12.

### WORKSPACE-07

Multiple views of the same resource MUST share cached domain data, operation outcomes, and persistent draft identities. Reopening the same compose or review draft must reuse its draft ID. Use one active editor per draft by default; other views show the current draft preview and a Focus editor action. Do not allow independent unsynchronized copies to overwrite the same draft or submit the same pending operation twice. Separate intentional drafts remain separate records.

Initial owner: Sprint 07. Initial verification: Sprint 09. Draft ownership primitives in 02 and local editors in 04; integrate shared pending issue/comment, review and merge operations in 07-09.

### WORKSPACE-08

Save and restore the workspace per account, including pane order and proportions, open tab order, active tabs, focused pane, per-tab view state, scroll position or anchors, navigation context, and draft references. Normal exit MUST retain this state. On restart, render cached visible content before waiting for any API response or renewed login; then start eligible background work through the shared scheduler. Load inactive tabs lazily and expose missing or inaccessible content without discarding its navigation target. Restoring a workspace MUST NOT submit unsubmitted drafts, create duplicate operations, or expose another account's private tabs. The independent outbox worker resumes already submitted eligible actions under OUTBOX-04 and the restored authentication/pause settings; view restoration is not a dispatch trigger.

Initial owner: Sprint 04. Initial verification: Sprint 12. Restore local workspace/drafts before networking in 04; integrate all views and writers as built. Final normal-exit, signed-out, expired-PAT and delayed-network restoration closes in 12. View restoration never submits drafts.

### SEARCH-01

Provide fast local full-text search of downloaded titles, descriptions, and comments. Include available downloaded review text. Search results MUST identify the repository and item, highlight the match where practical, and navigate to the matching discussion or review location.

Initial owner: Sprint 04. Initial verification: Sprint 08. FTS schema in 02 and conversations in 04; formal review ingestion in 06 and confirmed submitted reviews in 08.

### CACHE-05

Commit downloaded pages and their progress atomically. Retry and resume reads without duplicating records. Persist enough progress to resume interrupted pagination. If a cursor or page boundary becomes unreliable, safely re-enumerate the affected range using stable IDs and deduplication. [S07]

Initial owner: Sprint 04. Initial verification: Sprint 05. Atomic progress primitives in 02, resumable UI pagination in 04, scheduled read resumption in 05.

### OFFLINE-01

Once an account and content have been established, the application MUST start and remain usable without a network check or renewed login blocking local access. Users can browse/search downloaded issues, PRs, discussions, reviews, commits, diffs, graph history, and the activity feed; use tabs and panes; edit local settings and drafts; and inspect or cancel pending work. State the cached coverage and last fetch honestly. Unseen remote content and new server events cannot be available until fetched. First-time authentication and initial acquisition of private content still require access; do not require a full repository download.

Initial owner: Sprint 04. Initial verification: Sprint 12. Offline startup/workspace in 04; each new view and action integrates as built; complete all-feature cold-start offline matrix in 12.

### OFFLINE-04

Show submitted local work immediately as clearly pending in all matching views: for example a pending comment, a review action, or an edit to an existing issue awaiting confirmation. Keep pending projections separate from confirmed cached server records; a refresh cannot erase them, and cancellation removes only the pending projection while preserving recoverable user text. A pending merge or approval is not a confirmed merge or approval. Offline pins and minimal draft/outbox dependencies must survive ordinary cache cleanup until explicitly released or the action is resolved.

Initial owner: Sprint 04. Initial verification: Sprint 09. Storage protection in 02, pending-view foundation in 04, actual issue/comment/review/merge projections and cancellation integration in 07-09.

### SYNC-09

Run synchronization, connectivity checks, and the outbox worker in the application or optional tray process. A fully exited application or sleeping machine cannot perform local polling or send actions. Normal exit preserves local state and the protected PAT; it does not sign the user out. On launch or wake, restore the local workspace and persisted control settings first, then resume eligible actions and bounded incremental read catch-up under SYNC-11 when connectivity and authorization permit. No hosted service is required for the initial release.

Initial owner: Sprint 05. Initial verification: Sprint 12. Lifecycle scheduling begins in 05, all writers integrate by 09, graph/feed families by 11, and retained-credential plus local-first incremental restart acceptance closes in 12.

### SYNC-11

Resume from persisted per-account, per-repository, per-resource-family synchronization state on restart, reconnection, and same-user PAT renewal. Retain the last successful checkpoint, overlapping update window or supported cursor, pagination progress, conditional-request validators, resource completeness/freshness, and notification baselines. Advance content and its successful progress transactionally after each completed unit; failures or partial pages must not skip unseen changes. Prioritize visible resources and run bounded, rate-aware incremental catch-up, using supported changed-since/update feeds, conditional reads, and targeted reconciliation. Do not blindly redownload the entire cache, re-enumerate every historical discussion, reset notification baselines, or use the largest issue/PR number as the only cursor. If a cursor expires or an endpoint has no reliable delta interface, re-enumerate the necessary resource/range with stable-ID deduplication and preserved coverage while keeping other completed families intact. An update-feed page one, overlap, unchanged revalidation, or targeted repair may be necessary; no universal exact-delta or zero-unchanged-bytes guarantee is made. Preserve old-item update discovery and honor restored sign-out, offline, and pause state. [S06] [S07] [S11] [S13]

Initial owner: Sprint 05. Initial verification: Sprint 12. Persist checkpoints and stable account identity in 02/03; issue/comment restart catch-up in 05 and PR-family AC-75/76 in 06; all graph/notification families and full lifecycle request traces complete in 12.

### DIFF-03

Distinguish PR comparison, individual-commit comparison, and arbitrary two-commit comparison. The default PR view must match the intended GitHub comparison. GitHub PRs use the merge base for a three-dot comparison; endpoint comparisons can differ. Label the comparison and use the correct source identities in the cache. [S16]

Initial owner: Sprint 10. Initial verification: Sprint 10. Typed comparison model in 02; correct PR comparison in 06; individual/two-commit comparisons complete in 10.

### NOTIFY-02

Activity monitoring MUST run independently of demand-loaded full content. Enabled events must be detectable on selected repositories even for issues and PRs the user has never opened. Monitoring must not require a complete historical download.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-13

Establish a baseline for each repository and event category when monitoring begins. Downloading old content for the first time MUST NOT itself create a new-activity notification. A genuinely new comment on an old unopened issue may still qualify based on its trustworthy event identity and timestamp.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-14

Where no trustworthy earlier state or event time exists, establish state without inventing a transition. Deduplicate events across polling endpoints and multiple notification categories. A comment that mentions the user should not produce duplicate comment and mention banners by default.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### NOTIFY-15

Persist event keys, eligibility decisions, grouping keys, and delivery state. Reuse stable native notification identities where supported and report unsupported replacement/retraction capabilities truthfully. Prevent ordinary duplicate delivery across retries and restart; do not claim an unconditional exactly-once guarantee across application crashes and OS delivery.

Initial owner: Sprint 05. Initial verification: Sprint 11. Persistence contracts in 02; item/comment observations and baselines in 05; review/check and branch observers in 06/10; full decisions and delivery in 11.

### WRITE-01

Keep unsubmitted drafts separate from submitted operations. Model submitted operations using queued, validating, sending, outcome unknown, waiting for remote completion, blocked/needs attention, confirmed, and cancelled states as defined in OUTBOX-02. Persist intent before delivery; show pending local changes distinctly until GitHub confirms them. Share operation state and server results across all views, independently of the originating tab's lifetime.

Initial owner: Sprint 07. Initial verification: Sprint 09. Journal primitives in 02; shared write engine in 07; integrate reviews and merges before final closure.

### WRITE-02

Save a draft snapshot and durably enqueue the user's explicit submission before attempting a request. Preserve them after authorization errors, validation failures, offline transitions, server errors, and closing their tab. Enforce shared draft ownership and one operation identity across panes. Merely saving, editing, or reopening an unsubmitted draft MUST NOT enqueue it. Submitted eligible actions MUST resume automatically after restart or reconnection without asking the user to submit them again, subject to the outbox rules.

Initial owner: Sprint 07. Initial verification: Sprint 09. Local drafts/outbox persistence first; automatic replay, cancellation, preflight and uncertain-outcome handling expand from issues/comments in 07 to reviews/merges in 08-09.

### WRITE-03

A timed-out issue, comment, review, or merge operation may have succeeded remotely. Keep the submitted operation in the outbox and automatically reconcile reliable identities or server state before considering another attempt. Retry automatically only when the action is known not to have been applied or the provider has a verified safe idempotency mechanism. If the result remains ambiguous, show Outcome unknown and allow inspection or stopping further attempts without claiming remote undo. Do not blindly replay a non-idempotent request; matching text alone is not proof of success. A deliberate retry of an unresolved action must explain the possible duplicate and cannot be triggered by reconnection alone.

Initial owner: Sprint 07. Initial verification: Sprint 09. Local drafts/outbox persistence first; automatic replay, cancellation, preflight and uncertain-outcome handling expand from issues/comments in 07 to reviews/merges in 08-09.

### WRITE-04

Preserve concurrent remote edits, submitted intent, and local input. Revalidate affected fields, permissions, and revisions immediately before dispatch, including automatic replay; explain conflicts and keep the action visible as Needs attention. Retain the draft for correction. Document unavoidable races where GitHub supplies no atomic conditional operation. A temporary connectivity interruption alone must not require resubmission of an already queued action.

Initial owner: Sprint 07. Initial verification: Sprint 09. Local drafts/outbox persistence first; automatic replay, cancellation, preflight and uncertain-outcome handling expand from issues/comments in 07 to reviews/merges in 08-09.

### OUTBOX-05

Prevent duplicate local submission with stable operation IDs, transactional claims, and per-attempt evidence; do not assume this creates server idempotency. For a connection loss, timeout, crash, or ambiguous server error after dispatch, preserve Outcome unknown and reconcile before another mutation. Automatically retry only if reliable evidence shows no remote effect, or verified endpoint idempotency makes it safe. Persist known remote object/operation IDs and confirm through server state where possible. Matching body text or a brief absence from a list is not proof. If still unresolved, retain the action for attention rather than silently replaying or marking success. No unconditional exactly-once delivery guarantee is made.

Initial owner: Sprint 07. Initial verification: Sprint 09. Storage/dispatcher contracts precede actual issue/comment replay in 07; finish uncertain-outcome, cancellation and current-intent validation for reviews and merges by 09.

### OUTBOX-07

Cancellation of an unsent queued or validating action MUST durably win against dispatch through an atomic state transition checked immediately before the mutation is sent. If cancellation wins, no request for that operation may later be sent, including after restart. If dispatch has already won, show Cancellation requested/May already be sent, stop further mutation attempts, and continue permitted read-only reconciliation. Cancelling an HTTP task is not proof GitHub did nothing. Do not claim remote rollback: a confirmed comment remains a comment and a remotely completed merge remains merged. GitHub merge-queue withdrawal is outside first-release scope. Keep original text, dispatch evidence, and dependent actions recoverable even if sign-out delays reconciliation.

Initial owner: Sprint 07. Initial verification: Sprint 09. Storage/dispatcher contracts precede actual issue/comment replay in 07; finish uncertain-outcome, cancellation and current-intent validation for reviews and merges by 09.

### OUTBOX-08

Freshly validate the original target, permission, edit base values, review anchors, intended commit, merge method, and applicable policy immediately before mutation dispatch. Automatically execute unchanged valid intent without routine reconfirmation. A conflicting remote edit, changed reviewed/merge head, missing target, or changed permission moves the action to Needs attention. Never silently rebase a queued edit, re-anchor a review, approve a new revision, substitute a merge head, or change the chosen merge method. Show the current-versus-intended context so the user can correct or cancel it. Live server enforcement remains authoritative. [S09] [S10]

Initial owner: Sprint 07. Initial verification: Sprint 09. Storage/dispatcher contracts precede actual issue/comment replay in 07; finish uncertain-outcome, cancellation and current-intent validation for reviews and merges by 09.

### TEST-01

Implement a documented development test runner and relevant tests in every sprint, not only at release. The required command contract is cargo xtask test unit; cargo xtask test integration; cargo xtask test ui; cargo xtask test e2e --suite smoke or --suite full; cargo xtask test install --artifact <path>; and cargo xtask test live --repo OWNER/REPO with an explicit --allow-live-writes opt-in for approved mutations. Implement these commands or a documented equivalent wrapper before claiming their gates passed. They are planned commands, not tools already provided by this requirements ZIP. Each command returns a meaningful exit status, reports selected/skipped/blocked tests and environment, and writes machine-readable results plus concise logs. End users install prebuilt binaries and need none of this development tooling.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

### TEST-02

Unit tests MUST cover nontrivial domain and state-machine decisions introduced by each sprint: shared item identities, absent-versus-null updates, reference parsing/type guards, completeness, schedule/DST rules, notification precedence, draft ownership, dependencies, cancellation, retry eligibility, conflict/revision checks and diff source anchors. Use controlled clocks, seeded fixtures and small independently checkable expectations. Test meaningful boundaries and regressions rather than mirroring implementation lines or pursuing an arbitrary coverage percentage. Add the relevant tests with the feature; future feature tests remain pending.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce runner/fixtures/evidence or initial native smoke in 01; each sprint adds relevant tests. Full Windows coverage and exact published-package evidence close in 12; platform parity closes separately in 13/14.

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

FND-03, CACHE-03.

Use requirement_platform_traceability.csv for applicability and release-gate evidence. Initial ledger statuses refer only to their initial phase; no single Windows result closes all platforms. Keep future options and unavailable services explicit.

## Later scope and handoff

Later sprints integrate these contracts with real provider data, panes, reviews, and merges. Storage tests do not complete their UI or live acceptance scenarios. Full draft-type integration closes in Sprint 08; all comparison kinds in Sprint 10.

## Coding LLM instruction

Implement Sprint 02 against v1.7. Keep the app buildable and preserve the current product constraints. Complete listed implementation and test work, use actual native controls for E2E, protect original targets/drafts/Outbox state, and add only the named platform integrations. Do not add issue/PR creation, merge queues or browser engines. Keep secrets out of code/results; live writes require explicit allowlisted authorization. Implement the short verified prebuilt install/update contract for the relevant publication phase and never claim placeholder commands runnable. Update initial/per-platform/scenario ledgers with commands, environment, package identity and evidence, then use SPRINT_REPORT_TEMPLATE.md. Continue only within the assigned sprint/Windows/all-platform scope without new routine approvals.

## Technical sources

- [S04 SQLite write ahead logging](https://www.sqlite.org/wal.html)
- [S05 SQLite FTS5 full text search](https://www.sqlite.org/fts5.html)
- [S06 GitHub issue and PR conversation comments](https://docs.github.com/en/rest/issues/comments)
- [S07 GitHub REST pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api)
- [S09 GitHub pull request reviews](https://docs.github.com/en/rest/pulls/reviews)
- [S10 GitHub pull request reads and merges](https://docs.github.com/en/rest/pulls/pulls)
- [S11 GitHub REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)
- [S13 GitHub inline review comments](https://docs.github.com/en/rest/pulls/comments)
- [S16 GitHub branch comparison semantics](https://docs.github.com/en/pull-requests/reference/branches)
- [S27 GitHub shared issue and PR numbering](https://docs.github.com/en/enterprise-cloud%40latest/rest/using-the-rest-api/issue-event-types)
- [S28 GitHub issue representations identifiers and updates](https://docs.github.com/en/rest/issues/issues)
- [S29 GitHub linking pull requests and issues](https://docs.github.com/en/issues/tracking-your-work-with-issues/using-issues/linking-a-pull-request-to-an-issue)
- [S30 GitHub repository auto-close settings](https://docs.github.com/en/repositories/managing-your-repositorys-settings-and-features/managing-repository-settings/managing-auto-closing-issues)
- [S32 GPUI Kit testing facilities and limits](https://gpui-kit.com/docs/test/)
- [S36 Keyring native protected credential stores](https://docs.rs/keyring/latest/keyring/v1/index.html)

Additional current primary sources and platform/install/test constraints are in the three companion plans. Verify pinned tool/API behavior during implementation.

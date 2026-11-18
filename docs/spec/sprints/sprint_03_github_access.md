# Sprint 03: GitHub authentication, SSO, provider, and repository selection

Requirements version 1.7 | Initial sprint status Planned

## Outcome

The app can securely access an explicitly selected SSO-protected GitHub.com repository as the user.

## Phase and prerequisites

- Release phase: Windows first release.
- Prerequisites: Sprint 01, Sprint 02.
- Code areas: auth, github, platform_windows credential store, storage/account profiles, ui/repository selection.
- Read the master, TESTING_PLAN.md, INSTALLATION_PLAN.md, CROSS_PLATFORM_PLAN.md and all relevant scenario/platform ledgers.
- Use actual prior evidence; a missing dependency remains blocked rather than passed. Later-platform release work does not silently move into the Windows phase.

## Implementation tickets

### S03-T01

Implement the confirmed PAT login path, protected native credential storage, visible active identity, and owner-scoped credential profiles. Prefer a fine-grained PAT where actual endpoint and organization compatibility permit; support an approved classic PAT when needed. Validate host and redirect routing; never send one owner's credential to an unrelated destination. OAuth/device authorization is future optional scope.

### S03-T02

Implement typed REST/GraphQL provider methods, pagination envelopes, capability reporting, read cancellation, structured API errors, and explicit API version handling. Verify current primary endpoint documentation and record required permissions.

### S03-T03

Represent SSO authorization, pending organization approval when detectable, partial SSO results, expired/revoked credentials, and missing permissions separately. Provide a validated external authorization action while preserving local context.

### S03-T04

Implement repository discovery, selection, revisit/removal, access status, last successful check, and cached-usage display. Removing selection must mark monitoring disabled; Sprint 05 connects that state to the running scheduler.

### S03-T05

Validate a designated private repository, not only the user endpoint. Test account switching, cached-content isolation, revoked access, and readable functions remaining available when write permissions are absent. Establish the permissions matrix for later issue/review/merge integrations.

### S03-T06

Emit typed API reachability evidence separately from authentication, SSO, service errors, and rate limiting. Bind queued work to its original identity and credential scope; same-user authorization repair may wake eligible work, while account switching or repository deselection holds affected actions.

### S03-T07

Distinguish ordinary exit from explicit sign-out. Retain the protected PAT across ordinary exit; sign-out persists a network/action hold and removes or invalidates local PAT material while retaining readable cache, drafts, workspace, checkpoints, and outbox. Reuse the same local account and resource identities after verified same-user PAT replacement. Expiry or revocation never creates a local login wall or a full resync. Exercise provider/storage cases now; full cache UI, writer, and release cases follow in Sprints 04, 09, and 12.

### S03-T08

Implement and test the Windows protected credential adapter and portable path contracts, including locked/denied/unavailable storage, Unicode user paths and same-user renewal. Keep macOS/Linux adapters as explicit phase tasks. Complete AC-85 and AC-90 with real local storage, actual HTTP serialization/pagination/error mapping, and controlled server failures; live read/write validation remains separately allowlisted.

## Required tests and commands

- Integration: drive actual HTTP serialization/pagination/SSO/errors against the local simulator plus real SQLite; include partial/private access and credential-routing checks.
- Native E2E: use Windows account/repository controls with isolated fake credentials/server; separately test the real protected store and locked/denied conditions.
- Live: run allowlisted read-only repository access when credentials exist, mark missing SSO/approval blocked, and keep mutation tests off by default.

Implement and use the canonical runner from TESTING_PLAN.md: unit, integration, ui, native e2e smoke/full, install with an artifact, and separately guarded live tests. These are development commands to build, not commands already supplied by this ZIP. Record actual OS/build/fixture evidence and mark unavailable checks Blocked/Not run. Native test-build E2E and exact published-package smoke remain separate.

Scenario IDs introduced or closing here: UT-010, UT-020, IT-004, IT-005, IT-020, E2E-002, LIVE-001.

Live-provider cases are shared gates on an explicitly authorized host. A later platform can reference existing evidence only with a recorded applicability assessment; rerun changed provider/credential-policy or target-specific behavior. Missing authorization remains Blocked/Not run and never permits unapproved writes.

Use test_scenarios.csv for fixture IDs, expected results, first/closure ownership and evidence. Release sprints rerun all scenarios applicable to their target; earlier sprint scenario lists do not waive regression checks.

## Deliverables

Live read-capable provider; protected token profiles; SSO/access diagnostics; selected-repository settings; endpoint/permission matrix; authentication and routing fixtures.

## Exit gates

- Demonstrate real authenticated access to the designated private repository and record the environment without exposing secrets.
- Pass AC-02 and AC-03 with controlled SSO/approval/partial-result cases. Verify 403/404 handling never assumes bulk deletion.
- Show account isolation and credential routing tests. If credentials or organization approval are unavailable, mark the live check blocked and continue independent work; do not mark it passed.

## Initial acceptance scenarios owned here

- **AC-02** A token requiring SSO authorization or organization approval produces an actionable access state without claiming that private repositories were deleted.
- **AC-03** Partial SSO results are identified; the UI does not claim that the visible repository list is complete.
- **AC-85** Exercise the actual supported platform's protected credential store and per-user data path, including unavailable/locked store, Unicode paths, restart and same-user PAT renewal. No plaintext fallback or new duplicate profile appears; offline cached reading and original outbox identity survive. Windows closes initially in Sprint 03; later OS adapters repeat it in their phases.
- **AC-90** Use actual SQLite storage and the real HTTP provider adapter against a deterministic local server to verify transactional identity/cache behavior, pagination, partial failures and authentication/error mapping. Storage foundations are tested in Sprint 02 and the initial complete adapter scenario closes in Sprint 03. Later integration scenarios extend this foundation rather than being claimed complete early.

## Full owned requirements

### FND-05

Use direct GitHub APIs. Do not scrape rendered GitHub pages or depend on a user's browser session. Keep REST and GraphQL behind a provider interface with explicit capabilities and version handling.

Initial verification owner: Sprint 03. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-01

The application MUST support GitHub.com accounts accessing SSO-protected organization repositories. Use `https://api.github.com` for REST, `https://api.github.com/graphql` for GraphQL, and `https://github.com` as the authorization web origin. Validate credential routing and any authorized redirects. GitHub Enterprise Server compatibility is not a first-release requirement.

Initial verification owner: Sprint 03. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-02

Authenticate as the user for issue changes, reviews, approvals, and merges. Show the active identity and relevant organization or credential profile. Do not submit the user's reviews as an installation bot identity.

Initial verification owner: Sprint 09. Verify user identity and actual endpoint permissions for issue, review, and merge actions as each becomes available.

### AUTH-03

Use a personal access token as the confirmed first-release authentication path. Prefer a fine-grained PAT when all required endpoints and organization policy support it; support an organization-approved classic PAT when endpoint coverage or organization compatibility requires that alternative. Do not switch to OAuth or device authorization as an unrequested first-release dependency. A fine-grained token can be limited to one resource owner and selected repositories, so credential profiles must route appropriate PATs for different selected owners without sending an unrelated token. [S20]

Initial verification owner: Sprint 03. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-04

For organizations using SAML SSO, account for the distinction between token creation, SSO authorization, and organization approval. Fine-grained tokens receive SSO authorization during creation; classic tokens require separate SSO authorization. Fine-grained tokens may also require an organization owner's approval before private resources are accessible. [S21] [S22]

Initial verification owner: Sprint 03. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-05

Handle expired or revoked tokens, insufficient permissions, pending approval when identifiable, organization restrictions, and required SSO authorization as distinct actionable states. Interpret `X-GitHub-SSO` when present, including partial results. A 403 or 404 caused by authorization MUST NOT be treated automatically as resource deletion. [S21]

Initial verification owner: Sprint 03. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-06

Provide a user-initiated action to open an appropriate GitHub PAT or SSO authorization page in the external default browser. Validate returned authorization URLs before opening them. Preserve the current screen, cached content, drafts, outbox, and synchronization checkpoints while authorization is repaired. After verifying that a replacement PAT belongs to the same GitHub user and restores the required access, reuse the existing account/resource identities and progress, retry eligible reads, and automatically resume eligible submitted actions under the persisted pause/offline settings. A different user must receive a separate account context; replacing a token must not create a new cache for the same user.

Initial verification owner: Sprint 09. Authorization repair restores the same identity in 03; reconnect signals in 05 and automatic eligible outbox resumption across all writers in 07-09.

### AUTH-07

Store PATs through the platform credential adapter in Windows Credential Manager or an equivalent native protected store; published macOS/Linux builds follow PORT-04. Persist only credential references in SQLite. Normal window close or application exit retains the protected token so the next launch can resume without re-entry while it remains valid. Explicit sign-out follows AUTH-11 and clears the relevant local credential material. Never include tokens in logs, URLs, diagnostics, exports, or source control.

Initial verification owner: Sprint 03. Implement and verify in the initial owning phase; repeat applicable behavior with actual evidence on each later released platform.

### AUTH-08

Request and document the permissions actually required by the chosen endpoints. Issue writes, PR review writes, commit access, and merging must each be validated against current endpoint documentation. Check access to an explicitly selected private repository, not merely whether `/user` succeeds. Preserve readable functions when a write permission is missing. [S06] [S09] [S10]

Initial verification owner: Sprint 09. Verify user identity and actual endpoint permissions for issue, review, and merge actions as each becomes available.

### AUTH-11

Explicit sign-out MUST durably mark the affected account profile as signed out, prevent new network dispatch under that profile, and remove or invalidate its locally stored PAT material. Preserve readable private cached content, drafts, saved workspace, settings, synchronization checkpoints, activity baselines, and outbox records on the PC. Users may reopen that signed-out local profile for offline work without a renewed login; show its signed-out status and hold its remote actions until a PAT for the same verified user restores access. Keep that hold across restart. Ordinary exit is not sign-out. Expired or revoked PATs likewise preserve offline use and progress while holding affected remote work. A sign-out during an in-flight mutation does not prove remote cancellation; retain its evidence for reconciliation after access is restored. Do not expose one profile's data through a different active account or assume sign-out revokes the PAT at GitHub.

Initial verification owner: Sprint 12. Signed-out credential/dispatch holds begin in 03; readable local-profile UI in 04 and all-writer PAT repair in 09; complete private-data retention and in-flight lifecycle matrix in 12.

### BROWSE-01

Users MUST be able to select, remove, and revisit accessible repositories. Show organization, repository name, access state, last successful check, and cached storage usage. Removing a monitored repository stops its polling; deleting its cached data is a separate explicit action.

Initial verification owner: Sprint 05. Repository selection/storage usage in 03-04; removal stops the running scheduler in 05.

### DATA-04

Credential switching must isolate private cached data, drafts, and queued actions. Bind each action to its original GitHub user/account access context and credential owner scope. Pause dispatch for inactive identities, signed-out profiles, or known revoked access; resume eligible work under the same verified identity after PAT repair and subject to persisted controls. Replace credentials for the same user without recreating the account, item identities, cache, outbox, or synchronization baselines. Never reroute queued work through another signed-in user. Failed authorization and explicit sign-out preserve local content under AUTH-11; a signed-out local profile remains readable without exposing it through another active account.

Initial verification owner: Sprint 09. Identity isolation in 03; verify cached workspace, drafts and deferred issue/review/merge execution remain bound to the original account.

### CONNECT-02

Derive reachability from verified HTTPS responses from the configured GitHub API and current transport errors, not from cached data or a network adapter icon alone. Successful responses, including 304, establish contact. A genuine API 401, authorization 403/404, 429, or 5xx also demonstrates a reachable API endpoint, while separate status explains Sign-in required, SSO approval, Throttled, or Service error and blocks affected writes as appropriate. DNS, connection, TLS, and timeout failures indicate inability to reach the service securely; they do not prove that every Internet destination is down. Request cancellation caused by tab navigation is not an offline signal. Suppress older/out-of-order observations so they cannot overwrite newer connectivity evidence; scope authorization errors separately by account/repository.

Initial verification owner: Sprint 05. Typed provider evidence in 03; UI consumes it in 04 and full stale/out-of-order/transport/error cases close in 05.

### OUTBOX-09

Bind each action permanently to the original host, user/account context, repository/resource identity, and credential owner scope; resolve credentials by protected reference at dispatch, never from a stored token in the payload. Switching users pauses the inactive account's work and isolates its queue contents. Reauthorization may resume the original user's eligible work; another user cannot inherit it. Removing a repository selection preserves its pending work and marks it paused for scope review instead of deleting or secretly sending it. Confirmed locator changes use verified identity and preserve the queue's stable local target.

Initial verification owner: Sprint 09. Original-user credential/scope binding begins in 03; panel isolation in 04; deferred issue/review/merge dispatch verified through 09.

### PORT-04

Implement protected credential and application-data adapters per published OS. Use Windows Credential Manager or equivalent on Windows, the platform keychain on macOS, and a documented supported secure credential service on Linux. Store only credential references in SQLite; never fall back silently to plaintext tokens when a store is unavailable or locked. Preserve offline cached access while affected network work is held. Use appropriate per-user data/config/cache locations, stable app identity and relative managed asset paths rather than Windows-specific absolute paths. Test filesystem permissions, file locking, SQLite WAL/FTS availability, path/Unicode handling, and stable location across upgrades. Credentials and caches are per-device; this does not add cross-device synchronization. [S36]

Initial verification owner: Sprint 12. Path/storage contracts in 02 and real Windows credential tests in 03; installed path and retention behavior closes in 12, with OS adapters in 13/14.

### TEST-07

Live-provider validation MUST use an explicitly designated disposable repository and appropriate test identities with no credentials embedded in code or results. Read-only live tests are the default; mutation tests require explicit --allow-live-writes and a configured allowlisted owner/repository, and cover only supported edits/comments/reviews/merges. Use existing issue/PR fixtures and do not add an app issue/PR-creation flow for test setup. Preserve ordinary SSO and permission enforcement. Keep destructive cases isolated, identify any effects and cleanup, and mark unavailable credentials/permissions Blocked rather than inventing successful integration evidence.

Initial verification owner: Sprint 12. Guarded live read setup in 03; explicitly allowed supported mutations in 07-09; required live evidence closes at each applicable platform release without inventing credentials.

## Integration work owned elsewhere

### FND-04

Use Tokio for asynchronous application work and reqwest for HTTP. Use typed request and response models, explicit cancellation, structured errors, and a centralized request scheduler. A different library is acceptable only with a recorded compatibility reason.

Initial owner: Sprint 01. Initial verification: Sprint 05. Background/provider contracts begin in 01; complete central scheduler in 05.

### PORT-02

Share domain models, GitHub integration, cache/search, synchronization, Outbox, review/diff logic, history, notification rules, and workspace state across platforms. Isolate operating-system APIs behind small capability interfaces with conditional dependencies inside platform implementations. Common code must not depend on Windows handles, registry calls, COM types, fixed Windows paths, or platform-specific credential objects. Reuse portable GPUI facilities where suitable rather than wrapping the entire toolkit in a new framework. Test interface contracts with fakes and actual adapters; do not duplicate the product's business logic for each OS.

Initial owner: Sprint 01. Initial verification: Sprint 12. Introduce platform boundaries in 01 and keep shared modules portable throughout; verify the full Windows implementation boundary in 12 and parity in later platform rows.

### PORT-06

Implement and test platform-appropriate keyboard shortcuts, text input, accessibility, clipboard, high-DPI/scaling, theme changes, timezone defaults, external-link opening, startup registration, close versus Quit, background residence, sleep/wake and network-change hints. Feed normalized lifecycle events into the shared scheduler. Restore offline/pause settings before probes or dispatch. If a supported environment lacks a tray/menu-bar capability, keep the app reachable and explain background behavior rather than hiding an inaccessible running instance. Closing/quit must preserve drafts, workspace, Outbox and credentials according to the shared lifecycle contract. [S33] [S35]

Initial owner: Sprint 01. Initial verification: Sprint 12. Keep native interaction/lifecycle boundaries explicit from 01; complete Windows accessibility, restart and installed behavior in 12, and repeat for later targets.

### WORKSPACE-08

Save and restore the workspace per account, including pane order and proportions, open tab order, active tabs, focused pane, per-tab view state, scroll position or anchors, navigation context, and draft references. Normal exit MUST retain this state. On restart, render cached visible content before waiting for any API response or renewed login; then start eligible background work through the shared scheduler. Load inactive tabs lazily and expose missing or inaccessible content without discarding its navigation target. Restoring a workspace MUST NOT submit unsubmitted drafts, create duplicate operations, or expose another account's private tabs. The independent outbox worker resumes already submitted eligible actions under OUTBOX-04 and the restored authentication/pause settings; view restoration is not a dispatch trigger.

Initial owner: Sprint 04. Initial verification: Sprint 12. Restore local workspace/drafts before networking in 04; integrate all views and writers as built. Final normal-exit, signed-out, expired-PAT and delayed-network restoration closes in 12. View restoration never submits drafts.

### CACHE-08

Namespace records by host and account access context, with stable repository and resource identities. Issues and PRs share one repository-local number namespace. Map their API representations to a common local item while preserving distinct issue and pull-request API object IDs. Follow DATA-05 through DATA-09 for keys and ingestion. Never expose another account's private cached content after switching identities. [S27] [S28]

Initial owner: Sprint 02. Initial verification: Sprint 04. Storage scoping in 02, real identity switching in 03, cached browsing isolation in 04.

### CACHE-10

A 404, authentication failure, or temporary API error MUST NOT trigger bulk cache deletion. Record the reason when known, stop inappropriate retries, and reconcile after access is restored. Distinguish an unknown resource from a confirmed deletion.

Initial owner: Sprint 02. Initial verification: Sprint 05. Storage preservation in 02; authorization/provider handling in 03; live sync reconciliation in 05.

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

### CONNECT-01

Show a persistent, prominent green/red connection indicator in the application chrome, visible across panes, with an accessible icon and text label; color alone is insufficient. Green means recently verified GitHub API reachability. Red means Offline or Cannot reach GitHub after a current transport failure or an explicit Work offline choice. Use a neutral Checking or Status stale state before an initial result and when evidence is too old, rather than asserting a connection indefinitely. Show last checked time, last successful contact, reason, and Check now in its details, plus a pending-action count linking to the Outbox.

Initial owner: Sprint 04. Initial verification: Sprint 05. Provider status contract in 03, persistent accessible chrome in 04, real shared observer and reachability classification in 05.

### DATA-03

Keep secrets and private content out of routine diagnostic output. Export diagnostics only through an explicit user action with appropriate redaction. Treat remote Markdown, links, and filenames as data rather than instructions or executable paths.

Initial owner: Sprint 02. Initial verification: Sprint 12. Establish redaction/content safety boundaries early; verify all parsers and final explicit diagnostic export in 12.

### DATA-05

Store the visible GitHub number separately from the local primary key and API object IDs. GitHub issues and PRs use the same increasing number sequence within a repository: an issue numbered 41 can be followed by PR 42 and issue 43. There cannot be two distinct items consisting of issue 42 and PR 42 in that repository. Enforce UNIQUE(repository_id, number), without kind in that constraint. Here repository_id identifies a local repository record already scoped to host and account access context. The same number in a different repository or account context must not collide. [S27]

Initial owner: Sprint 02. Initial verification: Sprint 04. Database identity contract in 02; live provider and browsing ingestion in 03/04.

### DATA-09

Use the stable local item_id for relationships, including drafts, tabs, comments, fetch state, and search records. The repository number is the current locator. Resolve a confirmed issue transfer or redirect through verified provider identity before changing that locator; preserve local relationships and the previous locator as an alias where appropriate. Never combine unrelated items merely because their numbers match, and never treat repository names or URLs as immutable database identity. If identity cannot be confirmed, preserve the existing record and surface the unresolved relocation. [S28]

Initial owner: Sprint 02. Initial verification: Sprint 04. Fixture transfer in 02; provider resolution and workspace/search/draft references integrated in 03/04.

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

FND-05, AUTH-01, AUTH-03, AUTH-04, AUTH-05, AUTH-07.

Use requirement_platform_traceability.csv for applicability and release-gate evidence. Initial ledger statuses refer only to their initial phase; no single Windows result closes all platforms. Keep future options and unavailable services explicit.

## Later scope and handoff

User-identity, endpoint permission, and same-user PAT renewal across all writers finish in Sprints 07-09. Signed-out local reading begins in Sprint 04; the all-content lifecycle matrix closes in Sprint 12. AUTH-10 remains inactive future scope. Repository removal is exercised against polling in Sprint 05.

## Coding LLM instruction

Implement Sprint 03 against v1.7. Keep the app buildable and preserve the current product constraints. Complete listed implementation and test work, use actual native controls for E2E, protect original targets/drafts/Outbox state, and add only the named platform integrations. Do not add issue/PR creation, merge queues or browser engines. Keep secrets out of code/results; live writes require explicit allowlisted authorization. Implement the short verified prebuilt install/update contract for the relevant publication phase and never claim placeholder commands runnable. Update initial/per-platform/scenario ledgers with commands, environment, package identity and evidence, then use SPRINT_REPORT_TEMPLATE.md. Continue only within the assigned sprint/Windows/all-platform scope without new routine approvals.

## Technical sources

- [S06 GitHub issue and PR conversation comments](https://docs.github.com/en/rest/issues/comments)
- [S07 GitHub REST pagination](https://docs.github.com/en/rest/using-the-rest-api/using-pagination-in-the-rest-api)
- [S09 GitHub pull request reviews](https://docs.github.com/en/rest/pulls/reviews)
- [S10 GitHub pull request reads and merges](https://docs.github.com/en/rest/pulls/pulls)
- [S11 GitHub REST best practices](https://docs.github.com/en/rest/using-the-rest-api/best-practices-for-using-the-rest-api)
- [S13 GitHub inline review comments](https://docs.github.com/en/rest/pulls/comments)
- [S20 GitHub personal access token capabilities](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)
- [S21 GitHub REST authentication and SSO responses](https://docs.github.com/en/rest/authentication/authenticating-to-the-rest-api)
- [S22 Organization approval of fine grained tokens](https://docs.github.com/en/organizations/managing-programmatic-access-to-your-organization/managing-requests-for-personal-access-tokens-in-your-organization)
- [S27 GitHub shared issue and PR numbering](https://docs.github.com/en/enterprise-cloud%40latest/rest/using-the-rest-api/issue-event-types)
- [S28 GitHub issue representations identifiers and updates](https://docs.github.com/en/rest/issues/issues)
- [S32 GPUI Kit testing facilities and limits](https://gpui-kit.com/docs/test/)
- [S33 GPUI Kit accessibility](https://gpui-kit.com/docs/accessibility/)
- [S35 GPUI Kit packaging](https://gpui-kit.com/docs/packaging/)
- [S36 Keyring native protected credential stores](https://docs.rs/keyring/latest/keyring/v1/index.html)

Additional current primary sources and platform/install/test constraints are in the three companion plans. Verify pinned tool/API behavior during implementation.

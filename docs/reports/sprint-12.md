# Sprint 12 Windows installer development

The Windows 11 x64 local installer passed ten setup and recovery checks on build 22631. An installed predecessor and candidate also opened the same offline workspace across a SQLite schema upgrade. The user selected local installer testing and left publication pending. The exact published-package checks remain open. [Setup outcomes and package digests](evidence/sprint-12/installer-lifecycle.json) · [Windows installation plan](../spec/INSTALLATION_PLAN.md)

| Windows test | Observed result |
| --- | --- |
| Per-user install and repair | Predecessor installation and repair both exited 0. The local fixture created a matching shortcut and registration. The current build also repaired successfully. |
| Interrupted update and retry | The canonical matrix held the executable open and received exit 14. A setup built with a staged-write fault returned 16. The previous installation survived both checks. A subsequent upgrade exited 0. |
| Downgrade and uninstall | The older setup returned 12 both before and after uninstall. Uninstall removed the executable and registration, retained the preseeded profile, and allowed the current package to reinstall. |
| Local trust fixture | The verifier accepted a fixture-signed manifest and its matching setup digest. It rejected a different publisher pin and a same-length modified setup before either could execute. Production signing and independent publisher trust remain pending. |

The canonical matrix ran in 64-bit Windows PowerShell using `tools/windows/test-local-lifecycle.ps1` with all three setup paths, their expected SHA-256 values and the preseeded profile digest. The independently recorded output contains the package hashes and the exit code for each step. The installer and harness are recorded at source `95f9066`. [Lifecycle driver](../../tools/windows/test-local-lifecycle.ps1) · [Local build and recovery commands](../../tools/windows/README.md) · [Trust and native app receipts](evidence/sprint-12/local-package.json)

The canonical `cargo xtask test install --case S12-LOCAL-PACKAGE --artifact <candidate> --local-fixture <manifest>` run passed all ten setup steps on clean `5de91d4`. PowerShell 5.1 installed local builds 6 and 7 and retained the seeded schema 12 database digest across the failure and uninstall checks. [Canonical install runner receipt](evidence/sprint-12/canonical-local-install.json)

## Installed application and retained state

The offline fixture held 30 items, 90 comments, 20 tabs, one draft, ten Outbox operations and one step receipt. Opening the installed candidate from `1ba4fff` after the installed predecessor from `8d93bc1` moved the database from schema 9 to 10. Nine ordered projections matched, including those for queue payloads, tab state, cached resources and local credential metadata. There were zero synchronization preference rows. The receipt gives the selected SQL columns and both hashes for every projection. [Database comparison](evidence/sprint-12/retained-profile.json)

The installed executable had SHA-256 `783bc377629896a4f5102ee5c50b94166cb4d214b00cfa28c933c79fb351381a`. The native smoke driver verified the executable owning the window, invoked an initially unselected cached tab through Windows UI Automation, observed its activation and exited Threadline normally. A later uninstall retained the database byte for byte. [Installed smoke driver](../../tools/windows/smoke-installed.ps1) · [Installed app and uninstall results](evidence/sprint-12/local-package.json)

Local build 5 from `725632e` upgraded the same offline workspace. The installer and registration matched the expected binary. On the first Windows UI Automation invocation, the installed process selected issue 6 in the second pane and exited normally. The closed database reported schema 12. Eight nonempty ordered projections and the empty synchronization preferences projection still matched the schema 9 and 10 digests. The application produced migration backups for schemas 9 and 10. [Build 5 install, navigation and retained projections](evidence/sprint-12/installed-build-5.json)

Windows PowerShell 5.1 also ran the release verifier against the exact local matrix setup with its correct size and digest and a fixture publisher pin. It exited 1 because Windows could not confirm a trusted Authenticode signature. [Release verifier](../../tools/windows/verify-release-installer.ps1) · [Verification receipt](evidence/sprint-12/installed-build-5.json)

Installed local build 10 opened the retained schema 12 fixture and migrated it to schema 15. All nine ordered projections matched, SQLite reported zero integrity or foreign-key errors, and Work Offline stayed on. The normal window selected an initially inactive tab on the first invocation and quit cleanly. Uninstall exited 0, removed the installed binaries, shortcut and registration, and preserved every retained file byte for byte. The tester archived the owned profile and verified that the default data directory was absent. [Migration, native navigation and uninstall](evidence/sprint-12/installed-build-10.json)

## Windows release gates

PKG-001 through PKG-006 and PKG-009 still require applicable release evidence. Outstanding checks include the published one-to-three-command path for a clean supported Windows user, release signature and independent trust, and offline smoke on the exact shipping artifact. Local architecture checks cover an incompatible manifest on Windows x64 and a Windows ARM64 application presented to the package builder. The full Windows release matrix remains open. macOS and Linux packaging have separate release gates. [Windows package scenarios](../spec/test_platform_traceability.csv) · [Acceptance criteria](../spec/acceptance_platform_traceability.csv) · [Sprint requirements](../spec/sprints/sprint_12_release_verification.md)

The native Windows publisher helper accepted the independent Microsoft signer pin for a valid Windows SDK executable and rejected a different publisher pin for the same bytes. The successful fixture was Microsoft's SDK tool. Threadline's own signing and publication remain pending. [Publisher verification receipt](evidence/sprint-12/windows-publisher-verification.json)

GitHub Actions run 44409703561 at `4f1e208` recorded four zero-step jobs and one optional skipped job after the account exhausted its free Actions credits. The hosted check is Blocked. Earlier completed jobs and independently recorded Windows and Linux runs retain their original results. [Run and jobs](https://github.com/joshmarkovic/threadline/actions/runs/44409703561) · [Windows job](https://github.com/joshmarkovic/threadline/actions/runs/44409703561/job/135174240807)

## Release selection and registered local setup

On Windows 11 x64, the fixture-configured native bootstrap at `43a288a` accepted the independently pinned Microsoft-signed SDK fixture in offline verify-only mode. It refused the correctly hashed unsigned setup at publisher verification and refused a mismatched manifest digest before staging the asset. Each invocation used the normal bootstrap executable. No installer was launched. The test did not use Threadline publisher credentials. [Bootstrap inputs and outcomes](evidence/sprint-12/bootstrap-windows.json) · [Trust and offline selection](../../tools/windows/README.md)

Local build 8 exited 16 while staging notification registration. Its installed-app registration remained absent and the retained highest build stayed at 7. The staged helper ran successfully when called separately, so the original setup result does not identify a specific failing system call. Local build 9 installed the same schema 15 app with the compiled registration helper. The Windows checks verified the expected app digest, Start shortcut, activation command and sender identity. [Setup and registration receipts](evidence/sprint-12/registered-package.json) · [Installed activity journeys](sprint-11.md)

The Windows metadata finalizer wrote a fixture manifest after verifying the independently pinned Microsoft-signed executable. It refused an unsigned setup and refused to overwrite the existing manifest. [Metadata finalizer observations](evidence/sprint-12/bootstrap-windows.json)

Six bootstrap checks passed on Windows 11 x64 with local build 11 installed. In a disconnected session, artifact verification accepted the complete signed Microsoft fixture, refused an independently pinned ARM64 manifest and a half-length payload, and reported a real denied write to its test-owned staging directory. A fresh retry passed after the driver restored access. Normal setup refused to run without an interactive desktop. Every check left the staging directory empty and preserved the installed binary, registry state and archived database. The earlier verifier had required a desktop even for verification. [Six outcomes and earlier refusal](evidence/sprint-12/bootstrap-faults.json) · [Windows fault driver](../../tools/windows/test-bootstrap-faults.ps1)

A separate native Windows test closed a loopback HTTP response after four of 27 bytes. The bootstrap discarded the partial transfer, preserved the previous fixture and staged a fresh complete response. On Linux, the normal package builder rejected a real ARM64 Windows application before emitting an installer. The test did not execute on ARM hardware. [Native transfer and ARM64 package input](evidence/sprint-12/bootstrap-faults.json)

## Installed native runtime

E2E-020 passed on Windows 11 x64 build 22631.7652. Clean runner `1bf7e77` launched the normal installed executable from local package 9. Through the window's native controls, it opened an offline Markdown image attachment, observed the unavailable-image state, selected cached PR file `second.txt`, and switched the diff to split mode. The saved workspace corroborated the file and view. The app quit normally. [Native actions, persisted state and artifact identities](evidence/sprint-12/installed-runtime.json)

The matched build contained 393 unique Windows dependency names and imported 39 libraries from its PE executable. The three process observations each found 72 loaded modules, zero descendants and zero recognized browser components. These observations cover the installed local artifact and the sampled UI states. The exact published package remains pending. [Inventory inputs and counts](evidence/sprint-12/installed-runtime.json) · [Canonical runner and fixture](../../tools/native/README.md)

E2E-020 also passed on installed local build 10. Its build contained 393 dependency names and 39 PE imports. Each of the three native Markdown and diff observations again recorded 72 modules, zero child processes and zero browser matches. [Build 10 runtime and artifact identities](evidence/sprint-12/installed-build-10.json)

Local build 11 from `8da02ce` installed successfully and passed E2E-020 again. The native Markdown and split diff journey recorded 72 modules, zero child processes and zero browser matches in each sampled state. The same executable passed the unregistered-sender fallback. Windows retained its notification group after exit, and the physical click remained blocked by the desktop focus guard. [Build 11 setup, runtime and notification results](evidence/sprint-12/installed-build-11.json)

## Cached reader performance

The Windows 11 cached-reader runs each invoked 20 searches, 20 opens, and 20 tab changes through the app's native UI Automation controls. Every external p95 exceeded its budget. The runs used the synthetic Section 16 profile. [Builds, displays, invocation arguments and source receipt hashes](evidence/sprint-12/reader-performance.json)

| Optimized app | DPI | Cached open p95 | Local search p95 | Cached tab p95 |
| --- | ---: | ---: | ---: | ---: |
| Budget | | <100 ms | <200 ms | <100 ms |
| `7f78edf` | 144 | 507.14 ms | 202.92 ms | 312.88 ms |
| `40fe4d4` | 144 | 2291.14 ms | 2161.72 ms | 2121.26 ms |
| `3b1be96` | 96 | 2197.06 ms | 2229.16 ms | 2140.28 ms |

The `3b1be96` run correlated all 60 actions to app handler and paint timestamps. The native invocation p95 was 2015.07 ms for open, 2014.47 ms for search, and 2016.35 ms for switching. The corresponding handler-to-first-useful-paint p95s were 170.82 ms, 140.04 ms, and 38.27 ms. The external p95s remain the budget results. [Timing attribution](evidence/sprint-12/reader-performance.json)

E2E-014 failed on optimized app and driver `b88773b` at 96 DPI. The 50,000-item, 250,000-comment profile restored both panes and all 20 tabs offline. Warm launch to a verified useful workspace took 3919.64 ms against the 2000 ms budget. The app's earlier generic ready stamp was 1600.65 ms. The idle working set was 92,811,264 bytes. After settling, the app recorded zero draws and zero presents in each of three seconds. One app GPU counter sample was available. The run stopped after the second tab was widened by its downloaded title and fell outside the visible strip. The driver kept its action log. The timing arrays and subsequent backlog measurements were unavailable. [Large workspace result](evidence/sprint-12/reader-performance.json)

The reader now constrains tab width independently of the downloaded title and keeps the full accessible name. The driver retains partial timing receipts and records the app's first paint of both populated panes alongside the external startup measurement. The concurrent Section 16 budget remains open. [Native driver](../../tools/native/windows-cached-reader.ps1)

## Local regression suites

Formatting and strict Clippy passed on clean `f06de5e` in WSL Linux. Clippy checked all workspace targets with the app's E2E feature enabled and warnings treated as errors. The canonical runner used seed 12345 for the local test lanes. [Commands and test receipts](evidence/sprint-12/local-suite-validation.json)

| Linux test lane | Selectors passed | Test results passed |
| --- | ---: | ---: |
| Unit | 23 | 96 |
| Integration | 36 | 136 |
| In-process GPUI | 9 | 18 |

On Windows 11 x64 build 22631.7652, 32 native test executables from `3dc5b64` recorded 238 passes, two failures and two ignored credential tests. The original IT-021 failure came from a staged bootstrap path. The corrected test at `575b609` passed with the actual Windows installer executable supplied explicitly. The other failure came from a one-nanosecond subtraction in an idle sampling test that rounded to the same tick on Windows. The `4619aa5` native app rerun passed all 42 tests, including the corrected idle test. [Original outcomes, test names and follow-ups](evidence/sprint-12/local-suite-validation.json)

A separate Windows test delivered modeled suspend and resume messages to its own hidden platform window. The broker emitted one normal resume event. Physical sleep still needs target evidence. [Native resume receipt](evidence/sprint-12/local-suite-validation.json)

## Windows closure and remaining checks

Sixteen Windows unit, SQLite and in-process GPUI selectors now cite the matching tests in the native executable logs. The corrected IT-021 CLI passed separately. Windows Credential Manager also passed a separately enabled synthetic round-trip. A new process read the same protected entry and the test removed both of its keys. The original default-suite totals still include the two skipped protected-store entries. [Exact tests and receipt hashes](evidence/sprint-12/windows-traceability.json) · [Windows scenario ledger](../spec/test_platform_traceability.csv)

E2E-015 invoked the owned Load preview control through Windows UI Automation on `6ea3e43` at 96 DPI. Windows then refused foreground focus and the driver stopped before sending physical input. Earlier Sprint 01 runs separately verified native text input and clipboard at 96 and 144 DPI. The E2E-015 run still needs its combined keyboard, clipboard, exact Unicode and high-DPI checks in an unlocked Windows session, and a separate signed visual and screen-reader record. [E2E-015 receipt](evidence/sprint-12/windows-traceability.json) · [Earlier native input](sprint-01.md)

| Local Windows work | Remaining proof |
| --- | --- |
| Section 16 performance and Outbox, E2E-014 / AC-35, AC-48, AC-71 | Complete the concurrent drain, navigation and cancellation measurements and record all available GPU samples. Keep the recorded p95 and startup failures visible. The user's earlier scrolling decision still applies to the initial phase. |
| Restored private workspace and failure handling, AC-56, AC-68, AC-73, AC-74, AC-91, AC-92 | In one mixed profile, verify useful cached state before a delayed API response, a valid protected credential across restart, safe signed-out isolation, no repeat delivery, and retained drafts, dependencies and receipts across the remaining faults. Sprint 04, 07, 09 and 11 cover constituent paths. |
| Remaining feature cases, AC-16, AC-18, AC-27, AC-28, AC-29, AC-31, AC-32, AC-39, AC-40, AC-63, AC-65, AC-77 | Exercise multiple submitted inline notes, reviewer attribution, queued thread outcomes and required-merge-queue handling. Verify the app's unopened-item, baseline, grouped-review, reconnect and watched-branch/merge notification paths against the local provider. |
| Local installer recovery, PKG-003/004/006/009 and AC-96 through AC-98 | Verify retained protected credentials and sync checkpoints, interrupted transfer, denied permission, missing prerequisite and wrong architecture. The separate data and credential cleanup journey also remains open. |

The release gates still require the exact signed artifact, published commands on a clean supported Windows user, and the published installer and production smoke results (PKG-001/005, AC-36/94/99). The dedicated PAT and SSO endpoint check (LIVE-001, AC-72) requires an approved account and repository. Hosted runs remain blocked by exhausted Actions credits. The macOS and Linux release gates have separate milestones. [Windows acceptance ledger](../spec/acceptance_platform_traceability.csv) · [Windows scenario ledger](../spec/test_platform_traceability.csv)

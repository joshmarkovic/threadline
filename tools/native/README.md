# Sprint 01 verification

Run the commands from the repository root. The runner writes a unique result directory under `artifacts/xtask` unless you pass `--report-dir`. Each result includes `result.json`, `junit.xml`, the selected cases, and any available build and driver logs. A blocked case returns exit code 78. A failed test or invalid command returns 1.

`result.json` records the runner arguments in `command.argv`, in their original order. It keeps named workspace-relative fixture, binary, and report paths. Other paths, repository names, and unrecognized arguments use placeholders. `command.redacted` identifies a receipt with substitutions. The run timestamps are milliseconds since the Unix epoch. `exit_code` is the runner's exit code, and `child_exit_code` records a test process exit code where available. `package_version_source` identifies the app manifest from either the reported checkout or the runner build used to fill `package_version`. The digest in `binary.json` identifies a supplied executable. Case requirement and acceptance IDs identify related criteria, including criteria with later release gates.

| Command | Sprint 01 evidence |
| --- | --- |
| `cargo xtask test unit` | Runner checks, portable domain and clock checks, and diff fixture checks |
| `cargo xtask test integration` | Isolated simulator and platform contract checks |
| `cargo xtask test ui` | In-process GPUI view tests |
| `cargo xtask test e2e --suite smoke` | External driver for the separate feasibility process on Windows and X11 |
| `cargo xtask test e2e --suite full` | Native smoke plus the legacy Windows shell and tray case and the registered toast case on Windows |
| `cargo xtask test install --artifact <path>` | Inspect the candidate digest and size. Installer verification remains blocked until packaging. |
| `cargo xtask test live --repo OWNER/REPO` | Validate the requested scope without connecting to GitHub. Sprint 01 has no live adapter. |

Use `--case E2E-001` or `--case UT-021` to select a case and `--seed 12345` to replay fixture input. Known cases assigned to later sprints return Blocked. Internal Sprint 01 results use the `S01-` prefix. Native commands build with the `e2e` feature and release optimization. A matching `--binary <path>` lets you exercise an existing instrumented executable and records it as an external override. Review the binary identity alongside each result.

The live command checks exact matches for `THREADLINE_LIVE_READ_REPO` and the requested repository, plus a named `THREADLINE_LIVE_PROFILE`. The write option also requires `THREADLINE_LIVE_WRITE_REPO` to match. These values cannot grant authorization to an unattended run. Release-phase live testing requires a separately authorized disposable scope and a verified credential profile. The Sprint 01 runner retrieves neither credentials nor remote content.

`cargo xtask test live --case LIVE-002 --repo joshmarkovic/threadline --allow-live-writes --credential-stdin` requires the authorized issue fixture, `THREADLINE_LIVE_WRITE_REPO=joshmarkovic/threadline`, `THREADLINE_LIVE_EXPECTED_USER_ID=52184130`, and `THREADLINE_LIVE_PROFILE=threadline-disposable-issue-1`. Supply the authorized credential over a private pipe. `--allow-existing-cli-oauth` records that separate credential class. The run makes one bounded edit of issue #1, creates and edits one synthetic comment, and restores the original title only while its own change is still current. The comment remains as evidence. The journal under `artifacts/verification/live-write-journal` prevents rerunning a prior or interrupted attempt. Review that evidence before any further writes to the fixture.

`cargo xtask test live --case LIVE-003 --repo joshmarkovic/threadline --allow-live-writes --credential-stdin` uses the same repository and actor settings with `THREADLINE_LIVE_PROFILE=threadline-disposable-pr-2`. Use only with approval for the fixture in PR #2. It verifies the pinned head, file blob and RIGHT line 5, submits one comment-only review, and reads back its exact review and inline comment IDs. Use `--allow-existing-cli-oauth` only with authorized CLI OAuth over a private pipe. After a confirmed attempt, the journal blocks another run. To verify an interrupted attempt, replace `--allow-live-writes` with `--readback-only`. This requires the matching pending-review and submit journals, and issues only reads for the acknowledged review and its comment. Approval and change-request coverage are recorded separately.

`cargo xtask test live --case LIVE-004 --repo joshmarkovic/threadline --allow-live-writes --credential-stdin` uses the approved merge fixture with `THREADLINE_LIVE_PROFILE=threadline-disposable-pr-3` and the same repository and actor settings. It sends one direct merge-commit request for the expected head of PR #3 after verifying the exact branch and current GitHub policy. The durable journal prevents replay. For an acknowledged merge whose readback was interrupted, replace `--allow-live-writes` with `--readback-only`. An uncertain dispatch without a recorded acknowledgement requires separate investigation.

## Windows native input

Run on an unlocked Windows 11 x64 desktop. Windows PowerShell 5.1 drives Win32 keyboard, pointer, window, and clipboard APIs. Reserve the desktop for the test while it runs. The driver snapshots the clipboard in memory and restores it if the clipboard still contains the last synthetic copy. It never writes clipboard contents to a report.

```powershell
cargo xtask test e2e --suite smoke --case E2E-001 --seed 12345
cargo xtask test e2e --suite full --case S01-WINDOWS-SHELL
cargo xtask test e2e --suite full --case S01-WINDOWS-TOAST
```

The smoke test checks the actual clipboard, pointer-driven splitters, draft input, focus, tab and context selection, background preview, forwarding into the existing instance, quit, and relaunch. Review the PNG window captures and `native.json` with the build digest. The GPUI observer supplies current bounds and the results of the externally injected interactions. Capture a separate run on a 125% or 150% display to verify high-DPI behavior. The driver records the actual DPI and does not modify display settings.

`S01-WINDOWS-TOAST` uses a unique Start Menu shortcut and COM registration under the current user. It submits a fixture from the running app, opens Windows Notification Center, activates the matching action in the signed shell, and verifies that one COM activation opens PR #72 in the original process. The case checks removal of its notification history, shortcut, and COM entry before reporting Pass in `toast-native.json`. The Windows full suite includes this case and the separate legacy shell case. Smoke does not create the toast fixture. Run `--case S01-WINDOWS-TOAST` when you need just the registered path.

`cargo xtask test e2e --suite full --case E2E-002` uses Windows input against a fixture account and a temporary loopback GitHub API. It checks partial discovery, SSO, denied and revoked access, and the saved repository selection after restart. The account test uses a synthetic credential in memory. The endpoint trace includes only route labels, status codes and authorization matches.

`cargo xtask test e2e --suite full --case E2E-005` opens two pull requests from that local simulator in independent native tabs. The simulator holds a verified linked-issue response while the second tab becomes active. The case checks that each tab keeps its own comparison and that the first linked issue appears only in its original tab.

Run `cargo xtask test e2e --suite full --case S06-MIXED-CATCHUP` on Windows to test a restart with a partial issue discussion, completed PR review and check records, and a saved commit-status ETag. The synthetic provider changes the issue, an old comment, the existing review and check while the app is closed. The reopened reader first shows the saved profile offline. The trace checks the five-minute overlap against the recorded comment checkpoint, page two of the discussion and the conditional HTTP 304. The fixture uses a 20-second refresh interval for its isolated account.

`cargo xtask test integration --case IT-020` exercises Windows Credential Manager and the native application paths separately. It uses fresh fixture entries, verifies the renewed value from a second process and checks removal.

When source and a test-build executable are already local to Windows, use the direct driver. Supply a new evidence directory for each invocation. Use a disposable profile or provide the driver a test-owned temporary directory. The canonical Rust command automatically removes the temporary profiles it creates.

```powershell
$binary = 'C:\threadline\target\release\threadline.exe'
$evidence = Join-Path ([IO.Path]::GetTempPath()) ('threadline-shell-' + [Guid]::NewGuid())
powershell.exe -NoProfile -Sta -File .\tools\native\windows-smoke.ps1 -Binary $binary -ReportDir $evidence -PlatformProof -ManualTrayClick -ManualNotificationClick
```

For the manual shell checkpoints, open the notification area overflow if needed and click the Threadline icon after the driver closes the window. When the driver requests a notification click, click the visible **Threadline review** banner. The driver observes the Windows shell display callback, checks that the click selects PR #72 in the same process, then disables the tray and verifies that close exits. Capture a redacted screenshot of the actual banner and tray icon where permitted. If Windows suppresses the banner, record the active session and notification setting. Report an unavailable or unobserved click as Blocked.

### Hosted Windows development build

Download `windows-reference-test-build` from the trusted private Windows GUI workflow run. From the unpacked directory, verify the run ID, checked-out commit, and executable digest before launching it. Take the expected run ID and commit from the workflow run.

```powershell
$expectedRun = '<workflow run ID>'
$expectedCommit = '<full commit SHA>'
$build = Get-Content -LiteralPath .\build.json -Raw | ConvertFrom-Json
if ([string]$build.workflow_run -cne $expectedRun -or $build.source_revision -cne $expectedCommit) {
    throw 'The build manifest does not match the selected workflow run.'
}
$binary = (Resolve-Path -LiteralPath .\threadline.exe).Path
$sha = [Security.Cryptography.SHA256]::Create()
try {
    $stream = [IO.File]::OpenRead($binary)
    try { $digest = [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
    finally { $stream.Dispose() }
} finally { $sha.Dispose() }
if ($digest -cne $build.executable_sha256) { throw 'The executable digest does not match the build manifest.' }
$driver = (Resolve-Path -LiteralPath .\windows-smoke.ps1).Path
```

If RemoteSigned or another local policy blocks the downloaded script, use the driver from a trusted local checkout of the same commit when policy permits it.

```powershell
$checkout = 'C:\src\threadline'
if ((git -C $checkout rev-parse HEAD) -cne $expectedCommit) { throw 'Use the build manifest commit.' }
git -C $checkout diff --quiet HEAD -- tools/native
if ($LASTEXITCODE -ne 0) { throw 'Review the checked-out driver files.' }
$driver = Join-Path $checkout 'tools\native\windows-smoke.ps1'
```

Run the selected driver on an unlocked Windows 11 x64 desktop.

```powershell
$evidence = Join-Path ([IO.Path]::GetTempPath()) ('threadline-reference-' + [Guid]::NewGuid())
powershell.exe -NoProfile -Sta -File $driver -Binary $binary -ReportDir $evidence
```

This artifact is for development tests. Keep the completed report with `build.json`. The included `windows-budgets.json` supplies the budget for a separate run with `-Measure -BudgetFile .\windows-budgets.json`.

Use `-ToastProof` with a fresh evidence directory to run just the registered case from a verified development build that includes the toast helpers.

To replay a development build on hosted Windows Server x64, dispatch **Hosted Windows desktop replay** from `main` and select the ID of a successful **Build and test** push run on `main`. Download `hosted-windows-server-replay` for the original measurement and shell reports. Keep this hosted evidence separate from the Windows 11 reference desktop results. Package measurement still requires a candidate installer.

## Windows measurements

For sidebar badges, build the test-only compositor helper on Windows with `cargo build --locked -p xtask --example sidebar_compositor`. Set `THREADLINE_E2E_SIDEBAR_COMPOSITOR` to its absolute executable path and `THREADLINE_E2E_SIDEBAR_COMPOSITOR_SHA256` to the independently recorded build digest. Run `tools/native/windows-smoke.ps1 -ReaderProof SidebarBadges -Binary <E2E executable> -ReportDir <fresh directory>`. The driver first tries `PrintWindow`. If that image has no badge color, it verifies the fixture window owner and DPI, then uses Windows Graphics Capture on that window. Only color counts, bounds and digit offsets for the three accessible controls enter the receipt. The window capture API requires Windows 10 version 1903 or later. [Microsoft window capture API](https://learn.microsoft.com/windows/win32/api/windows.graphics.capture.interop/nf-windows-graphics-capture-interop-igraphicscaptureiteminterop-createforwindow)

```powershell
cargo xtask measure windows --seed 12345
cargo xtask measure windows --seed 12345 --artifact '<test-package-path>'
```

The driver measures a fresh app process with a fresh isolated profile. It does not flush the operating system disk cache. Record whether it is the first run after boot when comparing cold results. It records elapsed time to the ready fixture window, idle and peak working sets, GPU process-memory samples where Windows exposes them, and the executable's digest and byte size. Separate native wheel passes measure the discussion and large diff. The larger p95 is compared with the scrolling budget. Package size requires an actual candidate artifact. The result records its digest separately from the executable digest. Missing metrics are Blocked and measurements beyond the checked-in limits are Fail.

Run `cargo xtask test e2e --suite full --case S12-HISTORICAL-READER --binary <e2e-binary>` on Windows to browse a closed 2018 issue from a fresh profile. The native controls first search downloaded content, then search the selected repository on the loopback provider and refresh the exact issue. After Work Offline and a process restart, local body search must reopen that issue. The trace checker rejects a bulk archive request, another search or any offline traffic.

Run `cargo xtask test e2e --suite full --case S12-READER-CONCURRENT --binary <optimized-e2e-binary>` to repeat the twenty cached opens, searches, and tab switches with scheduled reads in flight. The isolated Section 16 fixture keeps queued writes paused. Each accepted input must reach its first useful paint while a different verified loopback feed request is still waiting for a response. The report retains completed samples if a native input or a matching request becomes unavailable. Use the same optimized binary, desktop, and input source for the offline comparison.

Pass `--artifact <setup-path>` with `cargo xtask test e2e --suite full` to record the candidate setup size and digest in E2E-014. The named `--case E2E-014` accepts the same option. Supply `--local-fixture <configuration.json>` separately for installed-package cases.

## Windows restart and interrupted writes

Run `cargo xtask test e2e --suite full --case S08-COMMENT-DEPENDENCY --binary <e2e-binary>` to check a queued ordinary comment and its dependent edit. The loopback provider paces writes after accepting the first comment. The driver quits after observing its confirmation and lets the new process apply the dependent edit to the returned comment ID. A second chain loses its create response and keeps that edit pending. The test requires the exact network sequence and one cached comment.

Run `cargo xtask test e2e --suite full --case S07-ISSUE-CONFLICT --binary <e2e-binary>` to queue an existing issue edit offline, restart, and reconnect after the isolated provider changes its title and description. The owned Outbox and editor show the original, local and current remote values for the same issue. The loopback trace requires an exact issue preflight and zero mutations.

Run `cargo xtask test e2e --suite full --case S12-NATIVE-LIFECYCLE --binary <optimized-e2e-binary> --seed 12345` from a Windows user session with Credential Manager available. The case uses a new temporary private workspace, the loopback provider and a unique synthetic protected credential. It uses the normal account, synchronization and Outbox controls. The runner terminates only the app process it launched, after the fixture received the exact known comment and while its response is held.

Run `cargo xtask test e2e --suite full --case S12-OFFLINE-MIXED-OUTBOX --binary <e2e-binary>` for a separate offline cache cleanup check with the same isolated fixture. The case clears unpinned content through Storage usage, inspects the four saved payloads, and cancels their common prerequisite while keeping the three dependent entries readable. It compares the closed profile with its seeded state, verifies cleanup of the synthetic credential, and requires an empty loopback trace.

Run `cargo xtask test e2e --suite full --case S12-PANE-LIFECYCLE --binary <e2e-binary>` to drive the saved two-repository fixture through Open beside, Maximize and restore, Tab left, Move tab, Close and Reopen tab, and Close pane. It restarts the same isolated profile offline and compares the saved tab order, cached titles, selected comment, and unsent draft. The loopback trace must be empty.

Allow about three minutes for the normal dispatch lease to expire. The driver checks the restored workspace before an authenticated startup response and compares the closed database before and after interruption and normal sign-out. It records one uncertain request, retained dependents and draft, and the unchanged prior delivery receipt. It removes only its synthetic Credential Manager entry. The prior delivery is seed data. Review installed notification delivery and the separate disk-fault checks in the sprint reports.

## Early Linux and macOS checks

The Linux driver needs a real X11 session, xdotool, xclip, ImageMagick with PNG support, jq, ripgrep, and GNU timeout. It uses the existing X11 clipboard and can preserve plain text. Use a disposable desktop if the clipboard contains other formats. WSLg results describe the WSLg/XWayland configuration. Run Ubuntu GNOME Wayland and X11 separately during Linux release verification.

The development matrix builds the app and runs available portable tests on native Windows, macOS, and Linux runners. Hosted Windows runs and Windows 11 reference desktop evidence have separate jobs. The supported reference desktop must register the `threadline-interactive` self-hosted runner label to run its optional dispatch job.

The hosted macOS 15 arm64 build job probes its existing debug executable and uploads `macos-window-probe.json`. The probe records Quartz session, display and Metal availability. It looks for an on-screen window owned by the launched process and requests AppKit Quit after verifying the executable. A blocked probe is reported separately from the build and core tests. This check covers launch, window discovery and Quit. Keyboard, clipboard, splitters, and other E2E-001 interactions still need a native input driver or separately recorded desktop evidence.

Run the same probe from an Apple Silicon macOS 15 desktop after building the app.

```bash
mkdir -p artifacts
THREADLINE_PROBE_COMMIT=$(git rev-parse HEAD) xcrun swift tools/native/macos-window-probe.swift \
  target/debug/threadline artifacts/macos-window-probe.json
```

### Installed runtime inventory

For a direct activation integration check, run `tools/windows/test-installed-activity.ps1` from 64-bit Windows PowerShell with the local fixture installed, its independently recorded `-AppSha256`, `-FixtureTool`, `-FixtureSha256`, and a fresh `-ReportDir`. Select one `-CallbackProbe` value per run: `ColdOpen`, `WarmOpen`, `MarkRead`, or `Snooze`. Each probe uses a fresh owned profile, reads the exact action from its Windows notification history, and invokes the registered local COM server. The receipt identifies the activation mechanism, process and retained database state. E2E-012 exercises the separate shell-click path.

Run `cargo xtask test e2e --suite full --case E2E-020 --local-fixture <configuration.json>` in a test-owned Windows account with the local package installed and the default Threadline Data directory absent. Build the ordinary app without the E2E feature. Build the separate `notification_fixture` storage example for Windows. Supply their independently recorded SHA-256 values with the same `app_sha256`, `fixture_tool` and `fixture_sha256` keys as the installed activity cases.

Add a `runtime_inventory` object with `app_source_revision` (full app build commit), `dependency_tree` (path relative to the configuration) and `dependency_sha256` (digest of those exact bytes). Produce the inventory from that app checkout with `cargo tree --locked -p threadline-app --target x86_64-pc-windows-msvc --edges normal --prefix none`. Use UTF-8 or plain ASCII. The runner checks its digest, parses regular and delayed imports from the installed executable, and records browser matches. The desktop driver opens an offline Markdown attachment, chooses a cached PR file and switches the native diff to split mode. It samples the installed process and its current descendants at three points. Only browser matches, counts and the exact tested artifact identities enter the receipt. The local test package has a separate identity from a published release.

To verify grouped review delivery on an installed local package, run the same Windows PowerShell driver with `-Case S11-GROUPED-REVIEW`, the independently pinned app and notification fixture helper, and a new report directory. The fixture commits a synthetic submitted human review and inline comment through Threadline’s PR page ingestion. It pauses background reads and queued writes and gives the two test processes a loopback-only proxy. The driver verifies one two-event card in app-scoped Windows history, closes the app and removes only that card. It replays both source pages and adds a feed-only check. A fresh installed process must publish the check while the retired card stays absent and its durable delivery remains unchanged.

Use `-Case S11-WATCHED-BRANCH` with the same installed identity guards to test a system card for three commits added to a watched fixture branch. The driver checks the registered sender and exact Windows history payload, requires a unique card with that text, and uses the signed Notification Center's default Invoke action. It verifies the full cached comparison endpoints in the original process and the saved branch on disk. Run the case on its own with a connected desktop.

To exercise the installed PR line selection, run `tools/windows/test-installed-runtime.ps1 -Case S06-INSTALLED-SELECTION -AppSha256 <installed-app-sha256> -FixtureTool <notification_fixture.exe> -FixtureSha256 <fixture-sha256> -ReportDir <empty-directory>` in the same test account. Use the ordinary installed app and independently pin the fixture helper built from this driver revision. On a fresh offline comparison, the driver chooses a changed source line, switches to Commits and back to Files, then creates an inline review draft. The `draft_anchor` result covers the visible anchor and its saved comparison, side and line. The `live_highlight` result compares colors from the installed window before and after navigation. If the colors cannot establish selection, the case reports Blocked and retains the independent draft result. A local proxy captures attempted requests without forwarding them. The driver archives the generated profile with its receipt.

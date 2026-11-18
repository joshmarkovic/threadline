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

```powershell
cargo xtask measure windows --seed 12345
cargo xtask measure windows --seed 12345 --artifact '<test-package-path>'
```

The driver measures a fresh app process with a fresh isolated profile. It does not flush the operating system disk cache. Record whether it is the first run after boot when comparing cold results. It records elapsed time to the ready fixture window, idle and peak working sets, GPU process-memory samples where Windows exposes them, and the executable's digest and byte size. Separate native wheel passes measure the discussion and large diff. The larger p95 is compared with the scrolling budget. Package size requires an actual candidate artifact. The result records its digest separately from the executable digest. Missing metrics are Blocked and measurements beyond the checked-in limits are Fail.

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

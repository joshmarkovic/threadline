# Simple installation and upgrades

Version 1.7 companion specification

## Purpose and release scope

The finished application must offer a copyable installation path of one, two, or three commands for every published platform. The commands install prebuilt release artifacts for the current user, with no Rust compiler, source checkout, Visual Studio, Xcode, or other development toolchain required on the user's computer. Declared operating-system runtime prerequisites and ordinary OS permission prompts still apply.

Windows is the first release target in Sprint 12. macOS and Linux receive their corresponding installation and upgrade gates in Sprints 13 and 14. A future platform must never be advertised as available merely because a shared bootstrap script recognizes its name.

This document specifies implementation deliverables under INSTALL-01 through INSTALL-08. The repository, product name, publisher identity, release URLs, signing credentials, and release binaries have not been supplied. The command examples below are templates for the coding LLM. They are not working public installation commands, and this requirements package does not contain a built application or a tested installer.

## User experience

The published getting-started page must identify the supported OS and architecture, show no more than three shell command lines for the normal path, and state the expected result. Installation should produce a launchable application, a normal desktop/start-menu application entry where supported, and an uninstall path. Display the installed version and destination when complete.

Use a public HTTPS release channel as the distribution default until the owner supplies the actual destination. Public installation must not ask for the application's GitHub PAT or organization SSO login. If private artifact distribution is later chosen, define its separate download authorization flow before publication; never silently reuse the application's credential.

The installer must not enroll users in startup-at-login, change repository selection, sign them out, submit queued GitHub actions, or reset offline and pause settings. Application launch after installation is permitted only through the documented install behavior, and startup networking must honor the existing persisted controls.

## Proposed package choices

| Platform and milestone | Default installed artifact | Installation scope |
| --- | --- | --- |
| Windows 11 x64, Sprint 12 | Signed native setup executable, with an ordinary user installer such as Inno Setup configured for non-administrative installation | Current user's application directory and Start menu; no elevation by default |
| macOS 15 or later on Apple Silicon, Sprint 13 | Signed and notarized application bundle, distributed as a verified archive or DMG | A documented writable user application location; no request to disable Gatekeeper |
| Ubuntu 24.04 x64, Sprint 14 | Verified binary-and-resources archive with a user installation script and desktop entry | Appropriate user data/application locations and an optional user executable launcher |

These are implementation defaults, not evidence of current support. The app owns packaging, installed identity, external resources, and release verification; GPUI Kit does not produce a complete installer automatically. [I1]

Optional package-manager routes such as WinGet, Homebrew, or a distribution package may shorten installation further after their actual package identities are published. They are not prerequisites for the first Windows release and must not be presented as existing packages before publication.

## Windows quick installation

The default Windows path must work from ordinary Windows PowerShell on the supported machine without executing a downloaded `.ps1` under a permissive policy. Prefer downloading and launching the signed native installer from inline PowerShell commands. `Restricted` permits individual commands while preventing script-file execution, so a downloaded script must not be the only route. Do not use `Set-ExecutionPolicy`, `-ExecutionPolicy Bypass`, or `Unblock-File` in the quick-install instructions. Organization-managed restrictions must be respected. [I2]

The following three-line template illustrates the required result. Replace every angle-bracket placeholder with published values and validate the exact command on a clean standard-user Windows machine before shipping it. The enclosing block makes it one command spanning three lines; an error stops the block before the installer can run.

```powershell
& { $ErrorActionPreference = 'Stop'; $installFile = Join-Path ([IO.Path]::GetTempPath()) ('github-client-setup-' + [guid]::NewGuid() + '.exe'); Invoke-WebRequest -UseBasicParsing -Uri '<PUBLISHED_WINDOWS_SETUP_HTTPS_URL>' -OutFile $installFile -ErrorAction Stop
$installSignature = Get-AuthenticodeSignature -LiteralPath $installFile; if ($installSignature.Status -ne 'Valid' -or $installSignature.SignerCertificate.Subject -ne '<EXPECTED_PUBLISHER_CERTIFICATE_SUBJECT>') { throw 'Installer publisher verification failed.' }
$installProcess = Start-Process -FilePath $installFile -ArgumentList '/SP-','/SILENT','/NORESTART','/NOCLOSEAPPLICATIONS' -Wait -PassThru; if ($installProcess.ExitCode -ne 0) { throw ('Installation did not complete. Exit code: ' + $installProcess.ExitCode) } }
```

The installer must be configured for current-user installation; the switches alone must not be relied on to change a privileged package into a user package. If Inno Setup is used, configure `PrivilegesRequired=lowest`, retain its supported exit-code behavior, and translate non-success codes into readable recovery instructions. The sample flags do not permit a forced process close or an automatic system restart. [I3] [I4]

Authenticode verification must match the expected project publisher, not merely accept any valid signature. The expected identity comes from trusted release configuration, with an explicit signer-rotation process. Microsoft documents `Get-AuthenticodeSignature` as the Windows API for inspecting the file's signature. [I5]

The three-line example is intentionally a contract template, not the final polished command. The shipping implementation may provide a shorter verified launcher or a published package-manager command, provided it preserves publisher verification, fail-closed execution, and the tested normal-user behavior. A download or verification failure must prevent execution even if the user pastes all lines at once. The final command must be implemented as a fail-closed block or equivalent that cannot accidentally continue after an earlier failure; test the exact published text, not only the installer internals. Use a finally-style cleanup path for the uniquely named downloaded installer after success or failure, removing only that installer-owned temporary file.

Where endpoint protection or organizational policy blocks installation, show the specific block and a permitted installer/manual-download alternative. Do not instruct the user to weaken security settings. Policy restrictions are a visible limitation, not a successful install result.

## macOS and Linux quick installation

Publish a POSIX-compatible shell bootstrap entry point for the supported target. It must download to a private temporary location, verify the selected release, and install without root for the ordinary path. Separate runtime prerequisites from development dependencies; do not copy the framework's developer-package installation list into end-user instructions.

This three-line template expresses the command shape. The chained success conditions are essential: a failed download must not run a partial file.

```sh
install_tmp="$(mktemp)" &&
curl --fail --location --proto '=https' --proto-redir '=https' '<PUBLISHED_INSTALL_SCRIPT_HTTPS_URL>' -o "$install_tmp" &&
sh "$install_tmp" --channel stable
```

The bootstrap URL must be the project's trusted published HTTPS origin. The reviewed bootstrap must verify payload integrity and publisher authenticity using the release trust policy before it extracts, installs, or launches application code. A checksum downloaded alongside an untrusted binary is not sufficient publisher authentication. Provide an inspect-before-run/manual-download route and a documented version-pinned command using the same bootstrap contract.

On macOS, preserve signing and notarization metadata and verify the final installed bundle. Never remove quarantine recursively or disable Gatekeeper to make the test pass. For direct public distribution, implement Apple's documented Developer ID signing and notarization workflow. A local unsigned development bundle is a distinct development artifact and cannot close the public distribution gate. [I6]

On Linux, honor valid XDG locations and keep durable SQLite content, drafts, pending actions, and receipts outside disposable cache and temporary directories. The XDG specification distinguishes persistent user data from cache and runtime locations. [I7] If a required graphics driver, notification service, credential service, or runtime library is missing, report the exact missing prerequisite and supported repair route. Do not silently run `sudo`, modify package sources, or install system packages.

## Installer behavior contract

| Requirement | Required behavior |
| --- | --- |
| Target detection, INSTALL-02 | Identify OS, architecture, release availability, minimum OS, and required runtime capabilities before changing the installation. Never silently install the wrong architecture or a build for an unsupported OS. |
| Version selection, INSTALL-05 | Default to the published stable release for the current target. Support an exact version and an explicit preview channel. Stable installation must not select a prerelease accidentally. Preserve installation identity across updates. |
| Trust and integrity, INSTALL-03 | Validate HTTPS endpoints and redirects, declared target/version, digest, and the applicable publisher signature or signed release metadata. Pin the trust root in reviewed configuration; do not derive trust from the same unverified payload. Reject mismatches before execution. |
| Extraction | Reject path traversal, unsafe archive links, destination escape, unexpected files, and writes through attacker-controlled symlinks. Stage with user-private permissions. |
| First installation, INSTALL-01 | Install prebuilt app/resources and desktop integration with at most three published commands after declared prerequisites. Do not require development tools, an app PAT, or administrative privileges for the default target. |
| Repeated invocation, INSTALL-05 | Installing the same version is harmless and may repair missing app files. Preserve account identity, settings, credentials, data, and launch preferences. Do not create duplicate shortcuts or installs. |
| Upgrade, INSTALL-04 | Fully download and validate the new app before replacing the working installation. Coordinate an orderly app exit and local-state flush, or stop with a clear request to close it. Never force-kill an app with active writes or silently dispatch Outbox actions. |
| Durable-data migration | Keep data separate from application files. Protect the old database through a valid SQLite backup/snapshot strategy before irreversible migration. A live database file copy without WAL handling is not a backup. Migration failures must preserve recoverable data and identify the supported recovery route. |
| Downgrade, INSTALL-05 | Require an explicit version choice and check database compatibility. Refuse an incompatible binary/schema combination rather than delete or reset user data. A binary rollback must not imply that every database migration can be reversed. |
| Interruption and repair | A cancelled transfer, full disk, lost connection, or failed extraction must leave the existing usable install and durable data intact. Clean or recover only installer-owned staging files. Re-running must resume or restart safely and report what happened. |
| Failure reporting, INSTALL-06 | Return nonzero on failure and distinguish unsupported target, unavailable release, trust failure, policy block, prerequisite failure, disk/write failure, app-busy state, and migration failure. Logs must be useful without exposing tokens or private repository content. |
| Uninstall, INSTALL-07 | Remove application files and registered app integration. Preserve local data, drafts, pending actions, receipts, and settings by default. Protected credentials follow the explicit sign-out/removal choice; never erase them incidentally during upgrade or repair. A separate clearly labeled data/credential removal path must explain unresolved work before deletion. |

Installation is an online action unless the user already has a verified offline package. A previously installed app must remain usable offline when an update check or download fails. Silent background auto-update is not required merely because a short install command exists; the required baseline is a reliable manual install/re-run upgrade path.

## Required implementation artifacts

By Sprint 12, provide the actual Windows package, publisher and package identity configuration, a machine-readable release manifest, the tested quick-install commands, a normal manual download option, upgrade/uninstall instructions, and an installer test report. Add equivalent target artifacts in Sprints 13 and 14. The release manifest records version, channel, target triple, minimum supported OS, artifact URL, size, digest, signing identity or signed-metadata reference, and schema compatibility information.

Source-control the packaging recipes and bootstrap source. Release jobs must use the same pinned application dependencies and version source as the app build. Production signing credentials belong in the release environment, never the source tree or this requirements ZIP. Missing publisher/repository configuration blocks public publication only; it does not block local packaging implementation and fixture-based tests.

## Required verification

These checks implement INSTALL-08 and AC-94 through AC-99, with Windows ownership in Sprint 12 and target-specific reruns in Sprints 13 and 14. Keep all application test statuses as Not run until real evidence exists.

1. Test the exact one-to-three-line published entry point on a clean supported standard-user machine with no Rust or developer tools installed. Verify launcher creation and first app launch.
2. Exercise unsupported OS/architecture, no available build, missing prerequisites, download interruption, bad digest/signature/publisher, hostile archive paths, unwritable destination, and full disk. No failure may launch an unverified payload or erase an existing installation.
3. Install version N, seed cached issues/PRs, open tabs, drafts, pending/unknown Outbox actions, settings, and protected credentials, then install N+1. Launch offline and verify every retained item and action identity before permitting networking.
4. Repeat installation and repair. Confirm one installation identity, retained preferences, no duplicate shortcuts, and no unexpected login or cache reset.
5. Exercise orderly-close coordination, app-busy refusal, crash/interruption at staging and migration boundaries, and incompatible downgrade refusal. Recovery must not repeat previously confirmed GitHub mutations.
6. Uninstall with default retention, reinstall, and recover the local workspace. Separately verify explicitly requested data and credential removal in a disposable profile.
7. For public releases, verify the exact downloaded signed/notarized artifact and then drive the installed application through the native E2E suite. A source-tree binary test cannot substitute for this release test.

Record OS/build, CPU architecture, desktop/display environment where relevant, user privilege level, shell version, app version/hash, exact install commands, exit codes, logs, and test outcomes. Mark a blocked platform gate as blocked with its cause, never passed based on a different OS.

## Sources

- [I1 GPUI Kit packaging](https://gpui-kit.com/docs/packaging/)
- [I2 Microsoft PowerShell execution policies](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_execution_policies)
- [I3 Inno Setup command-line parameters](https://jrsoftware.org/inno-setup/documentation/topic_setupcmdline.htm)
- [I4 Inno Setup privilege requirements](https://jrsoftware.org/inno-setup/documentation/topic_setup_privilegesrequired.htm)
- [I5 Microsoft Get AuthenticodeSignature](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.security/get-authenticodesignature)
- [I6 Apple notarization](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
- [I7 XDG base directories](https://specifications.freedesktop.org/basedir/latest/)

# Sprint 12 Windows installer development

The Windows 11 x64 local installer passed ten setup and recovery checks on build 22631. An installed predecessor and candidate also opened the same offline workspace across a SQLite schema upgrade. The user selected local installer testing and left publication pending. The exact published-package checks remain open. [Setup outcomes and package digests](evidence/sprint-12/installer-lifecycle.json) · [Windows installation plan](../spec/INSTALLATION_PLAN.md)

| Windows test | Observed result |
| --- | --- |
| Per-user install and repair | Predecessor installation and repair both exited 0. The local fixture created a matching shortcut and registration. The current build also repaired successfully. |
| Interrupted update and retry | The canonical matrix held the executable open and received exit 14. A setup built with a staged-write fault returned 16. The previous installation survived both checks. A subsequent upgrade exited 0. |
| Downgrade and uninstall | The older setup returned 12 both before and after uninstall. Uninstall removed the executable and registration, retained the preseeded profile, and allowed the current package to reinstall. |
| Local trust fixture | The verifier accepted a fixture-signed manifest and its matching setup digest. It rejected a different publisher pin and a same-length modified setup before either could execute. Production signing and independent publisher trust remain pending. |

The canonical matrix ran in 64-bit Windows PowerShell using `tools/windows/test-local-lifecycle.ps1` with all three setup paths, their expected SHA-256 values and the preseeded profile digest. The independently recorded output contains the package hashes and the exit code for each step. The installer and harness are recorded at source `95f9066`. [Lifecycle driver](../../tools/windows/test-local-lifecycle.ps1) · [Local build and recovery commands](../../tools/windows/README.md) · [Trust and native app receipts](evidence/sprint-12/local-package.json)

## Installed application and retained state

The offline fixture held 30 items, 90 comments, 20 tabs, one draft, ten Outbox operations and one step receipt. Opening the installed candidate from `1ba4fff` after the installed predecessor from `8d93bc1` moved the database from schema 9 to 10. Nine ordered projections matched, including those for queue payloads, tab state, cached resources and local credential metadata. There were zero synchronization preference rows. The receipt gives the selected SQL columns and both hashes for every projection. [Database comparison](evidence/sprint-12/retained-profile.json)

The installed executable had SHA-256 `783bc377629896a4f5102ee5c50b94166cb4d214b00cfa28c933c79fb351381a`. The native smoke driver verified the executable owning the window, invoked an initially unselected cached tab through Windows UI Automation, observed its activation and exited Threadline normally. A later uninstall retained the database byte for byte. [Installed smoke driver](../../tools/windows/smoke-installed.ps1) · [Installed app and uninstall results](evidence/sprint-12/local-package.json)

Local build 5 from `725632e` upgraded the same offline workspace. The installer and registration matched the expected binary. On the first Windows UI Automation invocation, the installed process selected issue 6 in the second pane and exited normally. The closed database reported schema 12. Eight nonempty ordered projections and the empty synchronization preferences projection still matched the schema 9 and 10 digests. The application produced migration backups for schemas 9 and 10. [Build 5 install, navigation and retained projections](evidence/sprint-12/installed-build-5.json)

Windows PowerShell 5.1 also ran the release verifier against the exact local matrix setup with its correct size and digest and a fixture publisher pin. It exited 1 because Windows could not confirm a trusted Authenticode signature. [Release verifier](../../tools/windows/verify-release-installer.ps1) · [Verification receipt](evidence/sprint-12/installed-build-5.json)

## Windows release gates

PKG-001 through PKG-006 and PKG-009 still require applicable release evidence. Outstanding checks include the published one-to-three-command path on a clean supported Windows user, release signature and independent trust, the exact shipping artifact's offline smoke, and wrong-architecture rejection. The local channel uses its own identity and seeded profile. The full Windows lifecycle, native E2E and release acceptance matrix remains open. macOS and Linux packaging have separate release gates. [Windows package scenarios](../spec/test_platform_traceability.csv) · [Acceptance criteria](../spec/acceptance_platform_traceability.csv) · [Sprint requirements](../spec/sprints/sprint_12_release_verification.md)

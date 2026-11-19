# Windows installer development

The local fixture installs a prebuilt x64 Threadline executable for the current Windows 11 user. It uses `%LOCALAPPDATA%\Programs\Threadline Installer Test`, a single current-user Start menu shortcut, and the `Threadline.InstallTest` entry under `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall`. The installer does not start Threadline or enroll it in startup. Install and uninstall leave `%LOCALAPPDATA%\Threadline` and Windows Credential Manager alone.

Build on Linux with the pinned NSIS 3.11 compiler and its matching `NSISDIR` data tree. Supply the application executable, setup output, local package version, monotonically increasing build ordinal, and the verified application schema version. Use `unknown` for a packaging-only executable whose schema cannot be proved.

```sh
export MAKENSIS=/path/to/pinned/nsis/usr/bin/makensis
export NSISDIR=/path/to/pinned/nsis/usr/share/nsis
tools/windows/build-installer.sh --fixture /path/to/threadline.exe /output/Threadline-Local-2.exe 0.1.0-local.2 2 10
```

The example uses local build ordinal 2 and schema 10. For schema 14 and later, build the notification registration helper in 64-bit Windows PowerShell. Place it beside the app or set `THREADLINE_NOTIFICATION_HELPER` to its path before running the package builder. For release staging, record the build output digest as `notification_helper_sha256` in the reviewed signer configuration. Setup embeds the compiled helper. It registers the current user's Start shortcut and activation command after placing the app in its final directory.

```powershell
.\tools\windows\build-notification-helper.ps1 -Output .\Threadline.NotificationRegistration.exe
```

Use the actual version and schema from the chosen source revision. The sibling `.exe.json` records the setup and exact installed executable digests. Verify the former against the separately recorded digest from the build before running the local fixture. Launch setup normally or with `/S` and wait for completion. Start the app through **Threadline Installer Test** in the Start menu. Use the Windows installed-apps entry or `Uninstall.exe` in the fixture directory to uninstall. The fixture retains `HKCU\Software\Threadline.InstallTest` build history to reject older test packages after uninstall.

### Remove a retained local workspace

Ordinary uninstall preserves your data and credentials. To remove them, quit Threadline and run the separate Windows bootstrap from a terminal. These commands work offline, including after uninstall.

```powershell
.\threadline-installer.exe --preview-local-removal
.\threadline-installer.exe --remove-local-data-and-credentials
```

The preview counts accounts, drafts, unresolved Outbox operations, managed SQLite files and recorded credential references in this Windows user's default workspace. It reports rows in migration backups separately. The removal command repeats the preview and asks you to type `REMOVE LOCAL WORKSPACE`. Confirming erases the listed databases and backups, including cached content, preferences, layouts, drafts and pending work for every local account. It also removes their recorded Windows Credential Manager entries. A request already sent to GitHub may still complete. User-named exports and other files stay in place.

Run the canonical local case from a native Windows checkout with the fixture uninstalled.

```powershell
cargo xtask test install --case S12-LOCAL-PACKAGE --artifact C:\fixtures\Threadline-Current.exe --local-fixture C:\fixtures\approved-local-packages.json
```

The independent configuration contains `current_sha256`, `predecessor` and `stage_failure` objects with `path` and `sha256`, and an optional `profile_sha256` for a closed synthetic database. Relative package paths resolve beside that configuration. Record the SHA-256 values from the approved builds before handing off the packages. Each setup needs its generated `.exe.json` sibling. Use build ordinals at or above any retained fixture history. The canonical receipt records the exact native fixture refusal and recovery steps. Published installation and shipping-package smoke retain their own scenario results.

| Setup exit code | Meaning and recovery |
| --- | --- |
| 0 | Installed, repaired, or uninstalled successfully |
| 11 | Windows 11 x64 is required |
| 12 | An older package was refused. Run the same or a newer package |
| 13 | Another setup or uninstall is active. Wait for it to finish and rerun |
| 14 | Threadline or another process holds application files open. Quit or release them and rerun |
| 15 | The staged executable did not match its expected digest. Obtain an intact setup |
| 16 | Setup could not write staged files or registrations. Check space and permissions, then rerun |
| 17 | The managed location or a staging file has unexpected ownership or a link. Restore the ordinary per-user path |
| 18 | The Windows PowerShell prerequisite or verification failed. Review the local policy or permission error |
| 19 | Setup kept staging files after a rollback was interrupted. Release open files and rerun the same setup |
| 20 | Windows could not verify the approved publisher. Obtain the trusted signed package |
| 21–22 | Windows could not verify the staged shortcut or snapshot, or an existing shortcut or sender has a different target. Inspect the named entry and rerun the matching setup |
| 23–24 | Windows denied access to the current user's Start menu, registry or staging folder. Check permissions and organization policy |
| 25–27 | Windows Shell could not save the shortcut (25), staging file access failed (26), or the installed .NET Framework could not run the helper (27). See setup details |

Build an explicit staging failure fixture with `THREADLINE_SETUP_FIXTURE_FAULT=stage-write` before the same build command. It exits before replacing a working installation and marks the injection in its manifest. Test a real open-file refusal by holding the installed fixture executable open without delete sharing. Compare installed hashes and registered version after each failure. For an upgrade against retained state, install an actual predecessor executable built from a source revision with a known schema. Seed the local test profile, close it, install the candidate, and open the installed executable against that same profile. Record the exact payloads and the application’s migration evidence.

Run `test-local-lifecycle.ps1` in 64-bit Windows PowerShell with the actual predecessor and current packages, a `stage-write` fixture with the exact current payload, and independently recorded SHA-256 digests for all three. Supply `-ProfileSha256` for a closed, checkpointed synthetic profile at the native data location. The script exercises repair, an open-file refusal, a staged-write failure, upgrade, downgrade refusal, default uninstall, and reinstall. It leaves the current fixture installed. Run `smoke-installed.ps1` separately to start the exact installed executable without test flags, invoke a known tab through its owned accessibility tree, and close it normally.

### Generated workspace, saved cursor and confirmed removal

Use a dedicated Windows 11 x64 test user with an absent Threadline Data folder and no installed test package. Build the test helper and the normal bootstrap from the intended source revision.

```powershell
cargo build --locked --release -p threadline-installer --example package_retention_fixture --bin threadline-installer
```

Add a `retention` object to the existing local package manifest and omit `profile_sha256`. Point the two executable entries at the reviewed builds and supply independently recorded SHA-256 values. Paths may be relative to the manifest. The optional `preserve_files` array accepts individual pinned files from earlier synthetic archives. The runner verifies their bytes before and after the lifecycle.

```json
{
  "fixture": {"path": "package_retention_fixture.exe", "sha256": "<independent SHA-256>"},
  "bootstrap": {"path": "threadline-installer.exe", "sha256": "<independent SHA-256>"},
  "preserve_files": []
}
```

Run the same `cargo xtask test install --case S12-LOCAL-PACKAGE --artifact <current-setup.exe> --local-fixture <approved-local-packages.json>` command. In this mode the script generates a marked workspace and one temporary Credential Manager entry. It checks the protected profile, queued changes, draft, receipt and comment page cursor throughout the normal ten-step package matrix. It then opens and closes the installed native reader, performs a final ordinary uninstall, and resumes the saved page on a disposable copy without network access. A second generated workspace tests the normal bootstrap preview and confirmation in a Windows pseudoconsole. The helper verifies that an unrelated test credential and a user export survive removal. The generated databases stay in test-owned temporary archives. The runner records the results in `windows-local-lifecycle.json` and `windows-retained-package.json`.

The current fixture seeder writes its own storage schema. Keep the separately versioned predecessor migration test when verifying the shipping package.

`verify-local-signed-manifest.ps1` accepts a local fixture and attached CMS metadata with its own separately recorded signer pin. It checks the signature, publisher pin, exact setup digest and byte size. Developers can create a disposable signer and use `openssl cms -sign -binary -nodetach -outform DER` to exercise a matching fixture, changed bytes and a different signer without changing Windows certificate trust. This local proof leaves the Authenticode release gate open.

## Stage a signed Windows release

Publication remains pending. For a release with approved publisher configuration, build the verifier with the same pinned Rust toolchain and Windows SDK as the app.

```sh
cargo build --locked --release --target x86_64-pc-windows-msvc -p threadline-platform --example verify_windows_publisher
tools/windows/build-installer.sh --release-stage /private/windows-signer.json /build/verify_windows_publisher.exe /build/threadline-signed.exe /output/Threadline-unsigned.exe 0.1.0 1 10
```

The reviewed signer configuration supplies `channel` (`stable` or `preview`), `publisher_display`, `publisher_certificate_sha256` (the approved leaf certificate), `signer_reference` (`CurrentUser/My/` followed by the certificate thumbprint), `publisher_verifier_sha256`, `signed_app_sha256`, `artifact_url` (the exact HTTPS URL), and `oldest_supported_schema`. Record both digests from the approved builds. The Windows release environment signs the application before packaging. Keep certificate keys outside the repository. Change the publisher pin only through the reviewed signer configuration.

Staging writes a setup and a `publication_pending` manifest. The release environment signs the exact staged setup using the configured signer and records the resulting size and SHA-256 as `size` and `sha256` in the trusted release manifest. `verify-release-installer.ps1` requires the verified signed bytes and checks both the supplied digest and the Windows-trusted Authenticode signer before any setup launch. The production setup also checks its own signature and the staged application’s signature with the approved pin and the Windows trust cache. Obtain a verified package or correct the trust configuration if either check fails. Both build modes leave certificate trust and publication to the release environment. [Microsoft WinVerifyTrust documentation](https://learn.microsoft.com/en-us/windows/win32/api/wintrust/nf-wintrust-winverifytrust)

## Select and verify the release package

The native `threadline-installer` bootstrap uses the exact version and channel requested by the user. Release engineering sets `THREADLINE_RELEASE_DIRECTORY` to the reviewed HTTPS directory ending in `/` and `THREADLINE_RELEASE_PUBLISHER_SHA256` to the approved Windows publisher leaf fingerprint when building it. A development build without these settings reports that publication is pending. The bootstrap and setup require Windows 11 x64, an interactive desktop and Direct3D feature level 10.1 or above.

After signing the staged setup, run `finalize-release-metadata.ps1` in Windows PowerShell with the signed installer, staged metadata, their independently recorded digests, the approved publisher pin and a new output directory. The script verifies the signed setup with Windows and writes the public metadata fields to `threadline-windows-x64.json`. It prints the digest of those exact manifest bytes. Distribute that digest through the trusted release announcement. The setup and manifest belong in the same versioned directory, `v<VERSION>/`. Sign and distribute the bootstrap under the approved publisher as well.

The following PowerShell command is a template for a future release. It assumes the user has obtained the publisher-verified, prebuilt bootstrap and trusted manifest digest. No public release endpoint or signed Threadline setup is available yet.

```powershell
.\threadline-installer.exe --version <VERSION> --channel stable --manifest-sha256 <TRUSTED_MANIFEST_SHA256>
```

Use the same command with a newer exact version and manifest digest to update. Add `--verify-only` to check the Windows target, retained build and database compatibility, byte length, digest and Authenticode without starting setup. Verification can run in a disconnected session. Installation checks the interactive desktop and Direct3D before downloading and again before launching setup. For an offline or private download, place the verified release manifest and its named setup file together, then add `--offline-dir <DIRECTORY>`. The bootstrap reads the same independent trust inputs in either mode. A separately authorized manual download can supply a private release. The app's personal access token belongs only in Accounts.

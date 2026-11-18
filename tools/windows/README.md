# Windows installer development

The local fixture installs a prebuilt x64 Threadline executable for the current Windows 11 user. It uses `%LOCALAPPDATA%\Programs\Threadline Installer Test`, a single current-user Start menu shortcut, and the `Threadline.InstallTest` entry under `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall`. The installer does not start Threadline or enroll it in startup. Install and uninstall leave `%LOCALAPPDATA%\Threadline` and Windows Credential Manager alone.

Build on Linux with the pinned NSIS 3.11 compiler and its matching `NSISDIR` data tree. Supply the application executable, setup output, local package version, monotonically increasing build ordinal, and the verified application schema version. Use `unknown` for a packaging-only executable whose schema cannot be proved.

```sh
export MAKENSIS=/path/to/pinned/nsis/usr/bin/makensis
export NSISDIR=/path/to/pinned/nsis/usr/share/nsis
tools/windows/build-installer.sh --fixture /path/to/threadline.exe /output/Threadline-Local-2.exe 0.1.0-local.2 2 10
```

The example uses local build ordinal 2 and schema 10. Use the actual version and schema from the chosen source revision. The sibling `.exe.json` records the setup and exact installed executable digests. Verify the former against the separately recorded digest from the build before running the local fixture. Launch setup normally or with `/S` and wait for completion. Start the app through **Threadline Installer Test** in the Start menu. Use the Windows installed-apps entry or `Uninstall.exe` in the fixture directory to uninstall. The fixture retains `HKCU\Software\Threadline.InstallTest` build history to reject older test packages after uninstall.

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

Build an explicit staging failure fixture with `THREADLINE_SETUP_FIXTURE_FAULT=stage-write` before the same build command. It exits before replacing a working installation and marks the injection in its manifest. Test a real open-file refusal by holding the installed fixture executable open without delete sharing. Compare installed hashes and registered version after each failure. For an upgrade against retained state, install an actual predecessor executable built from a source revision with a known schema. Seed the local test profile, close it, install the candidate, and open the installed executable against that same profile. Record the exact payloads and the application’s migration evidence.

Publication remains pending. The local compiler refuses to produce the production identity. `verify-release-installer.ps1` is the fail-closed verification step for an eventual signed package. It requires an independently trusted release manifest containing the target, minimum OS, channel, version, size, SHA-256 digest, and SHA-256 fingerprint of the approved signer certificate. It verifies both the digest and the Windows-trusted Authenticode signer. It does not install certificates, execute the installer, or publish anything. The unsigned local fixture cannot satisfy this release step.

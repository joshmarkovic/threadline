# Threadline

Threadline is a native desktop client for GitHub issues and pull requests. It keeps downloaded conversations, reviews, tabs, and drafts in a local SQLite profile. You can reopen cached work offline and inspect queued changes in the Outbox.

## Install the Windows preview

On Windows 11 x64, download and run the installer. Setup closes and opens Threadline. Downloads and automatic updates work without a GitHub account or personal access token (PAT). [Download Threadline for Windows (build 24)](https://github.com/joshmarkovic/threadline/releases/download/v0.1.0-local.24/Threadline-Setup-24.exe)

To download and open the same installer from PowerShell, paste this block.

```powershell
& {
    $setup = Join-Path $env:TEMP 'Threadline-Setup-24.exe'
    Invoke-WebRequest -UseBasicParsing -Uri 'https://github.com/joshmarkovic/threadline/releases/download/v0.1.0-local.24/Threadline-Setup-24.exe' -OutFile $setup -ErrorAction Stop
    Start-Process -FilePath $setup
}
```

**Upgrading an older installation?** If you have build 20 or earlier, or build 21 still asks you to set up release access, close Threadline and run the build 24 installer once. Setup preserves your workspace and settings.

After setup, open **Threadline Installer Test** from Start. It checks for a newer preview, verifies the manifest signature and checks the installer against it. Threadline then saves your workspace, installs the update quietly, and reopens. If you start working during the download, choose **Restart to update** when convenient or let it install the next time you open the app. **Work offline** and **Pause background refresh** also pause update checks.

For unattended deployment, download the installer and run `Start-Process .\Threadline-Setup-24.exe -ArgumentList '/S' -Wait` from its folder in PowerShell. A standalone silent install exits when setup finishes. Open the app from Start when you're ready.

## Install from source

You need Git and Rust 1.98 or later. Make sure Cargo’s bin directory is on your `PATH`. Anyone can clone the public repository over HTTPS.

On Windows, use the x64 MSVC toolchain, Visual Studio 2022 Build Tools with the Desktop development with C++ workload, a Windows SDK that includes `fxc.exe`, and CMake. See the [GPUI Kit installation guide](https://gpui-kit.com/docs/installation/) for the development prerequisites on other hosts.

Copy these commands into PowerShell or a POSIX shell.

```sh
git clone https://github.com/joshmarkovic/threadline.git
cd threadline
cargo install --locked --path crates/threadline-app
threadline
```

From an existing checkout, use `cargo run --locked -p threadline-app` to build and run a development copy.

## Open your workspace

On the Welcome screen, choose your display time zone and continue to GitHub. For New York, choose **New York (Eastern Time)**.

1. Enter a repository as `owner/name`, such as `octocat/hello-world`.
2. Choose **Sign in with GitHub** and follow the GitHub CLI prompts to sign in through your browser. If you're already signed in to GitHub CLI on this device, choose **Use signed-in GitHub CLI account** to use that account.
3. Check that Threadline shows the expected GitHub username and repository. Choose **Connect account**, select the repositories to monitor, then choose **Continue to Home**.

Browser sign-in requires the official GitHub CLI on the device running Threadline. To install it on Windows, run `winget install --id GitHub.cli --exact` in PowerShell, then reopen Threadline. Windows and WSL have separate CLI sign-ins. You can also choose **Use a personal access token**, paste a fine-grained token, and choose **Check access**. [GitHub CLI installation](https://cli.github.com/) · [Account setup and permissions](docs/architecture/github-api.md)

On **Home**, choose **Refresh all** to update the repository list and refresh monitored repositories. Open **Settings → Synchronization** to review sync status or change how Threadline refreshes in the background.

### Connect organization repositories

1. On the Welcome screen, or under **Settings → GitHub account → Add account**, enter an organization repository you can access, such as `example-org/project`.
2. Choose **Sign in with GitHub** with the account you use for work. Complete your organization's single sign-on when GitHub prompts. If your organization restricts OAuth apps, request access for **GitHub CLI** through the organization’s usual process.
3. Confirm the username in Threadline, then choose **Connect account**. Once access is approved, choose **Refresh all** on Home.

To use a token for specific repositories, choose **Use a personal access token**. Choose **Sign in with organization SSO** if required, then **Create read-only token**. On GitHub, select the organization as the **Resource owner** and choose the repositories. After approval, paste the token into Threadline and choose **Check access**. [GitHub token permissions and approval](https://docs.github.com/en/authentication/keeping-your-account-and-data-secure/managing-your-personal-access-tokens)

### Local data and updates

Windows stores the workspace in `%LOCALAPPDATA%\Threadline\Data\threadline.sqlite3` and the display preference in `%LOCALAPPDATA%\Threadline\Config\display-timezone`. Threadline stores timestamps in UTC. Change how they appear under **Settings → Display time zone**. Choose a preset or enter an IANA name such as `America/New_York` and save it. Use `system` to follow the device. Calendar-day filters use your selected zone. Existing saved filters retain their UTC cutoffs. You can also set the preference before opening a workspace with `threadline --display-timezone America/New_York`. [Time-zone and storage behavior](docs/architecture/synchronization.md#time-zones)

The Windows preview follows GitHub prereleases and verifies updates with a public key built into the app. For a source build, rerun the source install command after updating your checkout. [Windows packaging guide](tools/windows/README.md) · [Package evidence](docs/reports/sprint-12.md)

Uninstall preserves your local workspace and sign-in credentials. To preview or permanently remove the retained data, use the separate Windows cleanup command. [Remove a retained local workspace](tools/windows/README.md#remove-a-retained-local-workspace)

The reports record which configurations and scenarios have actually run. The sprint pack contains the acceptance criteria and traceability ledgers. [Sprint reports](docs/reports/) · [Architecture](docs/architecture/foundation.md) · [Version 1.7 sprint pack](docs/spec/README.md)

## Test the workspace

```sh
cargo xtask test unit
cargo xtask test integration
cargo xtask test ui
cargo xtask test e2e --suite smoke
```

The native smoke command needs an interactive desktop with the driver prerequisites for the host. Use `--report-dir` to choose where the runner writes its machine-readable results. See the [testing contract](docs/spec/TESTING_PLAN.md) for the full runner, artifact, and live commands.

## Generate a storage fixture

Use an unused database path in an existing directory. The generator fills a temporary database and publishes it at that path when it finishes.

```sh
cargo run --locked -p threadline-storage --example generate_fixture -- section16.sqlite section16 12345
```

Use `small` for a fast fixture. The `section16` fixture includes 50,000 issue and PR summaries, 250,000 comments, a multi-file diff with a review draft, branching history, two panes with twenty tabs, and 1,000 mixed Outbox entries. Runner results for these storage cases include their fixture revision.

To open either dataset in the cached reader, use `reader-small` or `reader-section16` and write `threadline.sqlite3` inside a new, empty test profile directory. These modes select two synthetic repositories for a signed-out account. The reader can browse and search the generated records offline.

```sh
cargo run --locked -p threadline-storage --example generate_fixture -- /path/to/test-profile/threadline.sqlite3 reader-small 12345
```

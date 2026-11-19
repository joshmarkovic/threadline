# Threadline

Threadline is a native desktop client for GitHub issues and pull requests. It keeps downloaded conversations, reviews, tabs, and drafts in a local SQLite profile. You can reopen cached work offline and inspect queued changes in the Outbox.

## Install the private Windows test build

On Windows 11 x64, sign in to GitHub with an account that can read `joshmarkovic/threadline`, then download and run the installer. [Download Threadline-Setup-20.exe](https://github.com/joshmarkovic/threadline/releases/download/v0.1.0-local.20/Threadline-Setup-20.exe)

You can also download the installer with GitHub CLI. If you need the CLI, install it with `winget install --id GitHub.cli --exact` and open a new PowerShell window.

Run these commands from the folder where you want to keep the installer. Skip the first line if you're already signed in.

```powershell
gh auth login --hostname github.com --web
gh release download v0.1.0-local.20 --repo joshmarkovic/threadline --pattern Threadline-Setup-20.exe --dir . --clobber
if ($LASTEXITCODE -eq 0) { & .\Threadline-Setup-20.exe }
```

Open **Threadline Installer Test** from Start after setup finishes. This private prerelease is unsigned. The test installation keeps its own registration. Automatic updates require a signed stable installation.

## Install from source

You need Git, Rust 1.98 or later, and a GitHub account with access to the private Threadline repository. Make sure Cargo's bin directory is on your `PATH`.

If Git Credential Manager opens a sign-in window, choose the browser sign-in option and use your GitHub account with repository access. If you're already signed in with GitHub CLI, you can run `gh auth setup-git` once before cloning. [GitHub CLI credential setup](https://cli.github.com/manual/gh_auth_setup-git)

On Windows, use the x64 MSVC toolchain, Visual Studio 2022 Build Tools with the Desktop development with C++ workload, a Windows SDK that includes `fxc.exe`, and CMake. See the [GPUI Kit installation guide](https://gpui-kit.com/docs/installation/) for the development prerequisites on other hosts.

Copy these commands into PowerShell or a POSIX shell.

```sh
git clone https://github.com/joshmarkovic/threadline.git
cd threadline
cargo install --locked --path crates/threadline-app
threadline
```

On Windows, if the clone reports a missing credential helper or prompts for a password in the terminal, cancel with **Ctrl+C**. If Git Credential Manager is installed, use it for this clone only.

```powershell
git -c credential.helper= -c credential.helper=manager clone https://github.com/joshmarkovic/threadline.git
```

Sign in through the browser if prompted, then continue with `cd threadline` above.

From an existing checkout, use `cargo run --locked -p threadline-app` to build and run a development copy.

## Open your workspace

On the Welcome screen, choose your display time zone and continue to GitHub. For New York, choose **New York (Eastern Time)**. Enter the full repository name, such as `joshmarkovic/threadline`, paste a GitHub personal access token, and choose **Check access**. A fine-grained token needs access to that repository and the permissions for the features you use. Select the repositories you want to monitor, then choose **Continue to Home**. Windows Credential Manager protects the token. [Account setup and endpoint permissions](docs/architecture/github-api.md)

**Home** shows the cached workspace. Use **Refresh all** to request fresh data for monitored repositories. **Settings** lets you add an account, replace a token, change the repositories you monitor, or work offline.

Windows stores the workspace in `%LOCALAPPDATA%\Threadline\Data\threadline.sqlite3` and the display preference in `%LOCALAPPDATA%\Threadline\Config\display-timezone`. Threadline stores timestamps in UTC. Change how they appear under **Settings → Display time zone**. Choose a preset or enter an IANA name such as `America/New_York` and save it. Use `system` to follow the device. Calendar-day filters use your selected zone. Existing saved filters retain their UTC cutoffs. You can also set the preference before opening a workspace with `threadline --display-timezone America/New_York`. [Time-zone and storage behavior](docs/architecture/synchronization.md#time-zones)

Registered signed Windows installations check for the latest stable GitHub release on startup. Threadline downloads and verifies compatible signed updates in the background and offers **Restart to update** when ready. For a private release repository, save a separate token with **Contents: Read** in Settings. [Windows packaging guide](tools/windows/README.md) · [Package evidence](docs/reports/sprint-12.md)

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

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$ReportDir,
    [string]$ProfileDir,
    [UInt64]$Seed = 12345,
    [switch]$Measure,
    [switch]$PlatformProof,
    [switch]$ToastProof,
    [switch]$AccessProof,
    [ValidateSet('Offline', 'PanePositions', 'PaneLifecycle', 'Resume', 'CursorRecovery', 'Historical', 'Performance', 'PerformancePhysical', 'PerformanceConcurrent', 'LargeWorkspace', 'Sync', 'RateLimit', 'NotificationSchedule', 'PullRequest', 'SharedPullRequest', 'PullRequestMergeState', 'PullRequestOfflinePin', 'ReviewDraft', 'ReviewDecisions', 'ReviewPredecessor', 'ThreadQueue', 'ReviewContext', 'DirectMerge', 'MergeQueue', 'MergeCancel', 'CredentialBoundary', 'NativeLifecycle', 'OfflineMixedOutbox', 'NativeActivity', 'History', 'CommentDependency', 'IssueConflict', 'IssuePropagation', 'Writes', 'WriteSurface')][string]$ReaderProof,
    [switch]$AccessibilityProof,
    [string]$AccessAddress,
    [string]$AccessPhaseFile,
    [switch]$ManualTrayClick,
    [switch]$ManualNotificationClick,
    [string]$Artifact,
    [string]$BudgetFile
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$script:UsesNativeLifecycleFixture = $ReaderProof -in @('NativeLifecycle', 'OfflineMixedOutbox')
$script:UsesPaneFixture = $ReaderProof -in @('PanePositions', 'PaneLifecycle')
$null = New-Item -ItemType Directory -Force -Path $ReportDir
$reportName = if ($Measure) {
    'measurements.json'
} elseif ($PlatformProof) {
    'platform-native.json'
} elseif ($ToastProof) {
    'toast-native.json'
} elseif ($AccessProof) {
    'github-access-native.json'
} elseif ($ReaderProof -in @('Performance', 'PerformancePhysical', 'PerformanceConcurrent')) {
    'reader-performance.json'
} elseif ($ReaderProof -eq 'LargeWorkspace') {
    'large-workspace-native.json'
} elseif ($ReaderProof -in @('Sync', 'RateLimit')) {
    'sync-native.json'
} elseif ($ReaderProof -ceq 'CursorRecovery') {
    'reader-cursor-recovery-native.json'
} elseif ($ReaderProof -eq 'NotificationSchedule') {
    'notification-schedule-native.json'
} elseif ($ReaderProof -eq 'PullRequestOfflinePin') {
    'pr-offline-pin-native.json'
} elseif ($ReaderProof -in @('PullRequest', 'SharedPullRequest', 'PullRequestMergeState')) {
    'pr-native.json'
} elseif ($ReaderProof -in @('ReviewDraft', 'ReviewDecisions', 'ReviewPredecessor', 'ThreadQueue')) {
    'review-draft-native.json'
} elseif ($ReaderProof -eq 'ReviewContext') {
    'review-context-native.json'
} elseif ($ReaderProof -in @('DirectMerge', 'MergeQueue', 'MergeCancel')) {
    'direct-merge-native.json'
} elseif ($ReaderProof -eq 'CredentialBoundary') {
    'credential-boundary-native.json'
} elseif ($ReaderProof -eq 'OfflineMixedOutbox') {
    'offline-mixed-outbox-native.json'
} elseif ($ReaderProof -eq 'NativeLifecycle') {
    'native-lifecycle.json'
} elseif ($ReaderProof -eq 'NativeActivity') {
    'native-activity.json'
} elseif ($ReaderProof -eq 'History') {
    'history-native.json'
} elseif ($ReaderProof -eq 'IssueConflict') {
    'issue-conflict-native.json'
} elseif ($ReaderProof -eq 'IssuePropagation') {
    'issue-propagation-native.json'
} elseif ($ReaderProof -eq 'CommentDependency') {
    'comment-dependency-native.json'
} elseif ($ReaderProof -in @('Writes', 'WriteSurface')) {
    'writes-native.json'
} elseif ($ReaderProof) {
    'reader-native.json'
} elseif ($AccessibilityProof) {
    'accessibility-native.json'
} else {
    'native.json'
}
$reportPath = Join-Path $ReportDir $reportName
$events = New-Object 'System.Collections.Generic.List[object]'
$report = [ordered]@{
    schema = 1
    status = 'Fail'
    detail = 'The driver did not finish.'
    driver = 'Windows PowerShell / Win32 SendInput'
    shell_version = $PSVersionTable.PSVersion.ToString()
    seed = $Seed
    os_version = [Environment]::OSVersion.Version.ToString()
    session_id = [System.Diagnostics.Process]::GetCurrentProcess().SessionId
    architecture = $env:PROCESSOR_ARCHITECTURE
    app_pid = $null
    restarted_pid = $null
    native_window = $null
    dpi = $null
    binary_sha256 = $null
    executable_bytes = $null
    actions = $events
}
$measurement = [ordered]@{}
$app = $null
$firstProfile = $null
$reportFile = Join-Path $ReportDir 'ui-observation.json'
$blocked = $false
$clipboardBackup = $null
$copiedClipboardSequence = $null
$toastFixture = $null
$toastSubmissionAttempted = $false
$script:UseOwnedUiAutomation = $false
$script:UseSeededFixtureAuth = $ReaderProof -in @('Resume', 'CursorRecovery', 'Historical', 'Sync', 'RateLimit', 'NotificationSchedule', 'PullRequest', 'SharedPullRequest', 'PullRequestMergeState', 'PullRequestOfflinePin', 'ReviewDraft', 'ReviewDecisions', 'ReviewPredecessor', 'ThreadQueue', 'ReviewContext', 'DirectMerge', 'MergeQueue', 'MergeCancel', 'History', 'CommentDependency', 'IssueConflict', 'IssuePropagation', 'Writes', 'WriteSurface', 'PerformanceConcurrent', 'NativeActivity')

function Get-FileSha256 {
    param([string]$Path)

    $stream = [IO.File]::OpenRead($Path)
    try {
        $algorithm = [Security.Cryptography.SHA256]::Create()
        try {
            $digest = $algorithm.ComputeHash($stream)
            return [BitConverter]::ToString($digest).Replace('-', '').ToLowerInvariant()
        } finally {
            $algorithm.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
}

function Get-IsolatedSqlite([string]$Profile, [switch]$Live) {
    $database = Join-Path $Profile 'threadline.sqlite3'
    if (!(Test-Path -LiteralPath $database -PathType Leaf)) {
        throw 'The isolated profile database was not created.'
    }
    $header = New-Object byte[] 16
    $sharing = if ($Live) { [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete } else { [IO.FileShare]::Read }
    $stream = [IO.File]::Open($database, [IO.FileMode]::Open, [IO.FileAccess]::Read, $sharing)
    try {
        $read = $stream.Read($header, 0, $header.Length)
    } finally {
        $stream.Dispose()
    }
    if ($read -ne $header.Length -or [Text.Encoding]::ASCII.GetString($header) -cne ("SQLite format 3" + [char]0)) {
        throw 'The isolated profile database does not have a SQLite header.'
    }
    return [ordered]@{
        sha256 = if ($Live) { $null } else { Get-FileSha256 $database }
        creation_time_ticks = [IO.File]::GetCreationTimeUtc($database).Ticks
    }
}

function Add-Evidence([string]$Action, $Observed) {
    $events.Add([ordered]@{ action = $Action; observed = $Observed })
}

function Stop-Blocked([string]$Message) {
    $script:blocked = $true
    throw $Message
}

function Wait-For([string]$Description, [scriptblock]$Probe, [int]$Timeout = 8000, [int]$Interval = 45) {
    $wait = [Diagnostics.Stopwatch]::StartNew()
    while ($wait.ElapsedMilliseconds -lt $Timeout) {
        if (& $Probe) { return }
        Start-Sleep -Milliseconds $Interval
    }
    throw "Timed out waiting for $Description."
}

function Read-Observation {
    if (![IO.File]::Exists($script:reportFile)) { return $null }
    $stream = $null
    $reader = $null
    try {
        $sharing = [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete
        $stream = [IO.File]::Open($script:reportFile, [IO.FileMode]::Open, [IO.FileAccess]::Read, $sharing)
        $reader = [IO.StreamReader]::new($stream)
        return $reader.ReadToEnd() | ConvertFrom-Json
    } catch [System.IO.IOException] {
        return $null
    } catch [System.ArgumentException] {
        return $null
    } finally {
        if ($null -ne $reader) { $reader.Dispose() }
        elseif ($null -ne $stream) { $stream.Dispose() }
    }
}

function Wait-State([string]$Description, [scriptblock]$Check, [int]$Interval = 45) {
    Wait-For $Description {
        $state = Read-Observation
        $null -ne $state -and (& $Check $state)
    } 8000 $Interval
    return Read-Observation
}

function Wait-SettledLayout([string]$Description, [scriptblock]$Check) {
    $stable = @{ signature = $null; state = $null; elapsed = [Diagnostics.Stopwatch]::new() }
    Wait-For $Description {
        $state = Read-Observation
        if ($null -eq $state -or !(& $Check $state)) {
            $stable.signature = $null
            return $false
        }
        $signature = @(
            $state.outer_ratio, $state.context_ratio,
            ($state.bounds.outer_divider -join ','), ($state.bounds.context_divider -join ',')
        ) -join '|'
        if ($signature -ne $stable.signature) {
            $stable.signature = $signature
            $stable.state = $state
            $stable.elapsed.Restart()
        }
        return $stable.elapsed.ElapsedMilliseconds -ge 160
    }
    return $stable.state
}

function Get-ThreadlineArguments([string]$Observation, [switch]$OpenReview, [string]$SeedReader, [switch]$SeedQuietEvent) {
    $arguments = '--e2e-report "' + $Observation + '"'
    if ($SeedReader) { return "--e2e-seed-reader $SeedReader $arguments" }
    if ($SeedQuietEvent) { return "--e2e-seed-notification $arguments" }
    if (!$AccessProof -and !$ReaderProof) { $arguments += ' --fixture-s01' }
    if ($Measure) { $arguments += ' --measure' }
    if ($OpenReview) { $arguments += ' --open-review' }
    return $arguments
}

function Start-Threadline([string]$Profile, [string]$Observation, [switch]$OpenReview, [string]$SeedReader, [switch]$ReaderIdle, [switch]$SeedQuietEvent, [switch]$ReaderBacklog, [ValidateSet('seed','seed-offline-outbox','inspect','cleanup','probe','offline-outbox')][string]$Lifecycle, [ValidateSet('seed','inspect')][string]$ActivityFixture, [ValidateSet('arm','snapshot','release')][string]$ReaderRecovery) {
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $script:Binary
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.WorkingDirectory = $Profile
    $info.Arguments = Get-ThreadlineArguments $Observation -OpenReview:$OpenReview -SeedReader $SeedReader -SeedQuietEvent:$SeedQuietEvent
    $info.EnvironmentVariables['THREADLINE_PROFILE_DIR'] = $Profile
    $info.EnvironmentVariables['THREADLINE_TEST_SEED'] = $script:Seed.ToString([Globalization.CultureInfo]::InvariantCulture)
    $info.EnvironmentVariables.Remove('THREADLINE_TEST_TOAST_TOKEN')
    $info.EnvironmentVariables.Remove('THREADLINE_E2E_GITHUB_ADDR')
    $info.EnvironmentVariables.Remove('THREADLINE_E2E_SYNC')
    $info.EnvironmentVariables.Remove('THREADLINE_E2E_FIXTURE_AUTH')
    $info.EnvironmentVariables.Remove('THREADLINE_E2E_ACCOUNT_FORM')
    $info.EnvironmentVariables.Remove('THREADLINE_E2E_READER_TIMING')
    $info.EnvironmentVariables.Remove('THREADLINE_E2E_READER_IDLE')
    $info.EnvironmentVariables.Remove('THREADLINE_E2E_NATIVE_LIFECYCLE')
    $info.EnvironmentVariables.Remove('THREADLINE_E2E_NATIVE_ACTIVITY')
    $info.EnvironmentVariables.Remove('THREADLINE_E2E_SHARED_PR')
    $info.EnvironmentVariables.Remove('THREADLINE_E2E_PANE_POSITIONS')
    $info.EnvironmentVariables.Remove('THREADLINE_E2E_READER_RECOVERY')
    if ($ReaderRecovery) {
        if ($ReaderProof -cne 'CursorRecovery') { throw 'The discussion recovery stage requires its named local case.' }
        $info.EnvironmentVariables['THREADLINE_E2E_READER_RECOVERY'] = '1'
        $info.Arguments = "--e2e-read-recovery $ReaderRecovery " + $info.Arguments
    }
    if ($ReaderProof -eq 'SharedPullRequest') { $info.EnvironmentVariables['THREADLINE_E2E_SHARED_PR'] = '1' }
    if ($ReaderProof -eq 'PanePositions') { $info.EnvironmentVariables['THREADLINE_E2E_PANE_POSITIONS'] = '1' }
    if ($ReaderProof -eq 'PaneLifecycle') { $info.EnvironmentVariables['THREADLINE_E2E_PANE_POSITIONS'] = 'layout' }
    if ($ReaderProof -in @('Performance', 'PerformancePhysical', 'PerformanceConcurrent', 'LargeWorkspace')) { $info.EnvironmentVariables['THREADLINE_E2E_READER_TIMING'] = '1' }
    if ($ReaderIdle) { $info.EnvironmentVariables['THREADLINE_E2E_READER_IDLE'] = '1' }
    if ($ToastProof -and $null -ne $script:toastFixture) {
        $info.EnvironmentVariables['THREADLINE_TEST_TOAST_TOKEN'] = $script:toastFixture.Token
    }
    if ($AccessProof -or $ReaderProof) {
        $info.EnvironmentVariables['THREADLINE_E2E_GITHUB_ADDR'] = $script:AccessAddress
    }
    if ($script:UseSeededFixtureAuth -or $ReaderBacklog) { $info.EnvironmentVariables['THREADLINE_E2E_FIXTURE_AUTH'] = '1' }
    if ($ReaderProof -eq 'CredentialBoundary' -or ($ReaderProof -eq 'NativeLifecycle' -and !$Lifecycle)) {
        $info.EnvironmentVariables['THREADLINE_E2E_ACCOUNT_FORM'] = '1'
    }
    if ($ReaderBacklog) { $info.EnvironmentVariables['THREADLINE_E2E_SYNC'] = '1' }
    if ($ReaderProof -in @('CursorRecovery', 'Historical', 'Sync', 'RateLimit', 'NotificationSchedule', 'PullRequestOfflinePin', 'ReviewDraft', 'ReviewDecisions', 'ReviewPredecessor', 'ThreadQueue', 'DirectMerge', 'MergeQueue', 'MergeCancel', 'CredentialBoundary', 'CommentDependency', 'IssueConflict', 'IssuePropagation', 'Writes', 'WriteSurface', 'PerformanceConcurrent')) { $info.EnvironmentVariables['THREADLINE_E2E_SYNC'] = '1' }
    if ($script:UsesNativeLifecycleFixture) {
        $info.EnvironmentVariables['THREADLINE_E2E_NATIVE_LIFECYCLE'] = '1'
        $info.EnvironmentVariables['THREADLINE_E2E_SYNC'] = '1'
        if ($Lifecycle) { $info.Arguments = "--e2e-native-lifecycle $Lifecycle " + $info.Arguments }
    }
    if ($ReaderProof -eq 'NativeActivity') {
        $info.EnvironmentVariables['THREADLINE_E2E_NATIVE_ACTIVITY'] = '1'
        $info.EnvironmentVariables['THREADLINE_E2E_SYNC'] = '1'
        if ($ActivityFixture) { $info.Arguments = "--e2e-native-activity $ActivityFixture " + $info.Arguments }
    }
    return [Diagnostics.Process]::Start($info)
}

function Get-ReadyWindow {
    Wait-For 'first rendered state and visible native window' {
        $script:app.Refresh()
        if ($script:app.HasExited) { throw "Threadline exited with code $($script:app.ExitCode) on launch." }
        $state = Read-Observation
        $null -ne $state -and $state.ready -and
            $script:app.MainWindowHandle -ne [IntPtr]::Zero -and
            [ThreadlineWinInput]::IsWindowVisible($script:app.MainWindowHandle)
    } 20000
    return $script:app.MainWindowHandle
}

function Set-WindowFocus([IntPtr]$Window) {
    if ([ThreadlineWinInput]::IsIconic($Window)) { [ThreadlineWinInput]::ShowWindow($Window, 9) | Out-Null }
    $shell = New-Object -ComObject WScript.Shell
    try { [void]$shell.AppActivate($script:app.Id) } finally { [void][Runtime.InteropServices.Marshal]::ReleaseComObject($shell) }
    [void][ThreadlineWinInput]::SetForegroundWindow($Window)
    if ([ThreadlineWinInput]::GetForegroundWindow() -ne $Window) {
        $foreground = [ThreadlineWinInput]::GetForegroundWindow()
        $foregroundProcess = [ThreadlineWinInput]::WindowProcessId($foreground)
        $clicked = [ThreadlineWinInput]::FocusByPointer($Window)
        Add-Evidence 'Attempt focus through a verified native pointer target' ([ordered]@{ prior_foreground_pid = $foregroundProcess; injected = $clicked })
    }
    try {
        Wait-For 'Threadline foreground focus' { [ThreadlineWinInput]::GetForegroundWindow() -eq $Window } 3000
    } catch { Stop-Blocked 'Windows prevented focus. Run the driver from the unlocked interactive desktop with Threadline visible.' }
    [ThreadlineWinInput]::TargetWindow = $Window
}

function To-ClientPoint([double]$X, [double]$Y, [double]$Scale) {
    $point = New-Object ThreadlineWinInput+Point
    $point.X = [int][Math]::Round($X * $Scale)
    $point.Y = [int][Math]::Round($Y * $Scale)
    return $point
}

function To-ScreenPoint([IntPtr]$Window, [double]$X, [double]$Y, [double]$Scale) {
    $point = To-ClientPoint $X $Y $Scale
    if (![ThreadlineWinInput]::ClientToScreen($Window, [ref]$point)) {
        throw 'Could not map the current control bounds to the desktop.'
    }
    return $point
}

function Crop-Screenshot([IntPtr]$Window, [string]$Name) {
    if ([ThreadlineWinInput]::GetForegroundWindow() -ne $Window) {
        throw 'Threadline must be the foreground window to capture evidence.'
    }
    $rect = New-Object ThreadlineWinInput+Rect
    if (![ThreadlineWinInput]::GetClientRect($Window, [ref]$rect)) {
        throw 'Could not determine the native window size for evidence.'
    }
    $origin = To-ScreenPoint $Window 0 0 1
    $bitmap = New-Object Drawing.Bitmap $rect.Right, $rect.Bottom
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen($origin.X, $origin.Y, 0, 0, $bitmap.Size)
        $bitmap.Save((Join-Path $ReportDir $Name), [Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

function Invoke-ToolbarClick([IntPtr]$Window, [string]$Control) {
    $state = Read-Observation
    $property = $state.bounds.PSObject.Properties[$Control]
    if ($null -eq $property -or $property.Value.Count -ne 4) { throw "Missing current bounds for $Control." }
    $bounds = $property.Value
    $point = To-ClientPoint ($bounds[0] + $bounds[2] / 2) ($bounds[1] + $bounds[3] / 2) $state.scale_factor
    [ThreadlineWinInput]::ClickClient($Window, $point, $script:app.Id, [uint32][Math]::Round($state.scale_factor * 96))
}

function Find-ShellControl([string]$Name, [switch]$TrayArea) {
    $knownShells = @(Get-Process -Name explorer, ShellExperienceHost, ShellHost -ErrorAction SilentlyContinue)
    $shellIds = @($knownShells | ForEach-Object { $_.Id })
    $nameCondition = New-Object Windows.Automation.PropertyCondition ([Windows.Automation.AutomationElement]::NameProperty), $Name
    $topLevel = [Windows.Automation.AutomationElement]::RootElement.FindAll(
        [Windows.Automation.TreeScope]::Children, [Windows.Automation.Condition]::TrueCondition)
    foreach ($element in $topLevel) {
        try {
            $root = $element.Current
            if ($root.ProcessId -notin $shellIds -or $root.ClassName -in @('CabinetWClass', 'Progman', 'WorkerW')) { continue }
            if ($TrayArea -and $root.ClassName -notin @(
                'Shell_TrayWnd', 'Shell_SecondaryTrayWnd', 'TopLevelWindowForOverflowXamlIsland', 'NotifyIconOverflowWindow'
            )) { continue }
            if (!$TrayArea -and $root.Name -ceq $Name -and !$root.IsOffscreen) { return $element }
            foreach ($target in $element.FindAll([Windows.Automation.TreeScope]::Descendants, $nameCondition)) {
                $control = $target.Current
                if ($control.IsOffscreen -or $control.ProcessId -notin $shellIds -or $control.Name -cne $Name) { continue }
                if ($TrayArea -and !$control.ClassName.StartsWith('SystemTray.') -and $root.ClassName -ne 'NotifyIconOverflowWindow') { continue }
                if ($Name -ceq 'Show Hidden Icons' -and (
                    $control.ControlType -ne [Windows.Automation.ControlType]::Button -or $control.AutomationId -ne 'SystemTrayIcon'
                )) { continue }
                return $target
            }
        } catch [Windows.Automation.ElementNotAvailableException] {
            continue
        }
    }
    return $null
}

function Wait-ShellControl([string]$Name, [int]$Timeout = 2500, [switch]$TrayArea) {
    $wait = [Diagnostics.Stopwatch]::StartNew()
    while ($wait.ElapsedMilliseconds -lt $Timeout) {
        $control = Find-ShellControl $Name -TrayArea:$TrayArea
        if ($null -ne $control) { return $control }
        Start-Sleep -Milliseconds 90
    }
    return $null
}

function Invoke-ShellClick($Element) {
    $rect = $Element.Current.BoundingRectangle
    if ($rect.IsEmpty -or $rect.Width -le 0 -or $rect.Height -le 0) { throw 'No visible shell control bounds.' }
    $point = New-Object ThreadlineWinInput+Point
    $point.X = [int][Math]::Round($rect.X + $rect.Width / 2)
    $point.Y = [int][Math]::Round($rect.Y + $rect.Height / 2)
    [ThreadlineWinInput]::ClickShellControl($point, $Element.Current.ProcessId)
}

function Invoke-TrayIconClick {
    try {
        $icon = Wait-ShellControl 'Threadline' 700 -TrayArea
        if ($null -eq $icon) {
            $overflow = Wait-ShellControl 'Show Hidden Icons' 700 -TrayArea
            if ($null -ne $overflow) {
                Invoke-ShellClick $overflow
                Add-Evidence 'Open the identified Windows notification area overflow' $true
                $icon = Wait-ShellControl 'Threadline' 2200 -TrayArea
            }
        }
        if ($null -eq $icon) { return $false }
        Invoke-ShellClick $icon
        return $true
    } catch {
        Add-Evidence 'Find and click the exact Windows notification area icon' 'No verified, clickable shell target was available'
        return $false
    }
}

function Wait-CopiedText([uint32]$Before, [string]$Expected, [switch]$Exact) {
    $wait = [Diagnostics.Stopwatch]::StartNew()
    while ($wait.ElapsedMilliseconds -lt 1600) {
        $sequence = [ThreadlineWinInput]::GetClipboardSequenceNumber()
        if ($sequence -ne $Before) {
            try {
                $text = [Windows.Forms.Clipboard]::GetText()
                if ([ThreadlineWinInput]::GetClipboardSequenceNumber() -eq $sequence -and
                    (($Exact -and [string]::Equals($text, $Expected, [StringComparison]::Ordinal)) -or
                     (!$Exact -and $text.IndexOf($Expected, [StringComparison]::Ordinal) -ge 0))) {
                    return $sequence
                }
            } catch {
                # The app or the desktop clipboard service can briefly hold the clipboard open.
            }
        }
        Start-Sleep -Milliseconds 45
    }
    return $null
}

function Assert-ClipboardContains([string]$Expected, [switch]$SelectAll, [switch]$Exact) {
    $before = [ThreadlineWinInput]::GetClipboardSequenceNumber()
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        if ($SelectAll) { [ThreadlineWinInput]::Chord(@(0x11, 0x41)) }
        Start-Sleep -Milliseconds 80
        [ThreadlineWinInput]::Chord(@(0x11, 0x43))
        $sequence = Wait-CopiedText $before $Expected -Exact:$Exact
        if ($null -ne $sequence) {
            $script:copiedClipboardSequence = $sequence
            return $attempt
        }
    }
    throw "Native clipboard did not change to the expected synthetic token '$Expected' after three attempts."
}

function Stop-ThroughUi {
    if ($script:UseOwnedUiAutomation) {
        $window = $script:app.MainWindowHandle
        if (!(Test-OwnedFixtureWindow $window) -or !$script:app.CloseMainWindow()) {
            Stop-Blocked 'The verified fixture process did not accept its native window close.'
        }
    } else {
        [ThreadlineWinInput]::Chord(@(0x11, 0x51))
    }
    Wait-For 'process to exit after the normal native close' { $script:app.HasExited }
    if ($script:app.ExitCode -ne 0) { throw "Threadline exited with code $($script:app.ExitCode)." }
}

function Invoke-Smoke([IntPtr]$Window) {
    $draftText = 'native draft 72'
    if ($AccessibilityProof) {
        $draftText += ' ' + [char]0x00e9 + [char]0x03bb + [char]0x65e5
    }
    $initial = Read-Observation
    if ($initial.left_tab -ne 41 -or $initial.right_tab -ne 72 -or
        [Math]::Abs($initial.context_ratio - 0.33) -gt 0.03) {
        throw 'The expected two default fixture tabs and 33% context were not visible.'
    }
    Add-Evidence 'Launch native window' ([ordered]@{ left_tab = $initial.left_tab; right_tab = $initial.right_tab; context_ratio = $initial.context_ratio })
    Crop-Screenshot $Window 'launch.png'

    [ThreadlineWinInput]::Chord(@(0x11, 0x31))
    $left = Wait-State 'left pane focus' { param($state) $state.focused_pane -eq 'left' }
    $markdown = $left.bounds.markdown_copy
    if ($null -eq $markdown -or $markdown.Count -ne 4 -or $markdown[2] -le 0 -or $markdown[3] -le 0) {
        throw 'Missing current selectable Markdown bounds.'
    }
    $origin = To-ClientPoint ($markdown[0] + 1) ($markdown[1] + $markdown[3] * 0.5) $left.scale_factor
    $end = To-ClientPoint ($markdown[0] + $markdown[2] - 1) ($markdown[1] + $markdown[3] * 0.5) $left.scale_factor
    [ThreadlineWinInput]::DragClient($Window, $origin, $end, $script:app.Id, [uint32][Math]::Round($left.scale_factor * 96))
    $markdownAttempts = Assert-ClipboardContains 'Offline review'
    Add-Evidence 'Pointer select formatted Markdown and copy' ([ordered]@{ verified = $true; copy_attempts = $markdownAttempts })

    [ThreadlineWinInput]::Chord(@(0x11, 0x32))
    $null = Wait-State 'right pane focus' { param($state) $state.focused_pane -eq 'right' }
    [ThreadlineWinInput]::Chord(@(0x11, 0x52))
    $null = Wait-State 'background preview starts' { param($state) $state.preview_state -eq 'loading' }
    [ThreadlineWinInput]::Chord(@(0x11, 0x46))
    [ThreadlineWinInput]::TypeText($draftText)
    $draft = Wait-State 'text typed into PR review input' { param($state) $state.right_input -ceq $draftText -and $state.left_input -eq '' }
    if ($draft.preview_state -ne 'loading') { throw 'Review input was not observed before planned background completion.' }
    $null = Wait-State 'background preview completes' { param($state) $state.preview_state -eq 'ready' }
    $draftAttempts = Assert-ClipboardContains $draftText -SelectAll -Exact:$AccessibilityProof
    Add-Evidence 'Type into the right review input while background preview is pending and copy its value' ([ordered]@{ tab = $draft.right_tab; text = $draft.right_input; copy_attempts = $draftAttempts })

    [ThreadlineWinInput]::Chord(@(0x11, 0x09))
    $switched = Wait-State 'next PR tab without changing the other pane' { param($state) $state.right_tab -eq 68 -and $state.left_tab -eq 41 }
    [ThreadlineWinInput]::Chord(@(0x11, 0x09))
    $restored = Wait-State 'return to PR 72 with draft intact' { param($state) $state.right_tab -eq 72 -and $state.right_input -ceq $draftText }
    Add-Evidence 'Change the active tab and return to the same draft target' ([ordered]@{ switched = $switched.right_tab; restored = $restored.right_tab; draft = $restored.right_input })

    [ThreadlineWinInput]::Chord(@(0x11, 0x10, 0x4C))
    $context = Wait-State 'linked issue context' { param($state) $state.context_mode -eq 'issue' -and $state.right_tab -eq 72 }
    Add-Evidence 'Switch context to linked issue' $context.context_mode

    $outer = $context.bounds.outer_divider
    $start = To-ClientPoint ($outer[0] + $outer[2] / 2) ($outer[1] + $outer[3] / 2) $context.scale_factor
    $end = To-ClientPoint ($outer[0] - 62) ($outer[1] + $outer[3] / 2) $context.scale_factor
    [ThreadlineWinInput]::DragClient($Window, $start, $end, $script:app.Id, [uint32][Math]::Round($context.scale_factor * 96))
    $outerState = Wait-SettledLayout 'settled outer divider resize' { param($state) [Math]::Abs($state.outer_ratio - $context.outer_ratio) -gt 0.025 }
    Add-Evidence 'Resize workspace panes with the pointer' $outerState.outer_ratio

    $inner = $outerState.bounds.context_divider
    $start = To-ClientPoint ($inner[0] + $inner[2] / 2) ($inner[1] + $inner[3] / 2) $outerState.scale_factor
    $end = To-ClientPoint ($inner[0] + 48) ($inner[1] + $inner[3] / 2) $outerState.scale_factor
    [ThreadlineWinInput]::DragClient($Window, $start, $end, $script:app.Id, [uint32][Math]::Round($outerState.scale_factor * 96))
    $innerState = Wait-SettledLayout 'settled PR context divider resize' { param($state) [Math]::Abs($state.context_ratio - $outerState.context_ratio) -gt 0.025 }
    Add-Evidence 'Resize PR context and code with the pointer' $innerState.context_ratio

    [ThreadlineWinInput]::Chord(@(0x11, 0x32))
    [ThreadlineWinInput]::Chord(@(0x11, 0x09))
    $prior = Wait-State 'second PR before routing external activation' { param($state) $state.right_tab -eq 68 }
    $forwarded = Start-Threadline $firstProfile (Join-Path $ReportDir 'activation-observation.json') -OpenReview
    try {
        Wait-For 'secondary process to forward and exit' { $forwarded.HasExited }
        if ($forwarded.ExitCode -ne 0) { throw "Secondary launch returned $($forwarded.ExitCode)." }
        $routed = Wait-State 'existing app routes external activation to PR 72' {
            param($state) $state.activations -gt $prior.activations -and $state.right_tab -eq 72 -and $state.right_input -ceq $draftText
        }
        if ($routed.pid -ne $script:app.Id) { throw 'PR activation did not stay in the existing process.' }
        Add-Evidence 'Forward a second launch through the Windows instance channel' ([ordered]@{ existing_pid = $routed.pid; secondary_pid = $forwarded.Id; right_tab = $routed.right_tab })
    } finally {
        if (!$forwarded.HasExited) { $forwarded.Kill() }
        $forwarded.Dispose()
    }

    Crop-Screenshot $Window 'interaction.png'
    Stop-ThroughUi
    Add-Evidence 'Quit with the native keyboard shortcut' 0
}

function Get-GpuProcessMemory([int]$ProcessId) {
    try {
        $instances = @(Get-CimInstance -ClassName Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory |
            Where-Object { $_.Name -like "pid_${ProcessId}_*" })
        if ($instances.Count -eq 0) { return $null }
        $dedicated = ($instances | Measure-Object -Property DedicatedUsage -Sum).Sum
        $shared = ($instances | Measure-Object -Property SharedUsage -Sum).Sum
        return [ordered]@{ dedicated_bytes = [uint64]$dedicated; shared_bytes = [uint64]$shared }
    } catch {
        return $null
    }
}

function Assert-ScrollProcess {
    if ($script:app.HasExited) {
        throw "Threadline exited during scroll measurement with code $($script:app.ExitCode) before the driver requested Quit."
    }
}

function Stop-ScrollBlocked([string]$Reason) {
    Assert-ScrollProcess
    Stop-Blocked $Reason
}

function Measure-Scroll([IntPtr]$Window, [string]$Control) {
    Assert-ScrollProcess
    $state = Read-Observation
    $dpi = [ThreadlineWinInput]::GetDpiForWindow($Window)
    if ($null -eq $state -or !$state.ready -or $state.pid -ne $script:app.Id -or $dpi -eq 0 -or
        [Math]::Abs($state.scale_factor * 96 - $dpi) -gt 0.01) {
        Stop-ScrollBlocked 'The native window and rendered wheel target disagree on child identity or DPI.'
    }
    $property = $state.bounds.PSObject.Properties[$Control]
    if ($null -eq $property -or @($property.Value).Count -ne 4 -or $property.Value[2] -le 0 -or $property.Value[3] -le 0) {
        Stop-ScrollBlocked "Threadline did not report a visible $Control target."
    }
    $bounds = $property.Value
    $before = @($state.scroll_frame_times_ms).Count
    $point = To-ClientPoint ($bounds[0] + $bounds[2] / 2) ($bounds[1] + $bounds[3] / 2) $state.scale_factor
    $wheel = [ThreadlineWinInput+WheelEvidence]::new()
    try {
        [ThreadlineWinInput]::Scroll($Window, $script:app.Id, $dpi, $point, $wheel)
    } catch {
        $cause = $_.Exception.GetBaseException()
        if ($cause -is [ThreadlineWinInput+TargetUnavailableException]) {
            Stop-ScrollBlocked $cause.Message
        }
        throw
    } finally {
        $measurement.wheel_input[$Control] = [ordered]@{
            calibrated = $wheel.Calibrated
            accepted_down = $wheel.AcceptedDown
            accepted_up = $wheel.AcceptedUp
            physical_target_readbacks = $wheel.PhysicalReadbacks
            repositioned_strokes = $wheel.Repositioned
            interrupted_stroke = $wheel.StrokeInProgress
            first_missed_readback = $wheel.FirstMissedReadback
            mean_gap_ms = [Math]::Round($wheel.MeanGapMs, 2)
            longest_gap_ms = [Math]::Round($wheel.LongestGapMs, 2)
        }
    }

    $latest = $state
    $lastCount = $before
    $deadline = [Diagnostics.Stopwatch]::StartNew()
    $settling = [Diagnostics.Stopwatch]::StartNew()
    while ($deadline.ElapsedMilliseconds -lt 1000) {
        Assert-ScrollProcess
        $observation = Read-Observation
        if ($null -ne $observation) {
            $count = @($observation.scroll_frame_times_ms).Count
            if ($count -gt $lastCount) {
                $latest = $observation
                $lastCount = $count
                $settling.Restart()
            }
        }
        if ($lastCount -gt $before -and $settling.ElapsedMilliseconds -ge 240) { break }
        Start-Sleep -Milliseconds 40
    }
    Assert-ScrollProcess

    $newFrames = @($latest.scroll_frame_times_ms |
        Select-Object -Skip $before | Where-Object { $_ -gt 0 })
    $sortedFrames = @($newFrames | Sort-Object)
    return [ordered]@{
        sample_count = $newFrames.Count
        frame_times_ms = $newFrames
        p95_ms = if ($wheel.Complete -and $newFrames.Count -ge 10) {
            [double]$sortedFrames[[int][Math]::Ceiling($newFrames.Count * 0.95) - 1]
        } else { $null }
        input_detail = if (!$wheel.Complete) { 'At least one accepted wheel stroke lacked a matching physical pointer readback.' } else { $null }
    }
}

function Compare-Measurement($Actual, [double]$Limit) {
    if ($null -eq $Actual) { return 'Blocked' }
    if ($Actual -gt $Limit) { return 'Fail' }
    return 'Pass'
}

function Get-GpuBudget($ObservedMaximum, [int]$Samples, [double]$Limit) {
    $status = Compare-Measurement $ObservedMaximum $Limit
    if ($Samples -lt 2 -and $status -eq 'Pass') { $status = 'Blocked' }
    return [ordered]@{ value = $ObservedMaximum; sample_count = $Samples; limit = $Limit; status = $status }
}

function Get-ScrollBudget($Breakdown, $Worst, [double]$Limit) {
    $surfaces = [ordered]@{}
    foreach ($surface in $Breakdown.Keys) {
        $result = $Breakdown[$surface]
        $value = if ($null -ne $result) { $result.p95_ms } else { $null }
        $sampleCount = if ($null -ne $result) { $result.sample_count } else { $null }
        $surfaces[$surface] = [ordered]@{
            value = $value
            sample_count = $sampleCount
            limit = $Limit
            status = Compare-Measurement $value $Limit
        }
    }
    $statuses = $surfaces.Values.status
    $status = if ($statuses -contains 'Fail') { 'Fail' } elseif ($statuses -contains 'Blocked') { 'Blocked' } else { 'Pass' }
    return [ordered]@{ value = $Worst; limit = $Limit; status = $status; surfaces = $surfaces }
}

function Invoke-PlatformProof([IntPtr]$Window) {
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, WindowsBase
    $initial = Read-Observation
    if ($initial.tray_enabled) { throw 'The tray preference was unexpectedly enabled in the fresh profile.' }
    Invoke-ToolbarClick $Window 'tray-toggle'
    $null = Wait-State 'tray opt-in acknowledgment' { param($state) $state.tray_enabled -eq $true }
    Add-Evidence 'Use Keep in tray to opt into native background residence' $true

    [ThreadlineWinInput]::Chord(@(0x12, 0x73))
    Wait-For 'close to minimize the live app' { !$script:app.HasExited -and [ThreadlineWinInput]::IsIconic($Window) }
    Add-Evidence 'Close the main window while tray is enabled' ([ordered]@{ pid = $script:app.Id; minimized = $true })
    $beforeRestore = Read-Observation
    $trayOpened = $false
    if ($ManualTrayClick) {
        Write-Host 'Open the Windows notification area overflow if needed and click the Threadline tray icon.'
    }
    if ($ManualTrayClick -or (Invoke-TrayIconClick)) {
        try {
            $null = Wait-State 'system tray restores the existing app' {
                param($state) $state.activations -gt $beforeRestore.activations -and $state.pid -eq $script:app.Id
            }
            $trayOpened = $true
            Add-Evidence 'Click the actual Windows tray icon' $script:app.Id
        } catch {
            Add-Evidence 'Click the actual Windows tray icon' 'No activation observed within the bounded wait'
        }
    } else {
        Add-Evidence 'Click the actual Windows tray icon' 'Manual click evidence pending'
    }
    if (!$trayOpened) {
        $restore = Start-Threadline $firstProfile (Join-Path $ReportDir 'tray-restore-observation.json') -OpenReview
        try {
            Wait-For 'tray instance secondary activation' { $restore.HasExited }
            if ($restore.ExitCode -ne 0) { throw "Tray restore launch returned $($restore.ExitCode)." }
            $null = Wait-State 'secondary activation reached the background process' { param($state) $state.activations -gt $beforeRestore.activations }
            Add-Evidence 'Restore the existing background process over the instance channel' $script:app.Id
        } finally {
            if (!$restore.HasExited) { $restore.Kill() }
            $restore.Dispose()
        }
    }

    Set-WindowFocus $Window
    [ThreadlineWinInput]::Chord(@(0x11, 0x32))
    [ThreadlineWinInput]::Chord(@(0x11, 0x09))
    $before = Wait-State 'second PR before notification delivery' { param($state) $state.right_tab -eq 68 }
    Invoke-ToolbarClick $Window 'test-notification'
    try {
        $shown = Wait-State 'NIN_BALLOONSHOW callback from the Windows shell' { param($state) $state.notifications_shown -gt $before.notifications_shown }
    } catch {
        Stop-Blocked 'The Windows shell did not report showing the test banner. Inspect Windows session, app notification rules and Do not disturb.'
    }
    Add-Evidence 'Windows shell confirmed the test banner displayed' $shown.notifications_shown
    if ($ManualNotificationClick) {
        Write-Host 'Click the visible Threadline review banner while it remains displayed.'
    } else {
        $banner = Wait-ShellControl 'Threadline review'
        if ($null -eq $banner) {
            Stop-Blocked 'Windows reported the banner shown, but UI Automation could not identify its clickable title. Use the manual notification click mode on this target.'
        }
        try { Invoke-ShellClick $banner } catch { Stop-Blocked 'UI Automation found the title, but a safe physical click target could not be verified. Use manual notification click mode.' }
    }
    try {
        $activated = Wait-State 'notification click routes to PR 72 in the existing app' {
            param($state) $state.activations -gt $before.activations -and $state.right_tab -eq 72
        }
    } catch {
        Stop-Blocked 'The actual Windows banner click was not observed by the running app.'
    }
    if ($activated.pid -ne $script:app.Id) { throw 'Notification opened an unexpected process.' }
    Add-Evidence 'Click the actual Windows notification and open the review in the original process' ([ordered]@{ pid = $activated.pid; right_tab = $activated.right_tab })
    Set-WindowFocus $Window
    Invoke-ToolbarClick $Window 'tray-toggle'
    $null = Wait-State 'tray opt-out acknowledgment' { param($state) !$state.tray_enabled }
    Crop-Screenshot $Window 'platform-activated.png'
    [ThreadlineWinInput]::Chord(@(0x12, 0x73))
    Wait-For 'close without tray to exit the process' { $script:app.HasExited }
    if ($script:app.ExitCode -ne 0) { throw "Threadline exited with code $($script:app.ExitCode)." }
    Add-Evidence 'Disable the tray and close to quit' 0
    if (!$trayOpened) {
        Stop-Blocked 'Native tray opt-in and persistence passed. An actual tray-icon click still needs manual evidence.'
    }
}

function Invoke-ToastProof([IntPtr]$Window) {
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, WindowsBase
    . (Join-Path $PSScriptRoot 'windows-notification-focus.ps1')
    $fixtureTitle = 'Threadline review ' + $script:toastFixture.Token.Substring(0, 8)
    $fixtureActionLabel = 'Open review ' + $script:toastFixture.Token.Substring(0, 8)
    $fixtureAppDisplayName = $script:toastFixture.Display

    $initial = Wait-State 'PR 72 in the original foreground app' {
        param($state)
        $state.pid -eq $script:app.Id -and $state.right_tab -eq 72 -and
            [ThreadlineWinInput]::GetForegroundWindow() -eq $Window
    }
    if ($null -eq $initial.PSObject.Properties['notifications_submitted'] -or
        $null -eq $initial.PSObject.Properties['notification_com_activations']) {
        throw 'The selected executable lacks the registered toast test observations.'
    }
    [ThreadlineWinInput]::Chord(@(0x11, 0x32))
    [ThreadlineWinInput]::Chord(@(0x11, 0x09))
    $before = Wait-State 'PR 68 selected by native input in the original app' {
        param($state) $state.pid -eq $initial.pid -and $state.right_tab -eq 68
    }
    Add-Evidence 'Select PR 68 before submitting the test toast' ([ordered]@{ pid = $before.pid; right_tab = $before.right_tab })
    Initialize-FixtureNotificationProof
    $script:toastSubmissionAttempted = $true
    [ThreadlineWinInput]::Chord(@(0x11, 0x10, 0x78))
    try {
        $submitted = Wait-State 'one registered toast submission from the original app' {
            param($state) $state.pid -eq $before.pid -and $state.notifications_submitted -eq $before.notifications_submitted + 1
        }
    } catch {
        if ($script:app.HasExited) { throw 'Threadline exited before submitting the registered toast.' }
        Stop-Blocked 'The original process did not report a successful registered toast submission.'
    }
    Add-Evidence 'Observe one new WinRT submission from the original app' $submitted.notifications_submitted
    if (!(Invoke-FixtureCenterClick $Window)) {
        Stop-Blocked 'The signed Notification Center did not expose a usable default card action or Open review button for this fixture.'
    }

    $activated = Wait-State 'registered shell action reaches COM and opens PR 72 in the original app' {
        param($state)
        $state.pid -eq $before.pid -and $state.right_tab -eq 72 -and
            $state.activations -gt $before.activations -and
            $state.notification_com_activations -eq $before.notification_com_activations + 1
    }
    Add-Evidence 'Open PR 72 through one COM activation in the existing process' ([ordered]@{
        pid = $activated.pid; right_tab = $activated.right_tab
        com_activations_before = $before.notification_com_activations
        com_activations_after = $activated.notification_com_activations
    })
    $report.winrt_activation_verified = $true
    Wait-For 'the original Threadline window returns to the foreground' { [ThreadlineWinInput]::GetForegroundWindow() -eq $Window }
    Stop-ThroughUi
    Add-Evidence 'Quit the isolated app with Ctrl+Q' 0
}

function Complete-AppToastFixture {
    $historyClean = !$script:toastSubmissionAttempted
    for ($attempt = 0; $attempt -lt 2 -and !$historyClean; $attempt++) {
        try { $historyClean = Clear-AppToastFixtureHistory }
        catch { $historyClean = $false }
    }
    if (!$historyClean) {
        Add-Evidence 'Retain the owned registration so history cleanup can be retried' ([ordered]@{
            app_identity = $script:toastFixture.Identity
            class_id = $script:toastFixture.ClassName
        })
        $script:report.status = 'Fail'
        $script:report.detail = 'The registered toast proof could not verify fixture history cleanup.'
        return
    }
    if (!(Remove-AppToastFixture)) {
        $script:report.status = 'Fail'
        $script:report.detail = 'The registered toast proof could not verify shortcut and COM cleanup.'
    }
}

function Invoke-Measurement([IntPtr]$Window, [double]$ColdMs) {
    # Wait once to sample the documented idle state. Interaction waits use live observation conditions.
    Start-Sleep -Seconds 3
    $script:app.Refresh()
    $measurement.idle_working_set_bytes = $script:app.WorkingSet64
    $measurement.cold_start_ms = [Math]::Round($ColdMs, 2)
    $gpuIdle = Get-GpuProcessMemory $script:app.Id
    $measurement.wheel_input = [ordered]@{}
    $measurement.scroll_breakdown = [ordered]@{ large_diff = $null; markdown_discussion = $null }
    $scrollInterrupted = $null
    $gpuAfterScroll = $null
    try {
        $measurement.scroll_breakdown.large_diff = Measure-Scroll $Window 'diff'
        $measurement.scroll_breakdown.markdown_discussion = Measure-Scroll $Window 'markdown_copy'
    } catch {
        if (!$script:blocked) { throw }
        $script:blocked = $false
        Assert-ScrollProcess
        $scrollInterrupted = $_.Exception.Message
        $measurement.scroll_interruption = $scrollInterrupted
    }
    if (!$scrollInterrupted) {
        $script:app.Refresh()
        $measurement.peak_working_set_bytes = $script:app.PeakWorkingSet64
        $gpuAfterScroll = Get-GpuProcessMemory $script:app.Id
    }
    $measurement.gpu_breakdown = [ordered]@{ idle = $gpuIdle; after_scroll = $gpuAfterScroll }
    $gpuTotals = @($gpuIdle, $gpuAfterScroll | Where-Object { $null -ne $_ } | ForEach-Object { $_.dedicated_bytes + $_.shared_bytes })
    $measurement.gpu_memory_bytes = if ($gpuTotals.Count -gt 0) { ($gpuTotals | Measure-Object -Maximum).Maximum } else { $null }
    $surfaceP95 = @($measurement.scroll_breakdown.Values | Where-Object { $null -ne $_ -and $null -ne $_.p95_ms } | ForEach-Object { $_.p95_ms })
    $measurement.scroll_frame_p95_ms = if ($surfaceP95.Count -eq 2) { [Math]::Max($surfaceP95[0], $surfaceP95[1]) } else { $null }
    $measurement.package_bytes = if ($Artifact) { (Get-Item -LiteralPath $Artifact).Length } else { $null }
    $measurement.package_sha256 = if ($Artifact) { Get-FileSha256 $Artifact } else { $null }
    if (!$scrollInterrupted) {
        Assert-ScrollProcess
        Crop-Screenshot $Window 'measurement.png'
        Assert-ScrollProcess
        Stop-ThroughUi
    }

    if (!$BudgetFile -or !(Test-Path -LiteralPath $BudgetFile -PathType Leaf)) {
        if ($scrollInterrupted) { Stop-Blocked $scrollInterrupted }
        Stop-Blocked 'Measurements captured. The reviewed Windows budget file is unavailable.'
    }
    $budgets = Get-Content -LiteralPath $BudgetFile -Raw | ConvertFrom-Json
    $evaluations = [ordered]@{}
    foreach ($metric in @('idle_working_set_bytes', 'peak_working_set_bytes', 'gpu_memory_bytes', 'package_bytes', 'cold_start_ms', 'scroll_frame_p95_ms')) {
        $actual = $measurement[$metric]
        $limit = $budgets.$metric
        if ($limit -le 0) { throw "The budget for $metric must be positive." }
        $evaluations[$metric] = if ($metric -eq 'scroll_frame_p95_ms') {
            Get-ScrollBudget $measurement.scroll_breakdown $actual $limit
        } elseif ($metric -eq 'gpu_memory_bytes') {
            Get-GpuBudget $actual $gpuTotals.Count $limit
        } else {
            [ordered]@{ value = $actual; limit = $limit; status = Compare-Measurement $actual $limit }
        }
    }
    $report.budgets = $evaluations
    if ($evaluations.Values.status -contains 'Fail') { throw 'One or more Windows performance budgets were exceeded.' }
    if ($scrollInterrupted) { Stop-Blocked $scrollInterrupted }
    if ($evaluations.Values.status -contains 'Blocked') { Stop-Blocked 'One or more Windows performance measurements are unavailable. See individual budget results.' }
}

try {
    if (@($Measure, $PlatformProof, $ToastProof, $AccessProof, [bool]$ReaderProof, $AccessibilityProof | Where-Object { $_ }).Count -gt 1) {
        throw 'Select one native evidence mode per run.'
    }
    if (($ManualTrayClick -or $ManualNotificationClick) -and !$PlatformProof) { throw 'Manual shell checkpoints require -PlatformProof.' }
    if ($report.session_id -eq 0 -or ![Environment]::UserInteractive) {
        Stop-Blocked 'The driver needs an unlocked interactive Windows desktop outside Session 0.'
    }
    if ($report.architecture -ne 'AMD64') {
        Stop-Blocked 'The driver expects the Windows x64 target.'
    }
    Add-Type -Path (Join-Path $PSScriptRoot 'ThreadlineWinInput.cs')
    [void][ThreadlineWinInput]::SetProcessDpiAwarenessContext([IntPtr](-4))
    Add-Type -AssemblyName System.Windows.Forms, System.Drawing

    $Binary = (Resolve-Path -LiteralPath $Binary).Path
    $ReportDir = (Resolve-Path -LiteralPath $ReportDir).Path
    $reportPath = Join-Path $ReportDir $reportName
    $reportFile = Join-Path $ReportDir 'ui-observation.json'
    $report.binary_sha256 = Get-FileSha256 $Binary
    $report.executable_bytes = (Get-Item -LiteralPath $Binary).Length
    $operatingSystem = Get-CimInstance Win32_OperatingSystem
    $report.windows_product = $operatingSystem.Caption
    $report.os_version = $operatingSystem.Version
    $platform = Get-CimInstance Win32_ComputerSystem
    $report.machine = [ordered]@{
        manufacturer = $platform.Manufacturer
        model = $platform.Model
        ram_bytes = $platform.TotalPhysicalMemory
        processors = @(Get-CimInstance Win32_Processor | Select-Object Name, NumberOfCores, NumberOfLogicalProcessors)
        graphics = @(Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion)
        storage = @(Get-CimInstance Win32_DiskDrive | Select-Object Model, InterfaceType, MediaType)
        display = [ordered]@{ width = [Windows.Forms.Screen]::PrimaryScreen.Bounds.Width; height = [Windows.Forms.Screen]::PrimaryScreen.Bounds.Height }
    }
    if (!$ProfileDir) {
        $ProfileDir = Join-Path ([IO.Path]::GetTempPath()) ('threadline-e2e-' + [guid]::NewGuid().ToString('N'))
    }
    if ($AccessProof -or $ReaderProof) {
        $fixtureAddress = [regex]::Match($AccessAddress, '^127\.0\.0\.1:([1-9][0-9]{0,4})$')
        if ($Measure -or $PlatformProof -or $ToastProof -or ($AccessProof -and $ReaderProof) -or
            !$fixtureAddress.Success -or [int]$fixtureAddress.Groups[1].Value -gt 65535 -or !$AccessPhaseFile -or
            [IO.Path]::GetFullPath($AccessPhaseFile) -ine [IO.Path]::GetFullPath((Join-Path $ProfileDir 'access-fixture-phase.txt')) -or
            !(Test-Path -LiteralPath $AccessPhaseFile -PathType Leaf)) {
            throw 'Use the canonical runner with an isolated loopback GitHub fixture.'
        }
        . (Join-Path $PSScriptRoot 'windows-github-access.ps1')
    }
    if ($AccessibilityProof -or $AccessProof -or $script:UsesNativeLifecycleFixture -or $script:UsesPaneFixture -or $ReaderProof -in @('Resume', 'CursorRecovery', 'Historical', 'Sync', 'RateLimit', 'NotificationSchedule', 'PullRequest', 'SharedPullRequest', 'PullRequestMergeState', 'PullRequestOfflinePin', 'ReviewDraft', 'ReviewDecisions', 'ReviewPredecessor', 'ThreadQueue', 'ReviewContext', 'DirectMerge', 'MergeQueue', 'MergeCancel', 'CredentialBoundary', 'NativeActivity', 'History', 'CommentDependency', 'IssueConflict', 'IssuePropagation', 'Writes', 'WriteSurface', 'Performance', 'PerformancePhysical', 'PerformanceConcurrent', 'LargeWorkspace')) {
        Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
        . (Join-Path $PSScriptRoot 'windows-owned-uia.ps1')
        $script:UseOwnedUiAutomation = !$AccessibilityProof -and $ReaderProof -ne 'PerformancePhysical'
        $report.driver = 'Windows PowerShell / owned UI Automation and guarded Win32 SendInput'
    }
    if ($ReaderProof) { . (Join-Path $PSScriptRoot 'windows-cached-reader.ps1') }
    if ($script:UsesPaneFixture) { . (Join-Path $PSScriptRoot 'windows-pane-positions.ps1') }
    if ($ReaderProof -eq 'Historical') { . (Join-Path $PSScriptRoot 'windows-historical-reader.ps1') }
    if ($ReaderProof -in @('CursorRecovery', 'Historical', 'Sync', 'RateLimit', 'NotificationSchedule', 'PullRequestOfflinePin', 'ReviewDraft', 'ReviewDecisions', 'ReviewPredecessor', 'ThreadQueue', 'DirectMerge', 'MergeQueue', 'MergeCancel', 'NativeLifecycle', 'NativeActivity', 'PerformanceConcurrent')) { . (Join-Path $PSScriptRoot 'windows-sync.ps1') }
    if ($ReaderProof -in @('PullRequest', 'SharedPullRequest', 'PullRequestMergeState', 'PullRequestOfflinePin', 'ReviewDraft', 'ReviewDecisions', 'ReviewPredecessor', 'ThreadQueue', 'ReviewContext', 'DirectMerge', 'MergeQueue', 'MergeCancel', 'NativeLifecycle', 'NativeActivity', 'History')) { . (Join-Path $PSScriptRoot 'windows-pr-reader.ps1') }
    if ($script:UsesPaneFixture -or $ReaderProof -in @('CursorRecovery', 'RateLimit', 'NotificationSchedule', 'ReviewDraft', 'ReviewDecisions', 'ReviewPredecessor', 'ThreadQueue', 'ReviewContext', 'DirectMerge', 'MergeQueue', 'MergeCancel', 'CredentialBoundary', 'History', 'CommentDependency', 'IssueConflict', 'IssuePropagation', 'Writes', 'WriteSurface', 'PullRequestMergeState', 'PullRequestOfflinePin', 'OfflineMixedOutbox')) { . (Join-Path $PSScriptRoot 'windows-issue-writes.ps1') }
    if ($ReaderProof -in @('SharedPullRequest', 'ReviewDraft', 'ReviewDecisions', 'ReviewPredecessor', 'ThreadQueue', 'ReviewContext', 'NativeLifecycle', 'History')) { . (Join-Path $PSScriptRoot 'windows-review-context.ps1') }
    if ($ReaderProof -in @('DirectMerge', 'MergeQueue', 'MergeCancel')) { . (Join-Path $PSScriptRoot 'windows-merge.ps1') }
    if ($ReaderProof -in @('CredentialBoundary', 'NativeLifecycle')) { . (Join-Path $PSScriptRoot 'windows-credential-boundary.ps1') }
    if ($script:UsesNativeLifecycleFixture) { . (Join-Path $PSScriptRoot 'windows-native-lifecycle.ps1') }
    if ($ReaderProof -eq 'NativeActivity') { . (Join-Path $PSScriptRoot 'windows-native-activity.ps1') }
    if ($ReaderProof -eq 'PullRequestOfflinePin') { . (Join-Path $PSScriptRoot 'windows-pr-offline-pin.ps1') }
    if ($ReaderProof -ceq 'CursorRecovery') { . (Join-Path $PSScriptRoot 'windows-reader-cursor-recovery.ps1') }
    if ($ReaderProof -in @('History', 'NativeLifecycle')) { . (Join-Path $PSScriptRoot 'windows-history.ps1') }
    $firstProfile = Join-Path $ProfileDir ('initial-' + [guid]::NewGuid().ToString('N'))
    $null = New-Item -ItemType Directory -Path $firstProfile -Force
    if ($ReaderProof -eq 'Offline') { Initialize-ReaderProfile $firstProfile 'small' }
    if ($script:UsesPaneFixture) { Initialize-ReaderProfile $firstProfile 'small-pane-positions' }
    if ($ReaderProof -in @('Performance', 'PerformancePhysical')) { Initialize-ReaderProfile $firstProfile 'section-16' }
    if ($ReaderProof -in @('LargeWorkspace', 'PerformanceConcurrent')) { Initialize-ReaderProfile $firstProfile 'section-16-backlog' }
    if ($script:UsesNativeLifecycleFixture) { Initialize-NativeLifecycle $firstProfile }
    if ($ReaderProof -eq 'NativeActivity') { Initialize-NativeActivity $firstProfile 'native-activity-seeded.json' }
    if ($script:UseSeededFixtureAuth) {
        $fixturePhase = switch ($ReaderProof) {
            Historical { 'historical-local' }
            Resume { 'reader-failure' }
            CursorRecovery { 'recover-baseline' }
            Sync { 'sync-live' }
            RateLimit { 'sync-rate-ready' }
            NotificationSchedule { 'sync-live' }
            NativeActivity { 'activity-offline' }
            PullRequest { 'pr-load' }
            SharedPullRequest { 'pr-shared-ready' }
            PullRequestMergeState { 'pr-merge-state' }
            PullRequestOfflinePin { 'pr-pin-load' }
            ReviewDraft { 'pr-load' }
            ReviewDecisions { 'review-decisions' }
            ReviewPredecessor { 'review-predecessor' }
            ThreadQueue { 'thread-queue' }
            ReviewContext { 'pr-load' }
            DirectMerge { 'merge-prepare' }
            MergeQueue { 'merge-prepare' }
            MergeCancel { 'merge-prepare' }
            History { 'pr-load' }
            IssueConflict { 'issue-edit-baseline' }
            IssuePropagation { 'issue-edit-baseline' }
            PerformanceConcurrent { 'backlog-auth' }
            default { 'write-online' }
        }
        Set-AccessPhase $fixturePhase
        $report.authentication_setup = 'Fixed isolated loopback identity, verified by the application'
    }
    if ($ToastProof) {
        if (!(Test-Path -LiteralPath (Join-Path $PSScriptRoot 'windows-notification-focus.ps1') -PathType Leaf)) {
            throw 'The registered toast action helper is missing from this test build.'
        }
        . (Join-Path $PSScriptRoot 'windows-app-toast.ps1')
        Register-AppToastFixture
    }
    $launchBegin = [Diagnostics.Stopwatch]::GetTimestamp()
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $app = Start-Threadline $firstProfile $reportFile
    $launchReturned = [Diagnostics.Stopwatch]::GetTimestamp()
    $report.app_pid = $app.Id
    $window = Get-ReadyWindow
    if ($ReaderProof -eq 'LargeWorkspace') { $cachedReady = Wait-LargeWorkspaceReady $window }
    $coldMs = $clock.Elapsed.TotalMilliseconds
    if ($ReaderProof -eq 'LargeWorkspace') {
        $initialStartupTiming = Get-LargeStartupTiming $cachedReady $launchBegin $launchReturned ([Diagnostics.Stopwatch]::GetTimestamp())
    }
    $report.native_window = $window.ToInt64()
    $report.dpi = [ThreadlineWinInput]::GetDpiForWindow($window)
    if ($AccessibilityProof) {
        Wait-For 'the owned native Load preview accessibility action' {
            Invoke-OwnedFixtureControl $window 'load-preview' $null 'Load preview'
        }
        $null = Wait-State 'the preview loaded by the accessibility action' {
            param($state) $state.preview_state -ceq 'ready'
        }
        $report.accessibility_action_verified = $true
    }
    if (!$script:UseOwnedUiAutomation) { Set-WindowFocus $window }
    $report.loaded_modules = @($app.Modules | ForEach-Object { $_.ModuleName } | Sort-Object -Unique)
    if ($report.loaded_modules | Where-Object { $_ -match '^(lib)?(webkit|wry|electron|cef|chromium|msedgewebview2)' }) {
        throw 'The native process loaded a browser runtime module.'
    }

    if ($Measure) {
        $report.measurements = $measurement
        Invoke-Measurement $window $coldMs
        $report.detail = 'Windows baseline captured and compared with its numeric budgets.'
    } elseif ($ToastProof) {
        Invoke-ToastProof $window
        $report.detail = 'The registered notification action reached COM and opened PR 72 in the original Threadline process.'
    } elseif ($PlatformProof) {
        Invoke-PlatformProof $window
        $report.detail = 'Observed native tray behavior, shell notification delivery and notification click into the existing process.'
    } elseif ($AccessProof) {
        Invoke-GitHubAccessProof $window $firstProfile
        $report.detail = 'Native account controls verified the synthetic GitHub access and SSO journey and restored repository selection after restart.'
    } elseif ($ReaderProof) {
        switch ($ReaderProof) {
            Offline { Invoke-OfflineReaderProof $window $firstProfile }
            PanePositions { Invoke-CrossRepositoryPaneProof $window }
            PaneLifecycle { Invoke-PaneLifecycleProof $window $firstProfile }
            Historical { Invoke-HistoricalReader $window $firstProfile }
            Resume { Invoke-ReaderResumeProof $window $firstProfile }
            CursorRecovery { Invoke-ReaderCursorRecoveryProof $window $firstProfile }
            Performance { Invoke-ReaderPerformance $window }
            PerformancePhysical { Invoke-ReaderPerformance $window -Physical }
            PerformanceConcurrent { Invoke-ConcurrentReaderPerformance $window }
            LargeWorkspace { Invoke-LargeWorkspaceProof $window $firstProfile $coldMs $initialStartupTiming }
            Sync { Invoke-SyncProof $window $firstProfile }
            RateLimit { Invoke-RateLimitedCacheProof $window }
            NotificationSchedule { Invoke-NotificationScheduleProof $window $firstProfile }
            PullRequest { Invoke-PrReaderProof $window }
            SharedPullRequest { Invoke-SharedPrPanesProof $window }
            PullRequestMergeState { Invoke-PrMergeStateProof $window $firstProfile }
            PullRequestOfflinePin { Invoke-PrOfflinePinProof $window $firstProfile }
            ReviewDraft { Invoke-ReviewDraftProof $window $firstProfile }
            ReviewDecisions { Invoke-ReviewDecisionsProof $window $firstProfile }
            ReviewPredecessor { Invoke-ReviewDecisionsProof $window $firstProfile -Predecessor }
            ThreadQueue { Invoke-ThreadQueueProof $window $firstProfile }
            ReviewContext { Invoke-ReviewContextProof $window $firstProfile }
            DirectMerge { Invoke-MergeProof $window }
            MergeQueue { Invoke-MergeQueueProof $window }
            MergeCancel { Invoke-MergeCancellationProof $window }
            CredentialBoundary { Invoke-CredentialBoundaryProof $window $firstProfile }
            NativeLifecycle { Invoke-NativeLifecycleProof $window $firstProfile }
            OfflineMixedOutbox { Invoke-OfflineMixedOutboxProof $window $firstProfile }
            NativeActivity { Invoke-NativeActivityProof $window $firstProfile }
            History { Invoke-HistoryProof $window $firstProfile }
            CommentDependency { Invoke-CommentDependencyProof $window $firstProfile }
            IssueConflict { Invoke-IssueConflictProof $window $firstProfile }
            IssuePropagation { Invoke-IssuePropagationProof $window }
            Writes { Invoke-IssueWriteProof $window $firstProfile }
            WriteSurface { Invoke-IssueWriteSurfaceProof $window }
        }
    } else {
        try {
            $clipboardBackup = [Windows.Forms.Clipboard]::GetDataObject()
        } catch {
            Stop-Blocked 'Could not preserve the Windows clipboard. Retry on a disposable or idle desktop.'
        }
        Invoke-Smoke $window
        $app.Dispose()
        $app = $null
        $initialDatabase = Get-IsolatedSqlite $firstProfile
        $reportFile = Join-Path $ReportDir 'restart-observation.json'
        $app = Start-Threadline $firstProfile $reportFile
        $report.restarted_pid = $app.Id
        $window = Get-ReadyWindow
        Set-WindowFocus $window
        Stop-ThroughUi
        $restartDatabase = Get-IsolatedSqlite $firstProfile
        if ($restartDatabase.creation_time_ticks -ne $initialDatabase.creation_time_ticks) {
            throw 'The SQLite database was replaced during the same-profile restart.'
        }
        Add-Evidence 'Relaunch the same isolated profile and retain its SQLite database' ([ordered]@{
            pid = $report.restarted_pid
            sqlite_header = $true
            initial_sqlite_sha256 = $initialDatabase.sha256
            restart_sqlite_sha256 = $restartDatabase.sha256
            retained = $true
        })
        if ($AccessibilityProof) {
            $report.native_input_verified = $true
            $report.high_dpi_verified = $report.dpi -ge 144
            $report.automated_checks_passed = $report.high_dpi_verified
            $report.manual_screen_reader_and_visual = 'Pending'
            if (!$report.high_dpi_verified) {
                Stop-Blocked 'Native keyboard, focus, exact Unicode clipboard and owned UI Automation passed at the recorded DPI. Repeat at 150% or higher and retain signed human visual and screen-reader observations.'
            }
            Stop-Blocked 'Native keyboard, focus, exact Unicode clipboard, high DPI and owned UI Automation passed. Signed human visual and screen-reader observations are pending.'
        }
        $report.detail = 'Exercised the native window, Markdown selection, independent focus and tabs, review input, and both splitters.'
    }
    $report.status = 'Pass'
} catch {
    if (($AccessProof -or $ReaderProof) -and $app -and !$app.HasExited -and
        $_.Exception.InnerException -is [ThreadlineWinInput+TargetUnavailableException]) {
        $blocked = $true
    }
    $report.status = if ($blocked) { 'Blocked' } else { 'Fail' }
    $report.detail = $_.Exception.Message
    $report.driver_location = if ($AccessProof -or $ReaderProof -or $AccessibilityProof) { $_.InvocationInfo.ScriptLineNumber } else { $_.InvocationInfo.PositionMessage }
    if (!$ToastProof -and !$AccessProof -and !$ReaderProof -and $app -and !$app.HasExited -and $app.MainWindowHandle -ne [IntPtr]::Zero) {
        try {
            if ([ThreadlineWinInput]::GetForegroundWindow() -eq $app.MainWindowHandle) {
                Crop-Screenshot $app.MainWindowHandle 'failure.png'
            }
        } catch { Write-Warning "Could not capture the failure window: $_" }
    }
} finally {
    if ($null -ne $copiedClipboardSequence -and
        [ThreadlineWinInput]::GetClipboardSequenceNumber() -eq $copiedClipboardSequence) {
        try {
            if ($null -eq $clipboardBackup) { [Windows.Forms.Clipboard]::Clear() }
            else { [Windows.Forms.Clipboard]::SetDataObject($clipboardBackup, $true, 5, 100) }
            $report.original_clipboard_restored = $true
        } catch {
            $report.original_clipboard_restored = $false
            $report.status = 'Fail'
            $report.detail = 'Could not restore the original Windows clipboard.'
        }
    }
    try {
        if ($app) {
            if (!$app.HasExited) {
                $app.Kill()
                $app.WaitForExit()
            }
            $app.Dispose()
        }
    } finally {
        if ($ToastProof -and $null -ne $toastFixture) {
            Complete-AppToastFixture
        }
        if ($script:UsesNativeLifecycleFixture -and $firstProfile -and
            (Test-Path -LiteralPath (Join-Path $firstProfile 'native-lifecycle.owner') -PathType Leaf)) {
            try {
                $null = Read-NativeLifecycle $firstProfile 'cleanup' 'native-lifecycle-cleanup.json'
            } catch {
                $report.fixture_cleanup_error = 'Could not verify cleanup of the isolated Windows protected fixture key.'
                if ($report.status -eq 'Pass') {
                    $report.status = 'Fail'
                    $report.detail = $report.fixture_cleanup_error
                }
            }
        }
    }
    # The canonical Rust runner owns profile cleanup. An invocation made directly keeps isolated
    # fixture profiles for manual inspection until its caller removes them.
    $json = $report | ConvertTo-Json -Depth 10
    [IO.File]::WriteAllText($reportPath, $json, (New-Object System.Text.UTF8Encoding($false)))
    Write-Output "$($report.status): $($report.detail)"
}

switch ($report.status) {
    'Pass' { exit 0 }
    'Blocked' { exit 78 }
    default { exit 1 }
}

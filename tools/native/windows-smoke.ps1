[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$ReportDir,
    [string]$ProfileDir,
    [UInt64]$Seed = 12345,
    [switch]$Measure,
    [switch]$PlatformProof,
    [switch]$ManualTrayClick,
    [switch]$ManualNotificationClick,
    [string]$Artifact,
    [string]$BudgetFile
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$null = New-Item -ItemType Directory -Force -Path $ReportDir
$reportName = if ($Measure) { 'measurements.json' } elseif ($PlatformProof) { 'platform-native.json' } else { 'native.json' }
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
$secondProfile = $null
$reportFile = Join-Path $ReportDir 'ui-observation.json'
$blocked = $false
$clipboardBackup = $null
$copiedClipboardSequence = $null

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

function Add-Evidence([string]$Action, $Observed) {
    $events.Add([ordered]@{ action = $Action; observed = $Observed })
}

function Stop-Blocked([string]$Message) {
    $script:blocked = $true
    throw $Message
}

function Wait-For([string]$Description, [scriptblock]$Probe, [int]$Timeout = 8000) {
    $wait = [Diagnostics.Stopwatch]::StartNew()
    while ($wait.ElapsedMilliseconds -lt $Timeout) {
        if (& $Probe) { return }
        Start-Sleep -Milliseconds 45
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

function Wait-State([string]$Description, [scriptblock]$Check) {
    Wait-For $Description {
        $state = Read-Observation
        $null -ne $state -and (& $Check $state)
    }
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

function Start-Threadline([string]$Profile, [string]$Observation, [switch]$OpenReview) {
    $info = New-Object System.Diagnostics.ProcessStartInfo
    $info.FileName = $script:Binary
    $info.UseShellExecute = $false
    $info.WorkingDirectory = $Profile
    $info.Arguments = '--e2e-report "' + $Observation + '"'
    if ($Measure) { $info.Arguments += ' --measure' }
    if ($OpenReview) { $info.Arguments += ' --open-review' }
    $info.EnvironmentVariables['THREADLINE_PROFILE_DIR'] = $Profile
    $info.EnvironmentVariables['THREADLINE_TEST_SEED'] = $Seed.ToString([Globalization.CultureInfo]::InvariantCulture)
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

function To-ScreenPoint([IntPtr]$Window, [double]$X, [double]$Y, [double]$Scale) {
    $point = New-Object ThreadlineWinInput+Point
    $point.X = [int][Math]::Round($X * $Scale)
    $point.Y = [int][Math]::Round($Y * $Scale)
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
    $point = To-ScreenPoint $Window ($bounds[0] + $bounds[2] / 2) ($bounds[1] + $bounds[3] / 2) $state.scale_factor
    [ThreadlineWinInput]::Click($point)
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

function Wait-CopiedText([uint32]$Before, [string]$Expected) {
    $wait = [Diagnostics.Stopwatch]::StartNew()
    while ($wait.ElapsedMilliseconds -lt 1600) {
        $sequence = [ThreadlineWinInput]::GetClipboardSequenceNumber()
        if ($sequence -ne $Before) {
            try {
                $text = [Windows.Forms.Clipboard]::GetText()
                if ([ThreadlineWinInput]::GetClipboardSequenceNumber() -eq $sequence -and
                    $text.IndexOf($Expected, [StringComparison]::Ordinal) -ge 0) {
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

function Assert-ClipboardContains([string]$Expected, [switch]$SelectAll) {
    $before = [ThreadlineWinInput]::GetClipboardSequenceNumber()
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        if ($SelectAll) { [ThreadlineWinInput]::Chord(@(0x11, 0x41)) }
        Start-Sleep -Milliseconds 80
        [ThreadlineWinInput]::Chord(@(0x11, 0x43))
        $sequence = Wait-CopiedText $before $Expected
        if ($null -ne $sequence) {
            $script:copiedClipboardSequence = $sequence
            return $attempt
        }
    }
    throw "Native clipboard did not change to the expected synthetic token '$Expected' after three attempts."
}

function Stop-ThroughUi {
    [ThreadlineWinInput]::Chord(@(0x11, 0x51))
    Wait-For 'process to exit after Ctrl+Q' { $script:app.HasExited }
    if ($script:app.ExitCode -ne 0) { throw "Threadline exited with code $($script:app.ExitCode)." }
}

function Invoke-Smoke([IntPtr]$Window) {
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
    $origin = To-ScreenPoint $Window ($markdown[0] + 1) ($markdown[1] + $markdown[3] * 0.5) $left.scale_factor
    $end = To-ScreenPoint $Window ($markdown[0] + $markdown[2] - 1) ($markdown[1] + $markdown[3] * 0.5) $left.scale_factor
    [ThreadlineWinInput]::Drag($origin, $end)
    $markdownAttempts = Assert-ClipboardContains 'Offline review'
    Add-Evidence 'Pointer select formatted Markdown and copy' ([ordered]@{ verified = $true; copy_attempts = $markdownAttempts })

    [ThreadlineWinInput]::Chord(@(0x11, 0x32))
    $null = Wait-State 'right pane focus' { param($state) $state.focused_pane -eq 'right' }
    [ThreadlineWinInput]::Chord(@(0x11, 0x52))
    $null = Wait-State 'background preview starts' { param($state) $state.preview_state -eq 'loading' }
    [ThreadlineWinInput]::Chord(@(0x11, 0x46))
    [ThreadlineWinInput]::TypeText('native draft 72')
    $draft = Wait-State 'text typed into PR review input' { param($state) $state.right_input -eq 'native draft 72' -and $state.left_input -eq '' }
    if ($draft.preview_state -ne 'loading') { throw 'Review input was not observed before planned background completion.' }
    $null = Wait-State 'background preview completes' { param($state) $state.preview_state -eq 'ready' }
    $draftAttempts = Assert-ClipboardContains 'native draft 72' -SelectAll
    Add-Evidence 'Type into the right review input while background preview is pending and copy its value' ([ordered]@{ tab = $draft.right_tab; text = $draft.right_input; copy_attempts = $draftAttempts })

    [ThreadlineWinInput]::Chord(@(0x11, 0x09))
    $switched = Wait-State 'next PR tab without changing the other pane' { param($state) $state.right_tab -eq 68 -and $state.left_tab -eq 41 }
    [ThreadlineWinInput]::Chord(@(0x11, 0x09))
    $restored = Wait-State 'return to PR 72 with draft intact' { param($state) $state.right_tab -eq 72 -and $state.right_input -eq 'native draft 72' }
    Add-Evidence 'Change the active tab and return to the same draft target' ([ordered]@{ switched = $switched.right_tab; restored = $restored.right_tab; draft = $restored.right_input })

    [ThreadlineWinInput]::Chord(@(0x11, 0x10, 0x4C))
    $context = Wait-State 'linked issue context' { param($state) $state.context_mode -eq 'issue' -and $state.right_tab -eq 72 }
    Add-Evidence 'Switch context to linked issue' $context.context_mode

    $outer = $context.bounds.outer_divider
    $start = To-ScreenPoint $Window ($outer[0] + $outer[2] / 2) ($outer[1] + $outer[3] / 2) $context.scale_factor
    $end = To-ScreenPoint $Window ($outer[0] - 62) ($outer[1] + $outer[3] / 2) $context.scale_factor
    [ThreadlineWinInput]::Drag($start, $end)
    $outerState = Wait-SettledLayout 'settled outer divider resize' { param($state) [Math]::Abs($state.outer_ratio - $context.outer_ratio) -gt 0.025 }
    Add-Evidence 'Resize workspace panes with the pointer' $outerState.outer_ratio

    $inner = $outerState.bounds.context_divider
    $start = To-ScreenPoint $Window ($inner[0] + $inner[2] / 2) ($inner[1] + $inner[3] / 2) $outerState.scale_factor
    $end = To-ScreenPoint $Window ($inner[0] + 48) ($inner[1] + $inner[3] / 2) $outerState.scale_factor
    [ThreadlineWinInput]::Drag($start, $end)
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
            param($state) $state.activations -gt $prior.activations -and $state.right_tab -eq 72 -and $state.right_input -eq 'native draft 72'
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

function Measure-Scroll([IntPtr]$Window, [string]$Control) {
    $state = Read-Observation
    $bounds = $state.bounds.PSObject.Properties[$Control].Value
    $before = @($state.scroll_frame_times_ms).Count
    $point = To-ScreenPoint $Window ($bounds[0] + $bounds[2] / 2) ($bounds[1] + $bounds[3] / 2) $state.scale_factor
    [ThreadlineWinInput]::Scroll($point, -25, 24)
    [ThreadlineWinInput]::Scroll($point, 25, 24)

    $latest = $state
    $lastCount = $before
    $deadline = [Diagnostics.Stopwatch]::StartNew()
    $settling = [Diagnostics.Stopwatch]::StartNew()
    while ($deadline.ElapsedMilliseconds -lt 1000) {
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

    $newFrames = @($latest.scroll_frame_times_ms |
        Select-Object -Skip $before | Where-Object { $_ -gt 0 })
    $sortedFrames = @($newFrames | Sort-Object)
    return [ordered]@{
        sample_count = $newFrames.Count
        frame_times_ms = $newFrames
        p95_ms = if ($newFrames.Count -ge 10) {
            [double]$sortedFrames[[int][Math]::Ceiling($newFrames.Count * 0.95) - 1]
        } else { $null }
    }
}

function Compare-Measurement($Actual, [double]$Limit) {
    if ($null -eq $Actual) { return 'Blocked' }
    if ($Actual -gt $Limit) { return 'Fail' }
    return 'Pass'
}

function Get-ScrollBudget($Breakdown, $Worst, [double]$Limit) {
    $surfaces = [ordered]@{}
    foreach ($surface in $Breakdown.Keys) {
        $result = $Breakdown[$surface]
        $surfaces[$surface] = [ordered]@{
            value = $result.p95_ms
            sample_count = $result.sample_count
            limit = $Limit
            status = Compare-Measurement $result.p95_ms $Limit
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

function Invoke-Measurement([IntPtr]$Window, [double]$ColdMs) {
    # Wait once to sample the documented idle state. Interaction waits use live observation conditions.
    Start-Sleep -Seconds 3
    $script:app.Refresh()
    $measurement.idle_working_set_bytes = $script:app.WorkingSet64
    $measurement.cold_start_ms = [Math]::Round($ColdMs, 2)
    $gpuIdle = Get-GpuProcessMemory $script:app.Id
    $diffScroll = Measure-Scroll $Window 'diff'
    $markdownScroll = Measure-Scroll $Window 'markdown_copy'
    $script:app.Refresh()
    $measurement.peak_working_set_bytes = $script:app.PeakWorkingSet64
    $gpuAfterScroll = Get-GpuProcessMemory $script:app.Id
    $measurement.gpu_breakdown = [ordered]@{ idle = $gpuIdle; after_scroll = $gpuAfterScroll }
    $gpuTotals = @($gpuIdle, $gpuAfterScroll | Where-Object { $null -ne $_ } | ForEach-Object { $_.dedicated_bytes + $_.shared_bytes })
    $measurement.gpu_memory_bytes = if ($gpuTotals.Count -gt 0) { ($gpuTotals | Measure-Object -Maximum).Maximum } else { $null }
    $measurement.scroll_breakdown = [ordered]@{ large_diff = $diffScroll; markdown_discussion = $markdownScroll }
    $measurement.scroll_frame_p95_ms = if ($null -ne $diffScroll.p95_ms -and $null -ne $markdownScroll.p95_ms) {
        [Math]::Max($diffScroll.p95_ms, $markdownScroll.p95_ms)
    } else { $null }
    $measurement.package_bytes = if ($Artifact) { (Get-Item -LiteralPath $Artifact).Length } else { $null }
    $measurement.package_sha256 = if ($Artifact) { Get-FileSha256 $Artifact } else { $null }
    Crop-Screenshot $Window 'measurement.png'
    Stop-ThroughUi

    if (!$BudgetFile -or !(Test-Path -LiteralPath $BudgetFile -PathType Leaf)) {
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
        } else {
            [ordered]@{ value = $actual; limit = $limit; status = Compare-Measurement $actual $limit }
        }
    }
    $report.budgets = $evaluations
    if ($evaluations.Values.status -contains 'Fail') { throw 'One or more Windows performance budgets were exceeded.' }
    if ($evaluations.Values.status -contains 'Blocked') { Stop-Blocked 'One or more Windows performance measurements are unavailable. See individual budget results.' }
}

try {
    if ($Measure -and $PlatformProof) { throw 'Select one native evidence mode per run.' }
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
    $firstProfile = Join-Path $ProfileDir ('initial-' + [guid]::NewGuid().ToString('N'))
    $null = New-Item -ItemType Directory -Path $firstProfile -Force
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $app = Start-Threadline $firstProfile $reportFile
    $report.app_pid = $app.Id
    $window = Get-ReadyWindow
    $coldMs = $clock.Elapsed.TotalMilliseconds
    $report.native_window = $window.ToInt64()
    $report.dpi = [ThreadlineWinInput]::GetDpiForWindow($window)
    Set-WindowFocus $window
    $report.loaded_modules = @($app.Modules | ForEach-Object { $_.ModuleName } | Sort-Object -Unique)
    if ($report.loaded_modules | Where-Object { $_ -match '^(lib)?(webkit|wry|electron|cef|chromium|msedgewebview2)' }) {
        throw 'The native process loaded a browser runtime module.'
    }

    if ($Measure) {
        $report.measurements = $measurement
        Invoke-Measurement $window $coldMs
        $report.detail = 'Windows baseline captured and compared with its numeric budgets.'
    } elseif ($PlatformProof) {
        Invoke-PlatformProof $window
        $report.detail = 'Observed native tray behavior, shell notification delivery and notification click into the existing process.'
    } else {
        try {
            $clipboardBackup = [Windows.Forms.Clipboard]::GetDataObject()
        } catch {
            Stop-Blocked 'Could not preserve the Windows clipboard. Retry on a disposable or idle desktop.'
        }
        Invoke-Smoke $window
        $app.Dispose()
        $app = $null
        $secondProfile = Join-Path $ProfileDir ('restart-' + [guid]::NewGuid().ToString('N'))
        $null = New-Item -ItemType Directory -Path $secondProfile -Force
        $reportFile = Join-Path $ReportDir 'restart-observation.json'
        $app = Start-Threadline $secondProfile $reportFile
        $report.restarted_pid = $app.Id
        $window = Get-ReadyWindow
        Set-WindowFocus $window
        Stop-ThroughUi
        Add-Evidence 'Relaunch a fresh isolated feasibility profile and quit again' $report.restarted_pid
        $report.detail = 'Exercised the native window, Markdown selection, independent focus and tabs, review input, and both splitters.'
    }
    $report.status = 'Pass'
} catch {
    $report.status = if ($blocked) { 'Blocked' } else { 'Fail' }
    $report.detail = $_.Exception.Message
    $report.driver_location = $_.InvocationInfo.PositionMessage
    if ($app -and !$app.HasExited -and $app.MainWindowHandle -ne [IntPtr]::Zero) {
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
    if ($app) {
        if (!$app.HasExited) {
            $app.Kill()
            $app.WaitForExit()
        }
        $app.Dispose()
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

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Binary,
    [Parameter(Mandatory = $true)][string]$NativeDriver,
    [Parameter(Mandatory = $true)][string]$ReportDir
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$null = New-Item -ItemType Directory -Path $ReportDir -Force
$reportPath = Join-Path (Resolve-Path -LiteralPath $ReportDir).Path 'display-scale.json'
$sessionId = [Diagnostics.Process]::GetCurrentProcess().SessionId
$report = [ordered]@{
    schema = 1
    scope = 'Opt-in hosted Windows Server high-DPI diagnostic'
    status = 'Blocked'
    detail = 'The display scale diagnostic did not finish.'
    session_id = $sessionId
    windows_product = $null
    monitor_count = $null
    resolution_before = $null
    advertised_resolutions = @()
    mode_enumeration_complete = $null
    resolution_tests = @()
    resolution_apply_code = $null
    resolution_ready = $null
    resolution_restore_code = $null
    resolution_restored = $null
    monitor_dpi_before = $null
    offered_percent = @()
    selectable_percent = @()
    option_sources = @()
    scale_options_settled = $false
    owned_popup_count = $null
    requested_percent = $null
    monitor_dpi_ready = $null
    expected_window_dpi = $null
    smoke_exit_code = $null
    smoke_window_dpi = $null
}
$blockedDetail = $null
$restoreMode = $null

function Stop-Blocked([string]$Detail) {
    $script:blockedDetail = $Detail
    throw 'The hosted display scale diagnostic is blocked.'
}

function Format-Resolution($Mode) {
    if ($null -ne $Mode) { return "$($Mode.Width)x$($Mode.Height)" }
}

function Test-DisplayMode($Actual, $Mode, [switch]$Exact) {
    return $null -ne $Actual -and $Actual.Width -eq $Mode.Width -and $Actual.Height -eq $Mode.Height -and (
        !$Exact -or ($Actual.Frequency -eq $Mode.Frequency -and $Actual.BitsPerPixel -eq $Mode.BitsPerPixel -and
            $Actual.DisplayFlags -eq $Mode.DisplayFlags)
    )
}

function Wait-Resolution($Mode, [switch]$Exact) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.ElapsedMilliseconds -lt 4000) {
        if ([ThreadlineDisplayProbe]::GetSystemMetrics(80) -ne 1) { return $false }
        $actual = [ThreadlineDisplayProbe]::ReadMode(-1)
        if (Test-DisplayMode $actual $Mode -Exact:$Exact) { return $true }
        Start-Sleep -Milliseconds 150
    }
    return $false
}

function Initialize-DisplayResolution {
    $original = [ThreadlineDisplayProbe]::ReadMode(-1)
    if ($null -eq $original -or $original.Width -eq 0 -or $original.Height -eq 0) {
        Stop-Blocked 'The active display resolution could not be read.'
    }
    $report.resolution_before = Format-Resolution $original
    $report.resolution_ready = $report.resolution_before
    if ($original.Width -ge 1920 -and $original.Height -ge 1080) { return }
    $advertised = [Collections.Generic.List[object]]::new()
    $report.mode_enumeration_complete = $false
    for ($index = 0; $index -lt 512; $index++) {
        $mode = [ThreadlineDisplayProbe]::ReadMode($index)
        if ($null -eq $mode) {
            $report.mode_enumeration_complete = $true
            break
        }
        $advertised.Add($mode)
    }
    $report.advertised_resolutions = @($advertised | ForEach-Object { Format-Resolution $_ } | Sort-Object -Unique)
    $qualifying = @($advertised | Where-Object {
        $_.Width -ge 1920 -and $_.Height -ge 1080 -and $_.BitsPerPixel -eq $original.BitsPerPixel -and
        $_.Orientation -eq $original.Orientation -and ($_.DisplayFlags -band 2) -eq 0
    } | Sort-Object @{Expression = { [long]$_.Width * $_.Height }}, @{Expression = { [Math]::Abs([long]$_.Frequency - $original.Frequency) }} |
        Select-Object -First 8)
    foreach ($mode in $qualifying) {
        if ([ThreadlineDisplayProbe]::GetSystemMetrics(80) -ne 1) { Stop-Blocked 'The display topology changed during mode discovery.' }
        $test = [ThreadlineDisplayProbe]::ChangeMode($mode, $true)
        $report.resolution_tests += [ordered]@{ resolution = (Format-Resolution $mode); refresh_hz = $mode.Frequency; cds_test_code = $test }
        if ($test -ne 0) { continue }
        $script:restoreMode = $original
        $report.resolution_apply_code = [ThreadlineDisplayProbe]::ChangeMode($mode, $false)
        if ($report.resolution_apply_code -eq 0 -and (Wait-Resolution $mode)) {
            $report.resolution_ready = Format-Resolution ([ThreadlineDisplayProbe]::ReadMode(-1))
            return
        }
        $actual = [ThreadlineDisplayProbe]::ReadMode(-1)
        if (Test-DisplayMode $actual $original -Exact) {
            $script:restoreMode = $null
            return
        }
        Stop-Blocked 'The tested display mode did not become active and the original mode was no longer present.'
    }
}

function Restore-DisplayResolution {
    if ($null -eq $restoreMode) { return }
    if ([ThreadlineDisplayProbe]::GetSystemMetrics(80) -eq 1) {
        $actual = [ThreadlineDisplayProbe]::ReadMode(-1)
        if (Test-DisplayMode $actual $restoreMode -Exact) {
            $report.resolution_restored = $true
            return
        }
        $report.resolution_restore_code = [ThreadlineDisplayProbe]::ChangeMode($restoreMode, $false)
        $report.resolution_restored = $report.resolution_restore_code -eq 0 -and (Wait-Resolution $restoreMode -Exact)
    } else {
        $report.resolution_restored = $false
    }
}

function Get-DisplayScaleControl {
    $foreground = [ThreadlineDisplayProbe]::GetForegroundWindow()
    if ($foreground -eq [IntPtr]::Zero) { return }
    $ownerId = [ThreadlineDisplayProbe]::Owner($foreground)
    $owner = Get-Process -Id $ownerId -ErrorAction SilentlyContinue
    if (!$owner -or $owner.SessionId -ne $sessionId -or $owner.Path -notin @($settingsPath, $framePath)) { return }
    $window = [Windows.Automation.AutomationElement]::FromHandle($foreground)
    if ($window.Current.Name -cne 'Settings' -or $window.Current.IsOffscreen) { return }
    $desktop = [Windows.Automation.TreeScope]::Descendants
    $condition = [Windows.Automation.AndCondition]::new(
        [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, 'Display'),
        [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::Text)
    )
    $headings = $window.FindAll($desktop, $condition)
    if ($headings.Count -ne 1 -or $headings[0].Current.IsOffscreen) { return }
    $settingsId = $headings[0].Current.ProcessId
    $settings = Get-Process -Id $settingsId -ErrorAction SilentlyContinue
    if (!$settings -or $settings.SessionId -ne $sessionId -or $settings.Path -ne $settingsPath) { return }
    $combo = [Windows.Automation.AndCondition]::new(
        [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty, $settingsId),
        [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::ComboBox)
    )
    $knownLabels = @('Scale', 'Change the size of text, apps, and other items')
    $matches = @($window.FindAll($desktop, $combo) | Where-Object {
        $view = $_.Current
        $view.IsEnabled -and !$view.IsOffscreen -and (
            $view.Name -cin $knownLabels -or ($view.LabeledBy -and $view.LabeledBy.Current.Name -cin $knownLabels)
        )
    })
    if ($matches.Count -eq 1) {
        return [pscustomobject]@{ Window = $foreground; OwnerId = $ownerId; Selector = $matches[0]; ProcessId = $settingsId }
    }
}

function Test-ElementInScope($Element, $Scope) {
    $walker = [Windows.Automation.TreeWalker]::RawViewWalker
    for ($depth = 0; $Element -and $depth -lt 12; $depth++) {
        if ([Windows.Automation.Automation]::Compare($Element, $Scope)) { return $true }
        $Element = $walker.GetParent($Element)
    }
    return $false
}

function Get-OwnedScalePopup($Page) {
    $owner = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty, $Page.ProcessId)
    $roots = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children, $owner)
    $anchor = $Page.Selector.Current.BoundingRectangle
    foreach ($popup in $roots) {
        $view = $popup.Current
        $window = [IntPtr]$view.NativeWindowHandle
        if ($window -eq [IntPtr]::Zero -or $window -eq $Page.Window -or $view.IsOffscreen -or
            [ThreadlineDisplayProbe]::Owner($window) -ne $Page.ProcessId -or
            [ThreadlineDisplayProbe]::GetAncestor($window, 3) -ne $Page.Window) { continue }
        $bounds = $view.BoundingRectangle
        if ($bounds.IsEmpty -or $anchor.IsEmpty) { continue }
        $overlap = [Math]::Min($bounds.Right, $anchor.Right) - [Math]::Max($bounds.Left, $anchor.Left)
        $gap = [Math]::Max($bounds.Top - $anchor.Bottom, $anchor.Top - $bounds.Bottom)
        if ($overlap -ge [Math]::Min($bounds.Width, $anchor.Width) / 2 -and $gap -le $anchor.Height) {
            [pscustomobject]@{ Element = $popup; Window = $window; Source = 'owned-popup' }
        }
    }
}

function Read-ScaleOptions($Page) {
    $popups = @(Get-OwnedScalePopup $Page)
    $scopes = @([pscustomobject]@{ Element = $Page.Selector; Window = $Page.Window; Source = 'combo' })
    if ($popups.Count -eq 1) { $scopes += $popups }
    $controlType = [Windows.Automation.AutomationElement]::ControlTypeProperty
    $optionKind = [Windows.Automation.OrCondition]::new(
        [Windows.Automation.PropertyCondition]::new($controlType, [Windows.Automation.ControlType]::ListItem),
        [Windows.Automation.PropertyCondition]::new($controlType, [Windows.Automation.ControlType]::MenuItem),
        [Windows.Automation.PropertyCondition]::new($controlType, [Windows.Automation.ControlType]::DataItem)
    )
    $options = [Collections.Generic.List[object]]::new()
    foreach ($scope in $scopes) {
        foreach ($option in $scope.Element.FindAll([Windows.Automation.TreeScope]::Descendants, $optionKind)) {
            $item = $option.Current
            if ($item.ProcessId -ne $Page.ProcessId -or $item.Name -cnotmatch '^([0-9]{2,3})\s*%(?: \(Recommended\))?$') { continue }
            $percent = [int]$Matches[1]
            $pattern = $null
            $container = $null
            if (!$item.IsOffscreen -and $item.IsEnabled -and
                $option.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern)) {
                $actualContainer = $pattern.Current.SelectionContainer
                if ($actualContainer -and $actualContainer.Current.ProcessId -eq $Page.ProcessId -and (
                    [Windows.Automation.Automation]::Compare($Page.Selector, $actualContainer) -or
                    ($scope.Source -ceq 'owned-popup' -and (Test-ElementInScope $actualContainer $scope.Element) -and
                        (Test-ElementInScope $option $actualContainer))
                )) { $container = $actualContainer }
            }
            $same = $options | Where-Object { [Windows.Automation.Automation]::Compare($_.Element, $option) } | Select-Object -First 1
            if ($same -and (!$container -or $same.Container)) { continue }
            if ($same) { [void]$options.Remove($same) }
            $options.Add([pscustomobject]@{
                Percent = $percent; Name = $item.Name; Element = $option; Scope = $scope
                Container = $container; Selection = $pattern
            })
        }
    }
    return [pscustomobject]@{ PopupCount = $popups.Count; Options = $options.ToArray() }
}

function Wait-ScaleOptions($Page) {
    $deadline = [Diagnostics.Stopwatch]::StartNew()
    $previous = $null
    $stable = 0
    while ($deadline.ElapsedMilliseconds -lt 4000) {
        try { $snapshot = Read-ScaleOptions $Page }
        catch [Windows.Automation.ElementNotAvailableException] { Start-Sleep -Milliseconds 150; continue }
        $report.owned_popup_count = $snapshot.PopupCount
        $report.offered_percent = @($snapshot.Options | ForEach-Object { $_.Percent } | Sort-Object -Unique)
        $report.selectable_percent = @($snapshot.Options | Where-Object Container | ForEach-Object { $_.Percent } | Sort-Object -Unique)
        $report.option_sources = @($snapshot.Options | ForEach-Object { $_.Scope.Source } | Sort-Object -Unique)
        $signature = @($snapshot.Options | ForEach-Object {
            "$($_.Percent):$($_.Scope.Source):$([bool]$_.Container)"
        } | Sort-Object) -join ','
        $signature = "$($snapshot.PopupCount)|$signature"
        if ($snapshot.Options.Count -gt 0 -and $signature -ceq $previous) { $stable++ } else { $stable = 0 }
        if ($stable -ge 2) {
            $report.scale_options_settled = $true
            return $snapshot
        }
        $previous = $signature
        Start-Sleep -Milliseconds 150
    }
}

function Set-HostedDisplayScale {
    if (
        $env:THREADLINE_HOSTED_HIGH_DPI -cne 'opt-in-replay' -or $env:GITHUB_ACTIONS -cne 'true' -or
        $env:GITHUB_EVENT_NAME -cne 'workflow_dispatch' -or $env:GITHUB_REF -cne 'refs/heads/jm/windows-notification-ocr' -or
        $env:GITHUB_WORKFLOW_REF -cnotmatch '/\.github/workflows/windows-hosted-replay\.yml@refs/heads/jm/windows-notification-ocr$' -or
        $env:RUNNER_ENVIRONMENT -cne 'github-hosted' -or $env:RUNNER_OS -cne 'Windows' -or $env:RUNNER_ARCH -cne 'X64' -or
        ![Environment]::Is64BitProcess -or ![Environment]::UserInteractive -or $sessionId -eq 0
    ) { Stop-Blocked 'This opt-in requires the disposable GitHub-hosted Windows x64 diagnostic desktop.' }
    $os = Get-CimInstance -ClassName Win32_OperatingSystem
    $report.windows_product = $os.Caption
    if ($os.ProductType -ne 3 -or $os.Caption -cnotmatch 'Windows Server 2025') { Stop-Blocked 'The disposable Windows Server 2025 target is required.' }
    if (!(Test-Path -LiteralPath $Binary -PathType Leaf) -or !(Test-Path -LiteralPath $NativeDriver -PathType Leaf)) {
        throw 'The verified executable and original native driver are required.'
    }

    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class ThreadlineDisplayProbe {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern int GetSystemMetrics(int index);
    [DllImport("user32.dll")] public static extern IntPtr GetAncestor(IntPtr window, uint flags);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] private static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern IntPtr CreateWindowEx(
        uint extendedStyle, string className, string name, uint style, int x, int y, int width, int height,
        IntPtr parent, IntPtr menu, IntPtr instance, IntPtr parameters);
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern bool DestroyWindow(IntPtr window);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] private static extern IntPtr GetModuleHandle(string name);
    [DllImport("user32.dll", EntryPoint = "EnumDisplaySettingsW", CharSet = CharSet.Unicode, ExactSpelling = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool EnumDisplaySettings(string device, int mode, ref DisplayMode settings);
    [DllImport("user32.dll", EntryPoint = "ChangeDisplaySettingsW", CharSet = CharSet.Unicode, ExactSpelling = true)]
    private static extern int ChangeDisplaySettings(ref DisplayMode settings, uint flags);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    public struct DisplayMode {
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string DeviceName;
        public ushort SpecVersion, DriverVersion, Size, DriverExtra;
        public uint Fields;
        public int X, Y;
        public uint Orientation, FixedOutput;
        public short Color, Duplex, YResolution, TTOption, Collate;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string FormName;
        public ushort LogPixels;
        public uint BitsPerPixel, Width, Height, DisplayFlags, Frequency;
        public uint ICMMethod, ICMIntent, MediaType, DitherType, Reserved1, Reserved2, PanningWidth, PanningHeight;
    }

    public static DisplayMode? ReadMode(int index) {
        DisplayMode mode = new DisplayMode();
        mode.Size = (ushort)Marshal.SizeOf(typeof(DisplayMode));
        return EnumDisplaySettings(null, index, ref mode) ? (DisplayMode?)mode : null;
    }

    public static int ChangeMode(DisplayMode mode, bool test) {
        // Use only the five display fields returned by EnumDisplaySettings, without registry flags.
        mode.Fields = 0x007c0000;
        mode.DriverExtra = 0;
        return ChangeDisplaySettings(ref mode, test ? 2u : 0u);
    }

    public static int Owner(IntPtr window) {
        uint process;
        GetWindowThreadProcessId(window, out process);
        return (int)process;
    }

    public static uint ReadMonitorDpi() {
        IntPtr previous = SetThreadDpiAwarenessContext(new IntPtr(-4));
        if (previous == IntPtr.Zero) return 0;
        try {
            // A fresh hidden per-monitor-aware window takes the effective DPI of the single primary display.
            IntPtr window = CreateWindowEx(0, "STATIC", null, 0x80000000u, 0, 0, 1, 1,
                IntPtr.Zero, IntPtr.Zero, GetModuleHandle(null), IntPtr.Zero);
            if (window == IntPtr.Zero) return 0;
            try { return GetDpiForWindow(window); }
            finally { DestroyWindow(window); }
        } finally { SetThreadDpiAwarenessContext(previous); }
    }
}
'@
    $report.monitor_count = [ThreadlineDisplayProbe]::GetSystemMetrics(80)
    if ($report.monitor_count -ne 1) { Stop-Blocked 'The diagnostic needs exactly one visible display.' }
    Initialize-DisplayResolution
    $report.monitor_dpi_before = [ThreadlineDisplayProbe]::ReadMonitorDpi()
    if ($report.monitor_dpi_before -lt 96) { Stop-Blocked 'The primary display did not provide a usable effective DPI.' }
    if ($report.monitor_dpi_before -in @(144, 192)) {
        $report.monitor_dpi_ready = $report.monitor_dpi_before
        $report.expected_window_dpi = $report.monitor_dpi_ready
        return
    }

    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, WindowsBase
    $settingsPath = Join-Path $env:windir 'ImmersiveControlPanel\SystemSettings.exe'
    $framePath = Join-Path $env:windir 'System32\ApplicationFrameHost.exe'
    try { Start-Process 'ms-settings:display' }
    catch [ComponentModel.Win32Exception], [InvalidOperationException] {
        Stop-Blocked 'Windows could not open its Display Settings page.'
    }
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $page = $null
    while ($watch.ElapsedMilliseconds -lt 8000 -and !$page) {
        Start-Sleep -Milliseconds 250
        try { $page = Get-DisplayScaleControl } catch [Windows.Automation.ElementNotAvailableException] { continue }
    }
    if (!$page) { Stop-Blocked 'Settings did not expose an unambiguous enabled Display scale selector.' }
    $expand = $null
    if (!$page.Selector.TryGetCurrentPattern([Windows.Automation.ExpandCollapsePattern]::Pattern, [ref]$expand)) {
        Stop-Blocked 'The verified Display scale selector cannot be expanded through UI Automation.'
    }
    $expand.Expand()
    $snapshot = Wait-ScaleOptions $page
    if (!$snapshot) { Stop-Blocked 'The verified scale options did not settle in the combo or an anchored Settings popup.' }
    if ($snapshot.PopupCount -gt 1) { Stop-Blocked 'Multiple Settings-owned popups were aligned with the Scale selector.' }
    $options = $snapshot.Options
    $target = $options | Where-Object { $_.Container -and $_.Percent -in @(150, 200) } | Sort-Object Percent | Select-Object -First 1
    if (!$target -or @($options | Where-Object { $_.Percent -eq $target.Percent }).Count -ne 1) {
        Stop-Blocked 'The Display selector did not offer a unique enabled 150% or 200% choice that changes scale.'
    }
    $current = $target.Element.Current
    $foreground = [ThreadlineDisplayProbe]::GetForegroundWindow()
    $latest = Read-ScaleOptions $page
    $matching = @($latest.Options | Where-Object {
        $_.Container -and $_.Percent -eq $target.Percent -and $_.Scope.Window -eq $target.Scope.Window -and
        [Windows.Automation.Automation]::Compare($_.Element, $target.Element) -and
        [Windows.Automation.Automation]::Compare($_.Container, $target.Container)
    })
    if ($latest.PopupCount -gt 1 -or $matching.Count -ne 1 -or $foreground -notin @($page.Window, $target.Scope.Window) -or
        [ThreadlineDisplayProbe]::Owner($page.Window) -ne $page.OwnerId -or [ThreadlineDisplayProbe]::GetSystemMetrics(80) -ne 1 -or
        [ThreadlineDisplayProbe]::ReadMonitorDpi() -ne $report.monitor_dpi_before -or
        $current.Name -cne $target.Name -or $current.ProcessId -ne $page.ProcessId -or !$current.IsEnabled -or $current.IsOffscreen -or
        ![Windows.Automation.Automation]::Compare($matching[0].Container, $matching[0].Selection.Current.SelectionContainer)) {
        Stop-Blocked 'The verified Settings window, display, or scale option changed before selection.'
    }
    if ([ThreadlineDisplayProbe]::GetForegroundWindow() -notin @($page.Window, $target.Scope.Window)) {
        Stop-Blocked 'The verified Settings target lost foreground ownership before selection.'
    }
    $report.requested_percent = $target.Percent
    $matching[0].Selection.Select()

    $expectedDpi = $target.Percent * 96 / 100
    $watch.Restart()
    while ($watch.ElapsedMilliseconds -lt 8000) {
        if ([ThreadlineDisplayProbe]::Owner($page.Window) -ne $page.OwnerId -or [ThreadlineDisplayProbe]::GetSystemMetrics(80) -ne 1) {
            Stop-Blocked 'The verified Settings window or display topology changed.'
        }
        $report.monitor_dpi_ready = [ThreadlineDisplayProbe]::ReadMonitorDpi()
        if ($report.monitor_dpi_ready -eq $expectedDpi) {
            $report.expected_window_dpi = $expectedDpi
            return
        }
        Start-Sleep -Milliseconds 250
    }
    Stop-Blocked 'The requested scale did not take effect in this session. An immediate scale change is required.'
}

$exitCode = 78
try {
    Set-HostedDisplayScale
    $report.status = 'Fail'
    $report.detail = 'The native smoke driver did not return a valid high-DPI result.'
    $exitCode = 1
    $profile = Join-Path $env:RUNNER_TEMP 'threadline-replay-high-dpi-profile'
    $absoluteDriver = (Resolve-Path -LiteralPath $NativeDriver).Path
    $absoluteBinary = (Resolve-Path -LiteralPath $Binary).Path
    & powershell.exe -NoLogo -NoProfile -NonInteractive -Sta -File $absoluteDriver `
        -Binary $absoluteBinary -ProfileDir $profile -ReportDir $ReportDir
    $report.smoke_exit_code = $LASTEXITCODE
    $native = Get-Content -LiteralPath (Join-Path $ReportDir 'native.json') -Raw | ConvertFrom-Json
    $report.smoke_window_dpi = $native.dpi
    if ($report.smoke_exit_code -eq 0 -and $native.status -ceq 'Pass') {
        if ($native.dpi -eq $report.expected_window_dpi) {
            $report.status = 'Pass'
            $report.detail = "The unchanged native smoke passed at the expected $($report.expected_window_dpi) DPI on the hosted desktop."
            $exitCode = 0
        } else {
            $report.detail = 'The app window DPI did not match the verified effective display DPI. See native.json.'
        }
    } elseif ($report.smoke_exit_code -eq 78 -and $native.status -ceq 'Blocked') {
        $report.status = 'Blocked'
        $report.detail = 'The unchanged native smoke was blocked. See native.json.'
        $exitCode = 78
    } elseif ($native.status -ceq 'Fail') {
        $report.detail = "The unchanged native smoke failed in the observed $($report.resolution_ready) display mode. See native.json for the failed action."
    }
} catch {
    $cause = $_.Exception.GetBaseException().GetType().FullName
    if ($blockedDetail) {
        $report.status = 'Blocked'
        $report.detail = $blockedDetail
        $exitCode = 78
    } elseif ($cause -cin @('System.Windows.Automation.ElementNotAvailableException', 'System.Windows.Automation.ElementNotEnabledException')) {
        $report.status = 'Blocked'
        $report.detail = 'The verified Display scale control became unavailable or disabled during the trial.'
        $exitCode = 78
    } else {
        $report.status = 'Fail'
        $report.detail = 'The high-DPI diagnostic failed unexpectedly.'
        $report.diagnostic_error_type = $cause
        $report.diagnostic_error_line = $_.InvocationInfo.ScriptLineNumber
        $exitCode = 1
    }
} finally {
    try {
        Restore-DisplayResolution
        if ($report.resolution_restored -eq $false) {
            $report.status = 'Fail'
            $report.detail = 'The original display resolution could not be verified after restoration.'
            $exitCode = 1
        }
    } catch {
        $report.resolution_restored = $false
        $report.status = 'Fail'
        $report.detail = 'Restoring the original display resolution failed unexpectedly.'
        $report.diagnostic_error_type = $_.Exception.GetBaseException().GetType().FullName
        $report.diagnostic_error_line = $_.InvocationInfo.ScriptLineNumber
        $exitCode = 1
    }
    $report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $reportPath -Encoding UTF8
}
Write-Output "$($report.status): $($report.detail)"
exit $exitCode

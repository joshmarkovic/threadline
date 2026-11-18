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
    monitor_dpi_before = $null
    offered_percent = @()
    requested_percent = $null
    monitor_dpi_ready = $null
    expected_window_dpi = $null
    smoke_exit_code = $null
    smoke_window_dpi = $null
}
$blockedDetail = $null

function Stop-Blocked([string]$Detail) {
    $script:blockedDetail = $Detail
    throw 'The hosted display scale diagnostic is blocked.'
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
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint process);
    [DllImport("user32.dll")] private static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] private static extern IntPtr CreateWindowEx(
        uint extendedStyle, string className, string name, uint style, int x, int y, int width, int height,
        IntPtr parent, IntPtr menu, IntPtr instance, IntPtr parameters);
    [DllImport("user32.dll")] private static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] private static extern bool DestroyWindow(IntPtr window);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] private static extern IntPtr GetModuleHandle(string name);

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
    Start-Sleep -Milliseconds 250
    $condition = [Windows.Automation.PropertyCondition]::new(
        [Windows.Automation.AutomationElement]::ControlTypeProperty, [Windows.Automation.ControlType]::ListItem
    )
    $options = @($page.Selector.FindAll([Windows.Automation.TreeScope]::Descendants, $condition) | ForEach-Object {
        $item = $_.Current
        if ($item.ProcessId -eq $page.ProcessId -and $item.IsEnabled -and !$item.IsOffscreen -and
            $item.Name -cmatch '^(150|200)\s*%(?: \(Recommended\))?$') {
            $percent = [int]$Matches[1]
            $pattern = $null
            $selectable = $_.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$pattern) -and
                [Windows.Automation.Automation]::Compare($page.Selector, $pattern.Current.SelectionContainer)
            [pscustomobject]@{
                Percent = $percent
                Name = $item.Name
                Element = $_
                Selection = if ($selectable) { $pattern } else { $null }
            }
        }
    })
    $report.offered_percent = @($options | ForEach-Object { $_.Percent } | Sort-Object -Unique)
    $target = $options | Where-Object Selection | Sort-Object Percent | Select-Object -First 1
    if (!$target -or @($options | Where-Object { $_.Percent -eq $target.Percent }).Count -ne 1) {
        Stop-Blocked 'The Display selector did not offer a unique enabled 150% or 200% choice that changes scale.'
    }
    $current = $target.Element.Current
    if ([ThreadlineDisplayProbe]::GetForegroundWindow() -ne $page.Window -or
        [ThreadlineDisplayProbe]::Owner($page.Window) -ne $page.OwnerId -or [ThreadlineDisplayProbe]::GetSystemMetrics(80) -ne 1 -or
        [ThreadlineDisplayProbe]::ReadMonitorDpi() -ne $report.monitor_dpi_before -or
        $current.Name -cne $target.Name -or $current.ProcessId -ne $page.ProcessId -or !$current.IsEnabled -or $current.IsOffscreen -or
        ![Windows.Automation.Automation]::Compare($page.Selector, $target.Selection.Current.SelectionContainer)) {
        Stop-Blocked 'The verified Settings window, display, or scale option changed before selection.'
    }
    $report.requested_percent = $target.Percent
    $target.Selection.Select()

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
    $report | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $reportPath -Encoding UTF8
}
Write-Output "$($report.status): $($report.detail)"
exit $exitCode

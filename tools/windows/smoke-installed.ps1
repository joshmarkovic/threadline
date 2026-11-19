[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$ExpectedExecutableSha256,
    [ValidatePattern('^reader-tab-[0-2]-[0-9]{1,2}$')][string]$TabId = 'reader-tab-0-1',
    [ValidateRange(0, 1000000)][int]$ExpectedIssueNumber = 0
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes

$install = Join-Path $env:LOCALAPPDATA 'Programs\Threadline Installer Test'
$exe = Join-Path $install 'threadline.exe'
$registry = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Threadline.InstallTest'
$registration = Get-ItemProperty -LiteralPath $registry
if ($registration.InstallLocation -ine $install -or
    (Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash -ine $ExpectedExecutableSha256) {
    throw 'The registered fixture and its installed executable do not match the selected package.'
}
if (Get-Process -Name threadline -ErrorAction SilentlyContinue) {
    throw 'Quit existing Threadline instances before verifying a new installed process.'
}

. (Join-Path $PSScriptRoot 'installed-controls.ps1')

function Wait-Installed([string]$Description, [scriptblock]$Check, [int]$Milliseconds = 15000) {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    while ($watch.ElapsedMilliseconds -lt $Milliseconds) {
        if ($child.HasExited) { throw "The installed process exited before $Description." }
        try {
            if (& $Check) { return $true }
        } catch [Windows.Automation.ElementNotAvailableException] { }
        Start-Sleep -Milliseconds 100
    }
    return $false
}

function Read-TabSelection([Diagnostics.Process]$Process, [string]$Id) {
    $root = Get-OwnedWindow $Process
    $value = Find-OwnedId $root $Process.Id $Id
    if ($null -eq $value) { return $false }
    $name = $value.Current.Name
    $signals = [ordered]@{ active_label=$name.EndsWith((' ' + [char]0x00B7 + ' Active tab'), [StringComparison]::Ordinal)
        selection_item=$null; toggle_on=$null }
    $selection = $null
    if ($value.TryGetCurrentPattern([Windows.Automation.SelectionItemPattern]::Pattern, [ref]$selection)) {
        $signals.selection_item = $selection.Current.IsSelected
    }
    $toggle = $null
    if ($value.TryGetCurrentPattern([Windows.Automation.TogglePattern]::Pattern, [ref]$toggle)) {
        $signals.toggle_on = $toggle.Current.ToggleState -eq [Windows.Automation.ToggleState]::On
    }
    $script:lastTabSignals = $signals
    if ($ExpectedIssueNumber -gt 0 -and $name -notmatch ("#" + $ExpectedIssueNumber + "(\s|$)")) { return $false }
    return $signals.active_label -or $signals.selection_item -or $signals.toggle_on
}

$child = $null
$ownedWindow = $false
$invoked = $false
$selected = $false
$openBefore = $false
$closeRequested = $false
$closedNormally = $false
$attempts = 0
$script:lastTabSignals = $null
$initialTabSignals = $null
try {
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = $exe
    $start.WorkingDirectory = $install
    $start.UseShellExecute = $false
    foreach ($name in @($start.EnvironmentVariables.Keys)) {
        if ($name -like 'THREADLINE_*') { $start.EnvironmentVariables.Remove($name) }
    }
    $child = [Diagnostics.Process]::Start($start)
    $ownedWindow = Wait-Installed 'a visible owned reader window and preseeded tab' {
        $root = Get-OwnedWindow $child
        $null -ne (Find-OwnedId $root $child.Id $TabId)
    }
    if (!$ownedWindow) { throw 'Timed out waiting for the owned reader and preseeded tab.' }
    $openBefore = Read-TabSelection $child $TabId
    $initialTabSignals = $script:lastTabSignals
    if (!$openBefore) {
        $invoked = Invoke-OwnedId $child $TabId
        if (!$invoked) { throw 'The exact preseeded tab did not support native activation.' }
        $attempts = 1
        $selected = Wait-Installed 'the selected preseeded tab' { Read-TabSelection $child $TabId } 3000
        # Older builds can commit pane focus before they accept the tab action.
        if (!$selected -and (Invoke-OwnedId $child $TabId)) {
            $attempts = 2
            $selected = Wait-Installed 'the selected preseeded tab' { Read-TabSelection $child $TabId }
        }
        if (!$selected) { throw 'Timed out waiting for evidence that the preseeded tab opened.' }
    }
    $closeRequested = Invoke-OwnedId $child 'quit'
    if (!$closeRequested -and $null -ne (Get-OwnedWindow $child)) {
        $closeRequested = $child.CloseMainWindow()
    }
    if (!$closeRequested) { throw 'The verified installed window did not accept an orderly close.' }
    if (!$child.WaitForExit(10000) -or $child.ExitCode -ne 0) {
        throw 'The verified installed application did not exit normally.'
    }
    $closedNormally = $true
} finally {
    if ($null -ne $child) {
        try {
            if (!$child.HasExited -and $null -ne (Get-OwnedWindow $child)) {
                $closeRequested = $child.CloseMainWindow() -or $closeRequested
            }
            if ($closeRequested -and $child.WaitForExit(10000)) {
                $closedNormally = $child.ExitCode -eq 0
            }
        } finally {
            $child.Dispose()
        }
    }
    [ordered]@{
        installed_executable_sha256 = $ExpectedExecutableSha256.ToLowerInvariant()
        owned_window = $ownedWindow
        tab_automation_id = $TabId
        selected_before_invocation = $openBefore
        tab_invoke_supported = $invoked
        tab_invoke_attempts = $attempts
        tab_activated = $selected
        initial_selection_signals = $initialTabSignals
        final_selection_signals = $script:lastTabSignals
        orderly_exit = $closedNormally
    } | ConvertTo-Json -Compress
}

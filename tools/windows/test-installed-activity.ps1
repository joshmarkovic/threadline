[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$AppSha256,
    [Parameter(Mandatory)][string]$FixtureTool,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$FixtureSha256,
    [Parameter(Mandatory)][string]$ReportDir,
    [ValidateSet('E2E-012','E2E-018')][string]$Case = 'E2E-012'
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, UIAutomationClient, UIAutomationTypes
$native = Join-Path (Split-Path -Parent $PSScriptRoot) native
Add-Type -Path (Join-Path $native 'ThreadlineWinInput.cs')
Add-Type -Path (Join-Path $native 'ThreadlineToastShortcut.cs')
. (Join-Path $PSScriptRoot 'installed-controls.ps1')
. (Join-Path $native 'windows-notification-focus.ps1')

$install = Join-Path $env:LOCALAPPDATA 'Programs\Threadline Installer Test'
$exe = Join-Path $install 'threadline.exe'
$dataParent = Join-Path $env:LOCALAPPDATA Threadline
$data = Join-Path $dataParent Data
$marker = 'threadline-s11-installed-fixture-v1'
$identity = 'Threadline.InstallTest'
$clsid = '{df3bb9a2-0794-44a6-b130-a35197093afe}'
$fixtureTitle = 'Threadline'
$fixtureBody = '37 new activity updates'
$fixtureAppDisplayName = 'Threadline Installer Test'
$fixtureActionLabel = 'Open'
$fixtureSenderGuard = {
    param($Card, $Root, [int]$Owner)
    $node = $Card
    for ($depth = 0; $depth -lt 5 -and $null -ne $node; $depth++) {
        if ($node.Current.ProcessId -ne $Owner -or (Get-ElementIdentity $node) -ceq (Get-ElementIdentity $Root)) { break }
        $name = $node.Current.Name
        if ($name -ceq $fixtureAppDisplayName -or
            $name.StartsWith(($fixtureAppDisplayName + ', ')) -or
            $name.StartsWith(('Notification from ' + $fixtureAppDisplayName + ': '))) { return $true }
        $sender = $node.FindAll([Windows.Automation.TreeScope]::Children, (Get-FixtureCondition $fixtureAppDisplayName $Owner))
        if ($sender.Count -eq 1) { return $true }
        $node = $rawView.GetParent($node)
    }
    return $false
}
$actions = New-Object 'System.Collections.Generic.List[object]'
$report = [ordered]@{ status='Fail'; case=$Case; session_id=[Diagnostics.Process]::GetCurrentProcess().SessionId
    app_sha256=$AppSha256.ToLowerInvariant(); fixture_sha256=$FixtureSha256.ToLowerInvariant(); actions=$actions }
$blocked = $false
$owned = $false
$child = $null
$focusForm = $null
$toastHistory = $null
$group = $null
$ReportDir = [IO.Path]::GetFullPath($ReportDir)
$reportPath = Join-Path $ReportDir 'installed-activity.json'
if (!(Test-Path -LiteralPath $ReportDir -PathType Container) -or (Test-Path -LiteralPath $reportPath)) {
    throw 'Supply a fresh evidence directory.'
}
function Stop-Blocked([string]$Message) { $script:blocked = $true; throw $Message }
function Add-Evidence([string]$Action, $Observed) { $actions.Add([ordered]@{ action=$Action; observed=$Observed }) }
function Wait-Activity([string]$Description, [scriptblock]$Probe, [int]$Timeout = 20000) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.ElapsedMilliseconds -lt $Timeout) {
        try { $value = & $Probe; if ($value) { return $value } }
        catch [Windows.Automation.ElementNotAvailableException] {}
        Start-Sleep -Milliseconds 140
    }
    throw "Timed out waiting for $Description."
}
function Read-Fixture([string]$Mode) {
    $out = & $FixtureTool $Mode $data
    if ($LASTEXITCODE -ne 0) { throw 'The fixture helper failed its database or ownership check.' }
    return ($out | Out-String | ConvertFrom-Json)
}
function Get-InstalledProcesses {
    return @(Get-Process -Name threadline -ErrorAction SilentlyContinue |
        Where-Object { $_.SessionId -eq $report.session_id -and $_.Path -ieq $exe })
}
function Close-Installed($Process) {
    $null = Wait-Activity 'the installed Quit control' { Find-OwnedId (Get-OwnedWindow $Process) $Process.Id 'quit' }
    if (!(Invoke-OwnedId $Process 'quit') -or !$Process.WaitForExit(12000) -or $Process.ExitCode -ne 0) {
        throw 'The verified installed window did not quit normally.'
    }
}
function Get-ExactHistory {
    $items = @($toastHistory.GetHistory($identity) | Where-Object {
        $_.Tag -ceq $group.Substring(0,16) -and $_.Group -ceq $group.Substring(16)
    })
    if ($items.Count -ne 1) { return $null }
    [xml]$xml = $items[0].Content.GetXml()
    if ($xml.toast.launch -cne ('activity1.'+$group+'.open') -or
        @($xml.toast.visual.binding.text | Where-Object { $_ -ceq $fixtureBody }).Count -ne 1 -or
        @($xml.toast.actions.action | Where-Object { $_.arguments -ceq ('activity1.'+$group+'.read') }).Count -ne 1 -or
        @($xml.toast.actions.action | Where-Object { $_.arguments -ceq ('activity1.'+$group+'.snooze') }).Count -ne 1) {
        throw 'The Windows app-scoped history did not contain the exact action payloads and event count.'
    }
    return $true
}

try {
    if (![Environment]::Is64BitProcess -or !$([Environment]::UserInteractive)) { Stop-Blocked 'An interactive 64-bit Windows session is required.' }
    $registration = Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Threadline.InstallTest'
    $link = Join-Path ([Environment]::GetFolderPath('Programs')) 'Threadline Installer Test.lnk'
    $localServer = Get-Item -LiteralPath ("HKCU:\Software\Classes\CLSID\$clsid\LocalServer32")
    if ($registration.InstallLocation -ine $install -or $localServer.GetValue('') -ine ('"'+$exe+'" --notification-com-server') -or
        !( [ThreadlineToastShortcut]::LinkMatches($link,$identity,$exe) -and [ThreadlineToastShortcut]::LinkHasActivator($link,[guid]$clsid) ) -or
        (Get-FileHash -LiteralPath $exe).Hash -ine $AppSha256 -or (Get-FileHash -LiteralPath $FixtureTool).Hash -ine $FixtureSha256) {
        Stop-Blocked 'The local installed registration, exact shortcut or pinned files do not match.'
    }
    if ((Get-Process -Name threadline -ErrorAction SilentlyContinue) -or (Test-Path -LiteralPath $data)) {
        Stop-Blocked 'A fresh test profile requires an absent default Data directory and no running Threadline.'
    }
    if (Test-Path -LiteralPath $dataParent) {
        if ((Get-Item -LiteralPath $dataParent).Attributes -band [IO.FileAttributes]::ReparsePoint) {
            Stop-Blocked 'The default fixture parent is a reparse point.'
        }
    }
    $null = New-Item -ItemType Directory -Path $data -ErrorAction Stop
    $owned = $true
    $before = Read-Fixture seed
    if ($before.event_count -ne 37 -or $before.feed_count -ne 0 -or $before.selected_issue -ne 41 -or
        !$before.draft_preserved -or !$before.draft_tab_preserved) { throw 'The initial issue, draft or observer fixture is invalid.' }
    Add-Evidence 'Create an isolated synthetic account with both network queues paused, a local draft on issue 41 and unseen activity on issue 42' $before
    if ($Case -ceq 'E2E-018') {
        # The copied executable cannot satisfy the registered shortcut and LocalServer32 checks.
        $unregistered = New-Item -ItemType Directory -Path (Join-Path $ReportDir 'unregistered-app')
        $copy = Join-Path $unregistered.FullName 'threadline.exe'
        Copy-Item -LiteralPath $exe -Destination $copy
        if ((Get-FileHash -LiteralPath $copy).Hash -ine $AppSha256) { throw 'The unregistered copy did not match the installed bytes.' }
        $exe = $copy
        $child = Start-Process -FilePath $exe -PassThru
        $feed = Wait-Activity 'the unsupported adapter and feed-only decisions' {
            $state = Read-Fixture inspect
            if ($state.registration_feed_only -eq 37 -and $state.group_count -eq 0 -and $state.feed_count -eq 37) { return $state }
        }
        $report.initial_pid = $child.Id
        $null = Wait-Activity 'the owned activity entry point' { Find-OwnedId (Get-OwnedWindow $child) $child.Id 'reader-activity' }
        if (!(Invoke-OwnedId $child 'reader-activity')) { throw 'Could not open the activity feed.' }
        $event = $feed.latest_feed_id
        $null = Wait-Activity 'the in-app Snooze control' { Find-OwnedId (Get-OwnedWindow $child) $child.Id "activity-snooze-$event" }
        if (!(Invoke-OwnedId $child "activity-snooze-$event")) { throw 'Could not use the in-app Snooze action.' }
        $null = Wait-Activity 'the durable snooze' { (Read-Fixture inspect).snoozed_targets -eq 1 }
        if (!(Invoke-OwnedId $child "activity-read-$event")) { throw 'Could not mark the feed event as read.' }
        $after = Wait-Activity 'the read state' { $state = Read-Fixture inspect; if ($state.read_count -eq 1) { return $state } }
        if (!$after.draft_preserved -or !$after.draft_tab_preserved) { throw 'The fallback changed the retained draft or original tab.' }
        Add-Evidence 'Start the same executable outside its registered path; all 37 events reach the feed without a native group' $feed
        Add-Evidence 'Snooze and mark read through owned feed controls; retain the original unrelated draft' $after
        Close-Installed $child
    } else {
        $toastHistory = [Windows.UI.Notifications.ToastNotificationManager,Windows.UI.Notifications,ContentType=WindowsRuntime]::History
        $child = Start-Process -FilePath $exe -PassThru
        $report.initial_pid = $child.Id
        $delivered = Wait-Activity 'Windows notification submission and committed delivery' {
            if ($child.HasExited) { throw 'The installed app exited before native delivery.' }
            $state = Read-Fixture inspect
            if ($state.feed_count -eq 37 -and $null -eq $state.native_id) {
                if ($state.registration_feed_only -gt 0) { throw 'Threadline did not recognize its installed notification registration.' }
                if ($state.adapter_feed_only -gt 0) { throw 'Threadline could not query the registered native notification adapter.' }
                if ($state.os_feed_only -gt 0) { Stop-Blocked 'Windows disabled notifications for this registered sender; the feed retained the events.' }
            }
            if ($state.outcome -ceq 'delivered' -and $state.group_count -eq 1 -and $state.native_id -cmatch '^[a-f0-9]{32}$') { return $state }
        }
        $group = $delivered.native_id
        $null = Wait-Activity 'the exact app-scoped history record' { Get-ExactHistory }
        Add-Evidence 'Verify Windows history, opaque Open/Snooze/Mark read payloads and the delivered journal' $delivered
        Close-Installed $child
        $child.Dispose(); $child = $null
        if (@(Get-InstalledProcesses).Count -ne 0 -or !(Get-ExactHistory)) { throw 'The exact submitted card did not survive full application exit.' }
        $focusForm = New-Object Windows.Forms.Form
        $focusForm.Text = 'Threadline notification test'
        $focusForm.Width = 320; $focusForm.Height = 100
        $focusForm.Show(); $focusForm.Activate()
        [Windows.Forms.Application]::DoEvents()
        if ([ThreadlineWinInput]::GetForegroundWindow() -ne $focusForm.Handle) { Stop-Blocked 'Windows did not grant focus to the fixture window.' }
        [ThreadlineWinInput]::TargetWindow = $focusForm.Handle
        if (!(Invoke-FixtureCenterClick $focusForm.Handle)) { Stop-Blocked 'Windows did not expose the scoped fixture card in Notification Center.' }
        $child = Wait-Activity 'the cold installed COM process' {
            $found = @(Get-InstalledProcesses)
            if ($found.Count -eq 1 -and $found[0].Id -ne $report.initial_pid -and (Get-OwnedWindow $found[0])) { return $found[0] }
        }
        $report.cold_pid = $child.Id
        $null = Wait-Activity 'the selected issue 42 and retained draft tab in the installed window' {
            $root = Get-OwnedWindow $child
            $first = Find-OwnedId $root $child.Id 'reader-tab-0-0'
            $next = Find-OwnedId $root $child.Id 'reader-tab-0-1'
            $null -ne $first -and $null -ne $next -and $first.Current.Name -match '#41(\s|$)' -and
                $next.Current.Name -match '#42(\s|$)' -and $next.Current.Name.EndsWith(' · Active tab')
        }
        Close-Installed $child
        $after = Read-Fixture inspect
        if (!$after.activated -or $after.pending_opens -ne 0 -or $after.selected_issue -ne 42 -or !$after.draft_preserved -or
            !$after.draft_tab_preserved -or $after.read_count -ne 37) { throw 'The cold route, consumed payload or unrelated draft was not retained.' }
        Add-Evidence 'Open the exact cached issue in the new process and retain the unrelated draft and original tab' $after
    }
    $report.status = 'Pass'
} catch {
    $report.status = if ($blocked) { 'Blocked' } else { 'Fail' }
    $report.detail = $_.Exception.Message
} finally {
    if ($focusForm) { $focusForm.Close(); $focusForm.Dispose() }
    if ($child) {
        if (!$child.HasExited) { try { Close-Installed $child } catch { $report.cleanup_detail = 'The owned window remained open.' } }
        $child.Dispose()
    }
    if ($group -and $null -ne $toastHistory) {
        try { $toastHistory.Remove($group.Substring(0,16),$group.Substring(16),$identity) } catch { $report.history_cleanup_error = $true }
    }
    if ($owned -and !(Get-Process -Name threadline -ErrorAction SilentlyContinue) -and
        (Test-Path -LiteralPath (Join-Path $data $marker) -PathType Leaf) -and
        [IO.File]::ReadAllText((Join-Path $data $marker)) -ceq $marker) {
        $files = @(Get-ChildItem -Force -LiteralPath $data)
        $unexpected = @($files | Where-Object { $_.PSIsContainer -or ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
            $_.Name -notin @($marker,'threadline.sqlite3','threadline.sqlite3-wal','threadline.sqlite3-shm') })
        $archive = Join-Path $ReportDir 'synthetic-activity-profile'
        if ($unexpected.Count -eq 0 -and !(Test-Path -LiteralPath $archive)) {
            Move-Item -LiteralPath $data -Destination $archive -ErrorAction Stop
            $report.synthetic_profile_archived = $true
        }
    }
    $report | ConvertTo-Json -Depth 7 | Set-Content -LiteralPath $reportPath -Encoding UTF8
    Write-Output $report.status
}
if ($report.status -ceq 'Blocked') { exit 78 }
if ($report.status -cne 'Pass') { exit 1 }

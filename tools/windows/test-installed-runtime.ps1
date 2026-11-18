[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('E2E-020')][string]$Case,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$AppSha256,
    [Parameter(Mandatory)][string]$FixtureTool,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$FixtureSha256,
    [Parameter(Mandatory)][string]$ReportDir
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
. (Join-Path $PSScriptRoot 'installed-controls.ps1')

$install = Join-Path $env:LOCALAPPDATA 'Programs\Threadline Installer Test'
$exe = Join-Path $install 'threadline.exe'
$parent = Join-Path $env:LOCALAPPDATA Threadline
$data = Join-Path $parent Data
$marker = 'threadline-s12-installed-runtime-v1'
$ReportDir = [IO.Path]::GetFullPath($ReportDir)
$destination = Join-Path $ReportDir 'installed-runtime.json'
if (!(Test-Path -LiteralPath $ReportDir -PathType Container) -or (Test-Path -LiteralPath $destination)) {
    throw 'Supply a fresh evidence directory.'
}
$build = Get-Content -LiteralPath (Join-Path $ReportDir 'runtime-build.json') -Raw | ConvertFrom-Json
$report = [ordered]@{case=$Case;status='Fail';app_sha256=$AppSha256.ToLowerInvariant();fixture_sha256=$FixtureSha256.ToLowerInvariant()
    build_evidence='runtime-build.json';observations=(New-Object 'System.Collections.Generic.List[object]')
    synthetic_profile_archived=$false;orderly_quit=$false}
$owned = $false
$child = $null
$blocked = $false
function Stop-Blocked([string]$Message) { $script:blocked = $true; throw $Message }
function Read-Fixture([string]$Mode) {
    $output = & $FixtureTool $Mode $data
    if ($LASTEXITCODE -ne 0) { throw 'The native fixture helper could not verify the owned database.' }
    return ($output | Out-String | ConvertFrom-Json)
}
function Wait-Control([string]$Id, [string]$Name = '') {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.ElapsedMilliseconds -lt 20000) {
        if ($child.HasExited) { throw 'The installed application exited before verification.' }
        try {
            $control = Find-OwnedId (Get-OwnedWindow $child) $child.Id $Id
            if ($null -ne $control -and ($Name -ceq '' -or $control.Current.Name -ceq $Name)) { return $control }
        } catch [Windows.Automation.ElementNotAvailableException] {}
        Start-Sleep -Milliseconds 120
    }
    throw "The installed window did not expose the expected $Id control."
}
function Invoke-Control([string]$Id) {
    $null = Wait-Control $Id
    if (!(Invoke-OwnedId $child $Id)) { throw "The installed window did not invoke $Id." }
}
function Read-Inventory([string]$Phase) {
    $window = Get-OwnedWindow $child
    if ($null -eq $window) { throw 'The installed product window was unavailable for observation.' }
    $todo = New-Object 'System.Collections.Generic.Queue[int]'
    $seen = New-Object 'System.Collections.Generic.HashSet[int]'
    $names = New-Object 'System.Collections.Generic.HashSet[string]'
    $todo.Enqueue($child.Id)
    while ($todo.Count -gt 0) {
        $currentId = $todo.Dequeue()
        if (!$seen.Add($currentId)) {continue}
        $current = Get-Process -Id $currentId -ErrorAction Stop
        try {
            $null = $names.Add(($current.ProcessName+'.exe').ToLowerInvariant())
            foreach ($module in $current.Modules) {
                $null = $names.Add($module.ModuleName.ToLowerInvariant())
            }
            $children = @(Get-CimInstance -ClassName Win32_Process -Filter ("ParentProcessId = " + $currentId) -Property ProcessId,CreationDate)
            foreach ($entry in $children) {
                if ($entry.CreationDate -ge $current.StartTime) { $todo.Enqueue([int]$entry.ProcessId) }
            }
        } finally { $current.Dispose() }
    }
    $matches = @($names | Where-Object {
        $name = $_
        $null -ne ($build.browser_prefixes | Where-Object { $name.StartsWith($_, [StringComparison]::Ordinal) } | Select-Object -First 1)
    } | Sort-Object)
    $report.observations.Add([ordered]@{phase=$Phase;accessible_root_pid=$window.Current.ProcessId
        module_count=@($child.Modules).Count;descendants=($seen.Count-1);browser_matches=$matches})
    if ($matches.Count -ne 0) { throw 'A browser component was found in the installed application process tree.' }
}
try {
    if (![Environment]::Is64BitProcess -or ![Environment]::UserInteractive) { Stop-Blocked 'An interactive 64-bit Windows session is required.' }
    $reg = Get-ItemProperty -LiteralPath 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Threadline.InstallTest'
    if ($reg.InstallLocation -ine $install -or (Get-FileHash -LiteralPath $exe).Hash -ine $AppSha256 -or
        (Get-FileHash -LiteralPath $FixtureTool).Hash -ine $FixtureSha256 -or $build.app_sha256 -ine $AppSha256 -or
        $build.browser_matches.Count -ne 0) { throw 'The installed package, seed helper or static build inventory did not match.' }
    if ((Get-Process -Name threadline -ErrorAction SilentlyContinue) -or (Test-Path -LiteralPath $data)) {
        Stop-Blocked 'Use a test account with an absent default Data directory and no running Threadline.'
    }
    if ((Test-Path -LiteralPath $parent) -and ((Get-Item -LiteralPath $parent).Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        Stop-Blocked 'The default profile parent is a reparse point.'
    }
    $null = New-Item -ItemType Directory -Path $data
    $owned = $true
    $before = Read-Fixture seed-runtime
    if ($before.fixture -cne $marker -or !$before.offline -or $before.active -ne 0 -or $before.tabs -ne 2) {
        throw 'The cached offline issue and pull request fixture did not verify.'
    }
    $child = Start-Process -FilePath $exe -PassThru
    $report.process_id = $child.Id
    $initial = Wait-Control 'reader-attachment-0' 'Open cached image'
    $report.markdown_initial = $initial.Current.Name
    Read-Inventory 'Native Markdown with parsed image attachment'
    Invoke-Control 'reader-attachment-0'
    $missing = Wait-Control 'reader-attachment-0' 'Image unavailable. Retry'
    $report.markdown_after = $missing.Current.Name
    Invoke-Control 'reader-tab-0-1'
    $null = Wait-Control 'pr-0-files'
    Invoke-Control 'pr-0-files'
    if (Find-OwnedId (Get-OwnedWindow $child) $child.Id 'pr-0-show-code') { Invoke-Control 'pr-0-show-code' }
    $second = Wait-Control 'pr-0-file-1'
    if ($second.Current.Name -notlike '*second.txt*') { throw 'The selected file control was bound to a different cached file.' }
    Invoke-Control 'pr-0-file-1'
    $null = Wait-Control 'pr-0-diff-split'
    Read-Inventory 'Native selected comparison file and diff toolbar'
    Invoke-Control 'pr-0-diff-split'
    $clock = [Diagnostics.Stopwatch]::StartNew()
    do {
        $state = Read-Fixture inspect-runtime
        if ($state.active -eq 1 -and $state.path -ceq 'second.txt' -and $state.mode -ceq 'Split' -and $state.offline) { break }
        if ($clock.ElapsedMilliseconds -ge 15000) { throw 'The selected file and split diff did not persist.' }
        Start-Sleep -Milliseconds 150
    } while ($true)
    $report.selected_file = $state.path
    $report.diff_mode = $state.mode
    $report.offline = $state.offline
    Read-Inventory 'Native split diff after the persisted action'
    Invoke-Control 'quit'
    if (!$child.WaitForExit(12000) -or $child.ExitCode -ne 0) { throw 'The installed window did not quit normally.' }
    $report.orderly_quit = $true
    $report.status = 'Pass'
} catch {
    $report.status = if ($blocked) { 'Blocked' } else { 'Fail' }
    $report.detail = $_.Exception.Message
} finally {
    if ($child) {
        if (!$child.HasExited) {
            try { if (Invoke-OwnedId $child 'quit') { $null = $child.WaitForExit(12000) } } catch {}
        }
        $child.Dispose()
    }
    if ($owned -and !(Get-Process -Name threadline -ErrorAction SilentlyContinue) -and
        (Test-Path -LiteralPath (Join-Path $data $marker) -PathType Leaf) -and
        [IO.File]::ReadAllText((Join-Path $data $marker)) -ceq $marker) {
        $files = @(Get-ChildItem -Force -LiteralPath $data)
        $unexpected = @($files | Where-Object { $_.PSIsContainer -or ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
            $_.Name -notin @($marker,'threadline.sqlite3','threadline.sqlite3-wal','threadline.sqlite3-shm') })
        $archive = Join-Path $ReportDir 'synthetic-runtime-profile'
        if ($unexpected.Count -eq 0 -and !(Test-Path -LiteralPath $archive)) {
            Move-Item -LiteralPath $data -Destination $archive
            $report.synthetic_profile_archived = $true
        }
    }
    if ($report.status -ceq 'Pass' -and !$report.synthetic_profile_archived) { $report.status = 'Fail'; $report.detail='The fixture could not be archived.' }
    $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $destination -Encoding UTF8
    Write-Output $report.status
}
if ($report.status -ceq 'Blocked') { exit 78 }
if ($report.status -cne 'Pass') { exit 1 }

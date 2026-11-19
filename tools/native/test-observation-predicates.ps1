Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Import the production functions without starting the native smoke driver.
$tokens = $null
$parseErrors = $null
$smoke = Join-Path $PSScriptRoot 'windows-smoke.ps1'
$ast = [System.Management.Automation.Language.Parser]::ParseFile($smoke, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -ne 0) { throw ($parseErrors -join [Environment]::NewLine) }
$helpers = $ast.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -cin @('Wait-For', 'Wait-State', 'Get-ThreadlineArguments', 'Get-IsolatedSqlite', 'Get-FileSha256')
}, $false)
if (@($helpers.Name | Sort-Object -Unique).Count -ne 5 -or $helpers.Count -ne 5) {
    throw 'Could not load the native driver test functions.'
}
. ([scriptblock]::Create(($helpers | ForEach-Object { $_.Extent.Text }) -join [Environment]::NewLine))
. (Join-Path $PSScriptRoot 'windows-github-access.ps1')
. (Join-Path $PSScriptRoot 'windows-cached-reader.ps1')
. (Join-Path $PSScriptRoot 'windows-sync.ps1')
. (Join-Path $PSScriptRoot 'windows-pr-reader.ps1')
. (Join-Path $PSScriptRoot 'windows-owned-uia.ps1')

$notLoaded = '{"connection":"Reachable","cache_generation":2,"panel_open":false,"controls":null}' | ConvertFrom-Json
if (Test-NotificationSyncReady $notLoaded) { throw 'Reachable fixture transport did not prove saved Sync controls had loaded.' }
$loaded = '{"connection":"Reachable","cache_generation":2,"panel_open":true,"controls":{"work_offline":false,"pause_background":false,"pause_queued_actions":false,"notification_quiet":false}}' | ConvertFrom-Json
if (!(Test-NotificationSyncReady $loaded)) { throw 'The observed loaded Sync controls did not permit the quiet-hours journey.' }
$quietChoice = 'Enable quiet hours ' + [char]0x00b7 + ' '
if ((Get-NotificationChoiceName 'Enable quiet hours' $false) -cne ($quietChoice + 'Unselected') -or
    (Get-NotificationChoiceName 'Enable quiet hours' $true) -cne ($quietChoice + 'Selected')) {
    throw 'The notification driver did not request the product accessibility name for the current setting state.'
}

function Read-Observation {
    if ($script:observations.Count -gt 1) { return $script:observations.Dequeue() }
    return $script:observations.Peek()
}

$script:observations = [System.Collections.Generic.Queue[object]]::new()
$script:observations.Enqueue([pscustomobject]@{ ready = $true })
$script:observations.Enqueue([pscustomobject]@{ account_access = $null })
$script:observations.Enqueue([pscustomobject]@{ account_access = @{ busy = $true; active_user_id = 41 } })
$script:observations.Enqueue([pscustomobject]@{ account_access = @{ busy = $false; active_user_id = 42 } })
$script:observations.Enqueue([pscustomobject]@{ account_access = @{ busy = $false; active_user_id = 41 } })

$visited = [System.Collections.Generic.List[int]]::new()
$state = Wait-Account 'available active fixture account' {
    param($account)
    $visited.Add($account.active_user_id)
    $account.active_user_id -eq 41
}
if ($state.account_access.busy -or $state.account_access.active_user_id -ne 41 -or
    ($visited -join ',') -cne '42,41') {
    throw 'Wait-Account did not reject missing, busy, and mismatched account observations.'
}

$control = [pscustomobject]@{
    ProcessId = 41
    AutomationId = 'reader-tab-0-0'
    IsOffscreen = $false
    IsEnabled = $true
    BoundingRectangle = @{ Width = 80; Height = 24 }
}
if (!(Test-OwnedAutomationProperties $control 'reader-tab-0-0' 41) -or
    (Test-OwnedAutomationProperties $control 'reader-tab-0-0' 42) -or
    (Test-OwnedAutomationProperties $control 'reader-tab-0-1' 41)) {
    throw 'The accessibility guard did not verify the exact native control and child process.'
}
$control.IsOffscreen = $true
if (Test-OwnedAutomationProperties $control 'reader-tab-0-0' 41) {
    throw 'The accessibility guard accepted an offscreen native control.'
}

$resultA = "Pull request fixture-org/private #42 $([char]0x00b7) First fixture pull request"
$resultB = "Pull request fixture-org/private #43 $([char]0x00b7) Second fixture pull request"
function Get-OwnedFixtureActivation($Element, [string]$Id) { return $Element.Activation }
function New-FixtureRow([string]$Name, [bool]$Offscreen, [bool]$Actionable) {
    [pscustomobject]@{
        Current = [pscustomobject]@{
            ProcessId = 41; AutomationId = 'reader-result-0'; Name = $Name
            IsOffscreen = $Offscreen; IsEnabled = $true
            BoundingRectangle = @{ Width = 80; Height = 24 }
        }
        Activation = if ($Actionable) { [pscustomobject]@{ Invoke = $true } } else { $null }
    }
}
$realRow = New-FixtureRow $resultA $false $true
$otherRepresentations = @(
    (New-FixtureRow $resultA $true $true)
    (New-FixtureRow $resultB $false $true)
    (New-FixtureRow $resultA $false $false)
)
if ($null -ne (Select-UniqueOwnedFixtureControl $otherRepresentations 'reader-result-0' 41 $resultA) -or
    (Select-UniqueOwnedFixtureControl ($otherRepresentations + $realRow) 'reader-result-0' 41 $resultA) -ne $realRow) {
    throw 'Native PR selection did not isolate the visible actionable exact-name row.'
}
function Stop-Blocked([string]$Message) { throw [InvalidOperationException]::new($Message) }
$refused = $false
try {
    $null = Select-UniqueOwnedFixtureControl @($realRow, (New-FixtureRow $resultA $false $true)) 'reader-result-0' 41 $resultA
} catch [InvalidOperationException] {
    $refused = $_.Exception.Message -like '*multiple eligible controls*'
}
if (!$refused) { throw 'Native PR selection accepted two independently actionable exact-name rows.' }

$serverOrder = @(
    [pscustomobject]@{ Id = 'reader-result-0'; Name = $resultB }
    [pscustomobject]@{ Id = 'reader-result-1'; Name = $resultA }
)
$restoredTabs = @(
    [pscustomobject]@{ Id = 'reader-tab-0-0'; Name = 'Issue fixture-org/private #41 Existing issue' }
    [pscustomobject]@{ Id = 'reader-tab-0-1'; Name = 'Pull request fixture-org/private #43 Second fixture pull request' }
    [pscustomobject]@{ Id = 'reader-tab-0-2'; Name = 'Pull request fixture-org/private #42 First fixture pull request' }
)
if ((Find-FixturePrChoice $serverOrder $resultA) -cne 'reader-result-1' -or
    (Find-FixturePrChoice $restoredTabs 'Pull request fixture-org/private #42 First fixture pull request') -cne 'reader-tab-0-2') {
    throw 'The native pull request journey assumed repository order or tab position.'
}

$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ bounds = [pscustomobject]@{} })
$script:observations.Enqueue([pscustomobject]@{ bounds = [pscustomobject]@{ 'account-open' = @(1, 2, 0, 1) } })
$script:observations.Enqueue([pscustomobject]@{ bounds = [pscustomobject]@{ 'account-open' = @(1, 2, 10, 12) } })
$state = Wait-AccountControl 'account-open'
if ($state.bounds.'account-open'[2] -ne 10) {
    throw 'The reconnect driver accepted a missing or zero-area native control.'
}

$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ cached_reader = $null })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = @{ active_item = 43; cached_content_ready = $true } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = @{ active_item = 44; cached_content_ready = $false } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = @{ active_item = 44; cached_content_ready = $true } })
$visited.Clear()
$state = Wait-Reader 'cached content for the expected item' {
    param($reader)
    $visited.Add($reader.active_item)
    $reader.active_item -eq 44 -and $reader.cached_content_ready
} 1
if (!$state.cached_reader.cached_content_ready -or $state.cached_reader.active_item -ne 44 -or
    ($visited -join ',') -cne '43,44,44') {
    throw 'Wait-Reader did not reject missing, mismatched, and loading reader states.'
}

$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ ready = $true })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = $null })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ active_item = 1 } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ pull_request = $null } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ active_item = 1; pull_request = @{ head = 'old' } } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ active_item = 2; pull_request = @{ head = 'current' } } })
$visited.Clear()
$state = Wait-PrReader 'isolated pull request observation' {
    param($pr, $reader)
    $visited.Add($reader.active_item)
    $reader.active_item -eq 2 -and $pr.head -ceq 'current'
}
if ($state.cached_reader.active_item -ne 2 -or ($visited -join ',') -cne '1,2') {
    throw 'Wait-PrReader did not verify its exact nested predicate.'
}

$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{} })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ shared_sync = @{ connection = 'Offline' } } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ shared_sync = @{ connection = 'Reachable' } } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ shared_sync = @{ connection = 'Unreachable' } } })
$states = [System.Collections.Generic.List[string]]::new()
$state = Wait-Sync 'the exact transport outcome' {
    param($sync)
    $states.Add($sync.connection)
    $sync.connection -ceq 'Unreachable'
}
if ($state.cached_reader.shared_sync.connection -cne 'Unreachable' -or
    ($states -join ',') -cne 'Offline,Reachable,Unreachable') {
    throw 'Wait-Sync did not verify its exact nested state predicate.'
}
$sample = Get-ReaderBudget ([double[]](1..20)) 20
if ($sample.sample_count -ne 20 -or $sample.p95_ms -ne 19 -or $sample.status -ne 'Pass' -or
    (Get-ReaderBudget ([double[]](1..20)) 19).status -ne 'Fail') {
    throw 'The Section 16 reader budget did not enforce the strict p95 ceiling.'
}

$AccessProof = $false
$Measure = $false
$ReaderProof = ''
$path = 'C:\isolated profile\observation.json'
if ((Get-ThreadlineArguments $path -OpenReview) -cne '--e2e-report "C:\isolated profile\observation.json" --fixture-s01 --open-review') {
    throw 'The standard native smoke launch arguments changed.'
}
$ReaderProof = 'Offline'
if ((Get-ThreadlineArguments $path) -cne '--e2e-report "C:\isolated profile\observation.json"' -or
    (Get-ThreadlineArguments $path -SeedReader 'small') -cne '--e2e-seed-reader small --e2e-report "C:\isolated profile\observation.json"' -or
    (Get-ThreadlineArguments $path -SeedReader 'section-16') -cne '--e2e-seed-reader section-16 --e2e-report "C:\isolated profile\observation.json"') {
    throw 'The cached reader and seeder receive distinct entry-point arguments.'
}

$fixture = Join-Path ([IO.Path]::GetTempPath()) ('threadline-shared-sqlite-' + [Guid]::NewGuid().ToString('N'))
$null = [IO.Directory]::CreateDirectory($fixture)
try {
    $database = Join-Path $fixture 'threadline.sqlite3'
    $sharing = [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete
    $writer = [IO.File]::Open($database, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, $sharing)
    try {
        $header = [Text.Encoding]::ASCII.GetBytes('SQLite format 3' + [char]0)
        $writer.Write($header, 0, $header.Length)
        $writer.Flush()
        $live = Get-IsolatedSqlite $fixture -Live
        if ($null -ne $live.sha256) { throw 'The live reader computed an unquiesced database digest.' }
    } finally {
        $writer.Dispose()
    }
    $closed = Get-IsolatedSqlite $fixture
    if ($live.creation_time_ticks -ne $closed.creation_time_ticks -or
        $closed.sha256 -cne '3f8434aff012ffcb594ad7527d6042841ce94f499887f86ab828f84a9a275f91') {
        throw 'The Windows SQLite header and clean-quit digest fixture did not match.'
    }
} finally {
    [IO.Directory]::Delete($fixture, $true)
}
. (Join-Path $PSScriptRoot 'windows-issue-writes.ps1')
$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ bounds = [pscustomobject]@{
    'reader-issue-title-0' = @(1, 2, 20, 10)
} })
$script:fixtureText = $null
$script:fixtureWrites = 0
function Set-OwnedFixtureEditValue([IntPtr]$Window, [string]$Id, [string]$Name, [string]$Value) {
    if ($Window -ne 123 -or $Id -cne 'reader-issue-title-0' -or $Name -cne 'Issue title') {
        if ($Window -ne 123 -or $Id -cne 'reader-search' -or $Name -cne 'Search downloaded content') {
            throw 'Fixture editing did not target the expected owned field.'
        }
    }
    $script:fixtureWrites++
    $script:fixtureText = $Value
    return $true
}
function Get-OwnedFixtureEditPattern([IntPtr]$Window, [string]$Id, [string]$Name) {
    return [pscustomobject]@{ Current = [pscustomobject]@{ Value = $script:fixtureText } }
}
Type-WriteFixtureField 123 'reader-issue-title-0' 'Cancelled fixture issue title'
$refused = $false
try {
    Type-WriteFixtureField 123 'account-token' 'Cancelled fixture issue title'
} catch {
    $refused = $_.Exception.Message -like '*unknown synthetic field or value*'
}
if (!$refused -or $script:fixtureWrites -ne 1) { throw 'The native edit journey accepted an out-of-scope field.' }


$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ bounds = [pscustomobject]@{ 'reader-search' = @(1, 2, 20, 10) } })
$script:fixtureWrites = 0
for ($index = 0; $index -lt 20; $index++) {
    $item = Set-ReaderPerformanceQuery 123 $index
    if ($item -ne (1000 + 3 * $index) -or $script:fixtureText -cne "Investigate relay behavior $item") {
        throw 'Wrong exact native benchmark search value.'
    }
}
$refused = $false
try { $null = Set-ReaderPerformanceQuery 123 20 } catch {
    $refused = $_.Exception.Message -like '*Unknown Section 16 search*'
}
if (!$refused -or $script:fixtureWrites -ne 20) { throw 'The benchmark wrote an unauthorized search fixture.' }

Write-Output 'Native observation predicate checks passed.'


$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ bounds = [pscustomobject]@{
    'reader-result-0' = @(1, 2, 20, 10)
    'reader-result-1' = @(1, 13, 20, 10)
} })
function Find-OwnedFixtureControl([IntPtr]$Window, [string]$Id, [string]$ExpectedName) {
    $candidate = $serverOrder | Where-Object { $_.Id -ceq $Id -and $_.Name -ceq $ExpectedName }
    if ($null -ne $candidate) { return [pscustomobject]@{ Current = $candidate } }
    return $null
}
$script:fixtureInvocations = 0
function Invoke-OwnedFixtureControl([IntPtr]$Window, [string]$Id, $InputClock, [string]$ExpectedName) {
    if ($Window -ne 123 -or $Id -cne 'reader-result-1' -or $ExpectedName -cne $resultA) {
        throw 'The native PR journey lost its selected identity before activation.'
    }
    $script:fixtureInvocations++
    return $script:fixtureInvocations -eq 2
}
function Add-Evidence([string]$Action, $Observed) {
    if ($Observed.automation_id -cne 'reader-result-1' -or $Observed.eligible_controls -ne 1) {
        throw 'The native PR journey did not record the unique expected target.'
    }
}
Activate-FixturePr 123 'reader-result-' 42 'First fixture pull request' -Result
if ($script:fixtureInvocations -ne 2) {
    throw 'The journey did not retry transient pre-activation unavailability within the wait.'
}

function Measure-ReaderPerformance([IntPtr]$Window, $Samples, $Attribution, [switch]$Physical) {
    for ($index = 0; $index -lt 20; $index++) { $Samples.cached_open.Add(100.0) }
    $Samples.cached_tab_switch.Add(90.0)
    $Attribution.cached_tab_switch.Add([ordered]@{ status = 'Correlated' })
    throw 'Native test interruption'
}
$script:report = [ordered]@{ budgets = [ordered]@{ warm_start_ms = @{ status = 'Fail' } } }
$interrupted = $false
try { Invoke-ReaderPerformance 123 -KeepOpen } catch {
    $interrupted = $_.Exception.Message -ceq 'Native test interruption'
}
if (!$interrupted -or $script:report.budgets.warm_start_ms.status -cne 'Fail' -or
    $script:report.budgets.cached_open.p95_ms -ne 100 -or $script:report.budgets.cached_open.status -cne 'Fail' -or
    $script:report.budgets.cached_tab_switch.sample_count -ne 1 -or
    $script:report.budgets.cached_tab_switch.samples_ms[0] -ne 90 -or
    $null -ne $script:report.budgets.cached_tab_switch.p95_ms -or
    $script:report.budgets.cached_tab_switch.status -cne 'Blocked' -or
    $script:report.budgets.local_search.sample_count -ne 0 -or
    $script:report.attribution.cached_tab_switch[0].status -cne 'Correlated') {
    throw 'The interrupted benchmark did not retain completed and partial measurements.'
}
$frequency = [Diagnostics.Stopwatch]::Frequency
$startup = [pscustomobject]@{ startup_ms = 1200; first_cached_workspace_paint = [pscustomobject]@{
    qpc = 3 * $frequency; frequency = $frequency; since_app_start_ms = 1500
} }
$timing = Get-LargeStartupTiming $startup $frequency (2 * $frequency) (4 * $frequency)
if ($timing.status -cne 'Correlated' -or $timing.process_start_call_ms -ne 1000 -or
    $timing.app_ready_ms -ne 1200 -or $timing.launch_to_cached_panes_paint_ms -ne 2000 -or
    $timing.app_start_to_cached_panes_paint_ms -ne 1500 -or $timing.paint_to_verified_receipt_ms -ne 1000) {
    throw 'Startup attribution did not keep the external endpoint and the app paint distinct.'
}
$timing = Get-LargeStartupTiming $startup $frequency (2 * $frequency) (2 * $frequency)
if ($timing.status -cne 'Unavailable' -or $null -ne $timing.launch_to_cached_panes_paint_ms) {
    throw 'Startup timing accepted an app stamp newer than the external observation.'
}

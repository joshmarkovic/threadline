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

function Read-Observation {
    if ($script:observations.Count -gt 1) { return $script:observations.Dequeue() }
    return $script:observations.Peek()
}

$script:observations = [System.Collections.Generic.Queue[object]]::new()
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
Write-Output 'Native observation predicate checks passed.'

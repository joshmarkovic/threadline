Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Import the production polling functions without starting the native smoke driver.
$tokens = $null
$parseErrors = $null
$smoke = Join-Path $PSScriptRoot 'windows-smoke.ps1'
$ast = [System.Management.Automation.Language.Parser]::ParseFile($smoke, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -ne 0) { throw ($parseErrors -join [Environment]::NewLine) }
$waits = $ast.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -cin @('Wait-For', 'Wait-State')
}, $false)
if (@($waits.Name | Sort-Object -Unique).Count -ne 2 -or $waits.Count -ne 2) {
    throw 'Could not load the two native polling functions.'
}
. ([scriptblock]::Create(($waits | ForEach-Object { $_.Extent.Text }) -join [Environment]::NewLine))
. (Join-Path $PSScriptRoot 'windows-github-access.ps1')

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
Write-Output 'Native observation predicate checks passed.'

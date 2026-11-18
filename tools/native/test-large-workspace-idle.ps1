$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'windows-cached-reader.ps1')

$detached = Get-LargeIdleStatus $null 37
if ($detached -cne 'Blocked') { throw 'Idle sampling requires an actual frame receipt.' }
$sample = [pscustomobject]@{
    pid = 37
    settle_ms = 2000
    sample_ms = 3000
    draws_per_second = @(0, 0, 0)
    presents_per_second = @(0, 0, 0)
}
if ((Get-LargeIdleStatus $sample 37) -cne 'Pass') { throw 'A silent idle window should pass.' }
$sample.draws_per_second[0] = 1
if ((Get-LargeIdleStatus $sample 37) -cne 'Blocked') { throw 'One paint must remain visible without proving continuous redraw.' }
$sample.draws_per_second = @(2, 3, 4)
if ((Get-LargeIdleStatus $sample 37) -cne 'Fail') { throw 'Continuous idle redraw should fail.' }
$sample.draws_per_second = @(0, 0, 0)
$sample.presents_per_second = @(1, 1, 1)
if ((Get-LargeIdleStatus $sample 37) -cne 'Fail') { throw 'Continuous presentation should fail.' }
$wrongPid = $false
try { $null = Get-LargeIdleStatus $sample 38 } catch { $wrongPid = $true }
if (!$wrongPid) { throw 'The idle receipt belongs to the wrong process.' }
Write-Output 'Large workspace idle thresholds passed.'

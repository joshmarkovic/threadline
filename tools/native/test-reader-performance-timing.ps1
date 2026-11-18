$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'windows-cached-reader.ps1')

$frequency = [Diagnostics.Stopwatch]::Frequency
$start = 10L * $frequency
$returned = $start + $frequency
$observed = $start + 2L * $frequency
$inputTiming = @{ begin = $start; returned = $returned; method = 'UIA InvokePattern' }
$app = [pscustomobject]@{ cached_reader = [pscustomobject]@{
    performance_timing = [pscustomobject]@{
        kind = 'open'; target = 1000; handler_qpc = $start + ($frequency / 8); frequency = $frequency
        painted_qpc = $start + ($frequency / 2)
    }
} }
$phases = Get-ReaderTimingAttribution $inputTiming $app 'open' 1000 $observed
if ($phases.status -cne 'Correlated' -or $phases.input_call_ms -ne 1000 -or
    $phases.input_to_handler_ms -ne 125 -or $phases.handler_to_useful_paint_ms -ne 375 -or
    $phases.paint_to_observation_ms -ne 1500 -or $phases.return_to_observation_ms -ne 1000 -or
    !$phases.paint_before_input_return) {
    throw 'Did not correlate an app paint occurring while native Invoke was pending.'
}
$app.cached_reader.performance_timing.painted_qpc = $start + ($frequency * 1.5)
$late = Get-ReaderTimingAttribution $inputTiming $app 'open' 1000 $observed
if ($late.status -cne 'Correlated' -or $late.paint_before_input_return -or $late.paint_to_observation_ms -ne 500) {
    throw 'Did not keep paint after Invoke distinct.'
}
$wrong = Get-ReaderTimingAttribution $inputTiming $app 'open' 1003 $observed
if ($wrong.status -cne 'Uncorrelated' -or ($wrong.Keys -contains 'handler_to_useful_paint_ms')) {
    throw 'A timestamp for the wrong item was attributed to the sample.'
}
$app.cached_reader.performance_timing.frequency = 1
$foreign = Get-ReaderTimingAttribution $inputTiming $app 'open' 1000 $observed
if ($foreign.status -cne 'Uncorrelated') { throw 'Accepted incompatible monotonic clock units.' }
$old = Get-ReaderTimingAttribution $inputTiming ([pscustomobject]@{ cached_reader = [pscustomobject]@{} }) 'open' 1000 $observed
if ($old.status -cne 'Unavailable' -or $old.input_call_ms -ne 1000) { throw 'Lost native call timing for the earlier app build.' }
Write-Output 'Reader performance timing attribution passed.'

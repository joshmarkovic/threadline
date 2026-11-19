function Open-MixedIssue([IntPtr]$Window, [switch]$Updated) {
    $title = if ($Updated) { 'Updated pagination fixture' } else { 'Reader pagination fixture' }
    $name = 'Issue fixture-org/private #41 ' + [char]0x00b7 + ' ' + $title
    Wait-For 'open the original closed issue from its downloaded repository' {
        $view=Read-Observation
        if (!$view) { return $false }
        foreach ($id in $view.bounds.PSObject.Properties.Name) {
            if ($id -cmatch '^reader-result-[0-9]+$' -and (Find-OwnedFixtureControl $Window $id $name)) {
                return Invoke-OwnedFixtureControl $Window $id $null $name
            }
        }
        return $false
    } 20000 150
    $null=Wait-Reader 'the cached existing issue' {param($r) $r.active_number -eq 41 -and $r.cached_content_ready}
}

function Read-MixedCheckpoint([string]$Profile,[string]$Output) {
    if ($ReaderProof -cne 'MixedCatchup') { throw 'Unexpected mixed checkpoint request.' }
    $child=Start-Threadline $Profile $Output -ReaderRecovery 'snapshot'
    try {
        if (!$child.WaitForExit(10000)) {
            $child.Kill()
            $null=$child.WaitForExit(3000)
            throw 'The isolated read-only checkpoint inspector timed out.'
        }
        if ($child.ExitCode -ne 0) { throw 'The read-only mixed checkpoint inspector failed.' }
    } finally { $child.Dispose() }
    return [IO.File]::ReadAllText($Output) | ConvertFrom-Json
}

function Wait-MixedCheckpoint([string]$Profile,[string]$Name,[scriptblock]$Ready) {
    if ($Name -cnotmatch '^mixed-(before|restored|after|review|check|status|changed-check|validated-status|old-feed)$') {
        throw 'Unknown mixed fixture receipt.'
    }
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {
        $temp=Join-Path $script:ProfileDir ('mixed-snapshot-'+[guid]::NewGuid().ToString('N')+'.json')
        try {
            $observed = Read-MixedCheckpoint $Profile $temp
            if (& $Ready $observed) {
                [IO.File]::Copy($temp,(Join-Path $script:ReportDir ($Name+'.json')),$false)
                return $observed
            }
        } finally {
            if ([IO.File]::Exists($temp)) { [IO.File]::Delete($temp) }
        }
        Start-Sleep -Seconds 1
    } while ($watch.ElapsedMilliseconds -lt 110000)
    throw "Timed out waiting for the scoped SQLite evidence for $Name."
}
function Invoke-MixedCatchup([IntPtr]$Window,[string]$Profile) {
    Wait-SeededFixtureAccount
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    Open-MixedIssue $Window
    Click-AccountControl $Window 'reader-fetch-timeline-0'
    $null=Wait-Reader 'the downloaded unrelated timeline' {param($r) $r.timeline_coverage -ceq 'Complete'}
    Click-AccountControl $Window 'reader-fetch-comments-0'
    $null=Wait-Reader 'a partial discussion and its first old comment' {param($r) $r.comment_count -eq 1 -and $r.comments_coverage -ceq 'Partial'}
    Activate-FixturePr $Window 'reader-result-' 42 'First fixture pull request' -Result
    $initial=Wait-PrReader 'the same-repository PR displays cached metadata' {param($pr,$r) $r.active_number -eq 42 -and $r.cached_content_ready}
    if (!$initial.cached_reader.pull_request.head) { Click-AccountControl $Window 'pr-0-refresh' }
    $null=Wait-PrReader 'the exact immutable comparison head' {param($pr) $pr.head -ceq ('b'*40)}
    Click-AccountControl $Window 'pr-0-conversation'
    Click-AccountControl $Window 'pr-0-reviews-fetch'
    $null=Wait-MixedCheckpoint $Profile 'mixed-review' {param($v) @($v.mixed.pr | Where-Object {$_.review_id -eq 7201 -and $_.review -ceq 'APPROVED'}).Count -eq 1}
    Click-AccountControl $Window 'pr-0-checks'
    Click-AccountControl $Window 'pr-0-checks-fetch'
    $null=Wait-MixedCheckpoint $Profile 'mixed-check' {param($v) @($v.mixed.pr | Where-Object {$_.check_id -eq 7202 -and $_.check -ceq 'in_progress'}).Count -eq 1}
    Click-AccountControl $Window 'pr-0-statuses-fetch'
    $complete={
        param($v)
        $v.mixed.comment_feed_watermark -gt 0 -and $v.mixed.timeline_digest -and
        @($v.mixed.monitoring_started.PSObject.Properties).Count -gt 0 -and
        @($v.mixed.pr | Where-Object {$_.status_id -eq 7203 -and $_.status -ceq 'success' -and $_.coverage -eq 2 -and $_.etag -ceq '"fixture-status-v1"'}).Count -eq 1 -and
        @($v.mixed.pr | Where-Object {$_.review_id -eq 7201 -and $_.review -ceq 'APPROVED'}).Count -eq 1 -and
        @($v.mixed.pr | Where-Object {$_.check_id -eq 7202 -and $_.check -ceq 'in_progress'}).Count -eq 1 -and
        @($v.comments | Where-Object {$_.node -ceq 'IC_5101' -and $_.author -ceq 'fixture-author' -and !$_.edited_original}).Count -eq 1 -and
        $v.discussion.cursor -ceq '2'
    }
    $null=Wait-MixedCheckpoint $Profile 'mixed-status' $complete
    Set-WriteFixtureOffline $Window $true
    $baseline=Wait-MixedCheckpoint $Profile 'mixed-before' $complete
    $watermark=Join-Path ([IO.Path]::GetDirectoryName($script:AccessPhaseFile)) 'mixed-comment-watermark.txt'
    $file=[IO.File]::Open($watermark,[IO.FileMode]::CreateNew)
    try {
        $value = ([long]$baseline.mixed.comment_feed_watermark).ToString([Globalization.CultureInfo]::InvariantCulture)
        $bytes = [Text.Encoding]::ASCII.GetBytes($value)
        $file.Write($bytes,0,$bytes.Length)
    } finally { $file.Dispose() }
    $window=Restart-Reader $Profile -FixturePhaseAfterQuit 'mixed-offline'
    $null=Wait-PrReader 'restore the same native PR before allowing any provider reply' {
        param($pr,$r) $r.active_number -eq 42 -and $r.offline -and $r.cached_content_ready -and $pr.head -ceq ('b'*40) -and $pr.checks_coverage -ceq 'Complete'
    }
    $restored=Wait-MixedCheckpoint $Profile 'mixed-restored' $complete
    foreach ($field in @('mixed','discussion','comments')) {
        if (($baseline.$field | ConvertTo-Json -Depth 8 -Compress) -cne ($restored.$field | ConvertTo-Json -Depth 8 -Compress)) {
            throw 'The closed-profile checkpoints changed before the first online response.'
        }
    }
    Add-Evidence 'Restarted offline with the original review, in-progress check, saved comment cursor and complete timeline' $restored
    Set-AccessPhase 'mixed-after'
    Set-WriteFixtureOffline $window $false
    Click-AccountControl $window 'pr-0-checks-fetch'
    $null=Wait-MixedCheckpoint $Profile 'mixed-changed-check' {param($v)
        @($v.mixed.pr | Where-Object {$_.check_id -eq 7202 -and $_.check -ceq 'completed' -and $_.conclusion -ceq 'success'}).Count -eq 1
    }
    $oldStatus=@($baseline.mixed.pr | Where-Object {$_.status_id -eq 7203})[0]
    Click-AccountControl $window 'pr-0-statuses-fetch'
    $null=Wait-MixedCheckpoint $Profile 'mixed-validated-status' {param($v)
        @($v.mixed.pr | Where-Object {$_.status_id -eq 7203 -and $_.validated_ms -gt $oldStatus.validated_ms}).Count -eq 1
    }
    Click-AccountControl $window 'pr-0-conversation'
    Click-AccountControl $window 'pr-0-reviews-fetch'
    $null=Wait-MixedCheckpoint $Profile 'mixed-old-feed' {param($v)
        @($v.comments | Where-Object {$_.id -ceq '00000000000000005101' -and $_.edited_original}).Count -eq 1 -and
        @($v.mixed.pr | Where-Object {$_.check_id -eq 7202 -and $_.check -ceq 'completed' -and $_.conclusion -ceq 'success'}).Count -eq 1 -and
        @($v.mixed.pr | Where-Object {$_.review_id -eq 7201 -and $_.review -ceq 'DISMISSED'}).Count -eq 1
    }
    Open-MixedIssue $window -Updated
    Click-AccountControl $window 'reader-fetch-comments-0'
    $null=Wait-Reader 'the ordinary reader finishes its saved second page' {param($r) $r.active_number -eq 41 -and $r.comments_coverage -ceq 'Complete' -and $r.comment_count -eq 2}
    $final=Wait-MixedCheckpoint $Profile 'mixed-after' {param($v) $v.discussion.coverage -eq 2 -and !$v.discussion.cursor -and @($v.comments).Count -eq 2}
    foreach($key in $baseline.mixed.monitoring_started.PSObject.Properties) {
        if (!$final.mixed.monitoring_started.PSObject.Properties[$key.Name] -or $final.mixed.monitoring_started.($key.Name) -ne $key.Value) {throw 'An existing repository monitoring baseline was reset.'}
    }
    $newStatus=@($final.mixed.pr | Where-Object {$_.status_id -eq 7203})[0]
    $oldComment=@($baseline.comments | Where-Object {$_.node -ceq 'IC_5101'})[0]
    $newComment=@($final.comments | Where-Object {$_.node -ceq 'IC_5101'})[0]
    if ($final.mixed.timeline_digest -cne $baseline.mixed.timeline_digest -or
        $newStatus.status -cne $oldStatus.status -or $newStatus.etag -cne $oldStatus.etag -or $newStatus.validated_ms -le $oldStatus.validated_ms -or
        !$newComment.edited_original -or $newComment.id -cne $oldComment.id -or $newComment.author -cne $oldComment.author -or
        $newComment.created_ms -ne $oldComment.created_ms -or $newComment.updated_ms -le $oldComment.updated_ms) {
        throw 'The cached baseline, status validators or original comment identity changed unexpectedly.'
    }
    Add-Evidence 'Catch up the edited old issue comment and PR review/check while retaining the conditional status and unrelated timeline' $final
    Stop-ThroughUi
    $script:report.detail='A process restart displayed cached content while offline. The bounded repository feed and saved second discussion page updated the existing issue, the same PR retained its review and check identities, and its unchanged commit status revalidated against the saved ETag.'
}

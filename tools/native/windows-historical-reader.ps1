function Invoke-HistoricalLocalSearch([IntPtr]$Window, [string]$Query) {
    $id = 'reader-search'
    $name = 'Search downloaded content'
    Wait-For 'the owned downloaded-content search editor' {
        Set-OwnedFixtureEditValue $Window $id $name $Query
    }
    Wait-For 'the exact synthetic query in the reader' {
        $pattern = Get-OwnedFixtureEditPattern $Window $id $name
        $null -ne $pattern -and $pattern.Current.Value -ceq $Query
    }
    $prior = (Read-Observation).cached_reader.search_generation
    Click-AccountControl $Window 'reader-search-submit'
    return Wait-Reader 'the current local search result' {
        param($reader)
        $null -ne $reader.search_generation -and ($null -eq $prior -or $reader.search_generation -gt $prior) -and
            $null -ne $reader.search_results
    }
}

function Invoke-HistoricalReader([IntPtr]$Window, [string]$Profile) {
    Wait-SeededFixtureAccount
    Click-AccountControl $Window 'reader-repository-1'
    $missing = Invoke-HistoricalLocalSearch $Window 'Observatory'
    if ($missing.cached_reader.search_results -ne 0) { throw 'The historical issue was already cached.' }
    Add-Evidence 'Search downloaded content before fetching the historical issue' ([ordered]@{local_matches=0})
    Set-AccessPhase 'historical-remote'
    Wait-For 'the owned Search GitHub control' {
        Invoke-OwnedFixtureControl $Window 'reader-search-github' -ExpectedName 'Search GitHub'
    }
    Wait-For 'the historical issue in the scoped GitHub results' {
        Invoke-OwnedFixtureControl $Window 'reader-remote-result-0' -ExpectedName '#719 Observatory logbook'
    }
    $opened = Wait-Reader 'the exact historical item opened from remote results' {
        param($reader) $reader.cached_content_ready -and $reader.active_number -eq 719
    }
    $item = $opened.cached_reader.active_item
    $pane = $opened.cached_reader.focused_pane
    Click-AccountControl $Window ("reader-fetch-metadata-" + $pane)
    $downloaded = Invoke-HistoricalLocalSearch $Window 'Xenolith'
    if ($downloaded.cached_reader.search_results -ne 1 -or $downloaded.cached_reader.first_search_item -ne $item) {
        throw 'Downloaded local search did not find the body returned only by the historical details request.'
    }
    Add-Evidence 'Open the old closed issue and refresh its details' ([ordered]@{
        number=719; state='closed'; fixture_closed_utc='2018-01-03'; local_body_matches=1
    })
    Open-SyncSettings $Window
    Click-AccountControl $Window 'sync-work-offline'
    $null = Wait-Sync 'saved Work Offline selection' { param($sync) $sync.controls.work_offline }
    Set-AccessPhase 'sync-offline'
    $restarted = Restart-Reader $Profile
    $null = Wait-Reader 'the historical issue restored from disk while offline' {
        param($reader) $reader.offline -and $reader.cached_content_ready -and $reader.active_number -eq 719 -and $reader.active_item -eq $item
    }
    $saved = Invoke-HistoricalLocalSearch $restarted 'Xenolith'
    if ($saved.cached_reader.search_results -ne 1 -or $saved.cached_reader.first_search_item -ne $item) {
        throw 'The downloaded historical body did not survive offline restart.'
    }
    Click-AccountControl $restarted 'reader-result-0'
    $null = Wait-Reader 'the exact offline local result in the reader' {
        param($reader) $reader.offline -and $reader.cached_content_ready -and $reader.active_number -eq 719 -and $reader.active_item -eq $item
    }
    if ($script:report.app_pid -eq $script:report.restarted_pid) { throw 'A separate reader process was required.' }
    Add-Evidence 'Find and reopen the historical issue body after an offline process restart' ([ordered]@{
        number=719; local_matches=1; offline=$true; separate_process=$true
    })
    Stop-ThroughUi
    $script:report.detail = 'The historical closed issue was absent from local search, opened from a scoped provider result, and remained searchable and readable after an offline restart.'
}

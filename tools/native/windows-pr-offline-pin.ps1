function Wait-FixturePrPin([string]$Expected, [long]$Item) {
    return Wait-PrReader "the exact PR shows an offline download state of $Expected" {
        param($pr, $reader)
        $reader.active_item -eq $Item -and $pr.download_state -ceq $Expected
    }
}

function Wait-FixturePrPinMarker([string]$File, [string]$Description, [int]$Timeout) {
    if ($File -cnotin @('pr-pin-interrupted.txt', 'pr-pin-resumed.txt')) { throw 'Unknown PR download fixture marker.' }
    $path = Join-Path ([IO.Path]::GetDirectoryName($script:AccessPhaseFile)) $File
    Wait-For $Description {
        [IO.File]::Exists($path) -and [IO.File]::ReadAllText($path) -ceq 'observed'
    } $Timeout 200
}

function Invoke-PrOfflinePinProof([IntPtr]$Window, [string]$Profile) {
    Wait-SeededFixtureAccount
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    Activate-FixturePr $Window 'reader-result-' 42 'First fixture pull request' -Result
    $first = Wait-PrReader 'the PR selected for the download test' {
        param($pr, $reader) $reader.active_number -eq 42 -and $reader.cached_content_ready
    }
    $item = $first.cached_reader.active_item
    if ($null -eq $first.cached_reader.pull_request.head) { Click-AccountControl $Window 'pr-0-refresh' }
    Click-AccountControl $Window 'pr-0-files'
    Wait-For 'the native 300-file API limit notice' {
        $null -ne (Find-OwnedFixtureControl $Window 'pr-0-files-api-cap' 'GitHub returned its 300-file comparison limit. Other changed files may be missing.' -AllowDisabled)
    } 30000 150
    $initial = Wait-PrReader 'the cached capped comparison has the original verified head' {
        param($pr, $reader)
        $reader.active_item -eq $item -and $pr.head -ceq ('b' * 40) -and
            $pr.files_coverage -ceq 'Partial' -and $pr.files_on_page -eq 20
    }
    $revision = $initial.cached_reader.pull_request.revision_id
    $fileStates = @(
        @{ name = 'binary.dat'; path = 'src/binary.dat'; label = 'Binary file. No text patch is available.' }
        @{ name = 'truncated.rs'; path = 'src/truncated.rs'; label = 'Partial patch. GitHub did not supply every changed line.' }
        @{ name = 'unavailable.rs'; path = 'src/unavailable.rs'; label = 'GitHub did not supply a text patch for this file.' }
    )
    for ($index = 0; $index -lt $fileStates.Count; $index++) {
        $file = $fileStates[$index]
        Wait-For 'select the exact fixture file' {
            Invoke-OwnedFixtureControl $Window "pr-0-file-$index" $null $file.name
        }
        $null = Wait-PrReader 'the selected file is bound to the same comparison' {
            param($pr, $reader) $reader.active_item -eq $item -and $pr.revision_id -eq $revision -and $pr.selected_file -ceq $file.path
        }
        Wait-For 'the specific native file patch status' {
            $null -ne (Find-OwnedFixtureControl $Window 'pr-0-patch-state' $file.label -AllowDisabled)
        }
        Add-Evidence 'Read the exact unavailable or incomplete patch state' @{ path=$file.path; label=$file.label; revision=$revision }
    }

    Set-WriteFixtureOffline $Window $true
    Set-AccessPhase 'pr-pin-offline'
    Click-AccountControl $Window 'pr-0-download-context'
    $null = Wait-FixturePrPin 'Pending' $item
    Click-AccountControl $Window 'pr-0-cancel-download'
    $null = Wait-FixturePrPin 'Cancelled' $item
    Add-Evidence 'Queue and cancel a selected source and media pin while offline' @{ item=$item; revision=$revision }
    Click-AccountControl $Window 'pr-0-download'
    $null = Wait-FixturePrPin 'Pending' $item

    Click-AccountControl $Window 'reader-browse'
    Activate-FixturePr $Window 'reader-result-' 43 'Second fixture pull request' -Result
    $second = Wait-Reader 'the independent PR remains active before reconnecting' {
        param($reader) $reader.active_number -eq 43 -and $reader.offline -and $reader.active_item -ne $item
    }
    $other = $second.cached_reader.active_item
    Set-AccessPhase 'pr-pin-interrupt'
    Set-WriteFixtureOffline $Window $false
    Wait-FixturePrPinMarker 'pr-pin-interrupted.txt' 'the first PR reached its interrupted second discussion page while another PR was active' 40000
    $null = Wait-Reader 'the other PR still owns the window when the first pin is interrupted' {
        param($reader) $reader.active_item -eq $other -and $reader.active_number -eq 43
    }
    Add-Evidence 'Reach an interrupted second discussion page while reading another PR' @{ pinned_item=$item; active_item=$other; fixture_status=502 }

    Set-AccessPhase 'pr-pin-continue'
    $restart = Restart-Reader $Profile
    $null = Wait-Reader 'restart restores the other PR before the prior download finishes' {
        param($reader) $reader.active_item -eq $other -and $reader.active_number -eq 43 -and $reader.cached_content_ready
    }
    Wait-FixturePrPinMarker 'pr-pin-resumed.txt' 'the first PR continues through its exact second page and required PR families' 75000
    $null = Wait-Reader 'the other PR remains active throughout the resumed download' {
        param($reader) $reader.active_item -eq $other -and $reader.active_number -eq 43
    }
    Activate-FixturePr $restart 'reader-result-' 42 'First fixture pull request' -Result
    $finished = Wait-FixturePrPin 'Partial' $item
    $cached = $finished.cached_reader
    if ($cached.pull_request.revision_id -ne $revision -or $cached.pull_request.files_coverage -cne 'Partial' -or
        $cached.comments_coverage -cne 'Complete' -or $cached.comment_count -ne 2) {
        throw 'The pin failed to retain its exact revision, two distinct discussion records, and partial capped file coverage.'
    }
    Add-Evidence 'Read complete resumed discussion and honest partial coverage on the original pinned PR' @{ item=$item; revision=$revision; comments=$cached.comment_count; files='Partial' }
    Stop-ThroughUi
    $script:report.detail = 'The native PR reader identified binary, truncated, unavailable and capped content. Offline cancellation made no requests. A second pin resumed its saved page after interruption and restart while another PR stayed active. Two discussion records became complete and the capped comparison remained partial.'
}

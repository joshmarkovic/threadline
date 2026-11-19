function Queue-FixtureDirectMerge([IntPtr]$Window, [string]$ExpectedHead, [int]$Pending = 1) {
    Click-AccountControl $Window 'pr-0-merge-open'
    Click-AccountControl $Window 'pr-0-merge-refresh'
    $null = Wait-PrReader 'the matching unmerged pull request with native squash controls' {
        param($pr) $pr.head -ceq $ExpectedHead -and $pr.merged -eq $false
    }
    Click-AccountControl $Window 'pr-0-merge-squash'
    Set-WriteFixtureOffline $Window $true
    Click-AccountControl $Window 'pr-0-merge-confirm'
    $null = Wait-PrReader 'the saved merge intent while GitHub still reports unmerged' {
        param($pr, $reader)
        $reader.offline -and $reader.outbox -eq $Pending -and $pr.head -ceq $ExpectedHead -and $pr.merged -eq $false
    }
    Click-AccountControl $Window 'reader-outbox'
    Click-AccountControl $Window 'reader-outbox-filter-pending'
}

function Invoke-MergeQueueProof([IntPtr]$Window) {
    Wait-SeededFixtureAccount
    Open-SyncSettings $Window
    Click-AccountControl $Window 'sync-pause-background'
    $null = Wait-Sync 'unrelated reads paused with queue dispatch enabled' {
        param($sync) $sync.controls.pause_background -and !$sync.controls.pause_queued_actions
    }
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    $fixtures = @(
        @{ index = 0; number = 42; name = 'First fixture pull request'; head = 'b' * 40; path = 'src/first.rs'; phase = 'merge-queue-preflight'; requests = 0 },
        @{ index = 1; number = 43; name = 'Second fixture pull request'; head = 'e' * 40; path = 'src/second.rs'; phase = 'merge-queue-rejected'; requests = 1 }
    )
    foreach ($fixture in $fixtures) {
        Set-AccessPhase 'merge-prepare'
        Click-AccountControl $Window 'reader-repository-1'
        $null = Open-FixturePr $Window 0 $fixture.number $fixture.name $fixture.head $fixture.path
        Queue-FixtureDirectMerge $Window $fixture.head ($fixture.index + 1)
        Inspect-WriteFixtureAction $Window $fixture.index 'Queued' 'Merge pull request'
        Set-AccessPhase $fixture.phase
        Set-WriteFixtureOffline $Window $false
        Click-AccountControl $Window 'reader-outbox'
        Click-AccountControl $Window 'reader-outbox-filter-attention'
        Inspect-WriteFixtureAction $Window $fixture.index 'NeedsAttention' 'Merge pull request'
        $null = Wait-Reader 'the selected merge shows the unsupported queue requirement' {
            param($reader) $reader.outbox_visible -and $reader.outbox_merge_queue_required -and
                $reader.outbox -eq ($fixture.index + 1)
        }
        Click-AccountControl $Window 'reader-outbox-open'
        $null = Wait-PrReader 'the original unmerged PR and saved head after the queue hold' {
            param($pr, $reader) $reader.active_number -eq $fixture.number -and
                $pr.head -ceq $fixture.head -and $pr.merged -eq $false
        }
        Add-Evidence 'Retain required-queue intent in native Outbox' ([ordered]@{
            phase = $fixture.phase; number = $fixture.number; expected_head = $fixture.head
            needs_attention = $true; expected_direct_puts = $fixture.requests
        })
    }
    Stop-ThroughUi
    $script:report.detail = 'The native Outbox retained both original unmerged intents when policy required a queue and when GitHub rejected the direct request with a queue requirement.'
}

function Invoke-MergeProof([IntPtr]$Window) {
    $firstHead = 'b' * 40
    $secondHead = 'e' * 40
    Wait-SeededFixtureAccount
    Open-SyncSettings $Window
    Click-AccountControl $Window 'sync-pause-background'
    $null = Wait-Sync 'only unrelated scheduled reads are paused' {
        param($sync) $sync.controls.pause_background -and !$sync.controls.pause_queued_actions
    }
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    $first = Open-FixturePr $Window 0 42 'First fixture pull request' $firstHead 'src/first.rs'
    if ($first.cached_reader.outbox -ne 0) { throw 'The isolated merge profile already has pending work.' }
    Queue-FixtureDirectMerge $Window $firstHead
    Inspect-WriteFixtureAction $Window 0 'Queued' 'Merge pull request'
    Add-Evidence 'Queue a native squash merge for the exact original head while offline' ([ordered]@{
        expected_head = $firstHead; merged_before_send = $false; outbox = 'Queued'
    })

    Set-AccessPhase 'merge-send'
    Set-WriteFixtureOffline $Window $false
    Click-AccountControl $Window 'reader-outbox'
    Click-AccountControl $Window 'reader-outbox-filter-history'
    Inspect-WriteFixtureAction $Window 0 'Confirmed' 'Merge pull request'
    Click-AccountControl $Window 'reader-outbox-open'
    $null = Wait-PrReader 'the original PR becomes merged only after authoritative provider readback' {
        param($pr, $reader) $reader.active_number -eq 42 -and $pr.head -ceq $firstHead -and $pr.merged -eq $true
    }
    Add-Evidence 'Confirm merge after authoritative provider readback' ([ordered]@{
        expected_head = $firstHead; merged = $true; outbox = 'Confirmed'
    })

    Set-AccessPhase 'merge-prepare'
    Click-AccountControl $Window 'reader-repository-1'
    $null = Open-FixturePr $Window 0 43 'Second fixture pull request' $secondHead 'src/second.rs'
    Queue-FixtureDirectMerge $Window $secondHead
    Inspect-WriteFixtureAction $Window 1 'Queued' 'Merge pull request'
    Set-AccessPhase 'merge-wrong-head'
    Set-WriteFixtureOffline $Window $false
    Click-AccountControl $Window 'reader-outbox'
    Click-AccountControl $Window 'reader-outbox-filter-attention'
    Inspect-WriteFixtureAction $Window 1 'NeedsAttention' 'Merge pull request'
    Click-AccountControl $Window 'reader-outbox-open'
    $null = Wait-PrReader 'the held second merge retains the original item and has no merged claim' {
        param($pr, $reader) $reader.active_number -eq 43 -and $pr.merged -eq $false
    }
    Add-Evidence 'Fresh provider head change holds the queued merge before any send' ([ordered]@{
        expected_head = $secondHead; fresh_head = ('9' * 40); merged = $false; outbox = 'NeedsAttention'
    })
    Stop-ThroughUi
    $script:report.detail = 'An offline native squash intent confirmed only after exact-head submission and readback. A second offline intent was held without sending when the provider returned a changed head.'
}

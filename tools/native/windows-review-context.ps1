function Select-FixtureRightReviewLine([IntPtr]$Window, [string]$FixturePath = 'src/first.rs') {
    if ($FixturePath -cnotin @('src/first.rs', 'src/second.rs')) { throw 'Unknown native fixture comparison.' }
    $settling = [Diagnostics.Stopwatch]::StartNew()
    $bounds = $null
    while ($settling.ElapsedMilliseconds -lt 5000) {
        $state = Read-Observation
        $property = $state.bounds.PSObject.Properties['pr-0-diff']
        if ($null -ne $property -and $property.Value.Count -eq 4 -and
            $property.Value[2] -ge 180 -and $property.Value[3] -ge 32) {
            $bounds = $property.Value
            break
        }
        Start-Sleep -Milliseconds 100
    }
    if ($null -eq $bounds) { Stop-Blocked 'The maximized split comparison does not have a reachable native code region.' }
    Set-WindowFocus $Window
    $originX = $bounds[0] + $bounds[2] * 0.75
    # The isolated fixture comparison contains one change on line one. Probe
    # only its visible right-side code region, then accept the reported source.
    foreach ($offset in @(20, 33, 46, 59, 72, 85, 98)) {
        if ($offset + 12 -gt $bounds[3]) { break }
        $state = Read-Observation
        $current = $state.bounds.PSObject.Properties['pr-0-diff']
        if ($null -eq $current -or (Compare-Object $bounds $current.Value -SyncWindow 0)) {
            Stop-Blocked 'The native comparison moved while selecting the fixture line.'
        }
        $point = To-ClientPoint $originX ($bounds[1] + $offset) $state.scale_factor
        [ThreadlineWinInput]::ClickClient($Window, $point, $script:app.Id, [uint32][Math]::Round($state.scale_factor * 96))
        $wait = [Diagnostics.Stopwatch]::StartNew()
        while ($wait.ElapsedMilliseconds -lt 900) {
            $state = Read-Observation
            $anchor = $state.cached_reader.pull_request.source_anchor
            if ($null -ne $anchor -and $anchor.path -ceq $FixturePath -and
                $anchor.side -ceq 'Right' -and $anchor.line -eq 1) { return }
            Start-Sleep -Milliseconds 75
        }
    }
    Stop-Blocked 'Could not select the guarded native source line in the fixture split comparison.'
}

function Wait-ExactFixtureReview([string]$DraftId, [string]$Head, [string]$FixturePath = 'src/first.rs') {
    Wait-PrReader 'the same saved native anchored review draft' {
        param($pr, $reader)
        $draft = $pr.review_draft
        $null -ne $draft -and $draft.id -ceq $DraftId -and $draft.head -ceq $Head -and
            $draft.owner_tab -and $draft.saved -and @($draft.anchors).Count -eq 1 -and
            $draft.anchors[0].path -ceq $FixturePath -and $draft.anchors[0].side -ceq 'Right' -and
            $draft.anchors[0].first_line -eq 1 -and $draft.anchors[0].last_line -eq 1 -and
            $draft.anchors[0].source_oid -ceq $Head -and $draft.anchors[0].comment_nonempty -and
            $pr.selected_file -ceq $FixturePath -and $pr.diff_mode -ceq 'Split'
    }
}

function Invoke-ReviewContextProof([IntPtr]$Window, [string]$Profile) {
    $head = 'b' * 40
    $otherHead = 'e' * 40
    $title = 'First fixture pull request'
    Wait-SeededFixtureAccount
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    $first = Open-FixturePr $Window 0 42 $title $head 'src/first.rs'
    $firstItem = $first.cached_reader.active_item
    Click-AccountControl $Window 'pr-0-diff-split'
    $null = Wait-PrReader 'the split comparison for the original fixture head' {
        param($pr) $pr.head -ceq $head -and $pr.diff_mode -ceq 'Split'
    }
    Click-AccountControl $Window 'reader-maximize-pane'
    Select-FixtureRightReviewLine $Window
    Click-AccountControl $Window 'pr-0-review-add-inline'
    $draftReady = Wait-PrReader 'one draft anchored to the chosen native source line' {
        param($pr)
        $null -ne $pr.review_draft -and @($pr.review_draft.anchors).Count -eq 1
    }
    $draftId = $draftReady.cached_reader.pull_request.review_draft.id
    $editor = 'pr-0-review-inline'
    $text = 'Threadline synthetic inline review note.'
    $null = Wait-AccountControl $editor
    Wait-For 'the owned inline review editor accepts the fixture note' {
        Set-OwnedFixtureEditValue $Window $editor 'Inline review comment' $text
    }
    $null = Wait-ExactFixtureReview $draftId $head
    Add-Evidence 'Enter and save a native inline review on the exact source line' ([ordered]@{
        item = $firstItem; draft = $draftId; expected_head = $head
        path = 'src/first.rs'; side = 'Right'; first_line = 1; last_line = 1
    })

    Click-PrContext $Window 0
    Click-AccountControl $Window 'pr-0-context-issue'
    Click-AccountControl $Window 'pr-0-verify-0'
    $null = Wait-PrReader 'the first directly verified issue' { param($pr) $pr.verified_targets -eq 1 }
    Click-AccountControl $Window 'pr-0-verify-1'
    $null = Wait-PrReader 'both directly verified issues' { param($pr) $pr.verified_targets -eq 2 }
    Click-AccountControl $Window 'pr-0-linked-0'
    $firstLinked = Wait-PrReader 'the first linked issue model' {
        param($pr) $pr.context_mode -ceq 'Issue' -and $null -ne $pr.linked_issue
    }
    $firstLinkedId = $firstLinked.cached_reader.pull_request.linked_issue
    Click-AccountControl $Window 'pr-0-linked-1'
    $secondLinked = Wait-PrReader 'the alternate verified linked issue' {
        param($pr)
        $pr.context_mode -ceq 'Issue' -and $null -ne $pr.linked_issue -and $pr.linked_issue -ne $firstLinkedId
    }
    Click-AccountControl $Window 'pr-0-context-body'
    $null = Wait-PrReader 'the original pull request body' { param($pr) $pr.context_mode -ceq 'Body' }
    Click-AccountControl $Window 'pr-0-context-more'
    Click-AccountControl $Window 'pr-0-context-collapse'
    $null = Wait-PrReader 'the collapsed original context' { param($pr) $pr.context_collapsed }
    $null = Wait-ExactFixtureReview $draftId $head
    Add-Evidence 'Switch both verified issues, return to the body, resize and collapse' ([ordered]@{
        first_link = $firstLinkedId; alternative_link = $secondLinked.cached_reader.pull_request.linked_issue
        original_head = $head; draft_retained = $true
    })

    Click-AccountControl $Window 'reader-maximize-pane'
    $second = Open-FixturePr $Window 0 43 'Second fixture pull request' $otherHead 'src/second.rs'
    if ($second.cached_reader.active_item -eq $firstItem -or $second.cached_reader.tab_counts[0] -ne 2 -or
        $null -ne $second.cached_reader.pull_request.review_draft) {
        throw 'The second pull request displayed the first draft or changed the expected tab set.'
    }
    Click-AccountControl $Window 'reader-tab-0-0'
    $null = Wait-ExactFixtureReview $draftId $head
    Click-AccountControl $Window 'reader-duplicate-tab'
    $null = Wait-Reader 'the independent duplicate pane' { param($reader) $reader.focused_pane -eq 1 }
    Click-AccountControl $Window 'reader-focus-pane'
    $null = Wait-Reader 'focus the original pane without replacing either view' {
        param($reader) $reader.focused_pane -eq 0 -and $reader.active_item -eq $firstItem
    }
    Click-AccountControl $Window 'pr-0-show-context'
    $null = Wait-PrReader 'the original tab in narrow context mode' { param($pr) $pr.narrow_surface -ceq 'Context' }
    Click-AccountControl $Window 'pr-0-show-code'
    $null = Wait-PrReader 'the original tab in narrow code mode' { param($pr) $pr.narrow_surface -ceq 'Code' }
    Click-AccountControl $Window 'reader-maximize-pane'
    $null = Wait-ExactFixtureReview $draftId $head
    Add-Evidence 'Switch PRs and use the narrow and restored code surfaces without moving the draft' ([ordered]@{
        original_item = $firstItem; alternate_item = $second.cached_reader.active_item; original_draft = $draftId
    })

    Set-WriteFixtureOffline $Window $true
    $restarted = Restart-Reader $Profile
    $retained = Wait-ExactFixtureReview $draftId $head
    if (!$retained.cached_reader.offline -or $retained.cached_reader.active_item -ne $firstItem) {
        throw 'Restart did not restore the original offline review item.'
    }
    Stop-ThroughUi
    $script:report.detail = 'The real Windows review draft retained its source line, code comparison and original head through verified issue switches, app layout changes and a separate offline restart.'
}

function Enter-FixtureAnchoredReview([IntPtr]$Window, [string]$Head, [string]$FixturePath) {
    Click-AccountControl $Window 'pr-0-diff-split'
    $null = Wait-PrReader 'the exact fixture split diff' {
        param($pr) $pr.head -ceq $Head -and $pr.selected_file -ceq $FixturePath -and $pr.diff_mode -ceq 'Split'
    }
    Click-AccountControl $Window 'reader-maximize-pane'
    Select-FixtureRightReviewLine $Window $FixturePath
    Click-AccountControl $Window 'pr-0-review-add-inline'
    $receipt = Wait-PrReader 'the anchored review opened in the chosen pane' {
        param($pr)
        $null -ne $pr.review_draft -and @($pr.review_draft.anchors).Count -eq 1
    }
    Wait-For 'the owned native inline review body' {
        Set-OwnedFixtureEditValue $Window 'pr-0-review-inline' 'Inline review comment' 'Threadline synthetic inline review note.'
    }
    Wait-For 'the owned native review summary' {
        Set-OwnedFixtureEditValue $Window 'pr-0-review-summary' 'Review summary' 'Threadline synthetic review summary.'
    }
    $id = $receipt.cached_reader.pull_request.review_draft.id
    $null = Wait-ExactFixtureReview $id $Head $FixturePath
    $null = Wait-PrReader 'the original review summary is durable' {
        param($pr) $pr.review_draft.saved -and $pr.review_draft.summary_nonempty
    }
    return $id
}

function Invoke-ReviewDraftProof([IntPtr]$Window, [string]$Profile) {
    $headA = 'b' * 40
    $headB = 'e' * 40
    Wait-SeededFixtureAccount
    Open-SyncSettings $Window
    Click-AccountControl $Window 'sync-pause-background'
    $null = Wait-Sync 'unrelated reads paused while the review Outbox remains enabled' {
        param($sync) $sync.controls.pause_background -and !$sync.controls.pause_queued_actions
    }
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    $first = Open-FixturePr $Window 0 42 'First fixture pull request' $headA 'src/first.rs'
    if ($first.cached_reader.outbox -ne 0) { throw 'The isolated review profile unexpectedly has earlier pending operations.' }
    $draft = Enter-FixtureAnchoredReview $Window $headA 'src/first.rs'
    Set-WriteFixtureOffline $Window $true
    $ready = Restart-Reader $Profile
    $reopened = Wait-ExactFixtureReview $draft $headA
    if (!$reopened.cached_reader.offline) { throw 'The isolated review restarted with network reads enabled.' }
    Click-AccountControl $ready 'pr-0-review-submit'
    $null = Wait-PrReader 'the exact review draft is queued while offline' {
        param($pr, $reader) $reader.offline -and $reader.outbox -eq 1 -and $pr.review_draft.queued
    }
    Set-AccessPhase 'review-send'
    Set-WriteFixtureOffline $ready $false
    Click-AccountControl $ready 'reader-outbox'
    Click-AccountControl $ready 'reader-outbox-filter-history'
    $null = Wait-Reader 'the exact unchanged review has a confirmed native Outbox row' {
        param($reader) $reader.outbox_visible -and $reader.outbox -eq 0
    }
    Inspect-WriteFixtureAction $ready 0 'Confirmed'
    Add-Evidence 'Save and resume an unchanged native review with an anchored comment' ([ordered]@{
        draft = $draft; expected_head = $headA; source = 'src/first.rs:1/right'; confirmed = $true
    })
    Click-AccountControl $ready 'reader-outbox-open'
    $null = Wait-PrReader 'return to the original item from the confirmed review' {
        param($pr) $pr.head -ceq $headA
    }
    Click-AccountControl $ready 'reader-maximize-pane'
    Click-AccountControl $ready 'reader-repository-1'
    $second = Open-FixturePr $ready 0 43 'Second fixture pull request' $headB 'src/second.rs'
    $draftB = Enter-FixtureAnchoredReview $ready $headB 'src/second.rs'
    Click-AccountControl $ready 'pr-0-review-approve'
    $null = Wait-PrReader 'the approval choice is saved for the original second head' {
        param($pr) $pr.review_draft.event -ceq 'Approve' -and $pr.review_draft.saved
    }
    Set-WriteFixtureOffline $ready $true
    Click-AccountControl $ready 'pr-0-review-submit'
    $null = Wait-PrReader 'the approval is explicitly queued and has not been transmitted' {
        param($pr, $reader) $reader.offline -and $reader.outbox -eq 1 -and $pr.review_draft.queued
    }
    Set-AccessPhase 'review-changed'
    Set-WriteFixtureOffline $ready $false
    Click-AccountControl $ready 'reader-outbox'
    Click-AccountControl $ready 'reader-outbox-filter-pending'
    Inspect-WriteFixtureAction $ready 0 'NeedsAttention'
    Click-AccountControl $ready 'reader-outbox-open'
    $kept = Wait-ExactFixtureReview $draftB $headB 'src/second.rs'
    if ($kept.cached_reader.active_item -ne $second.cached_reader.active_item) {
        throw 'The held approval reopened a different item.'
    }
    Add-Evidence 'Changed provider head holds the original approval and keeps its original note' ([ordered]@{
        draft = $draftB; expected_head = $headB; observed_replacement_head = ('9' * 40)
        source = 'src/second.rs:1/right'; held = $true
    })
    Stop-ThroughUi
    $script:report.detail = 'Native review input survived an offline restart and confirmed the unchanged expected head. A second approval was held when provider preflight observed a different head, with original source and draft retained.'
}

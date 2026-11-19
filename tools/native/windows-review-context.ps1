function Select-FixtureRightReviewLine([IntPtr]$Window, [string]$Head, [string]$FixturePath = 'src/first.rs', [int]$Line = 1) {
    if ($FixturePath -cnotin @('src/first.rs', 'src/second.rs')) { throw 'Unknown native fixture comparison.' }
    $null = Wait-PrReader 'the exact loaded comparison before source navigation' {
        param($pr)
        $pr.head -ceq $Head -and $pr.selected_file -ceq $FixturePath -and $pr.diff_mode -ceq 'Split'
    }
    Wait-For 'the owned native Next change action' {
        Invoke-OwnedFixtureControl $Window 'pr-0-next-change' $null 'Next change'
    }
    $null = Wait-PrReader 'the exact modified source line selected by the native action' {
        param($pr)
        $anchor = $pr.source_anchor
        $selected = if ($Line -eq 1) { $pr.selected_source_line -eq 1 } else { $null -ne $pr.selected_source_line }
        $pr.head -ceq $Head -and $selected -and $null -ne $anchor -and
            $anchor.path -ceq $FixturePath -and $anchor.side -ceq 'Right' -and $anchor.line -eq $Line
    }
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
    Select-FixtureRightReviewLine $Window $head
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
    Wait-For 'the owned Duplicate view beside accessibility action' {
        Invoke-OwnedFixtureControl $Window 'reader-duplicate-tab' $null 'Duplicate view beside'
    }
    $null = Wait-Reader 'the independent duplicate pane for the same pull request' {
        param($reader)
        $reader.focused_pane -eq 1 -and $reader.pane_count -eq 2 -and $reader.active_item -eq $firstItem
    }
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
    Select-FixtureRightReviewLine $Window $Head $FixturePath
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
    $heldLabel = 'Review ' + [char]0x00b7 + ' Needs attention'
    Wait-For 'the exact second native review Outbox row requiring attention' {
        Invoke-OwnedFixtureControl $ready 'reader-outbox-action-1' $null $heldLabel
    }
    $null = Wait-Reader 'the held review is selected in the native Outbox' {
        param($reader) $reader.outbox_visible -and $reader.outbox_action_state -ceq 'NeedsAttention'
    }
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

function Wait-FixtureReviewDecision($Fixture, [string]$Draft, [bool]$Queued) {
    return Wait-PrReader 'the exact native review decision and every durable inline anchor' {
        param($pr, $reader)
        $review = $pr.review_draft
        if ($null -eq $review -or $review.id -cne $Draft -or $review.head -cne $Fixture.head -or
            $reader.active_number -ne $Fixture.number -or $review.event -cne $Fixture.event -or
            !$review.saved -or !$review.summary_nonempty -or !$review.owner_tab -or
            ($Queued -and !$review.queued) -or @($review.anchors).Count -ne $Fixture.lines.Count) { return $false }
        for ($index = 0; $index -lt $Fixture.lines.Count; $index++) {
            $anchor = $review.anchors[$index]
            if ($anchor.path -cne $Fixture.path -or $anchor.side -cne 'Right' -or
                $anchor.source_oid -cne $Fixture.head -or $anchor.first_line -ne $Fixture.lines[$index] -or
                $anchor.last_line -ne $Fixture.lines[$index] -or !$anchor.comment_nonempty) { return $false }
        }
        return $true
    }
}

function Invoke-ReviewDecisionsProof([IntPtr]$Window, [string]$Profile, [switch]$Predecessor) {
    Wait-SeededFixtureAccount
    Open-SyncSettings $Window
    Click-AccountControl $Window 'sync-pause-background'
    $null = Wait-Sync 'scheduled reads paused while previously queued reviews can submit' {
        param($sync) $sync.controls.pause_background -and !$sync.controls.pause_queued_actions
    }
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    $fixtures = @(
        @{ index = 0; number = 42; title = 'First fixture pull request'; head = 'b' * 40; path = 'src/first.rs';
           event = 'Approve'; action = 'pr-0-review-approve'; lines = @(1, 3); changed = 4 },
        @{ index = 1; number = 43; title = 'Second fixture pull request'; head = 'e' * 40; path = 'src/second.rs';
           event = 'RequestChanges'; action = 'pr-0-review-request-changes'; lines = @(1); changed = 2 }
    )
    if ($Predecessor) { $fixtures = @($fixtures[0]) }
    foreach ($fixture in $fixtures) {
        $null = Open-FixturePr $Window 0 $fixture.number $fixture.title $fixture.head $fixture.path $fixture.changed
        $draft = Enter-FixtureAnchoredReview $Window $fixture.head $fixture.path
        if ($fixture.lines.Count -eq 2) {
            Select-FixtureRightReviewLine $Window $fixture.head $fixture.path 3
            Click-AccountControl $Window 'pr-0-review-add-inline'
            $null = Wait-PrReader 'the two distinct native source anchors' {
                param($pr) $null -ne $pr.review_draft -and @($pr.review_draft.anchors).Count -eq 2
            }
            Wait-For 'the owned second inline note editor' {
                Set-OwnedFixtureEditValue $Window 'pr-0-review-inline' 'Inline review comment' 'Threadline synthetic second inline review note.'
            }
        }
        Click-AccountControl $Window $fixture.action
        $null = Wait-FixtureReviewDecision $fixture $draft $false
        Set-WriteFixtureOffline $Window $true
        Click-AccountControl $Window 'pr-0-review-submit'
        $queued = Wait-FixtureReviewDecision $fixture $draft $true
        if (!$queued.cached_reader.offline -or $queued.cached_reader.outbox -ne 1) { throw 'The isolated review was not queued offline.' }
        $previous = $script:app.Id
        $Window = Restart-Reader $Profile
        $restored = Wait-FixtureReviewDecision $fixture $draft $false
        if (!$restored.cached_reader.offline -or $restored.cached_reader.outbox -ne 1 -or $script:app.Id -eq $previous) {
            throw 'The isolated review did not survive a separate offline process.'
        }
        Set-WriteFixtureOffline $Window $false
        if ($Predecessor) {
            $marker = Join-Path ([IO.Path]::GetDirectoryName($script:AccessPhaseFile)) 'review-predecessor-durable.txt'
            Wait-For 'the worker reads the durable pending review ID before submitting' {
                if ($script:app.HasExited) { throw 'Threadline exited before reading the pending review.' }
                [IO.File]::Exists($marker) -and [IO.File]::ReadAllText($marker).Trim() -ceq '92'
            } 30000 25
            Set-WriteFixtureOffline $Window $true
            Set-AccessPhase 'review-resume'
            $interrupted = $script:app.Id
            $Window = Restart-Reader $Profile
            $restored = Wait-FixtureReviewDecision $fixture $draft $false
            if (!$restored.cached_reader.offline -or $restored.cached_reader.outbox -ne 1 -or $script:app.Id -eq $interrupted) {
                throw 'The accepted pending review did not survive its interrupted preflight and restart.'
            }
            Add-Evidence 'Restore the incomplete submit after the exact pending-review ID was accepted' ([ordered]@{
                pending_review_id = 92; head = $fixture.head; distinct_process = $true; unfinished = $true
            })
            Set-WriteFixtureOffline $Window $false
        }
        Click-AccountControl $Window 'reader-outbox'
        Click-AccountControl $Window 'reader-outbox-filter-history'
        Wait-For 'the submitted review and predecessor are durably confirmed' {
            if ($script:app.HasExited) { throw 'Threadline exited before review confirmation.' }
            $current = Read-Observation
            if ($null -eq $current -or $current.pid -ne $script:app.Id) { return $false }
            $reader = $current.PSObject.Properties['cached_reader']
            $null -ne $reader -and $reader.Value.outbox_visible -and $reader.Value.outbox -eq 0
        } 30000 50
        Inspect-WriteFixtureAction $Window $fixture.index 'Confirmed' 'Review'
        Add-Evidence 'Restore and confirm the original queued native review decision' ([ordered]@{
            number = $fixture.number; head = $fixture.head; event = $fixture.event; source_lines = $fixture.lines
            distinct_process = $true; confirmed = $true
        })
        if ($fixture.index -eq 0 -and !$Predecessor) {
            Click-AccountControl $Window 'reader-outbox-open'
            Click-AccountControl $Window 'reader-maximize-pane'
            Click-AccountControl $Window 'reader-repository-1'
        }
    }
    Stop-ThroughUi
    $script:report.detail = if ($Predecessor) {
        'Windows resumed the exact pending review after an interrupted predecessor read and restart, then confirmed one approval without repeating its creation.'
    } else {
        'Windows restored a two-note approval and an anchored request for changes in separate offline processes. Both original reviews confirmed through the exact reviewer and revision checks.'
    }
}

function Show-FixtureThreadControl([IntPtr]$Window, [long]$Item, [string]$Id, [string]$Name, [int]$Direction, [switch]$Edit) {
    $head = 'b' * 40
    $viewportId = 'pr-0-conversation-records'
    for ($step = 0; $step -le 5; $step++) {
        $observed = Wait-PrReader 'the original PR and its downloaded review thread in Conversation' {
            param($pr, $reader)
            $coverage = $pr.PSObject.Properties['threads_coverage']
            $count = $pr.PSObject.Properties['threads_on_page']
            $reader.active_item -eq $Item -and $pr.head -ceq $head -and $pr.section -ceq 'Conversation' -and
                $null -ne $coverage -and $coverage.Value -ceq 'Complete' -and $null -ne $count -and $count.Value -eq 1
        }
        if ($observed.pid -ne $script:app.Id -or !(Test-OwnedFixtureWindow $Window)) {
            Stop-Blocked 'The isolated Threadline conversation is no longer owned by the expected process.'
        }
        if ($null -ne $observed.bounds.PSObject.Properties[$Id] -and
            $null -ne (Find-OwnedFixtureControl $Window $Id $Name -Edit:$Edit)) { return }
        $viewport = $observed.bounds.PSObject.Properties[$viewportId]
        if ($step -eq 5 -or $null -eq $viewport) { break }
        $rect = $viewport.Value
        if ($rect.Count -ne 4 -or $rect[2] -le 0 -or $rect[3] -le 0) { break }
        $point = To-ClientPoint ($rect[0] + $rect[2] / 2) ($rect[1] + $rect[3] / 2) $observed.scale_factor
        $dpi = [uint32][Math]::Round(96 * $observed.scale_factor)
        [ThreadlineWinInput]::ScrollOwnedConversation($Window, [uint32]$script:app.Id, $dpi, $point, $Direction)
        Add-Evidence 'Reveal a review thread control with a synthetic window message' ([ordered]@{
            automation_id = $Id; direction = $Direction; step = $step + 1; input = 'Owned HWND WM_MOUSEWHEEL'
        })
        Start-Sleep -Milliseconds 100
    }
    throw "The exact visible review thread control remained unavailable after bounded Conversation scrolling ($Id)."
}

function Invoke-ThreadQueueProof([IntPtr]$Window, [string]$Profile) {
    $head = 'b' * 40
    Wait-SeededFixtureAccount
    Open-SyncSettings $Window
    Click-AccountControl $Window 'sync-pause-background'
    $null = Wait-Sync 'the fixture write worker remains enabled while background reads are paused' {
        param($sync) $sync.controls.pause_background -and !$sync.controls.pause_queued_actions
    }
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    $first = Open-FixturePr $Window 0 42 'First fixture pull request' $head 'src/first.rs'
    $item = $first.cached_reader.active_item
    Click-AccountControl $Window 'reader-maximize-pane'
    Click-AccountControl $Window 'pr-0-conversation'
    Click-AccountControl $Window 'pr-0-threads-fetch'
    Show-FixtureThreadControl $Window $item 'pr-0-thread-0-reply' 'Reply' -1
    Set-WriteFixtureOffline $Window $true
    Show-FixtureThreadControl $Window $item 'pr-0-thread-0-reply' 'Reply' -1
    Click-AccountControl $Window 'pr-0-thread-0-reply'
    $reply = 'Threadline synthetic queued thread reply.'
    $null = Wait-Reader 'the original thread reply draft is opened for editing' {
        param($reader) $reader.active_item -eq $item -and $null -ne $reader.active_draft_saved
    }
    Show-FixtureThreadControl $Window $item 'pr-0-thread-0-reply-input' 'Reply to review thread' -1 -Edit
    Wait-For 'the owned review reply editor receives its exact synthetic text' {
        Set-OwnedFixtureEditValue $Window 'pr-0-thread-0-reply-input' 'Reply to review thread' $reply
    }
    Wait-WriteFixtureDraft $item $reply
    Show-FixtureThreadControl $Window $item 'pr-0-thread-0-reply-submit' 'Queue reply' -1
    Click-AccountControl $Window 'pr-0-thread-0-reply-submit'
    $null = Wait-Reader 'the original thread reply is queued offline' {
        param($reader) $reader.offline -and $reader.active_item -eq $item -and $reader.outbox -eq 1
    }
    Show-FixtureThreadControl $Window $item 'pr-0-thread-0-resolution' 'Queue resolution' 1
    Click-AccountControl $Window 'pr-0-thread-0-resolution'
    $null = Wait-Reader 'the native thread resolution is queued behind the reply' {
        param($reader) $reader.offline -and $reader.active_item -eq $item -and $reader.outbox -eq 2
    }
    $previous = $script:app.Id
    $Window = Restart-Reader $Profile
    if ($script:app.Id -eq $previous) { throw 'The isolated PR did not restart in a separate process.' }
    $null = Wait-PrReader 'original offline PR and both durable thread operations after restart' {
        param($pr, $reader) $reader.offline -and $reader.active_item -eq $item -and $reader.outbox -eq 2 -and $pr.head -ceq $head
    }
    Set-WriteFixtureOffline $Window $false
    Click-AccountControl $Window 'reader-outbox'
    Click-AccountControl $Window 'reader-outbox-filter-history'
    Wait-For 'both queued operations confirmed against the original thread' {
        if ($script:app.HasExited) { throw 'Threadline exited before the thread outcomes.' }
        $snapshot = Read-Observation
        if ($null -eq $snapshot -or $snapshot.pid -ne $script:app.Id) { return $false }
        $reader = $snapshot.PSObject.Properties['cached_reader']
        $null -ne $reader -and $reader.Value.outbox_visible -and $reader.Value.outbox -eq 0
    } 30000 50
    Inspect-WriteFixtureAction $Window 0 'Confirmed' 'Reply'
    Inspect-WriteFixtureAction $Window 1 'Confirmed' 'Resolve thread'
    Add-Evidence 'Restore and confirm the queued reply followed by resolution on the original thread' ([ordered]@{
        number = 42; head = $head; thread = 'PRRT_42'; root = 771; reply = 772; confirmed = 2; distinct_process = $true
    })
    Click-AccountControl $Window 'reader-outbox-open'
    Click-AccountControl $Window 'pr-0-conversation'
    Set-WriteFixtureOffline $Window $true
    Show-FixtureThreadControl $Window $item 'pr-0-thread-0-resolution' 'Queue reopening' -1
    Wait-For 'the confirmed thread provides a native reopening action' {
        Invoke-OwnedFixtureControl $Window 'pr-0-thread-0-resolution' $null 'Queue reopening'
    }
    $null = Wait-Reader 'the reopening is queued for the same offline pull request' {
        param($reader) $reader.offline -and $reader.active_item -eq $item -and $reader.outbox -eq 1
    }
    Set-AccessPhase 'thread-outdated'
    Set-WriteFixtureOffline $Window $false
    Click-AccountControl $Window 'reader-outbox'
    Click-AccountControl $Window 'reader-outbox-filter-attention'
    Inspect-WriteFixtureAction $Window 2 'NeedsAttention' 'Resolve thread'
    Add-Evidence 'Hold reopening when provider preflight finds the original thread outdated' ([ordered]@{
        head = $head; thread = 'PRRT_42'; needs_attention = $true
    })
    Stop-ThroughUi
    $script:report.detail = 'Windows restored two queued operations on the original review thread, confirmed the exact reply before resolution, and held reopening when the thread became outdated.'
}

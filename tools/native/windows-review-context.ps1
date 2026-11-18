function Select-FixtureRightReviewLine([IntPtr]$Window) {
    $state = Read-Observation
    $property = $state.bounds.PSObject.Properties['pr-0-diff']
    if ($null -eq $property -or $property.Value.Count -ne 4) {
        Stop-Blocked 'The visible fixture comparison has no safe native bounds.'
    }
    $bounds = $property.Value
    if ($bounds[2] -lt 180 -or $bounds[3] -lt 115) {
        Stop-Blocked 'Resize the isolated fixture window so the split comparison is visible.'
    }
    Set-WindowFocus $Window
    $originX = $bounds[0] + $bounds[2] * 0.75
    # The isolated fixture comparison contains one change on line one. Probe
    # only its visible right-side code region, then accept the reported source.
    foreach ($offset in @(20, 33, 46, 59, 72, 85, 98)) {
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
            if ($null -ne $anchor -and $anchor.path -ceq 'src/first.rs' -and
                $anchor.side -ceq 'Right' -and $anchor.line -eq 1) { return }
            Start-Sleep -Milliseconds 75
        }
    }
    Stop-Blocked 'Could not select the guarded native source line in the fixture split comparison.'
}

function Wait-ExactFixtureReview([string]$DraftId, [string]$Head) {
    Wait-PrReader 'the same saved native anchored review draft' {
        param($pr, $reader)
        $draft = $pr.review_draft
        $null -ne $draft -and $draft.id -ceq $DraftId -and $draft.head -ceq $Head -and
            $draft.owner_tab -and $draft.saved -and @($draft.anchors).Count -eq 1 -and
            $draft.anchors[0].path -ceq 'src/first.rs' -and $draft.anchors[0].side -ceq 'Right' -and
            $draft.anchors[0].first_line -eq 1 -and $draft.anchors[0].last_line -eq 1 -and
            $draft.anchors[0].source_oid -ceq $Head -and $draft.anchors[0].comment_nonempty -and
            $pr.selected_file -ceq 'src/first.rs' -and $pr.diff_mode -ceq 'Split'
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

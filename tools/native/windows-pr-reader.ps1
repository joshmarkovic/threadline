function Wait-PrReader([string]$Description, [scriptblock]$PrReaderCheck) {
    return Wait-Reader $Description {
        param($reader)
        $property = $reader.PSObject.Properties['pull_request']
        $null -ne $property -and $null -ne $property.Value -and (& $PrReaderCheck $property.Value $reader)
    }
}

function Click-PrContext([IntPtr]$Window, [int]$Pane) {
    $state = Read-Observation
    if ($null -ne $state.bounds.PSObject.Properties["pr-$Pane-show-context"]) {
        Click-AccountControl $Window "pr-$Pane-show-context"
    }
}

function Find-FixturePrChoice([object[]]$Choices, [string]$ExpectedName) {
    $candidates = @($Choices | Where-Object { $_.Name -ceq $ExpectedName })
    if ($candidates.Count -eq 1) { return $candidates[0].Id }
    if ($candidates.Count -gt 1) { Stop-Blocked 'The exact fixture pull request has ambiguous native controls.' }
    return $null
}

function Activate-FixturePr([IntPtr]$Window, [string]$Prefix, [int]$Number, [string]$Title, [switch]$Result) {
    $separator = if ($Result) { ' ' + [char]0x00b7 + ' ' } else { ' ' }
    $expectedName = "Pull request fixture-org/private #$Number$separator$Title"
    Wait-For "activation of fixture PR #$Number ($Prefix)" {
        $snapshot = Read-Observation
        if ($null -eq $snapshot) { return $null }
        $ids = @($snapshot.bounds.PSObject.Properties.Name | Where-Object {
            $_ -cmatch ('^' + [regex]::Escape($Prefix) + '[0-9]+$')
        })
        $choices = @(
            foreach ($candidate in $ids) {
                $node = Find-OwnedFixtureControl $Window $candidate $expectedName
                if ($null -ne $node) { [pscustomobject]@{ Id = $candidate; Name = $node.Current.Name } }
            }
        )
        $id = Find-FixturePrChoice $choices $expectedName
        if (!$id -or !(Invoke-OwnedFixtureControl $Window $id $null $expectedName)) { return $false }
        Add-Evidence 'Activate the exact native fixture pull request' ([ordered]@{
            automation_id = $id; name = $expectedName; eligible_controls = $choices.Count
        })
        return $true
    }
}

function Open-FixturePr([IntPtr]$Window, [int]$Pane, [int]$Number, [string]$Title, [string]$Head, [string]$Path, [int]$ChangedLines = 2) {
    Activate-FixturePr $Window 'reader-result-' $Number $Title -Result
    $initial = Wait-PrReader 'the chosen pull request tab' {
        param($pr, $reader)
        $reader.focused_pane -eq $Pane -and
            ($null -eq $pr.head -or $pr.head -ceq $Head)
    }
    if ($null -eq $initial.cached_reader.pull_request.head) {
        Click-AccountControl $Window "pr-$Pane-refresh"
    }
    $null = Wait-PrReader 'the original provider revision' { param($pr) $pr.head -ceq $Head }
    Click-AccountControl $Window "pr-$Pane-files"
    $null = Wait-PrReader 'the downloaded comparison and changed file' {
        param($pr) $pr.head -ceq $Head -and $pr.section -ceq 'Files' -and
            $pr.files_coverage -ceq 'Complete' -and $pr.files_on_page -eq 1
    }
    Click-AccountControl $Window "pr-$Pane-file-0"
    return Wait-PrReader 'the exact changed path and parsed patch' {
        param($pr) $pr.selected_file -ceq $Path -and $pr.patch_change_lines -eq $ChangedLines
    }
}

function Confirm-FixturePrMergeLabel([IntPtr]$Window, [string]$Name) {
    Wait-For 'the exact native pull request merge-state label' {
        $null -ne (Find-OwnedFixtureControl $Window 'pr-0-merge-state' $Name -AllowDisabled)
    }
}

function Invoke-PrMergeStateProof([IntPtr]$Window, [string]$Profile) {
    Wait-SeededFixtureAccount
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    $head = 'b' * 40
    $title = 'First fixture pull request'
    $updated = 'First fixture pull request after issue refresh'
    $other = 'Second fixture pull request'
    $dot = ' ' + [char]0x00b7 + ' '
    $mergedLabel = 'Merged' + $dot + 'Ready for review' + $dot + 'fixture-author' + $dot + 'topic ' + [char]0x2192 + ' main'
    $unknownLabel = 'Closed' + $dot + 'merge state unknown'

    Activate-FixturePr $Window 'reader-result-' 42 $title -Result
    Click-AccountControl $Window 'pr-0-refresh'
    $initial = Wait-PrReader 'the verified merged pull request and its two provider identities' {
        param($pr, $reader)
        $reader.active_number -eq 42 -and $pr.closed -eq $true -and $pr.merged -eq $true -and
            $pr.issue_rest_id -eq 442 -and $pr.pull_request_rest_id -eq 762 -and $pr.head -ceq $head
    }
    $first = $initial.cached_reader.active_item
    $revision = $initial.cached_reader.pull_request.revision_id
    Confirm-FixturePrMergeLabel $Window $mergedLabel
    Add-Evidence 'Read the authoritative merged pull request identity' ([ordered]@{ item = $first; number = 42; issue_id = 442; pull_request_id = 762; revision = $revision; label = $mergedLabel })

    Click-AccountControl $Window 'reader-browse'
    Click-AccountControl $Window 'reader-fetch-repository'
    Activate-FixturePr $Window 'reader-result-' 42 $updated -Result
    $null = Wait-PrReader 'the newer issue-shaped response retained the original confirmed merge' {
        param($pr, $reader)
        $reader.active_item -eq $first -and $pr.closed -eq $true -and $pr.merged -eq $true -and
            $pr.issue_rest_id -eq 442 -and $pr.pull_request_rest_id -eq 762 -and $pr.revision_id -eq $revision
    }
    Confirm-FixturePrMergeLabel $Window $mergedLabel
    Add-Evidence 'Consume the newer issue-shaped closed response and keep the merged label' ([ordered]@{ item = $first; updated_title_seen = $true; label = $mergedLabel })

    Set-WriteFixtureOffline $Window $true
    Click-AccountControl $Window 'reader-browse'
    Activate-FixturePr $Window 'reader-result-' 43 $other -Result
    $undownloaded = Wait-PrReader 'the closed second pull request has no downloaded merge facts' {
        param($pr, $reader)
        $reader.active_number -eq 43 -and $reader.offline -and $pr.closed -eq $true -and $null -eq $pr.merged -and
            $pr.issue_rest_id -eq 443 -and $null -eq $pr.pull_request_rest_id -and $null -eq $pr.head
    }
    $second = $undownloaded.cached_reader.active_item
    if (!$first -or !$second -or $first -eq $second) { throw 'The two pull request identities were conflated.' }
    Confirm-FixturePrMergeLabel $Window $unknownLabel
    Add-Evidence 'Display the closed pull request without inventing downloaded merge facts' ([ordered]@{ item = $second; number = 43; issue_id = 443; pull_request_id = $null; label = $unknownLabel })

    $restart = Restart-Reader $Profile
    $null = Wait-PrReader 'the closed PR reloads the same missing merge state from SQLite' {
        param($pr, $reader)
        $reader.active_item -eq $second -and $reader.offline -and $pr.closed -eq $true -and
            $pr.issue_rest_id -eq 443 -and $null -eq $pr.pull_request_rest_id -and $null -eq $pr.merged
    }
    Confirm-FixturePrMergeLabel $restart $unknownLabel
    Activate-FixturePr $restart 'reader-result-' 42 $updated -Result
    $null = Wait-PrReader 'the confirmed merge and separate provider identities reload from SQLite' {
        param($pr, $reader)
        $reader.active_item -eq $first -and $reader.offline -and $reader.tab_counts[0] -eq 2 -and
            $pr.closed -eq $true -and $pr.merged -eq $true -and
            $pr.issue_rest_id -eq 442 -and $pr.pull_request_rest_id -eq 762 -and $pr.revision_id -eq $revision
    }
    Confirm-FixturePrMergeLabel $restart $mergedLabel
    Add-Evidence 'Verify both exact merge-state labels and distinct stored identities after offline restart' ([ordered]@{ merged_item = $first; unknown_item = $second; merged_revision = $revision })
    Stop-ThroughUi
    $script:report.detail = 'The Windows reader kept an authoritative merged pull request and its identities after a newer issue-shaped closed response. A separate closed pull request retained unknown merge status through a SQLite restart. The fixture recorded no mutation or request for the unknown PR details.'
}

function Invoke-PrReaderProof([IntPtr]$Window) {
    Set-AccessPhase 'pr-load'
    Wait-SeededFixtureAccount
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'

    $headA = 'b' * 40
    $headB = 'e' * 40
    $fileA = 'src/first.rs'
    $fileB = 'src/second.rs'
    $titleA = 'First fixture pull request'
    $titleB = 'Second fixture pull request'
    $first = Open-FixturePr $Window 0 42 $titleA $headA $fileA
    $firstId = $first.cached_reader.active_item
    Click-AccountControl $Window 'pr-0-diff-split'
    $null = Wait-PrReader 'the split diff for the first pull request' {
        param($pr) $pr.head -ceq $headA -and $pr.diff_mode -ceq 'Split'
    }

    $second = Open-FixturePr $Window 0 43 $titleB $headB $fileB
    $secondId = $second.cached_reader.active_item
    if (!$firstId -or !$secondId -or $firstId -eq $secondId -or $second.cached_reader.tab_counts[0] -lt 2) {
        throw 'The native tabs did not identify two independent pull requests.'
    }
    Add-Evidence 'Load independent provider revisions and native diffs' ([ordered]@{
        first_item = $firstId
        second_item = $secondId
        first_head = $headA
        second_head = $headB
        first_file = $fileA
        second_file = $fileB
        pane_tab_count = $second.cached_reader.tab_counts[0]
    })
    Activate-FixturePr $Window 'reader-tab-0-' 42 $titleA
    $null = Wait-PrReader 'the first pull request with its selected file and mode' {
        param($pr, $reader)
        $reader.active_item -eq $firstId -and $pr.head -ceq $headA -and
            $pr.selected_file -ceq $fileA -and $pr.diff_mode -ceq 'Split'
    }
    Click-PrContext $Window 0
    Click-AccountControl $Window 'pr-0-context-issue'
    Set-AccessPhase 'pr-delay'
    Click-AccountControl $Window 'pr-0-verify-0'
    $pendingPath = Join-Path ([IO.Path]::GetDirectoryName($script:AccessPhaseFile)) 'pr-issue-pending.txt'
    $null = Wait-For 'the exact linked issue request now held by the fixture' {
        if ([IO.File]::Exists($pendingPath)) { return $true }
        return $null
    }
    try {
        Activate-FixturePr $Window 'reader-tab-0-' 43 $titleB
        $null = Wait-PrReader 'the second pull request while the first linked issue is still pending' {
            param($pr, $reader)
            $reader.active_item -eq $secondId -and $reader.pending_pr_reads -gt 0 -and
                $pr.head -ceq $headB -and $pr.selected_file -ceq $fileB -and
                $pr.context_mode -ceq 'Body' -and $null -eq $pr.linked_issue
        }
    } finally {
        Set-AccessPhase 'pr-release'
    }
    $null = Wait-PrReader 'the second pull request after processing the delayed first reply' {
        param($pr, $reader)
        $reader.active_item -eq $secondId -and $reader.pending_pr_reads -eq 0 -and
            $pr.head -ceq $headB -and $pr.selected_file -ceq $fileB -and
            $pr.context_mode -ceq 'Body' -and $null -eq $pr.linked_issue
    }
    Add-Evidence 'Reject the previous tab context while processing its delayed provider reply' ([ordered]@{
        foreground_item = $secondId
        foreground_head = $headB
        foreground_file = $fileB
        previous_issue_displayed = $false
    })

    Activate-FixturePr $Window 'reader-tab-0-' 42 $titleA
    Click-PrContext $Window 0
    $null = Wait-PrReader 'the first pull request and its verified linked target' {
        param($pr, $reader)
        $reader.active_item -eq $firstId -and $pr.head -ceq $headA -and $pr.verified_targets -eq 1
    }
    Click-AccountControl $Window 'pr-0-linked-0'
    $linked = Wait-PrReader 'the verified linked issue in the first pull request only' {
        param($pr, $reader)
        $reader.active_item -eq $firstId -and $pr.context_mode -ceq 'Issue' -and $null -ne $pr.linked_issue
    }
    Click-AccountControl $Window 'pr-0-context-body'
    $null = Wait-PrReader 'restore the original body, split diff and file on the first pull request' {
        param($pr, $reader)
        $reader.active_item -eq $firstId -and $pr.head -ceq $headA -and
            $pr.context_mode -ceq 'Body' -and $pr.selected_file -ceq $fileA -and $pr.diff_mode -ceq 'Split'
    }
    Add-Evidence 'Switch linked issue back to the original body and retain code state' ([ordered]@{
        item = $firstId
        linked_issue = $linked.cached_reader.pull_request.linked_issue
        file = $fileA
        diff_mode = 'Split'
    })
    Stop-ThroughUi
    $script:report.detail = 'Native tabs preserved their own provider revision and code state while a verified linked issue response was delayed across the tab switch.'
}

function Wait-SharedPrPanes([string]$Description, [long]$Item, [scriptblock]$CheckPanes) {
    return Wait-Reader $Description {
        param($reader)
        $property = $reader.PSObject.Properties['shared_pr_panes']
        if ($null -eq $property -or @($property.Value).Count -ne 2 -or $reader.cached_visible_panes -ne 2) { return $false }
        $left, $right = $property.Value
        $left.pane -eq 0 -and $right.pane -eq 1 -and $left.item -eq $Item -and $right.item -eq $Item -and
            $left.tab -and $right.tab -and $left.tab -cne $right.tab -and
            $left.view.revision_id -gt 0 -and $left.view.revision_id -eq $right.view.revision_id -and
            $left.view.head -ceq ('b' * 40) -and $right.view.head -ceq ('b' * 40) -and
            (& $CheckPanes $left $right $reader)
    }
}

function Wait-SharedPrViewport([long]$ExpectedItem, [bool]$FilesContext) {
    $pane = if ($FilesContext) { 1 } else { 0 }
    $id = if ($FilesContext) { 'pr-1-context-content' } else { 'pr-0-conversation-records' }
    $section = if ($FilesContext) { 'Files' } else { 'Conversation' }
    $field = if ($FilesContext) { 'files_context_offset' } else { 'conversation_offset' }
    $observation = Wait-State ("the precise visible $id viewport for the existing PR pane") {
        param($state)
        $reader = $state.PSObject.Properties['cached_reader']
        if ($null -eq $reader -or $null -eq $reader.Value) { return $false }
        $positions = $reader.Value.PSObject.Properties['shared_pr_panes']
        $bounds = $state.bounds.PSObject.Properties[$id]
        if ($null -eq $positions -or @($positions.Value).Count -ne 2 -or $null -eq $bounds -or
            $bounds.Value[2] -le 0 -or $bounds.Value[3] -le 0) { return $false }
        $target = $positions.Value[$pane]
        $target.item -eq $ExpectedItem -and $target.view.section -ceq $section -and
            $null -ne $target.PSObject.Properties[$field].Value
    }
    [pscustomobject]@{ observation = $observation; id = $id; pane = $pane; section = $section; field = $field }
}

function Move-SharedPrReadingPosition([IntPtr]$Window, [long]$Item, [bool]$FilesContext) {
    $viewport = Wait-SharedPrViewport $Item $FilesContext
    $surface = $viewport.observation
    $id = $viewport.id
    $pane = $viewport.pane
    $section = $viewport.section
    $field = $viewport.field
    $target = $surface.cached_reader.shared_pr_panes[$pane]
    $before = $target.PSObject.Properties[$field].Value
    $rect = $surface.bounds.PSObject.Properties[$id].Value
    $point = To-ClientPoint ($rect[0] + $rect[2] / 2) ($rect[1] + $rect[3] / 2) $surface.scale_factor
    [ThreadlineWinInput]::ScrollOwnedConversation($Window, [uint32]$script:app.Id,
        [uint32][Math]::Round(96 * $surface.scale_factor), $point, -1)
    $receipt = Wait-SharedPrPanes 'the same native viewport records a nonzero reading position' $Item {
        param($left, $right)
        $view = if ($FilesContext) { $right } else { $left }
        $view.view.section -ceq $section -and $view.PSObject.Properties[$field].Value -gt $before
    }
    $after = $receipt.cached_reader.shared_pr_panes[$pane].PSObject.Properties[$field].Value
    Add-Evidence 'Move the reading position of one exact owned PR viewport' ([ordered]@{
        automation_id = $id; input = 'Owned HWND WM_MOUSEWHEEL'; before = $before; after = $after
    })
    return $after
}

function Invoke-SharedPrPanesProof([IntPtr]$Window) {
    Set-AccessPhase 'pr-shared-ready'
    Wait-SeededFixtureAccount
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    $head = 'b' * 40
    $path = 'src/first.rs'
    $first = Open-FixturePr $Window 0 42 'First fixture pull request' $head $path
    $item = $first.cached_reader.active_item
    $draft = Enter-FixtureAnchoredReview $Window $head $path
    Click-AccountControl $Window 'pr-0-review-toggle-editor'
    $null = Wait-PrReader 'the exact saved draft with its editor collapsed' {
        param($pr) $pr.review_draft.id -ceq $draft -and $pr.review_draft.saved -and $pr.review_draft.collapsed
    }
    Wait-For 'open the same downloaded pull request beside its original view' {
        Invoke-OwnedFixtureControl $Window 'reader-duplicate-tab' $null 'Duplicate view beside'
    }
    $null = Wait-Reader 'the duplicate added a second pane to the layout' {
        param($reader) $reader.pane_count -eq 2 -and ($reader.tab_counts -join ',') -ceq '1,1'
    }
    $null = Wait-PrReader 'the maximized duplicate is focused with the original review in its other pane' {
        param($pr, $reader)
        $reader.focused_pane -eq 1 -and $reader.active_item -eq $item -and $pr.head -ceq $head -and
            $null -ne $pr.review_draft -and $pr.review_draft.id -ceq $draft -and !$pr.review_draft.owner_tab
    }
    Click-AccountControl $Window 'pr-1-files'
    Click-AccountControl $Window 'pr-1-file-0'
    Click-AccountControl $Window 'pr-1-diff-split'
    $null = Wait-PrReader 'the right pane has an exact independent split code selection' {
        param($pr, $reader)
        $reader.focused_pane -eq 1 -and $pr.head -ceq $head -and $pr.section -ceq 'Files' -and
            $pr.selected_file -ceq $path -and $pr.diff_mode -ceq 'Split'
    }
    Click-AccountControl $Window 'pr-1-next-change'
    $null = Wait-PrReader 'the right pane anchors its code to the exact modified source' {
        param($pr, $reader)
        $reader.focused_pane -eq 1 -and $null -ne $pr.source_anchor -and
            $pr.source_anchor.path -ceq $path -and $pr.source_anchor.side -ceq 'Right' -and $pr.source_anchor.line -eq 1
    }
    Wait-For 'restore both panes after setting the split view in the maximized duplicate' {
        Invoke-OwnedFixtureControl $Window 'reader-maximize-pane' $null 'Restore panes'
    }
    $null = Wait-SharedPrPanes 'two distinct tabs displaying the same immutable pull request and saved draft' $item {
        param($left, $right)
        $left.view.review_draft.id -ceq $draft -and $left.view.review_draft.owner_tab -and
            $right.view.review_draft.id -ceq $draft -and !$right.view.review_draft.owner_tab -and
            $right.view.section -ceq 'Files' -and $right.view.selected_file -ceq $path -and $right.view.diff_mode -ceq 'Split'
    }
    Click-AccountControl $Window 'pr-0-conversation'
    Click-PrContext $Window 1
    $null = Move-SharedPrReadingPosition $Window $item $true
    $filesContextOffset = Move-SharedPrReadingPosition $Window $item $true
    Click-AccountControl $Window 'pr-1-conversation'
    $baseline = Wait-SharedPrPanes 'the two existing views show conversation before either requests the review family' $item {
        param($left, $right, $reader)
        $left.view.section -ceq 'Conversation' -and $right.view.section -ceq 'Conversation' -and
            $left.provider_draft -eq $false -and $right.provider_draft -eq $false -and
            [int]$left.review_rows -eq 0 -and [int]$right.review_rows -eq 0 -and
            $left.review_coverage -cne 'Complete' -and $right.review_coverage -cne 'Complete' -and $reader.pending_pr_reads -eq 0
    }
    $conversationOffset = Move-SharedPrReadingPosition $Window $item $false
    if ($conversationOffset -eq $filesContextOffset) { throw 'The two reading viewports did not establish distinct positions.' }
    $tabs = @($baseline.cached_reader.shared_pr_panes.tab)
    $revision = $baseline.cached_reader.shared_pr_panes[0].view.revision_id
    Set-AccessPhase 'pr-shared-hold'
    try {
        Click-AccountControl $Window 'pr-0-reviews-fetch'
        $pending = Join-Path ([IO.Path]::GetDirectoryName($script:AccessPhaseFile)) 'pr-shared-review-pending.txt'
        Wait-For 'the authorized revision review response held by the local provider' {
            [IO.File]::Exists($pending)
        }
        Click-AccountControl $Window 'pr-1-reviews-fetch'
        Click-AccountControl $Window 'pr-1-files'
        $held = Wait-SharedPrPanes 'the left Conversation and right Files views while the shared request is held' $item {
            param($left, $right, $reader)
            $left.tab -ceq $tabs[0] -and $right.tab -ceq $tabs[1] -and $left.view.revision_id -eq $revision -and
                $left.view.section -ceq 'Conversation' -and $right.view.section -ceq 'Files' -and
                $right.view.selected_file -ceq $path -and $right.view.diff_mode -ceq 'Split' -and
            $null -ne $right.view.source_anchor -and $right.view.source_anchor.path -ceq $path -and
            $right.view.source_anchor.side -ceq 'Right' -and $right.view.source_anchor.line -eq 1 -and
            $left.conversation_offset -eq $conversationOffset -and $right.files_context_offset -eq $filesContextOffset -and
                [int]$left.review_rows -eq 0 -and $reader.pending_pr_reads -eq 1
        }
    } finally {
        Set-AccessPhase 'pr-shared-release'
    }
    Add-Evidence 'Both native panes stay usable while the two review requests share one held flight' $held.cached_reader.shared_pr_panes
    $after = Wait-SharedPrPanes 'the same two view selections and one partial shared review page after release' $item {
        param($left, $right, $reader)
        $anchor = @($left.view.review_draft.anchors)
        $left.tab -ceq $tabs[0] -and $right.tab -ceq $tabs[1] -and $left.view.revision_id -eq $revision -and
            $left.view.section -ceq 'Conversation' -and $right.view.section -ceq 'Files' -and
            $left.conversation_offset -eq $conversationOffset -and $right.files_context_offset -eq $filesContextOffset -and
            $left.provider_draft -eq $true -and $right.provider_draft -eq $true -and
            $left.review_coverage -ceq 'Partial' -and $left.review_rows -eq 1 -and
            $right.view.files_coverage -ceq 'Complete' -and $right.view.selected_file -ceq $path -and $right.view.diff_mode -ceq 'Split' -and
            $null -ne $right.view.source_anchor -and $right.view.source_anchor.path -ceq $path -and
            $right.view.source_anchor.side -ceq 'Right' -and $right.view.source_anchor.line -eq 1 -and
            $left.view.review_draft.id -ceq $draft -and $right.view.review_draft.id -ceq $draft -and
            $left.view.review_draft.saved -and $left.view.review_draft.owner_tab -and !$right.view.review_draft.owner_tab -and
            $anchor.Count -eq 1 -and $anchor[0].path -ceq $path -and $anchor[0].side -ceq 'Right' -and
            $anchor[0].first_line -eq 1 -and $anchor[0].last_line -eq 1 -and $anchor[0].source_oid -ceq $head -and
            $reader.pending_pr_reads -eq 0 -and $reader.outbox -eq 0
    }
    $null = Wait-AccountControl 'pr-0-conversation-records'
    $null = Wait-AccountControl 'pr-1-context-content'
    Add-Evidence 'Publish one shared refresh while the two visible reading positions and unsent review remain separate' $after.cached_reader.shared_pr_panes
    Stop-ThroughUi
    $script:report.detail = 'Two native tabs displayed Conversation and Files for the same pull request during a held shared review refresh. The same provider revision and draft remained bound, Files kept its split path, and the Conversation showed one partial review page.'
}

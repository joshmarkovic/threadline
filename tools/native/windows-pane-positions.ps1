function Wait-PanePositions([string]$Description, [scriptblock]$PositionCheck) {
    return Wait-State $Description {
        param($state)
        $reader = $state.PSObject.Properties['cached_reader']
        if ($null -eq $reader -or $null -eq $reader.Value) { return $false }
        $position = $reader.Value.PSObject.Properties['pane_positions']
        if ($null -eq $position -or $null -eq $position.Value -or
            @($position.Value.panes).Count -ne 2 -or ($reader.Value.tab_counts -join ',') -cne '10,10' -or
            $null -eq $state.bounds.PSObject.Properties['reader-tab-0-1'] -or
            $null -eq $state.bounds.PSObject.Properties['reader-tab-1-0']) { return $false }
        & $PositionCheck $position.Value.panes[0] $position.Value.panes[1] $position.Value $reader.Value
    }
}

function Move-FixturePaneDiscussion([IntPtr]$Window, [long]$Item) {
    $surface = Wait-Reader 'the exact left archive discussion before moving within it' {
        param($reader) $reader.active_item -eq $Item -and $reader.focused_pane -eq 0 -and $reader.offline
    }
    $bounds = $surface.bounds.PSObject.Properties['reader-conversation-0']
    if ($null -eq $bounds -or $bounds.Value[2] -le 0 -or $bounds.Value[3] -le 0) {
        $surface = Wait-AccountControl 'reader-conversation-0'
        $bounds = $surface.bounds.PSObject.Properties['reader-conversation-0']
        if ($surface.cached_reader.active_item -ne $Item -or $surface.cached_reader.focused_pane -ne 0) {
            throw 'The left archive discussion changed before its functional scroll action.'
        }
    }
    $rect = $bounds.Value
    $point = To-ClientPoint ($rect[0] + $rect[2] / 2) ($rect[1] + $rect[3] / 2) $surface.scale_factor
    [ThreadlineWinInput]::ScrollOwnedConversation($Window, [uint32]$script:app.Id,
        [uint32][Math]::Round(96 * $surface.scale_factor), $point, -1)
}

function Invoke-CrossRepositoryPaneProof([IntPtr]$Window) {
    $seed = $script:PanePositionsSeed
    $archive = $seed.pane_positions
    $null = Wait-Reader 'the isolated two-pane downloaded workspace' {
        param($reader) $reader.offline -and $reader.signed_out -and $reader.cached_visible_panes -eq 2
    }
    Click-AccountControl $Window ('reader-repository-' + $archive.archive)
    Click-AccountControl $Window 'reader-tab-0-1'
    $null = Wait-Reader 'the archive issue is active in the left pane' {
        param($reader) $reader.focused_pane -eq 0 -and $reader.active_item -eq $archive.item
    }
    Wait-For 'enter the exact saved archive comment search' {
        Set-OwnedFixtureEditValue $Window 'reader-search' 'Search downloaded content' 'Orchid reading checkpoint'
    }
    Click-AccountControl $Window 'reader-search-submit'
    $null = Wait-Reader 'local search returns only the original archive issue' {
        param($reader) $reader.search_results -eq 1 -and $reader.first_search_item -eq $archive.item
    }
    Click-AccountControl $Window 'reader-result-0'
    $null = Wait-PanePositions 'the same archive issue is anchored to its saved comment beside the original PR' {
        param($left, $right)
        $left.item -eq $archive.item -and $left.repository -eq $archive.archive -and !$left.pull_request -and
            $left.discussion_anchor -ceq $archive.anchor -and $right.item -eq $seed.review_item -and
            $right.repository -eq $seed.repository_id -and $right.pull_request
    }
    $null = Wait-AccountControl 'pr-1-conversation'

    Click-AccountControl $Window 'reader-maximize-pane'
    Click-AccountControl $Window 'reader-draft-comment-0'
    $draft = 'An unsent archive draft stays in its original pane.'
    Type-WriteFixtureField $Window 'reader-draft-input-0' $draft
    Wait-WriteFixtureDraft $archive.item $draft
    $receipt = Wait-Reader 'the archive draft is durable and unsubmitted' {
        param($reader) $reader.active_item -eq $archive.item -and $reader.active_draft_saved -and !$reader.active_draft_submitted
    }
    $digest = $receipt.cached_reader.active_draft_sha256
    $outbox = $receipt.cached_reader.outbox
    Click-AccountControl $Window 'reader-maximize-pane'
    $settled = Wait-PanePositions 'both native panes and the same disk-backed archive draft' {
        param($left, $right)
        $left.discussion_anchor -ceq $archive.anchor -and $left.saved_draft_sha256 -ceq $digest -and
            $right.item -eq $seed.review_item -and $null -ne $left.discussion_offset
    }
    $initialOffset = $settled.cached_reader.pane_positions.panes[0].discussion_offset
    Move-FixturePaneDiscussion $Window $archive.item
    $before = (Wait-PanePositions 'the native archive reading position changes in its own viewport' {
        param($left, $right)
        $left.discussion_anchor -ceq $archive.anchor -and $left.discussion_offset -gt $initialOffset -and
            $left.saved_draft_sha256 -ceq $digest -and $right.item -eq $seed.review_item
    }).cached_reader.pane_positions
    Add-Evidence 'Move within the visible owned archive discussion to establish its reading position' ([ordered]@{
        input = 'Owned HWND WM_MOUSEWHEEL'; before = $initialOffset; after = $before.panes[0].discussion_offset; anchor = $archive.anchor
    })

    $sameArchive = {
        param($left)
        $left.tab -ceq $before.panes[0].tab -and $left.item -eq $archive.item -and $left.repository -eq $archive.archive -and
            $left.discussion_anchor -ceq $archive.anchor -and $left.discussion_offset -eq $before.panes[0].discussion_offset -and
            $left.saved_draft_sha256 -ceq $digest
    }
    Click-AccountControl $Window 'reader-tab-1-1'
    $switched = Wait-PanePositions 'the other pane switches to a different PR without moving the archive view' {
        param($left, $right, $positions, $reader)
        (& $sameArchive $left) -and $right.item -ne $seed.review_item -and $right.pull_request -and
            $right.repository -eq $seed.repository_id -and $reader.focused_pane -eq 1 -and $reader.outbox -eq $outbox
    }
    Click-AccountControl $Window 'reader-narrow-pane'
    $resized = Wait-PanePositions 'the focused right pane resized while the archive discussion and draft stayed put' {
        param($left, $right, $positions, $reader)
        (& $sameArchive $left) -and $right.tab -ceq $switched.cached_reader.pane_positions.panes[1].tab -and
            $positions.proportions[1] -lt $before.proportions[1] -and $reader.focused_pane -eq 1
    }
    Click-AccountControl $Window 'reader-tab-1-0'
    $restored = Wait-PanePositions 'the original independent PR tab returns without changing the archive pane' {
        param($left, $right, $positions, $reader)
        (& $sameArchive $left) -and $right.tab -ceq $before.panes[1].tab -and $right.item -eq $seed.review_item -and
            $reader.offline -and $reader.outbox -eq $outbox
    }
    Add-Evidence 'Switch and resize the right-hand PR pane while preserving the left-hand archive draft and reading position' ([ordered]@{
        before = $before; resized = $resized.cached_reader.pane_positions; restored = $restored.cached_reader.pane_positions
    })
    Stop-ThroughUi
    $script:report.detail = 'Two visible panes kept different selected repositories. Switching and resizing the PR pane preserved the archive issue, anchored reading position and unsent draft.'
}

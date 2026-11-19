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

function Move-FixturePaneDiscussion([IntPtr]$Window, [long]$Item, [int]$Pane = 0) {
    $surface = Wait-Reader 'the exact active archive discussion before moving within it' {
        param($reader) $reader.active_item -eq $Item -and $reader.focused_pane -eq $Pane -and $reader.offline
    }
    $id = "reader-conversation-$Pane"
    $bounds = $surface.bounds.PSObject.Properties[$id]
    if ($null -eq $bounds -or $bounds.Value[2] -le 0 -or $bounds.Value[3] -le 0) {
        $surface = Wait-AccountControl $id
        $bounds = $surface.bounds.PSObject.Properties[$id]
        if ($surface.cached_reader.active_item -ne $Item -or $surface.cached_reader.focused_pane -ne $Pane) {
            throw 'The active archive discussion changed before its functional scroll action.'
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

function Wait-PaneLayout([string]$Description, [scriptblock]$LayoutCheck) {
    return Wait-State $Description {
        param($snapshot)
        $reader = $snapshot.PSObject.Properties['cached_reader']
        if ($null -eq $reader -or $null -eq $reader.Value -or !$reader.Value.offline -or !$reader.Value.signed_out) {
            return $false
        }
        $position = $reader.Value.PSObject.Properties['pane_positions']
        if ($null -eq $position -or $null -eq $position.Value) { return $false }
        $layout = $position.Value.PSObject.Properties['layout']
        if ($null -eq $layout -or $null -eq $layout.Value -or !$layout.Value.settled) { return $false }
        foreach ($strip in $layout.Value.strips) {
            foreach ($tab in $strip.tabs) {
                if ($tab.title_sha256 -cnotmatch '^[0-9a-f]{64}$') { return $false }
            }
        }
        return (& $LayoutCheck $layout.Value $position.Value.panes $reader.Value $snapshot.bounds)
    }
}

function Test-PaneLayoutOrder($Strip, [string[]]$Ids) {
    return @($Strip.tabs).Count -eq $Ids.Count -and ($Strip.tabs.id -join ',') -ceq ($Ids -join ',')
}

function Invoke-PaneLifecycleProof([IntPtr]$Window, [string]$Profile) {
    $fixture = $script:PanePositionsSeed
    $archive = $fixture.pane_positions
    $start = Wait-PaneLayout 'both exact downloaded tab strips and titles before moving anything' {
        param($layout, $panes, $reader)
        $panes.Count -eq 2 -and ($reader.tab_counts -join ',') -ceq '10,10' -and $panes[1].item -eq $fixture.review_item
    }
    $startLayout = $start.cached_reader.pane_positions.layout
    $leftOrder = @($startLayout.strips[0].tabs.id)
    $rightOrder = @($startLayout.strips[1].tabs.id)
    $archiveId = $leftOrder[1]
    $archiveTitleDigest = $startLayout.strips[0].tabs[1].title_sha256
    $originalOutbox = $start.cached_reader.outbox
    Wait-For 'the exact inactive archive tab title in the native tree' {
        $null -ne (Find-OwnedFixtureControl $Window 'reader-tab-0-1' 'Issue fixture/archive #2 Investigate relay behavior 2')
    }
    Click-AccountControl $Window ('reader-repository-' + $archive.archive)
    Wait-For 'enter the exact retained archive comment search' {
        Set-OwnedFixtureEditValue $Window 'reader-search' 'Search downloaded content' 'Orchid reading checkpoint'
    }
    Click-AccountControl $Window 'reader-search-submit'
    $null = Wait-Reader 'one cached result on the archive item' {
        param($reader) $reader.search_results -eq 1 -and $reader.first_search_item -eq $archive.item
    }
    Click-AccountControl $Window 'reader-tab-0-0'
    $null = Wait-PaneLayout 'the verified left origin for opening the archive result in the other pane' {
        param($layout, $panes, $reader)
        $reader.focused_pane -eq 0 -and $reader.active_item -eq $fixture.discussion_item -and
            $panes.Count -eq 2 -and $panes[0].tab -ceq $leftOrder[0] -and
            $reader.search_results -eq 1 -and $reader.first_search_item -eq $archive.item
    }
    Click-AccountControl $Window 'reader-beside-0'
    $beside = Wait-PaneLayout 'open the archive beside the original tabs without replacing them' {
        param($layout, $panes, $reader)
        ($reader.tab_counts -join ',') -ceq '10,11' -and $reader.focused_pane -eq 1 -and
            $panes.Count -eq 2 -and $panes[1].item -eq $archive.item -and $panes[1].tab -cne $archiveId -and
            (Test-PaneLayoutOrder $layout.strips[0] $leftOrder) -and
            ($layout.strips[1].tabs[0..9].id -join ',') -ceq ($rightOrder -join ',')
    }
    $draftMarker = [char]0x00b7
    $originalPrName = "Pull request fixture/relay #3 Improve relay workflow 3 $draftMarker Draft"
    Wait-For 'the original PR title and draft marker stay visible while its tab is inactive' {
        $null -ne (Find-OwnedFixtureControl $Window 'reader-tab-1-0' $originalPrName)
    }
    Add-Evidence 'Open the archive search result beside the original workspace and retain the inactive PR title' $beside.cached_reader.pane_positions.layout
    Click-AccountControl $Window 'reader-close-tab'
    $null = Wait-PaneLayout 'the original ten-tab right strip is intact after closing the temporary view' {
        param($layout, $panes, $reader)
        ($reader.tab_counts -join ',') -ceq '10,10' -and (Test-PaneLayoutOrder $layout.strips[1] $rightOrder)
    }
    Click-AccountControl $Window 'reader-tab-1-0'
    Click-AccountControl $Window 'reader-tab-0-1'
    $null = Wait-Reader 'the original archive tab is active before following its search anchor' {
        param($reader) $reader.focused_pane -eq 0 -and $reader.active_item -eq $archive.item
    }
    Click-AccountControl $Window 'reader-result-0'
    $null = Wait-PaneLayout 'the original archive view has its exact cached discussion anchor' {
        param($layout, $panes)
        $panes.Count -eq 2 -and $panes[0].tab -ceq $archiveId -and $panes[0].discussion_anchor -ceq $archive.anchor
    }
    Click-AccountControl $Window 'reader-maximize-pane'
    $null = Wait-PaneLayout 'maximize only the archive while retaining both layouts' {
        param($layout, $panes, $reader, $bounds)
        $layout.maximized -and $reader.focused_pane -eq 0 -and $null -eq $bounds.PSObject.Properties['reader-tab-1-0'] -and
            (Test-PaneLayoutOrder $layout.strips[0] $leftOrder) -and (Test-PaneLayoutOrder $layout.strips[1] $rightOrder)
    }
    Click-AccountControl $Window 'reader-draft-comment-0'
    $draft = 'An unsent archive draft stays in its original pane.'
    Type-WriteFixtureField $Window 'reader-draft-input-0' $draft
    Wait-WriteFixtureDraft $archive.item $draft
    $draftObservation = Wait-Reader 'the original archive draft is saved but unsubmitted' {
        param($reader) $reader.active_draft_saved -and !$reader.active_draft_submitted -and $reader.outbox -eq $originalOutbox
    }
    $draftSha = $draftObservation.cached_reader.active_draft_sha256
    Click-AccountControl $Window 'reader-maximize-pane'
    $restored = Wait-PaneLayout 'both original views are visible after restoring the split' {
        param($layout, $panes, $reader, $bounds)
        !$layout.maximized -and $panes.Count -eq 2 -and $panes[0].saved_draft_sha256 -ceq $draftSha -and
            $panes[1].item -eq $fixture.review_item -and $null -ne $bounds.PSObject.Properties['reader-tab-1-0']
    }
    $initialOffset = $restored.cached_reader.pane_positions.panes[0].discussion_offset
    Move-FixturePaneDiscussion $Window $archive.item
    $anchored = Wait-PaneLayout 'the saved archive discussion moves within its own viewport' {
        param($layout, $panes)
        $panes.Count -eq 2 -and $panes[0].discussion_offset -gt $initialOffset -and $panes[0].discussion_anchor -ceq $archive.anchor
    }
    $savedOffset = $anchored.cached_reader.pane_positions.panes[0].discussion_offset
    $archiveViewMatches = {
        param($pane)
        $pane.tab -ceq $archiveId -and $pane.item -eq $archive.item -and $pane.repository -eq $archive.archive -and
            $pane.discussion_anchor -ceq $archive.anchor -and $pane.discussion_offset -eq $savedOffset -and
            $pane.saved_draft_sha256 -ceq $draftSha
    }
    $owner = [uint32]$script:app.Id
    $dpi = [uint32][Math]::Round(96 * $anchored.scale_factor)
    $originalBounds = [ThreadlineWinInput]::SizeOwnedWindow($Window, $owner, $dpi, 0, 0)
    $originalWidth = $originalBounds.Right - $originalBounds.Left
    $originalHeight = $originalBounds.Bottom - $originalBounds.Top
    $compactWidth = [int][Math]::Round(940 * $dpi / 96)
    if ($originalWidth -le $compactWidth) { Stop-Blocked 'The owned Threadline window cannot exercise narrowing from two visible panes.' }
    try {
        $narrowBounds = [ThreadlineWinInput]::SizeOwnedWindow($Window, $owner, $dpi, $compactWidth, $originalHeight)
        $narrow = Wait-PaneLayout 'the actual narrower window hides the other pane without changing either tab strip' {
            param($layout, $panes, $reader, $bounds)
            !$layout.maximized -and $reader.pane_count -eq 2 -and $reader.focused_pane -eq 0 -and
                $panes.Count -eq 2 -and $panes[0].tab -ceq $archiveId -and
                $panes[0].discussion_anchor -ceq $archive.anchor -and $panes[0].saved_draft_sha256 -ceq $draftSha -and
                (Test-PaneLayoutOrder $layout.strips[0] $leftOrder) -and (Test-PaneLayoutOrder $layout.strips[1] $rightOrder) -and
                $null -ne $bounds.PSObject.Properties['reader-tab-0-0'] -and $null -eq $bounds.PSObject.Properties['reader-tab-1-0']
        }
    } finally {
        $normalBounds = [ThreadlineWinInput]::SizeOwnedWindow($Window, $owner, $dpi, $originalWidth, $originalHeight)
    }
    if ($normalBounds.Left -ne $originalBounds.Left -or $normalBounds.Top -ne $originalBounds.Top -or
        $normalBounds.Bottom -ne $originalBounds.Bottom) {
        throw 'Restoring the owned window width changed its original location or height.'
    }
    $null = Wait-PaneLayout 'restore both visible panes with their original selections and the archive reading position' {
        param($layout, $panes, $reader, $bounds)
        !$layout.maximized -and $panes.Count -eq 2 -and (& $archiveViewMatches $panes[0]) -and
            $panes[1].item -eq $fixture.review_item -and $null -ne $bounds.PSObject.Properties['reader-tab-1-0']
    }
    Add-Evidence 'Narrow and restore only the owned application window while preserving the saved pane state' ([ordered]@{
        input = 'Owned HWND SetWindowPos'; dpi = $dpi; physical_width_before = $originalWidth
        physical_width_narrow = $narrowBounds.Right - $narrowBounds.Left
        physical_width_restored = $normalBounds.Right - $normalBounds.Left
        physical_height = $originalHeight; tab_counts = $narrow.cached_reader.tab_counts
    })
    $reordered = @($leftOrder[1], $leftOrder[0]) + @($leftOrder | Select-Object -Skip 2)
    Click-AccountControl $Window 'reader-reorder-tab'
    $null = Wait-PaneLayout 'the original archive tab moves one slot left in the same pane' {
        param($layout, $panes)
        $layout.strips[0].active -eq 0 -and (Test-PaneLayoutOrder $layout.strips[0] $reordered) -and
            (Test-PaneLayoutOrder $layout.strips[1] $rightOrder) -and (& $archiveViewMatches $panes[0])
    }
    $leftAfterMove = @($reordered | Where-Object { $_ -cne $archiveId })
    $rightAfterMove = $rightOrder + @($archiveId)
    Click-AccountControl $Window 'reader-move-tab'
    $moved = Wait-PaneLayout 'move the saved archive view to the right pane with the same title and draft' {
        param($layout, $panes, $reader)
        $reader.focused_pane -eq 1 -and $layout.strips[1].active -eq 10 -and
            (Test-PaneLayoutOrder $layout.strips[0] $leftAfterMove) -and
            (Test-PaneLayoutOrder $layout.strips[1] $rightAfterMove) -and
            $layout.strips[1].tabs[10].title_sha256 -ceq $archiveTitleDigest -and (& $archiveViewMatches $panes[1])
    }
    Add-Evidence 'Restore both panes, reorder the archive tab and move it with the original anchor and draft' $moved.cached_reader.pane_positions

    Move-FixturePaneDiscussion $Window $archive.item 1
    $closing = Wait-PaneLayout 'the moved archive view has a newer reading position immediately before Close tab' {
        param($layout, $panes)
        $panes.Count -eq 2 -and $panes[1].tab -ceq $archiveId -and $panes[1].discussion_anchor -ceq $archive.anchor -and
            $panes[1].discussion_offset -gt $savedOffset
    }
    $savedOffset = $closing.cached_reader.pane_positions.panes[1].discussion_offset
    Click-AccountControl $Window 'reader-close-tab'
    $null = Wait-PaneLayout 'the original archive tab closes without changing the other entries' {
        param($layout) (Test-PaneLayoutOrder $layout.strips[0] $leftAfterMove) -and (Test-PaneLayoutOrder $layout.strips[1] $rightOrder)
    }
    Wait-For 'reopen the most recently closed original tab through its native command' {
        Invoke-OwnedFixtureControl $Window 'reader-reopen-tab' $null 'Reopen tab'
    }
    $null = Wait-PaneLayout 'reopen the same archive tab at its old slot with the matching draft and position' {
        param($layout, $panes, $reader)
        $reader.focused_pane -eq 1 -and $layout.strips[1].active -eq 10 -and
            (Test-PaneLayoutOrder $layout.strips[1] $rightAfterMove) -and (& $archiveViewMatches $panes[1])
    }
    Click-AccountControl $Window 'reader-focus-pane'
    $null = Wait-Reader 'the other pane is focused before closing it' { param($reader) $reader.focused_pane -eq 0 }
    Click-AccountControl $Window 'reader-close-pane'
    $before = Wait-PaneLayout 'close the other pane and retain the archive tab strip and view' {
        param($layout, $panes, $reader)
        $reader.pane_count -eq 1 -and $panes.Count -eq 1 -and $reader.focused_pane -eq 0 -and $layout.strips[0].active -eq 10 -and
            (Test-PaneLayoutOrder $layout.strips[0] $rightAfterMove) -and (& $archiveViewMatches $panes[0])
    }
    $originalPid = $script:app.Id
    $restarted = Restart-Reader $Profile
    $after = Wait-PaneLayout 'reopen the same offline pane with every inactive title and the original unsent draft' {
        param($layout, $panes, $reader)
        $reader.pane_count -eq 1 -and $panes.Count -eq 1 -and $reader.focused_pane -eq 0 -and $layout.strips[0].active -eq 10 -and
            (Test-PaneLayoutOrder $layout.strips[0] $rightAfterMove) -and (& $archiveViewMatches $panes[0]) -and
            ($layout.strips[0].tabs.title_sha256 -join ',') -ceq ($before.cached_reader.pane_positions.layout.strips[0].tabs.title_sha256 -join ',') -and
            $reader.outbox -eq $originalOutbox -and !$reader.active_draft_submitted
    }
    if ($after.pid -eq $originalPid -or $restarted -eq [IntPtr]::Zero) {
        throw 'The saved archive workspace did not start in a distinct native process.'
    }
    Add-Evidence 'Close and reopen the tab, close the other pane and restart offline with the exact saved order, titles and unsent draft' ([ordered]@{
        original_pid = $originalPid; restarted_pid = $after.pid
        before = $before.cached_reader.pane_positions; after = $after.cached_reader.pane_positions
    })
    Stop-ThroughUi
    $script:report.detail = 'Native tab and pane commands preserved the original archive reading location, cached titles, and unsent draft across a separate offline app restart.'
}

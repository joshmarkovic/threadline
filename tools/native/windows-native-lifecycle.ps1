function Read-NativeLifecycle([string]$Profile, [string]$Stage, [string]$Name) {
    if ($script:ReaderProof -cnotin @('NativeLifecycle', 'OfflineMixedOutbox', 'ScopeHold')) {
        throw 'The protected fixture is scoped to the native lifecycle and Outbox cases.'
    }
    $receipt = Join-Path $script:ReportDir $Name
    if (Test-Path -LiteralPath $receipt) { throw 'Refusing to reuse the protected lifecycle receipt.' }
    $child = Start-Threadline $Profile $receipt -Lifecycle $Stage
    try {
        if (!$child.WaitForExit(15000)) {
            $child.Kill()
            [void]$child.WaitForExit(3000)
            throw 'The isolated lifecycle checkpoint timed out.'
        }
        if ($child.ExitCode -ne 0) { throw "The isolated lifecycle checkpoint '$Stage' exited $($child.ExitCode)." }
    } finally {
        $child.Dispose()
    }
    return [IO.File]::ReadAllText($receipt) | ConvertFrom-Json
}

function Initialize-NativeLifecycle([string]$Profile) {
    Set-AccessPhase 'lifecycle-offline'
    if ($script:ReaderProof -ceq 'ScopeHold') {
        $script:lifecycleSeed = Read-NativeLifecycle $Profile 'seed-scope-hold' 'scope-hold-seeded.json'
        if ($lifecycleSeed.fixture_revision -cne 's09-deselected-outbox-v1' -or !$lifecycleSeed.offline -or
            !$lifecycleSeed.outbox_paused -or !$lifecycleSeed.protected_token_present -or
            ($lifecycleSeed.selected -join ',') -cne '72,71' -or
            ($lifecycleSeed.actions.kind -join ',') -cne 'Comment,EditIssue,Review,Merge' -or
            ($lifecycleSeed.actions.state -join ',') -cne 'NeedsAttention,Queued,Queued,Queued' -or
            ($lifecycleSeed.actions.attempts -join ',') -cne '1,0,0,0') {
            throw 'The scope fixture did not seed the exact independent denied and pending actions.'
        }
        Add-Evidence 'Seed independent actions against the original selected repository' $lifecycleSeed
        return
    }
    $mixedOutbox = $script:ReaderProof -ceq 'OfflineMixedOutbox'
    $stage = if ($mixedOutbox) { 'seed-offline-outbox' } else { 'seed' }
    $revision = if ($mixedOutbox) { 's12-offline-mixed-outbox-v1' } else { 's12-native-lifecycle-v2' }
    $script:lifecycleSeed = Read-NativeLifecycle $Profile $stage 'native-lifecycle-seeded.json'
    if ($lifecycleSeed.fixture_revision -cne $revision -or
        !$lifecycleSeed.work_offline -or !$lifecycleSeed.pause_queued_actions -or !$lifecycleSeed.protected_token_present -or
        ($lifecycleSeed.tab_counts -join ',') -cne '10,10' -or ($lifecycleSeed.actions -join ',') -cne 'Queued,Queued,Queued,Queued' -or
        !$lifecycleSeed.review_anchor_preserved -or $lifecycleSeed.prior_delivery -cne 'Delivered' -or $lifecycleSeed.feed_count -ne 1 -or
        ($lifecycleSeed.drafts.kind -join ',') -cne 'IssueEdit,Comment,Review,Reply' -or
        $lifecycleSeed.sync_checkpoint.cursor -cne '2' -or $lifecycleSeed.content.check_coverage -cne 'Missing') {
        throw 'The lifecycle fixture did not seed its exact retained layout, drafts, queue and delivery.'
    }
    Add-Evidence 'Seed mixed private cached work and install one unique protected fixture key' $lifecycleSeed
}

function Test-LifecycleWorkspace($Reader, [long]$Item, [bool]$Offline, [bool]$SignedOut) {
    return $null -ne $Reader -and $Reader.cached_content_ready -and $Reader.cached_visible_panes -eq 2 -and
        $Reader.focused_pane -eq 1 -and $Reader.active_item -eq $Item -and
        ($Reader.tab_counts -join ',') -ceq '10,10' -and $Reader.outbox -eq 4 -and
        $Reader.offline -eq $Offline -and $Reader.signed_out -eq $SignedOut
}

function Test-LifecycleSync($Sync, [bool]$Offline, [bool]$OutboxPaused, [switch]$Reachable) {
    if ($null -eq $Sync) { return $false }
    $saved = $Sync.PSObject.Properties['controls']
    if ($null -eq $saved -or $null -eq $saved.Value) { return $false }
    $controls = $saved.Value
    return $controls.work_offline -eq $Offline -and $controls.pause_background -and
        $controls.pause_queued_actions -eq $OutboxPaused -and
        (!$Reachable -or $Sync.connection -ceq 'Reachable')
}

function Wait-LifecycleWorkspace([bool]$Offline, [bool]$SignedOut) {
    return Wait-Reader 'the established native cached workspace and four retained actions' {
        param($reader)
        Test-LifecycleWorkspace $reader $script:lifecycleSeed.review_item $Offline $SignedOut
    }
}

function Start-LifecycleWindow([string]$Profile, [string]$Observation) {
    if ($null -ne $script:app) {
        if (!$script:app.HasExited) { throw 'The previous owned process is still running.' }
        $script:app.Dispose()
    }
    $script:reportFile = Join-Path $script:ReportDir $Observation
    $script:app = Start-Threadline $Profile $script:reportFile
    $script:report.restarted_pid = $script:app.Id
    return Get-ReadyWindow
}

function Wait-LifecycleMarker([string]$Name) {
    $marker = Join-Path $script:ProfileDir $Name
    Wait-For "the exact held loopback request $Name" {
        (Test-Path -LiteralPath $marker -PathType Leaf) -and [IO.File]::ReadAllText($marker).Trim() -ceq 'held'
    }
}

function Assert-LifecycleCacheUnchanged($Checkpoint) {
    foreach ($field in @('content', 'drafts', 'sync_checkpoint', 'schedule', 'feed_event', 'action_ids')) {
        $initial = $script:lifecycleSeed.PSObject.Properties[$field].Value | ConvertTo-Json -Compress -Depth 8
        $retained = $Checkpoint.PSObject.Properties[$field].Value | ConvertTo-Json -Compress -Depth 8
        if ($initial -cne $retained) { throw "The retained lifecycle fixture changed: $field." }
    }
}

function Wait-LifecycleReview([long]$Item) {
    $content = $script:lifecycleSeed.content
    $draft = $script:lifecycleSeed.drafts[2]
    $null = Wait-PrReader 'the original two-pane review, file, view and unsent draft' {
        param($pr, $reader)
        $reader.active_item -eq $Item -and $pr.head -ceq $content.pr_head -and
            $pr.diff_mode -ceq 'Split' -and $pr.selected_file -ceq $content.file -and
            $null -ne $pr.source_anchor -and $pr.source_anchor.line -eq 3 -and $pr.source_anchor.side -ceq 'Right' -and
            $null -ne $pr.review_draft -and $pr.review_draft.id -ceq $draft.id -and
            $pr.review_draft.saved -and !$pr.review_draft.queued
    }
}

function Wait-LifecycleDraft($Draft, [switch]$Discussion) {
    $null = Wait-Reader 'the same visible, saved and unsubmitted fixture editor' {
        param($reader)
        $reader.offline -and $reader.active_item -eq $Draft.item -and $reader.active_draft_saved -and
            !$reader.active_draft_submitted -and $reader.active_draft_sha256 -ceq $Draft.body_sha256 -and
            (!$Discussion -or ($reader.comments_coverage -ceq 'Complete' -and $reader.comment_count -gt 0))
    }
}

function Set-LifecycleFocusedPane([IntPtr]$Window, [int]$Pane, [bool]$FocusedOnly) {
    $label = if ($FocusedOnly) { 'Maximize pane' } else { 'Restore panes' }
    Wait-For 'the owned cached pane layout action' {
        Invoke-OwnedFixtureControl $Window 'reader-maximize-pane' $null $label
    }
    $nextLabel = if ($FocusedOnly) { 'Restore panes' } else { 'Maximize pane' }
    Wait-For 'the requested visible pane layout' {
        $null -ne (Find-OwnedFixtureControl $Window 'reader-maximize-pane' $nextLabel)
    }
    $null = Wait-Reader 'preserve both cached panes and their selected focus' {
        param($reader)
        $reader.focused_pane -eq $Pane -and $reader.cached_visible_panes -eq 2 -and
            ($reader.tab_counts -join ',') -ceq '10,10'
    }
}

function Invoke-LifecycleCachedSurfaces([IntPtr]$Window) {
    $content = $script:lifecycleSeed.content
    $drafts = $script:lifecycleSeed.drafts
    Wait-LifecycleReview $content.pr
    Set-LifecycleFocusedPane $Window 1 $true
    Click-AccountControl $Window 'pr-1-review-toggle-editor'
    $null = Wait-AccountControl 'pr-1-diff'
    Click-AccountControl $Window 'pr-1-checks'
    $null = Wait-PrReader 'the same pull request with explicit unchecked coverage' {
        param($pr, $reader)
        $reader.offline -and $reader.active_item -eq $content.pr -and
            $pr.head -ceq $content.pr_head -and $pr.section -ceq 'Checks' -and $pr.checks_coverage -ceq 'Missing'
    }
    $null = Wait-AccountControl 'pr-1-checks-coverage'
    Click-AccountControl $Window 'pr-1-files'
    Wait-LifecycleReview $content.pr
    Click-AccountControl $Window 'reader-tab-1-1'
    Wait-LifecycleDraft $drafts[3]
    $null = Wait-PrReader 'the exact downloaded formal review and original thread' {
        param($pr, $reader)
        $reader.offline -and $reader.active_item -eq $content.pr -and $pr.section -ceq 'Conversation' -and
            $pr.head -ceq $content.pr_head -and $pr.reviews_on_page -eq 1 -and $pr.threads_on_page -eq 1
    }
    Show-FixtureThreadControl $Window $content.pr 'pr-1-thread-0-reply-input' 'Reply to review thread' -1 -Edit -Head $content.pr_head -Pane 1
    Set-LifecycleFocusedPane $Window 1 $false

    Click-AccountControl $Window 'reader-tab-0-0'
    Wait-LifecycleDraft $drafts[0] -Discussion
    Set-LifecycleFocusedPane $Window 0 $true
    $null = Wait-AccountControl 'reader-draft-input-0'
    Click-AccountControl $Window 'reader-tab-0-1'
    Wait-LifecycleDraft $drafts[1]
    $null = Wait-AccountControl 'reader-draft-input-0'
    Set-LifecycleFocusedPane $Window 0 $false

    Wait-For 'the owned local search field accepts the exact fixture query' {
        Set-OwnedFixtureEditValue $Window 'reader-search' 'Search downloaded content' 'Investigate relay behavior 1'
    }
    Click-AccountControl $Window 'reader-search-submit'
    $null = Wait-Reader 'local search returns the same downloaded issue while offline' {
        param($reader) $reader.offline -and $reader.search_results -eq 1 -and $reader.first_search_item -eq $content.issue
    }
    Click-AccountControl $Window ('reader-repository-' + $content.repository)
    Click-AccountControl $Window 'reader-history'
    Click-AccountControl $Window 'history-ref-0'
    $null = Wait-History 'the selected branch has both downloaded historical commits' {
        param($history, $reader)
        $reader.offline -and $history.repository -eq $content.repository -and
            $history.commits -eq 2 -and $history.coverage -ceq 'Complete'
    }
    Click-AccountControl $Window 'history-commit-0'
    $null = Wait-History 'the original cached history tip opens in the native graph' {
        param($history) $history.selected -ceq $content.history_head
    }
    Click-AccountControl $Window 'reader-activity'
    Wait-For 'the original delivered activity item opens from the native feed' {
        Invoke-OwnedFixtureControl $Window ('activity-open-' + $script:lifecycleSeed.feed_event) $null 'Open'
    }
    $null = Wait-Reader 'the activity feed retained the original downloaded issue target' {
        param($reader) $reader.offline -and $reader.active_item -eq $content.issue
    }
    Click-AccountControl $Window 'reader-outbox'
    Click-AccountControl $Window 'reader-outbox-filter-pending'
    $null = Wait-Reader 'the visible paused Outbox retains four original pending actions' {
        param($reader) $reader.offline -and $reader.outbox_visible -and $reader.outbox -eq 4
    }
    $null = Wait-AccountControl 'reader-outbox-action-0'
    Click-AccountControl $Window 'reader-browse'
    Click-AccountControl $Window 'reader-tab-0-0'
    Click-AccountControl $Window 'reader-tab-1-0'
    $null = Wait-LifecycleWorkspace $true $false
    Add-Evidence 'Open the original cached issue, review and history with four retained drafts before any network request' ([ordered]@{
        item = $content.issue; pr = $content.pr; head = $content.pr_head; history_head = $content.history_head
        local_search_matches = 1; feed_event = $script:lifecycleSeed.feed_event; draft_kinds = $drafts.kind
        unfetched_checks = 'Missing'; pending_actions = 4
    })
}

function Wait-LifecycleOutboxText([IntPtr]$Window, [string]$AutomationId, [string[]]$Parts) {
    Wait-For ('the visible saved Outbox field ' + $AutomationId) {
        $element = Find-OwnedFixtureControl $Window $AutomationId $null -AllowDisabled
        if ($null -eq $element) { return $false }
        $name = $element.Current.Name
        foreach ($part in $Parts) {
            if (!$name.Contains($part)) { return $false }
        }
        return $true
    }
}

function Assert-OfflineOutboxCacheResult($Before, $After) {
    if ($After.fixture_revision -cne 's12-offline-mixed-outbox-v1' -or $After.fixture_revision -cne $Before.fixture_revision -or
        !$Before.offline -or !$After.offline -or $Before.account -ne $After.account -or
        $After.content_bytes -ge $Before.content_bytes -or
        $After.discussion_records -ge $Before.discussion_records -or $Before.offline_pins -ne $After.offline_pins -or
        ($After.tabs -join ',') -cne ($Before.tabs -join ',') -or
        $After.pending_dependents -ne 3 -or @($After.actions).Count -ne 4 -or
        ($After.drafts | ConvertTo-Json -Compress -Depth 6) -cne ($Before.drafts | ConvertTo-Json -Compress -Depth 6)) {
        throw 'Clearing unpinned content did not preserve the isolated offline workspace and four unsent drafts.'
    }
    for ($index = 0; $index -lt 4; $index++) {
        $original = $Before.actions[$index]
        $retained = $After.actions[$index]
        $state = if ($index -eq 0) { 'Cancelled' } else { 'Queued' }
        if ($retained.id -cne $original.id -or $retained.kind -cne $original.kind -or
            ($retained.target -join ',') -cne ($original.target -join ',') -or $retained.item -ne $original.item -or
            $retained.snapshot_sha256 -cne $original.snapshot_sha256 -or
            ($retained.dependencies -join ',') -cne ($original.dependencies -join ',') -or
            $retained.state -cne $state -or $retained.cancel_requested -ne ($index -eq 0) -or
            $retained.attempts -ne 0) {
            throw 'The original offline action, immutable snapshot, or dependency changed unexpectedly.'
        }
    }
}

function Invoke-OfflineMixedOutboxProof([IntPtr]$Window, [string]$Profile) {
    $initial = Wait-LifecycleWorkspace $true $false
    $fixture = $script:lifecycleSeed
    $before = $fixture.offline_outbox
    if ($before.drafts.Count -ne 4 -or $before.pending_dependents -ne 3 -or
        ($before.actions.kind -join ',') -cne 'Comment,EditIssue,Review,Merge') {
        throw 'This isolated fixture did not seed the mixed drafts and Outbox dependency chain.'
    }
    Click-AccountControl $Window 'reader-tab-0-1'
    $null = Wait-Reader 'the existing second issue tab is active before opening the original Outbox item' {
        param($reader) $reader.focused_pane -eq 0 -and $reader.active_item -eq $before.drafts[1].item -and
            $reader.active_item -ne $before.actions[0].item
    }
    Wait-For 'open native Storage usage while working offline' {
        Invoke-OwnedFixtureControl $Window 'reader-storage' $null 'Storage usage'
    }
    Wait-LifecycleOutboxText $Window 'reader-storage-policy' @(
        'unpinned downloads', 'Offline pins, drafts, saved tabs, and Outbox dependencies are retained.'
    )
    Wait-For 'clear unpinned downloads through its exact native button' {
        Invoke-OwnedFixtureControl $Window 'reader-clear-cache' $null 'Clear unpinned downloads'
    }
    Wait-LifecycleOutboxText $Window 'reader-status' @('Unpinned downloads were cleared.')
    Add-Evidence 'Read the cache policy and clear only unpinned downloads in Work Offline' $null

    Click-AccountControl $Window 'reader-outbox'
    Click-AccountControl $Window 'reader-outbox-filter-pending'
    Inspect-WriteFixtureAction $Window 0 'Queued' 'Comment'
    $parent = $before.actions[0]
    $middle = [char]0x00b7
    $issueTarget = "Comment $middle fixture-org/private #$($parent.target[1])"
    Wait-LifecycleOutboxText $Window 'reader-outbox-target' @($issueTarget)
    Wait-LifecycleOutboxText $Window 'reader-outbox-intent' @('New comment', 'Threadline lifecycle submitted comment.')
    Wait-LifecycleOutboxText $Window 'reader-outbox-dependent-impact' @('3 saved actions need this action to be confirmed')
    Click-AccountControl $Window 'reader-outbox-open'
    $null = Wait-Reader 'the original issue opens from its saved action after downloaded content is cleared' {
        param($reader) $reader.active_item -eq $parent.item -and $reader.offline
    }
    Click-AccountControl $Window 'reader-outbox'
    Inspect-WriteFixtureAction $Window 0 'Queued' 'Comment'
    Click-AccountControl $Window 'reader-outbox-cancel'
    $null = Wait-Reader 'only the prerequisite was cancelled locally' {
        param($reader) $reader.offline -and $reader.outbox_visible -and $reader.outbox -eq 3 -and
            $reader.outbox_action_state -ceq 'Cancelled' -and $reader.outbox_action_cancel_requested
    }
    Click-AccountControl $Window 'reader-outbox-filter-history'
    Wait-For 'inspect the exact cancelled prerequisite in native History' {
        Invoke-OwnedFixtureControl $Window 'reader-outbox-action-0' $null ("Comment $middle Cancelled")
    }
    Wait-LifecycleOutboxText $Window 'reader-outbox-dependent-impact' @('3 saved actions need this action to be confirmed')
    Wait-LifecycleOutboxText $Window 'reader-outbox-intent' @('Threadline lifecycle submitted comment.')
    Add-Evidence 'Inspect the original comment, open its item, cancel the parent and read the retained dependent impact' @($parent.id)

    Click-AccountControl $Window 'reader-outbox-filter-all'
    $children = @(
        @{ label = 'Issue edit'; fields = @('Lifecycle fixture title edit') }
        @{ label = 'Review'; fields = @('Request changes', 'Threadline lifecycle held review.', 'src/relay_00.rs', 'Right line 1', $fixture.content.pr_head, 'Threadline lifecycle held inline note.') }
        @{ label = 'Merge pull request'; fields = @('Squash and merge', 'Target branch main', $fixture.content.pr_head) }
    )
    for ($index = 1; $index -lt 4; $index++) {
        $child = $children[$index - 1]
        $number = $before.actions[$index].target[1]
        Inspect-WriteFixtureAction $Window $index 'Queued' $child.label
        Wait-LifecycleOutboxText $Window 'reader-outbox-target' @("$($child.label) $middle fixture-org/private #$number")
        Wait-LifecycleOutboxText $Window 'reader-outbox-intent' $child.fields
        Wait-LifecycleOutboxText $Window 'reader-outbox-dependencies' @($parent.id)
        Add-Evidence 'Read a saved dependent in the native Outbox after its parent was cancelled' $child.label
    }
    Stop-ThroughUi
    $after = Read-NativeLifecycle $Profile 'offline-outbox' 'offline-outbox-durable.json'
    Assert-OfflineOutboxCacheResult $before $after
    Add-Evidence 'Reopen the closed profile and verify exact snapshots, no attempts, four unsent drafts and less cached content' ([ordered]@{
        initial_pid = $initial.pid; before = $before; after = $after
    })
    $script:report.detail = 'The native offline cache clear retained four unsent drafts and all four inspectable submitted snapshots. The cancelled prerequisite remained in History and each queued child still showed its original payload and dependency.'
}

function Assert-ScopedOutbox($Before, $After, [bool]$Offline) {
    if ($After.fixture_revision -cne $Before.fixture_revision -or
        $After.account -ne $Before.account -or $After.user -ne $Before.user -or
        $After.owner -cne $Before.owner -or !$After.protected_token_present -or
        $After.offline -ne $Offline -or $After.outbox_paused -ne $Offline -or
        ($After.selected -join ',') -cne '72' -or
        ($After.drafts | ConvertTo-Json -Compress -Depth 6) -cne ($Before.drafts | ConvertTo-Json -Compress -Depth 6)) {
        throw 'The original identity, offline controls, alternate selection or draft set changed.'
    }
    if (@($After.actions).Count -ne 4) { throw 'The original four actions were not all retained.' }
    $dispatchHold = if ($Offline) { 'offline' } else { 'repository_unselected' }
    for ($index = 0; $index -lt 4; $index++) {
        $original = $Before.actions[$index]
        $retained = $After.actions[$index]
        foreach ($property in @('id', 'kind', 'item', 'snapshot_sha256', 'state', 'reason', 'attempts')) {
            if ($retained.PSObject.Properties[$property].Value -cne $original.PSObject.Properties[$property].Value) {
                throw "The original action's $property changed after repository removal."
            }
        }
        if (($retained.target -join ',') -cne ($original.target -join ',') -or
            @($retained.dependencies).Count -ne 0 -or $retained.send_starts -ne 0 -or $retained.cancel_requested -or
            $retained.scope_hold -cne 'repository_unselected' -or $retained.dispatch_hold -cne $dispatchHold) {
            throw 'An action left its original target or was not held as expected.'
        }
    }
}

function Invoke-RepositoryScopeHoldProof([IntPtr]$Window, [string]$Profile) {
    $before = $script:lifecycleSeed
    $null = Wait-Account 'the original cached account and both synthetic repositories' {
        param($account) $account.active_user_id -eq 41 -and $account.selected_count -eq 2 -and !$account.busy
    }
    Open-FixtureSettings $Window
    Wait-For 'remove only the native original repository row' {
        Invoke-OwnedFixtureControl $Window 'account-remove-1' $null 'Remove'
    }
    $null = Wait-Account 'the same user with only the other repository selected' {
        param($account) $account.active_user_id -eq 41 -and $account.selected_count -eq 1 -and !$account.busy
    }
    Open-FixtureHome $Window
    Stop-ThroughUi
    $offline = Read-NativeLifecycle $Profile 'scope-hold' 'scope-hold-before-restart.json'
    Assert-ScopedOutbox $before $offline $true
    Add-Evidence 'Remove the original repository in Work Offline and persist all original action snapshots' $offline

    $restarted = Start-LifecycleWindow $Profile 'scope-hold-restart-observation.json'
    Open-SyncSettings $restarted
    $null = Wait-Sync 'restore Work Offline and the Outbox pause after restart' {
        param($sync) Test-LifecycleSync $sync $true $true
    }
    Click-AccountControl $restarted 'sync-pause-outbox'
    $null = Wait-Sync 'release the Outbox pause while still offline' {
        param($sync) Test-LifecycleSync $sync $true $false
    }
    Set-AccessPhase 'scope-hold-online'
    Click-AccountControl $restarted 'sync-work-offline'
    $null = Wait-Sync 'persist online dispatch settings after leaving Work Offline' {
        param($sync) Test-LifecycleSync $sync $false $false
    }
    Click-AccountControl $restarted 'sync-check-now'
    $null = Wait-Sync 'verify authenticated connectivity with dispatch enabled and no eligible scope work' {
        param($sync) (Test-LifecycleSync $sync $false $false -Reachable) -and $sync.eligible_outbox -eq 0
    }
    Open-FixtureSettings $restarted
    Click-AccountControl $restarted 'account-refresh'
    $null = Wait-Account 'verify the still-selected archive on the original account' {
        param($account)
        $account.active_user_id -eq 41 -and $account.selected_count -eq 1 -and !$account.busy -and
            $account.profile_access -ceq 'Readable' -and $account.reachability -ceq 'ReceivedResponse'
    }
    Open-FixtureHome $restarted
    Add-Evidence 'Check the worker explicitly and refresh access to the only selected repository' 'archive'
    Click-AccountControl $restarted 'reader-outbox'
    Click-AccountControl $restarted 'reader-outbox-filter-all'
    $labels = @('Comment', 'Issue edit', 'Review', 'Merge pull request')
    for ($index = 0; $index -lt 4; $index++) {
        $action = $before.actions[$index]
        Inspect-WriteFixtureAction $restarted $index $action.state $labels[$index]
        Wait-LifecycleOutboxText $restarted 'reader-outbox-target' @("fixture-org/private #$($action.target[1])")
        Wait-LifecycleOutboxText $restarted 'reader-outbox-scope-hold' @('A repository required by this action is no longer selected.')
        Add-Evidence 'Read the original target and visible repository hold from the native Outbox' $action.id
    }
    Stop-ThroughUi
    $after = Read-NativeLifecycle $Profile 'scope-hold' 'scope-hold-after-reconnect.json'
    Assert-ScopedOutbox $before $after $false
    Add-Evidence 'Verify exact retained denied and pending actions after reconnect without another submission' $after
    $script:report.detail = 'The original pending issue, review, merge and denied comment survived deselection and offline restart. Explicit checks verified authenticated connectivity and the selected archive while dispatch stayed held.'
}

function Invoke-NativeLifecycleProof([IntPtr]$Window, [string]$Profile) {
    $initial = Wait-LifecycleWorkspace $true $false
    Add-Evidence 'Restore the useful two-pane native workspace with Work Offline and saved credential' ([ordered]@{
        pid = $initial.pid; panes = $initial.cached_reader.cached_visible_panes; outbox = $initial.cached_reader.outbox
    })
    Invoke-LifecycleCachedSurfaces $Window
    Stop-ThroughUi
    Assert-LifecycleCacheUnchanged (Read-NativeLifecycle $Profile 'inspect' 'native-lifecycle-before-offline-restart.json')
    $Window = Start-LifecycleWindow $Profile 'native-lifecycle-offline-restart-observation.json'
    $reopened = Wait-LifecycleWorkspace $true $false
    if ($reopened.pid -eq $initial.pid) { throw 'The offline profile did not reopen in a separate process.' }
    Wait-LifecycleReview $script:lifecycleSeed.content.pr
    Add-Evidence 'Quit normally and restore the same offline workspace and original review in a new process' $reopened.pid
    Open-SyncSettings $Window
    $null = Wait-Sync 'separate offline, read and Outbox holds' {
        param($sync) (Test-LifecycleSync $sync $true $true) -and $null -ne $sync.schedule -and
            $sync.schedule.timezone -ceq 'UTC' -and $sync.schedule.work_interval_secs -eq 180
    }
    Set-AccessPhase 'lifecycle-ready'
    Click-AccountControl $Window 'sync-work-offline'
    $null = Wait-Sync 'online mode and both workload pauses committed' {
        param($sync) Test-LifecycleSync $sync $false $true
    }
    Click-AccountControl $Window 'sync-check-now'
    try {
        $null = Wait-Sync 'reachability using the native protected credential while submissions are paused' {
            param($sync) Test-LifecycleSync $sync $false $true -Reachable
        }
    } catch {
        Stop-ThroughUi
        $diagnostic = Read-NativeLifecycle $Profile 'probe' 'native-lifecycle-probe-diagnostic.json'
        $script:report.probe_diagnostic = $diagnostic.provider_probe
        throw
    }
    Stop-ThroughUi

    $retained = Read-NativeLifecycle $Profile 'inspect' 'native-lifecycle-before-delayed-start.json'
    Assert-LifecycleCacheUnchanged $retained
    if (!$retained.protected_token_present -or $retained.work_offline -or !$retained.pause_queued_actions -or
        $retained.first_attempts -ne 0 -or $retained.feed_count -ne 1) {
        throw 'Normal close did not preserve the protected token, controls and delivered activity.'
    }
    Set-AccessPhase 'lifecycle-delay'
    $online = Start-LifecycleWindow $Profile 'native-lifecycle-online-observation.json'
    Wait-LifecycleMarker 'lifecycle-probe-held'
    $ready = Wait-LifecycleWorkspace $false $false
    Wait-LifecycleReview $script:lifecycleSeed.content.pr
    if (!(Test-Path -LiteralPath (Join-Path $script:ProfileDir 'lifecycle-probe-held') -PathType Leaf)) {
        throw 'The deliberately delayed provider response was released before the restored cache was observed.'
    }
    Add-Evidence 'Observe both restored panes and four actions while the authenticated startup response is held' ([ordered]@{
        pid = $ready.pid; cached_panes = $ready.cached_reader.cached_visible_panes
        queued_actions = $ready.cached_reader.outbox; protected_credential_reentered = $false
    })
    Set-AccessPhase 'lifecycle-ready'
    Open-SyncSettings $online
    $null = Wait-Sync 'the released startup probe reaches the actual loopback server' {
        param($sync) Test-LifecycleSync $sync $false $true -Reachable
    }

    Set-AccessPhase 'lifecycle-crash'
    Click-AccountControl $online 'sync-pause-outbox'
    Wait-LifecycleMarker 'lifecycle-comment-held'
    if (!(Test-OwnedFixtureWindow $online)) {
        Stop-Blocked 'The deliberate interruption lost the verified owned fixture window.'
    }
    $interruptedId = $script:app.Id
    $script:app.Kill()
    if (!$script:app.WaitForExit(4000)) { throw 'The owned fixture did not exit after the deliberate interruption.' }
    Set-AccessPhase 'lifecycle-recovery'
    Add-Evidence 'Interrupt only the owned process after the exact first comment reached the server before its reply' $interruptedId

    $interrupted = Read-NativeLifecycle $Profile 'inspect' 'native-lifecycle-after-crash.json'
    if ($interrupted.actions[0] -cne 'Sending' -or $interrupted.first_send_starts -ne 1 -or
        $null -eq $interrupted.next_recovery_at_ms) {
        throw 'The deliberate process interruption did not leave a real send lease in SQLite.'
    }
    $recovery = Start-LifecycleWindow $Profile 'native-lifecycle-recovery-observation.json'
    $null = Wait-LifecycleWorkspace $false $false
    Open-SyncSettings $recovery
    $null = Wait-Sync 'the replacement process reconnects without repeating the uncertain mutation' {
        param($sync) Test-LifecycleSync $sync $false $false -Reachable
    }
    $recoveryDeadline = [long]$interrupted.next_recovery_at_ms
    if ($recoveryDeadline - [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() -gt 180000) {
        throw 'The interrupted operation has an unexpected recovery lease.'
    }
    Add-Evidence 'Wait for normal dispatch lease expiry in the restarted application' $recoveryDeadline
    Wait-For 'the normal interrupted-send lease deadline' {
        [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds() -ge ($recoveryDeadline + 1500)
    } 182000 200
    Click-AccountControl $recovery 'reader-outbox'
    Click-AccountControl $recovery 'reader-outbox-filter-attention'
    Wait-For 'the original unresolved comment in the process-owned Outbox' {
        $name = 'Comment ' + [char]0x00b7 + ' Outcome unknown'
        Invoke-OwnedFixtureControl $recovery 'reader-outbox-action-0' $null $name
    }
    $null = Wait-Reader 'the reopened original comment shows an unknown native outcome' {
        param($reader) $reader.outbox_visible -and $reader.outbox_action_state -ceq 'OutcomeUnknown'
    }
    Stop-ThroughUi
    $recovered = Read-NativeLifecycle $Profile 'inspect' 'native-lifecycle-recovered.json'
    if (!$recovered.protected_token_present -or $recovered.actions[0] -cne 'OutcomeUnknown' -or
        $recovered.first_send_starts -ne 1 -or $recovered.feed_count -ne 1 -or
        $recovered.dependents -ne 3) {
        throw 'The interrupted mutation was not durably recovered as uncertain with its dependents and prior delivery.'
    }
    Add-Evidence 'Verify the same durable operation, original inline anchor, dependent intents and one prior delivery on restart' $recovered

    $signOut = Start-LifecycleWindow $Profile 'native-lifecycle-signout-observation.json'
    $null = Wait-LifecycleWorkspace $false $false
    Open-FixtureSettings $signOut
    Click-AccountControl $signOut 'account-sign-out'
    $null = Wait-Account 'the original private account signed out with its repository selection preserved' {
        param($account)
        $account.active_local_id -eq $script:lifecycleSeed.account -and
            $account.profile_access -ceq 'SignedOut' -and $account.selected_count -eq 2
    }
    Set-AccessPhase 'lifecycle-signed-out'
    Stop-ThroughUi
    $signedOut = Read-NativeLifecycle $Profile 'inspect' 'native-lifecycle-signed-out.json'
    if ($signedOut.protected_token_present -or $signedOut.access -cne 'SignedOut' -or
        $signedOut.first_send_starts -ne 1 -or $signedOut.actions[0] -cne 'OutcomeUnknown') {
        throw 'Native sign-out did not revoke the fixture entry and preserve uncertain local work.'
    }
    Add-Evidence 'Verify that normal sign-out removed the actual protected entry and retained the same mixed queue' $signedOut

    $isolated = Start-LifecycleWindow $Profile 'native-lifecycle-isolation-observation.json'
    $null = Wait-LifecycleWorkspace $false $true
    Open-FixtureSettings $isolated
    Wait-For 'the exact alternate local account control' {
        Invoke-OwnedFixtureControl $isolated ("account-switch-" + $script:lifecycleSeed.alternate) $null 'fixture-other'
    }
    $null = Wait-Account 'the alternate account has no selected private repositories' {
        param($account) $account.active_local_id -eq $script:lifecycleSeed.alternate -and $account.selected_count -eq 0
    }
    $null = Wait-Reader 'the alternate account does not see the original cache or Outbox' {
        param($reader) $reader.outbox -eq 0 -and $null -eq $reader.active_item
    }
    Wait-For 'switch back to the exact signed-out original account' {
        Invoke-OwnedFixtureControl $isolated ("account-switch-" + $script:lifecycleSeed.account) $null 'fixture-user'
    }
    Open-FixtureHome $isolated
    $null = Wait-LifecycleWorkspace $false $true
    Stop-ThroughUi
    $final = Read-NativeLifecycle $Profile 'inspect' 'native-lifecycle-final.json'
    Assert-LifecycleCacheUnchanged $final
    if ($final.protected_token_present -or $final.active_account -ne $lifecycleSeed.account -or
        $final.alternate_queue -ne 0 -or $final.feed_count -ne 1 -or $final.first_send_starts -ne 1 -or
        $final.delivery_groups -ne 1 -or
        $final.delivery_unchanged_at_ms -ne $lifecycleSeed.delivery_unchanged_at_ms) {
        throw 'Account switching did not preserve the isolated durable signed-out state.'
    }
    Add-Evidence 'Restore the original signed-out workspace after visiting the isolated account' $final

    Set-AccessPhase 'lifecycle-return'
    $repair = Start-LifecycleWindow $Profile 'native-lifecycle-repair-observation.json'
    $null = Wait-LifecycleWorkspace $false $true
    Open-FixtureSettings $repair
    Select-FixtureReplacement $repair $lifecycleSeed.account
    Enter-FixtureAccountForm $repair -LoopbackTokenAction
    Click-AccountControl $repair 'account-connect'
    $null = Wait-Account 'the same owner and GitHub user renew the original protected credential' {
        param($account)
        $account.active_local_id -eq $script:lifecycleSeed.account -and
            $account.active_user_id -eq 41 -and $account.profile_access -ceq 'Readable' -and $account.selected_count -eq 2
    }
    Open-FixtureHome $repair
    Open-SyncSettings $repair
    Click-AccountControl $repair 'sync-check-now'
    $null = Wait-Sync 'the renewed credential serves a new verified online connection' {
        param($sync) Test-LifecycleSync $sync $false $false -Reachable
    }
    Click-AccountControl $repair 'reader-outbox'
    Click-AccountControl $repair 'reader-outbox-filter-attention'
    Wait-For 'the same owned Outbox row remains uncertain after same-user repair' {
        $name = 'Comment ' + [char]0x00b7 + ' Outcome unknown'
        Invoke-OwnedFixtureControl $repair 'reader-outbox-action-0' $null $name
    }
    $null = Wait-Reader 'the uncertain parent and three dependents remain in the original renewed account' {
        param($reader) !$reader.signed_out -and $reader.outbox_visible -and $reader.outbox -eq 4 -and
            $reader.outbox_action_state -ceq 'OutcomeUnknown'
    }
    Stop-ThroughUi
    $renewed = Read-NativeLifecycle $Profile 'inspect' 'native-lifecycle-renewed.json'
    Assert-LifecycleCacheUnchanged $renewed
    if (!$renewed.protected_token_present -or !$renewed.protected_reference_matches -or
        $renewed.credential_generation -le $final.credential_generation -or
        ($renewed.actions -join ',') -cne 'OutcomeUnknown,Queued,Queued,Queued' -or
        ($renewed.dependent_holds -join ',') -cne 'dependency,dependency,dependency' -or
        $renewed.first_attempts -ne 1 -or $renewed.first_send_starts -ne 1 -or
        $renewed.delivery_unchanged_at_ms -ne $lifecycleSeed.delivery_unchanged_at_ms) {
        throw 'Renewal changed the scoped uncertain operation, its dependents or a prior delivery.'
    }
    Add-Evidence 'Renew the same protected identity and preserve uncertainty without replay or releasing dependents' $renewed
    $script:report.detail = 'The mixed workspace restored its offline reader and four unsent draft kinds before the delayed startup response. An interrupted ordinary comment stayed uncertain across restart, sign-out, profile isolation and same-user renewal. The original attempt and its three held dependents remained unchanged, and the prior delivery was not repeated.'
}

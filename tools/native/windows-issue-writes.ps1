function Open-WriteFixtureIssue([IntPtr]$Window, [switch]$UserConnected) {
    if (!$UserConnected) {
        Set-AccessPhase 'write-online'
        Wait-SeededFixtureAccount
    }
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    Click-AccountControl $Window 'reader-result-0'
    $state = Wait-Reader 'the downloaded existing fixture issue' {
        param($reader) $reader.cached_content_ready -and $reader.active_number -eq 41 -and $reader.outbox -eq 0
    }
    Add-Evidence 'Open existing GitHub issue 41 from the downloaded repository' $state.cached_reader.active_number
    return $state.cached_reader.active_item
}

function Set-WriteFixtureOffline([IntPtr]$Window, [bool]$Offline) {
    Click-AccountControl $Window 'account-open'
    $null = Wait-Account 'the open native account controls' { param($account) $account.panel_open }
    Click-AccountControl $Window 'account-offline'
    $null = Wait-Reader 'the saved connectivity choice' {
        param($reader) $reader.offline -eq $Offline
    }
    Click-AccountControl $Window 'account-close'
}

function Type-WriteFixtureField([IntPtr]$Window, [string]$Id, [string]$Value) {
    $name = switch -CaseSensitive ($Id) {
        'reader-issue-title-0' {
            if ($Value -cin @('Cancelled fixture issue title', 'Local change to the same issue')) { 'Issue title' }
        }
        'reader-draft-input-0' {
            if ($Value -cin @(
                'This cancelled issue draft stays in the same profile.',
                'Threadline S07 explicitly queued comment.',
                'Threadline S07 unsent local draft.',
                'Threadline S07 accepted comment with a lost response.',
                'Retain the local issue description across the conflict.',
                'Threadline dependency original comment.',
                'Threadline dependency revised comment.',
                'Threadline dependency comment with a lost response.',
                'Threadline dependency edit that must stay held.'
            )) { 'Local Markdown draft' }
        }
    }
    if (!$name) { throw 'The write journey requested an unknown synthetic field or value.' }
    $null = Wait-AccountControl $Id
    Wait-For "the exact editable fixture control $Id" {
        Set-OwnedFixtureEditValue $Window $Id $name $Value
    }
    Wait-For "confirmation from the exact native fixture editor $Id" {
        $pattern = Get-OwnedFixtureEditPattern $Window $Id $name
        try {
            $null -ne $pattern -and $pattern.Current.Value -ceq $Value
        } catch [Windows.Automation.ElementNotAvailableException] {
            $false
        }
    }
}

function Wait-WriteFixtureDraft([long]$Item, [string]$Value) {
    $hash = [Security.Cryptography.SHA256]::Create()
    try {
        $digest = [BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($Value))).Replace('-', '').ToLowerInvariant()
    } finally { $hash.Dispose() }
    $null = Wait-Reader 'exact synthetic Markdown acknowledged by the SQLite editor' {
        param($reader)
        $reader.active_item -eq $Item -and $reader.active_draft_saved -and $reader.active_draft_sha256 -ceq $digest
    }
}

function Queue-WriteFixtureDraft([IntPtr]$Window, [long]$Item, [int]$Expected, [string]$Action) {
    if ($Action) {
        Wait-For 'the matching enabled native draft submission action' {
            Invoke-OwnedFixtureControl $Window 'reader-submit-0' $null $Action
        }
    } else {
        Click-AccountControl $Window 'reader-submit-0'
    }
    $null = Wait-Reader 'the explicit submission reached the durable Outbox' {
        param($reader)
        $reader.active_item -eq $Item -and $reader.active_draft_submitted -and $reader.outbox -eq $Expected
    }
}

function Inspect-WriteFixtureAction([IntPtr]$Window, [int]$Index, [string]$Expected, [string]$ActionLabel) {
    $id = "reader-outbox-action-$Index"
    if ($ActionLabel) {
        if ($ActionLabel -cnotin @('Comment', 'Issue edit', 'Merge pull request', 'Review', 'Reply', 'Resolve thread')) { throw 'Unknown fixture action label.' }
        $displayState = switch -CaseSensitive ($Expected) {
            'Queued' { 'Queued' }
            'Confirmed' { 'Confirmed' }
            'NeedsAttention' { 'Needs attention' }
            default { throw 'Unknown fixture action state.' }
        }
        $label = $ActionLabel + ' ' + [char]0x00b7 + ' ' + $displayState
        Wait-For 'the exact visible native fixture Outbox row' {
            Invoke-OwnedFixtureControl $Window $id $null $label
        }
    } else {
        Click-AccountControl $Window $id
    }
    $null = Wait-Reader 'the selected action has the expected durable state' {
        param($reader) $reader.outbox_visible -and $reader.outbox_action_state -ceq $Expected
    }
}

function Wait-IssueConflictField([IntPtr]$Window, [string]$Id, [string]$Expected) {
    Wait-For "the exact visible native conflict field $Id" {
        $null -ne (Find-OwnedFixtureControl $Window $Id $Expected -AllowDisabled)
    }
}

function Invoke-IssueConflictProof([IntPtr]$Window, [string]$Profile) {
    Set-AccessPhase 'issue-edit-baseline'
    Wait-SeededFixtureAccount
    $item = Open-WriteFixtureIssue $Window -UserConnected
    Set-WriteFixtureOffline $Window $true
    $title = 'Local change to the same issue'
    $description = 'Retain the local issue description across the conflict.'
    Click-AccountControl $Window 'reader-edit-issue-0'
    Type-WriteFixtureField $Window 'reader-issue-title-0' $title
    Type-WriteFixtureField $Window 'reader-draft-input-0' $description
    Wait-WriteFixtureDraft $item $description
    Queue-WriteFixtureDraft $Window $item 1 'Queue issue edit'
    Add-Evidence 'Queue the exact existing issue title and description while offline' ([ordered]@{ issue_number = 41; queued = 1 })

    $restarted = Restart-Reader $Profile -FixturePhaseAfterQuit 'issue-edit-remote-change'
    Wait-WriteFixtureDraft $item $description
    $null = Wait-Reader 'the submitted edit and original issue survive the offline restart' {
        param($reader)
        $reader.active_item -eq $item -and $reader.active_number -eq 41 -and $reader.offline -and
            $reader.active_draft_submitted -and $reader.outbox -eq 1
    }
    Set-WriteFixtureOffline $restarted $false
    Wait-SeededFixtureAccount
    Click-AccountControl $restarted 'reader-outbox'
    Inspect-WriteFixtureAction $restarted 0 'NeedsAttention' 'Issue edit'
    $dot = ' ' + [char]0x00b7 + ' '
    Wait-IssueConflictField $restarted 'reader-conflict-original-title' ('Original title' + $dot + 'Existing issue for offline edits')
    Wait-IssueConflictField $restarted 'reader-conflict-remote-title' ('Current GitHub title' + $dot + 'Remote change to the same issue')
    Wait-IssueConflictField $restarted 'reader-conflict-original-description' ('Original description' + $dot + 'Synthetic existing issue.')
    Wait-IssueConflictField $restarted 'reader-conflict-remote-description' ('Current GitHub description' + $dot + 'The original issue was changed remotely.')
    $intent = ('Title' + $dot + $title) + '. ' + ('Description' + $dot + $description)
    Wait-IssueConflictField $restarted 'reader-outbox-intent' $intent
    Add-Evidence 'Read the original, local and remote values in the same pending native action' ([ordered]@{
        issue_number = 41; exact_original_local_remote_values = $true; operation_state = 'NeedsAttention'
    })

    Click-AccountControl $restarted 'reader-outbox-open'
    $null = Wait-Reader 'the exact issue and conflict draft remain in the original reader' {
        param($reader)
        $reader.active_item -eq $item -and $reader.active_number -eq 41 -and !$reader.outbox_visible -and
            $reader.active_draft_submitted -and $reader.outbox -eq 1
    }
    Wait-WriteFixtureDraft $item $description
    Wait-For 'the original native issue editor retains the local title' {
        $pattern = Get-OwnedFixtureEditPattern $restarted 'reader-issue-title-0' 'Issue title'
        try {
            $null -ne $pattern -and $pattern.Current.Value -ceq $title
        } catch [Windows.Automation.ElementNotAvailableException] {
            $false
        }
    }
    Add-Evidence 'Reopen the exact existing issue and verify the saved local edit' ([ordered]@{ issue_number = 41; draft_retained = $true })
    Stop-ThroughUi
    $script:report.detail = 'The native reader restored the queued edit after restart, detected changed GitHub fields during preflight, showed its original, local and remote values, and kept the original issue draft available.'
}

function Invoke-IssueWriteProof([IntPtr]$Window, [string]$Profile) {
    $item = Open-WriteFixtureIssue $Window
    Set-WriteFixtureOffline $Window $true
    Click-AccountControl $Window 'reader-edit-issue-0'
    Type-WriteFixtureField $Window 'reader-issue-title-0' 'Cancelled fixture issue title'
    $issueDraft = 'This cancelled issue draft stays in the same profile.'
    Type-WriteFixtureField $Window 'reader-draft-input-0' $issueDraft
    Wait-WriteFixtureDraft $item $issueDraft
    Queue-WriteFixtureDraft $Window $item 1

    Click-AccountControl $Window 'reader-draft-comment-0'
    $submitted = 'Threadline S07 explicitly queued comment.'
    Type-WriteFixtureField $Window 'reader-draft-input-0' $submitted
    Wait-WriteFixtureDraft $item $submitted
    Queue-WriteFixtureDraft $Window $item 2

    # Editing again saves an unsent draft revision. The earlier submission remains immutable.
    $unsent = 'Threadline S07 unsent local draft.'
    Type-WriteFixtureField $Window 'reader-draft-input-0' $unsent
    Wait-WriteFixtureDraft $item $unsent
    $null = Wait-Reader 'the next draft revision is separate from both pending submissions' {
        param($reader) $reader.offline -and !$reader.active_draft_submitted -and $reader.outbox -eq 2
    }
    Add-Evidence 'Save two explicit actions and a separate unsent draft while offline' ([ordered]@{
        issue_number = 41
        queued_actions = 2
        later_draft_submitted = $false
    })

    $restarted = Restart-Reader $Profile
    Wait-WriteFixtureDraft $item $unsent
    $null = Wait-Reader 'the original issue and two pending actions survive offline restart' {
        param($reader) $reader.offline -and $reader.active_item -eq $item -and $reader.outbox -eq 2
    }
    Click-AccountControl $restarted 'reader-outbox'
    Inspect-WriteFixtureAction $restarted 0 'Queued'
    Click-AccountControl $restarted 'reader-outbox-cancel'
    $null = Wait-Reader 'the unsent issue edit was cancelled before reconnection' {
        param($reader) $reader.outbox_action_state -ceq 'Cancelled' -and $reader.outbox -eq 1 -and $reader.offline
    }
    Add-Evidence 'Cancel the original issue edit while it is still offline' 'Cancelled'

    Set-WriteFixtureOffline $restarted $false
    Wait-SeededFixtureAccount
    $null = Wait-Reader 'the eligible comment completes without a second Submit' {
        param($reader) !$reader.offline -and $reader.outbox -eq 0
    }
    Click-AccountControl $restarted 'reader-outbox'
    Click-AccountControl $restarted 'reader-outbox-filter-history'
    Inspect-WriteFixtureAction $restarted 1 'Confirmed'
    Add-Evidence 'Reconcile the original queued ordinary comment after reconnect' 'Confirmed'

    Click-AccountControl $restarted 'reader-outbox-open'
    $null = Wait-Reader 'the original issue is visible in the reader' {
        param($reader) $reader.active_item -eq $item -and !$reader.outbox_visible
    }
    Set-AccessPhase 'write-drop'
    Click-AccountControl $restarted 'reader-draft-comment-0'
    $ambiguous = 'Threadline S07 accepted comment with a lost response.'
    Type-WriteFixtureField $restarted 'reader-draft-input-0' $ambiguous
    Wait-WriteFixtureDraft $item $ambiguous
    Queue-WriteFixtureDraft $restarted $item 1
    Click-AccountControl $restarted 'reader-outbox'
    Click-AccountControl $restarted 'reader-outbox-filter-pending'
    Inspect-WriteFixtureAction $restarted 2 'OutcomeUnknown'
    Click-AccountControl $restarted 'reader-outbox-cancel'
    $null = Wait-Reader 'cancellation preserves the uncertain outcome and stops further attempts' {
        param($reader)
        $reader.outbox_action_state -ceq 'OutcomeUnknown' -and $reader.outbox_action_cancel_requested
    }
    Add-Evidence 'Request cancellation after a mutation may have been sent' ([ordered]@{
        state = 'OutcomeUnknown'
        cancellation_requested = $true
    })
    Stop-ThroughUi
    $script:report.detail = 'The native editor restored two offline actions, cancelled the unsent issue edit and completed the retained ordinary comment after reconnect. A second ordinary comment lost its server response and retained an uncertain outcome after cancellation.'
}

function Assert-OwnedIssueCreationAbsent([IntPtr]$Window, [bool]$RequireEditorControls) {
    $script:app.Refresh()
    if ($script:app.HasExited -or $script:app.MainWindowHandle -ne $Window -or
        [ThreadlineWinInput]::WindowProcessId($Window) -ne $script:app.Id) {
        throw 'Cannot verify ownership of the isolated issue-controls window.'
    }
    $root = [Windows.Automation.AutomationElement]::FromHandle($Window)
    if ($null -eq $root -or $root.Current.ProcessId -ne $script:app.Id) {
        throw 'Native accessibility did not expose the owned issue-controls tree.'
    }
    $owner = [Windows.Automation.PropertyCondition]::new(
        [Windows.Automation.AutomationElement]::ProcessIdProperty, [int]$script:app.Id)
    $controls = $root.FindAll([Windows.Automation.TreeScope]::Descendants, $owner)
    $existingEdit = $false
    $existingComment = $false
    foreach ($control in $controls) {
        $properties = $control.Current
        if ($properties.ProcessId -ne $script:app.Id) { continue }
        if ($properties.ControlType -notin @([Windows.Automation.ControlType]::Button,
                [Windows.Automation.ControlType]::MenuItem, [Windows.Automation.ControlType]::Hyperlink)) { continue }
        if ($properties.AutomationId -ceq 'reader-edit-issue-0') { $existingEdit = $true }
        if ($properties.AutomationId -ceq 'reader-draft-comment-0') { $existingComment = $true }
        foreach ($value in @($properties.AutomationId, $properties.Name)) {
            if (($value -replace '[^a-zA-Z]', '') -match 'newissue|createissue') {
                throw 'The launched app exposes an unexpected issue-creation action.'
            }
        }
    }
    if ($RequireEditorControls -and (!$existingEdit -or !$existingComment)) {
        throw 'Expected existing-issue edit and ordinary comment controls were unavailable.'
    }
}

function Invoke-IssueWriteSurfaceProof([IntPtr]$Window) {
    Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
    $item = Open-WriteFixtureIssue $Window
    Assert-OwnedIssueCreationAbsent $Window $true
    Set-WindowFocus $Window
    [ThreadlineWinInput]::Chord(@(0x11, 0x4E))
    $null = Wait-Reader 'Ctrl+N leaves the original issue and one tab open' {
        param($reader)
        $reader.active_item -eq $item -and ($reader.tab_counts -join ',') -ceq '1' -and $reader.outbox -eq 0
    }
    Assert-OwnedIssueCreationAbsent $Window $true
    Click-AccountControl $Window 'reader-edit-issue-0'
    $null = Wait-Reader 'the existing issue draft appeared' { param($reader) $null -ne $reader.active_draft_chars }
    Assert-OwnedIssueCreationAbsent $Window $true
    Click-AccountControl $Window 'reader-draft-comment-0'
    $body = 'Threadline S07 explicitly queued comment.'
    Type-WriteFixtureField $Window 'reader-draft-input-0' $body
    Wait-WriteFixtureDraft $item $body
    Click-AccountControl $Window 'reader-submit-0'
    Click-AccountControl $Window 'reader-outbox'
    Assert-OwnedIssueCreationAbsent $Window $false
    Click-AccountControl $Window 'reader-outbox-filter-history'
    $null = Wait-Reader 'the ordinary comment is confirmed while no other work is pending' {
        param($reader) $reader.outbox_visible -and $reader.outbox -eq 0
    }
    Inspect-WriteFixtureAction $Window 0 'Confirmed'
    Add-Evidence 'Inspect owned native edit, comment and Outbox controls and exercise Ctrl+N' ([ordered]@{
        existing_issue = 41
        issue_creation_control_found = $false
        ordinary_comment_confirmed = $true
    })
    Stop-ThroughUi
    $script:report.detail = 'The owned native controls, Ctrl+N, existing issue draft and actual ordinary comment did not expose an issue-creation path.'
}

function Invoke-VisiblePendingCommentEdit([IntPtr]$Window, [long]$Item) {
    $id = 'reader-edit-pending-comment-0-0'
    $pane = 'reader-conversation-0'
    for ($step = 0; $step -le 5; $step++) {
        if (!(Test-OwnedFixtureWindow $Window)) {
            Stop-Blocked 'The isolated issue conversation is no longer owned by the expected app.'
        }
        $matched = @{ signature = $null; state = $null }
        Wait-For 'the exact offline issue and its stable native conversation viewport' {
            $current = Read-Observation
            if ($null -eq $current -or !$current.ready -or $current.pid -ne $script:app.Id -or $current.scale_factor -le 0) {
                $matched.signature = $null
                return $false
            }
            $reader = $current.PSObject.Properties['cached_reader']
            $account = $current.PSObject.Properties['account_access']
            $viewport = $current.bounds.PSObject.Properties[$pane]
            if ($null -eq $reader -or $null -eq $reader.Value -or $reader.Value.active_item -ne $Item -or
                !$reader.Value.offline -or !$reader.Value.active_draft_submitted -or
                $null -eq $account -or $null -eq $account.Value -or $account.Value.panel_open -or
                $null -eq $viewport -or $null -eq $viewport.Value -or $viewport.Value.Count -ne 4 -or
                $viewport.Value[2] -le 0 -or $viewport.Value[3] -le 0) {
                $matched.signature = $null
                return $false
            }
            $signature = (@($current.scale_factor) + $viewport.Value) -join ','
            $same = $matched.signature -ceq $signature
            $matched.signature = $signature
            $matched.state = $current
            return $same
        }
        $state = $matched.state
        if ($null -ne $state.bounds.PSObject.Properties[$id] -and
            (Invoke-OwnedFixtureControl $Window $id $null 'Draft a follow-up edit')) { return }
        if ($step -eq 5) { break }
        $rect = $state.bounds.PSObject.Properties[$pane].Value
        $point = To-ClientPoint ($rect[0] + $rect[2] / 2) ($rect[1] + $rect[3] / 2) $state.scale_factor
        $dpi = [uint32][Math]::Round(96 * $state.scale_factor)
        [ThreadlineWinInput]::ScrollOwnedConversation($Window, [uint32]$script:app.Id, $dpi, $point, -1)
        Add-Evidence 'Reveal the pending comment action in the owned issue conversation' ([ordered]@{
            automation_id = $id; step = $step + 1; input = 'Owned HWND WM_MOUSEWHEEL'
        })
        Start-Sleep -Milliseconds 100
    }
    throw 'The native follow-up edit action remained unavailable in the verified issue viewport.'
}

function Queue-CommentDependencyPair([IntPtr]$Window, [long]$Item, [string]$Original, [string]$Revised) {
    Click-AccountControl $Window 'reader-draft-comment-0'
    Wait-For 'the create-comment editor in the owned app' {
        $null -ne (Find-OwnedFixtureControl $Window 'reader-submit-0' 'Queue comment' -AllowDisabled)
    }
    Type-WriteFixtureField $Window 'reader-draft-input-0' $Original
    Wait-WriteFixtureDraft $Item $Original
    Queue-WriteFixtureDraft $Window $Item 1 'Queue comment'

    Invoke-VisiblePendingCommentEdit $Window $Item
    Wait-For 'the follow-up edit editor in the owned app' {
        $null -ne (Find-OwnedFixtureControl $Window 'reader-submit-0' 'Queue comment edit' -AllowDisabled)
    }
    Wait-WriteFixtureDraft $Item $Original
    Type-WriteFixtureField $Window 'reader-draft-input-0' $Revised
    Wait-WriteFixtureDraft $Item $Revised
    Queue-WriteFixtureDraft $Window $Item 2 'Queue comment edit'
}

function Invoke-CommentDependencyProof([IntPtr]$Window, [string]$Profile) {
    $item = Open-WriteFixtureIssue $Window
    Set-WriteFixtureOffline $Window $true
    Set-AccessPhase 'comment-dependency-offline'
    Queue-CommentDependencyPair $Window $item 'Threadline dependency original comment.' 'Threadline dependency revised comment.'
    $queued = Restart-Reader $Profile
    $null = Wait-Reader 'both ordered comment actions restored before networking' {
        param($reader) $reader.offline -and $reader.active_item -eq $item -and $reader.outbox -eq 2
    }
    Add-Evidence 'Save one ordinary comment and its explicit edit, then restore both offline in a new process' ([ordered]@{
        item = $item; pending = 2; original_projection_used = $true
    })

    Set-AccessPhase 'comment-dependency-create'
    Set-WriteFixtureOffline $queued $false
    $null = Wait-Reader 'confirmed predecessor and one remaining dependent while mutation pacing is active' {
        param($reader) !$reader.offline -and $reader.active_item -eq $item -and $reader.outbox -eq 1
    }
    Click-AccountControl $queued 'reader-outbox'
    Click-AccountControl $queued 'reader-outbox-filter-history'
    Inspect-WriteFixtureAction $queued 0 'Confirmed' 'Comment'
    Add-Evidence 'Observe confirmed original receipt with its dependent still pending before quitting' ([ordered]@{
        item = $item; pending = 1; predecessor_confirmed = $true
    })

    $resumed = Restart-Reader $Profile -FixturePhaseAfterQuit 'comment-dependency-resume'
    Wait-For 'the resumed edit confirms the same single cached comment' {
        $state = Read-Observation
        if ($null -eq $state) { return $false }
        $property = $state.PSObject.Properties['cached_reader']
        if ($null -eq $property -or $null -eq $property.Value) { return $false }
        $reader = $property.Value
        $reader.active_item -eq $item -and $reader.cached_content_ready -and
            $reader.comment_count -eq 1 -and $reader.outbox -eq 0
    } 45000 100
    Add-Evidence 'Confirm the dependent edit after restart with one cached comment and no pending actions' ([ordered]@{
        item = $item; cached_comments = 1; pending = 0
    })

    Set-WriteFixtureOffline $resumed $true
    Set-AccessPhase 'comment-dependency-offline'
    Queue-CommentDependencyPair $resumed $item 'Threadline dependency comment with a lost response.' 'Threadline dependency edit that must stay held.'
    Set-AccessPhase 'comment-dependency-lost'
    Set-WriteFixtureOffline $resumed $false
    Click-AccountControl $resumed 'reader-outbox'
    Click-AccountControl $resumed 'reader-outbox-filter-attention'
    Wait-For 'the exact original comment with an uncertain response' {
        $label = 'Comment ' + [char]0x00b7 + ' Outcome unknown'
        Invoke-OwnedFixtureControl $resumed 'reader-outbox-action-0' $null $label
    }
    $null = Wait-Reader 'the lost-response predecessor stays uncertain and preserves its held dependent' {
        param($reader) $reader.outbox_visible -and $reader.outbox_action_state -ceq 'OutcomeUnknown' -and $reader.outbox -eq 2
    }
    Click-AccountControl $resumed 'reader-outbox-filter-pending'
    Inspect-WriteFixtureAction $resumed 0 'Queued' 'Comment'
    Stop-ThroughUi
    Add-Evidence 'Preserve the uncertain predecessor and the separately visible queued edit without a second mutation' ([ordered]@{
        item = $item; uncertain = 1; held_dependent = 1
    })
    $script:report.detail = 'Native controls queued the comment and its dependent edit offline. The confirmed predecessor survived a separate process restart. The worker used the exact returned comment identity once. A distinct lost predecessor held its edit without replay.'
}

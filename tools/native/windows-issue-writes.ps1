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
            if ($Value -ceq 'Cancelled fixture issue title') { 'Issue title' }
        }
        'reader-draft-input-0' {
            if ($Value -cin @(
                'This cancelled issue draft stays in the same profile.',
                'Threadline S07 explicitly queued comment.',
                'Threadline S07 unsent local draft.',
                'Threadline S07 accepted comment with a lost response.',
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

function Queue-WriteFixtureDraft([IntPtr]$Window, [long]$Item, [int]$Expected) {
    Click-AccountControl $Window 'reader-submit-0'
    $null = Wait-Reader 'the explicit submission reached the durable Outbox' {
        param($reader)
        $reader.active_item -eq $Item -and $reader.active_draft_submitted -and $reader.outbox -eq $Expected
    }
}

function Inspect-WriteFixtureAction([IntPtr]$Window, [int]$Index, [string]$Expected, [string]$ActionLabel) {
    $id = "reader-outbox-action-$Index"
    if ($ActionLabel) {
        if ($ActionLabel -cnotin @('Comment', 'Merge pull request', 'Review', 'Reply', 'Resolve thread')) { throw 'Unknown fixture action label.' }
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

function Queue-CommentDependencyPair([IntPtr]$Window, [long]$Item, [string]$Original, [string]$Revised) {
    Click-AccountControl $Window 'reader-draft-comment-0'
    Type-WriteFixtureField $Window 'reader-draft-input-0' $Original
    Wait-WriteFixtureDraft $Item $Original
    Queue-WriteFixtureDraft $Window $Item 1

    Wait-For 'the native follow-up edit control on the first queued comment' {
        Invoke-OwnedFixtureControl $Window 'reader-edit-pending-comment-0-0' $null 'Draft a follow-up edit'
    }
    Wait-WriteFixtureDraft $Item $Original
    Type-WriteFixtureField $Window 'reader-draft-input-0' $Revised
    Wait-WriteFixtureDraft $Item $Revised
    Queue-WriteFixtureDraft $Window $Item 2
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

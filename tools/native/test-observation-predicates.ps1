Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Import the production functions without starting the native smoke driver.
$tokens = $null
$parseErrors = $null
$smoke = Join-Path $PSScriptRoot 'windows-smoke.ps1'
$ast = [System.Management.Automation.Language.Parser]::ParseFile($smoke, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count -ne 0) { throw ($parseErrors -join [Environment]::NewLine) }
$helpers = $ast.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -cin @('Wait-For', 'Wait-State', 'Get-ThreadlineArguments', 'Get-IsolatedSqlite', 'Get-FileSha256')
}, $false)
if (@($helpers.Name | Sort-Object -Unique).Count -ne 5 -or $helpers.Count -ne 5) {
    throw 'Could not load the native driver test functions.'
}
. ([scriptblock]::Create(($helpers | ForEach-Object { $_.Extent.Text }) -join [Environment]::NewLine))
$seedAssignments = @($ast.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
        $node.Left.Extent.Text -ceq '$info.EnvironmentVariables[''THREADLINE_TEST_SEED'']'
}, $true))
if ($seedAssignments.Count -ne 1) { throw 'Could not locate the single native child seed assignment.' }
function Test-NestedNativeSeed([scriptblock]$Assignment) {
    $seed = [pscustomobject]@{ fixture_revision = 's12-offline-mixed-outbox-v1' }
    $info = [Diagnostics.ProcessStartInfo]::new()
    & $Assignment
    return $info.EnvironmentVariables['THREADLINE_TEST_SEED']
}
$script:Seed = [uint64]::Parse('18446744073709551615', [Globalization.CultureInfo]::InvariantCulture)
try {
    if ((Test-NestedNativeSeed ([scriptblock]::Create($seedAssignments[0].Extent.Text))) -cne '18446744073709551615') {
        throw 'The native child did not receive the exact top-level unsigned seed.'
    }
} finally {
    Remove-Variable -Name Seed -Scope Script
}
. (Join-Path $PSScriptRoot 'windows-github-access.ps1')
. (Join-Path $PSScriptRoot 'windows-cached-reader.ps1')
. (Join-Path $PSScriptRoot 'windows-sync.ps1')
. (Join-Path $PSScriptRoot 'windows-native-lifecycle.ps1')
. (Join-Path $PSScriptRoot 'windows-native-activity.ps1')
. (Join-Path $PSScriptRoot 'windows-pr-reader.ps1')
. (Join-Path $PSScriptRoot 'windows-owned-uia.ps1')
. (Join-Path $PSScriptRoot 'windows-review-context.ps1')

$account = [pscustomobject]@{
    panel_open = $true
    busy = $false
    fixture_route_entered = $true
    fixture_route_focused = $false
    fixture_token_focused = $true
}
$focus = [pscustomobject]@{ pid = 123; account_access = $account }
if (!(Test-FixtureTokenFocus $focus 123) -or (Test-FixtureTokenFocus $focus 456) -or
    (Test-FixtureTokenFocus $null 123) -or (Test-FixtureTokenFocus ([pscustomobject]@{ pid = 123 }) 123)) {
    throw 'The masked editor check must use the owned process receipt.'
}
foreach ($field in @('panel_open', 'busy', 'fixture_route_entered', 'fixture_route_focused', 'fixture_token_focused')) {
    $account.$field = !$account.$field
    if (Test-FixtureTokenFocus $focus 123) {
        throw "The masked editor check accepted stale native account state for $field."
    }
    $account.$field = !$account.$field
}

$draftFocus = [pscustomobject]@{ pid = 123; cached_reader = [pscustomobject]@{
    active_draft_focus = 'reader-draft-input-1'; active_item = 3; focused_pane = 1
    offline = $true; signed_out = $true
} }
if (!(Test-OfflineDraftFocusReceipt $draftFocus 123 'reader-draft-input-1' 3 1) -or
    (Test-OfflineDraftFocusReceipt $draftFocus 456 'reader-draft-input-1' 3 1) -or
    (Test-OfflineDraftFocusReceipt $draftFocus 123 'reader-draft-input-0' 3 1) -or
    (Test-OfflineDraftFocusReceipt $draftFocus 123 'reader-draft-input-1' 4 1) -or
    (Test-OfflineDraftFocusReceipt $draftFocus 123 'reader-draft-input-1' 3 0) -or
    (Test-OfflineDraftFocusReceipt $draftFocus 123 'reader-draft-input-1' 4 1 -NativeFocus) -or
    (Test-OfflineDraftFocusReceipt ([pscustomobject]@{}) 123 'reader-draft-input-1' 3 1 -NativeFocus) -or
    (Test-OfflineDraftFocusReceipt ([pscustomobject]@{ pid = 123 }) 123 'reader-draft-input-1' 3 1)) {
    throw 'The offline draft focus receipt did not require the original child, pane, item and editor.'
}
foreach ($field in @('offline', 'signed_out')) {
    $draftFocus.cached_reader.$field = $false
    if ((Test-OfflineDraftFocusReceipt $draftFocus 123 'reader-draft-input-1' 3 1) -or
        (Test-OfflineDraftFocusReceipt $draftFocus 123 'reader-draft-input-1' 3 1 -NativeFocus)) {
        throw "The offline draft focus receipt accepted $field false."
    }
    $draftFocus.cached_reader.$field = $true
}
$draftFocus.cached_reader.PSObject.Properties.Remove('active_draft_focus')
if (Test-OfflineDraftFocusReceipt $draftFocus 123 'reader-draft-input-1' 3 1) {
    throw 'Draft persistence alone was mistaken for editor focus.'
}
if (!(Test-OfflineDraftFocusReceipt $draftFocus 123 'reader-draft-input-1' 3 1 -NativeFocus)) {
    throw 'Verified native editor focus should be sufficient for the exact offline item and pane.'
}

$script:AccessPhaseFile = [IO.Path]::GetTempFileName()
try {
    foreach ($phase in @('issue-edit-baseline', 'issue-edit-remote-change', 'issue-edit-commit',
        'merge-cancel-hold', 'merge-cancel-read', 'merge-cancel-finish')) {
        Set-AccessPhase $phase
        if ([IO.File]::ReadAllText($script:AccessPhaseFile) -cne $phase) {
            throw 'The simulator did not advance to the requested write phase.'
        }
    }
    $refused = $false
    try { Set-AccessPhase 'unlisted-issue-write' } catch {
        $refused = $_.Exception.Message -ceq 'Unknown access fixture phase.'
    }
    if (!$refused -or [IO.File]::ReadAllText($script:AccessPhaseFile) -cne 'merge-cancel-finish') {
        throw 'An unsupported phase changed the write fixture.'
    }
} finally {
    [IO.File]::Delete($script:AccessPhaseFile)
    Remove-Variable -Name AccessPhaseFile -Scope Script
}

$editorAction = [pscustomobject]@{
    ProcessId = 41; AutomationId = 'reader-submit-0'; Name = 'Queue comment edit'
    IsOffscreen = $false; IsEnabled = $false
    BoundingRectangle = [pscustomobject]@{ Width = 120; Height = 24 }
}
if (!(Test-OwnedAutomationProperties $editorAction 'reader-submit-0' 41 'Queue comment edit' -AllowDisabled) -or
    (Test-OwnedAutomationProperties $editorAction 'reader-submit-0' 41 'Queue comment edit') -or
    (Test-OwnedAutomationProperties $editorAction 'reader-submit-0' 41 'Queue comment' -AllowDisabled) -or
    (Test-OwnedAutomationProperties $editorAction 'reader-submit-0' 42 'Queue comment edit' -AllowDisabled)) {
    throw 'The read-only editor readiness check did not distinguish the owned disabled edit from an enabled submit.'
}

$notLoaded = '{"connection":"Reachable","cache_generation":2,"panel_open":false,"controls":null}' | ConvertFrom-Json
if (Test-NotificationSyncReady $notLoaded) { throw 'Reachable fixture transport did not prove saved Sync controls had loaded.' }
$loaded = '{"connection":"Reachable","cache_generation":2,"panel_open":true,"controls":{"work_offline":false,"pause_background":false,"pause_queued_actions":false,"notification_quiet":false}}' | ConvertFrom-Json
if (!(Test-NotificationSyncReady $loaded)) { throw 'The observed loaded Sync controls did not permit the quiet-hours journey.' }
if ((Test-LifecycleSync $notLoaded $false $true -Reachable) -or
    (Test-LifecycleSync ([pscustomobject]@{ connection = 'Reachable' }) $false $true -Reachable)) {
    throw 'Native lifecycle treated a connected startup before the Sync controls loaded as ready.'
}
$lifecycleRestored = '{"connection":"Reachable","controls":{"work_offline":false,"pause_background":true,"pause_queued_actions":true}}' | ConvertFrom-Json
if (!(Test-LifecycleSync $lifecycleRestored $false $true -Reachable) -or
    (Test-LifecycleSync $lifecycleRestored $false $false -Reachable)) {
    throw 'Native lifecycle did not distinguish saved paused work from released work.'
}
$lifecycleRestored.controls.pause_queued_actions = $false
if (!(Test-LifecycleSync $lifecycleRestored $false $false -Reachable)) {
    throw 'Native lifecycle did not recognize restored online state after interruption.'
}

$scopeActions = @(0..3 | ForEach-Object {
    [pscustomobject]@{
        id = "operation-$_"; kind = 'Review'; item = $_; snapshot_sha256 = "snapshot-$_"
        state = 'Queued'; reason = $null; attempts = 0; target = @(1, $_)
        dependencies = @(); send_starts = 0; cancel_requested = $false
        scope_hold = 'repository_unselected'; dispatch_hold = 'repository_unselected'
    }
})
$scope = [pscustomobject]@{
    fixture_revision = 's09-deselected-outbox-v1'; account = 1; user = 41; owner = 'fixture-org'
    protected_token_present = $true; offline = $false; outbox_paused = $false
    selected = @(72); drafts = @([pscustomobject]@{ id = 'saved' }); actions = $scopeActions
}
$originalScope = $scope | ConvertTo-Json -Depth 8 | ConvertFrom-Json
Assert-ScopedOutbox $originalScope $scope $false
$scope.actions[2].snapshot_sha256 = 'replaced'
$refused = $false
try { Assert-ScopedOutbox $originalScope $scope $false } catch {
    $refused = $_.Exception.Message -like '*snapshot_sha256 changed*'
}
if (!$refused) { throw 'The scope check accepted a different submitted review snapshot.' }
$lifecycleReader = '{"cached_content_ready":true,"cached_visible_panes":2,"focused_pane":1,"active_item":3,"tab_counts":[10,10],"outbox":4,"offline":false,"signed_out":true}' | ConvertFrom-Json
if (!(Test-LifecycleWorkspace $lifecycleReader 3 $false $true) -or
    (Test-LifecycleWorkspace $lifecycleReader 3 $false $false) -or
    (Test-LifecycleWorkspace $lifecycleReader 4 $false $true)) {
    throw 'Native lifecycle did not fence the restored signed-out workspace and exact cached item.'
}
$script:lifecycleSeed = '{"content":{"issue":1},"drafts":[{"id":"comment-1","version":2}],"sync_checkpoint":{"cursor":"2","started_at_ms":10},"schedule":{"timezone":"UTC"},"feed_event":1,"action_ids":["fixture-action"]}' | ConvertFrom-Json
$retained = $script:lifecycleSeed | ConvertTo-Json -Depth 8 | ConvertFrom-Json
Assert-LifecycleCacheUnchanged $retained
$retained.sync_checkpoint.cursor = '3'
$refusedCheckpoint = $false
try { Assert-LifecycleCacheUnchanged $retained } catch {
    $refusedCheckpoint = $_.Exception.Message -like '*sync_checkpoint*'
}
if (!$refusedCheckpoint) { throw 'Native lifecycle accepted a changed durable sync continuation.' }
$quietChoice = 'Enable quiet hours ' + [char]0x00b7 + ' '
if ((Get-NotificationChoiceName 'Enable quiet hours' $false) -cne ($quietChoice + 'Unselected') -or
    (Get-NotificationChoiceName 'Enable quiet hours' $true) -cne ($quietChoice + 'Selected')) {
    throw 'The notification driver did not request the product accessibility name for the current setting state.'
}

function Read-Observation {
    if ($script:observations.Count -gt 1) { return $script:observations.Dequeue() }
    return $script:observations.Peek()
}

function New-FixtureConversation([int]$Serial, [int]$Process, [bool]$PanelOpen, [int]$Top, [string]$Head) {
    $viewport = @{}
    if ($Top -gt 0) { $viewport['pr-0-conversation-records'] = @(200, $Top, 700, 220) }
    [pscustomobject]@{
        serial = $Serial; ready = $true; pid = $Process; scale_factor = 1.5
        account_access = [pscustomobject]@{ panel_open = $PanelOpen }
        bounds = [pscustomobject]$viewport
        cached_reader = [pscustomobject]@{
            active_item = 1
            pull_request = [pscustomobject]@{ head = $Head; section = 'Conversation'; threads_coverage = 'Complete'; threads_on_page = 1 }
        }
    }
}

$script:app = [pscustomobject]@{ Id = 41 }
$head = 'b' * 40
$script:observations = [System.Collections.Generic.Queue[object]]::new()
$script:observations.Enqueue((New-FixtureConversation 1 41 $true 400 $head))
$script:observations.Enqueue((New-FixtureConversation 2 41 $false 0 $head))
$script:observations.Enqueue((New-FixtureConversation 3 40 $false 424 $head))
$script:observations.Enqueue((New-FixtureConversation 4 41 $false 424 ('e' * 40)))
$script:observations.Enqueue((New-FixtureConversation 5 41 $false 400 $head))
$script:observations.Enqueue((New-FixtureConversation 6 41 $false 424 $head))
$script:observations.Enqueue((New-FixtureConversation 7 41 $false 0 $head))
$script:observations.Enqueue((New-FixtureConversation 8 41 $false 424 $head))
$script:observations.Enqueue((New-FixtureConversation 9 41 $false 424 $head))
$conversation = Wait-FixtureThreadViewport 1
if ($conversation.serial -ne 9 -or $conversation.bounds.'pr-0-conversation-records'[1] -ne 424) {
    throw 'The thread driver accepted an unsettled, covered, missing, or unrelated Conversation viewport.'
}

$script:observations = [System.Collections.Generic.Queue[object]]::new()
$script:observations.Enqueue([pscustomobject]@{ ready = $true })
$script:observations.Enqueue([pscustomobject]@{ account_access = $null })
$script:observations.Enqueue([pscustomobject]@{ account_access = @{ busy = $true; active_user_id = 41 } })
$script:observations.Enqueue([pscustomobject]@{ account_access = @{ busy = $false; active_user_id = 42 } })
$script:observations.Enqueue([pscustomobject]@{ account_access = @{ busy = $false; active_user_id = 41 } })

$visited = [System.Collections.Generic.List[int]]::new()
$state = Wait-Account 'available active fixture account' {
    param($account)
    $visited.Add($account.active_user_id)
    $account.active_user_id -eq 41
}
if ($state.account_access.busy -or $state.account_access.active_user_id -ne 41 -or
    ($visited -join ',') -cne '42,41') {
    throw 'Wait-Account did not reject missing, busy, and mismatched account observations.'
}

$control = [pscustomobject]@{
    ProcessId = 41
    AutomationId = 'reader-tab-0-0'
    IsOffscreen = $false
    IsEnabled = $true
    BoundingRectangle = @{ Width = 80; Height = 24 }
}
if (!(Test-OwnedAutomationProperties $control 'reader-tab-0-0' 41) -or
    (Test-OwnedAutomationProperties $control 'reader-tab-0-0' 42) -or
    (Test-OwnedAutomationProperties $control 'reader-tab-0-1' 41)) {
    throw 'The accessibility guard did not verify the exact native control and child process.'
}
$control.IsOffscreen = $true
if (Test-OwnedAutomationProperties $control 'reader-tab-0-0' 41) {
    throw 'The accessibility guard accepted an offscreen native control.'
}

Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$editKind = [Windows.Automation.ControlType]::Edit
function New-OwnedEdit($Kind, [bool]$Password) {
    [pscustomobject]@{ Current = [pscustomobject]@{
        ProcessId = 41; AutomationId = 'reader-draft-input-0'; Name = 'Local Markdown draft'
        IsOffscreen = $false; IsEnabled = $true; IsPassword = $Password; ControlType = $Kind
        BoundingRectangle = @{ Width = 80; Height = 24 }
    } }
}
$rejectedEdits = @(
    (New-OwnedEdit $null $false)
    (New-OwnedEdit ([Windows.Automation.AutomationElement]::NotSupported) $false)
    (New-OwnedEdit ([pscustomobject]@{ Id = $editKind.Id }) $false)
    (New-OwnedEdit ([Windows.Automation.ControlType]::Custom) $false)
    (New-OwnedEdit $editKind $true)
)
$validEdit = New-OwnedEdit $editKind $false
if ($null -ne (Select-UniqueOwnedFixtureControl $rejectedEdits 'reader-draft-input-0' 41 'Local Markdown draft' -Edit) -or
    (Select-UniqueOwnedFixtureControl ($rejectedEdits + $validEdit) 'reader-draft-input-0' 41 'Local Markdown draft' -Edit) -ne $validEdit) {
    throw 'The edit selector did not wait for the exact owned non-password UI Automation Edit type.'
}

$resultA = "Pull request fixture-org/private #42 $([char]0x00b7) First fixture pull request"
$resultB = "Pull request fixture-org/private #43 $([char]0x00b7) Second fixture pull request"
function Get-OwnedFixtureActivation($Element, [string]$Id) { return $Element.Activation }
function New-FixtureRow([string]$Name, [bool]$Offscreen, [bool]$Actionable) {
    [pscustomobject]@{
        Current = [pscustomobject]@{
            ProcessId = 41; AutomationId = 'reader-result-0'; Name = $Name
            IsOffscreen = $Offscreen; IsEnabled = $true
            BoundingRectangle = @{ Width = 80; Height = 24 }
        }
        Activation = if ($Actionable) { [pscustomobject]@{ Invoke = $true } } else { $null }
    }
}
$realRow = New-FixtureRow $resultA $false $true
$otherRepresentations = @(
    (New-FixtureRow $resultA $true $true)
    (New-FixtureRow $resultB $false $true)
    (New-FixtureRow $resultA $false $false)
)
if ($null -ne (Select-UniqueOwnedFixtureControl $otherRepresentations 'reader-result-0' 41 $resultA) -or
    (Select-UniqueOwnedFixtureControl ($otherRepresentations + $realRow) 'reader-result-0' 41 $resultA) -ne $realRow) {
    throw 'Native PR selection did not isolate the visible actionable exact-name row.'
}
function Stop-Blocked([string]$Message) { throw [InvalidOperationException]::new($Message) }
$refused = $false
try {
    $null = Select-UniqueOwnedFixtureControl @($realRow, (New-FixtureRow $resultA $false $true)) 'reader-result-0' 41 $resultA
} catch [InvalidOperationException] {
    $refused = $_.Exception.Message -like '*multiple eligible controls*'
}
if (!$refused) { throw 'Native PR selection accepted two independently actionable exact-name rows.' }

$serverOrder = @(
    [pscustomobject]@{ Id = 'reader-result-0'; Name = $resultB }
    [pscustomobject]@{ Id = 'reader-result-1'; Name = $resultA }
)
$restoredTabs = @(
    [pscustomobject]@{ Id = 'reader-tab-0-0'; Name = 'Issue fixture-org/private #41 Existing issue' }
    [pscustomobject]@{ Id = 'reader-tab-0-1'; Name = 'Pull request fixture-org/private #43 Second fixture pull request' }
    [pscustomobject]@{ Id = 'reader-tab-0-2'; Name = 'Pull request fixture-org/private #42 First fixture pull request' }
)
if ((Find-FixturePrChoice $serverOrder $resultA) -cne 'reader-result-1' -or
    (Find-FixturePrChoice $restoredTabs 'Pull request fixture-org/private #42 First fixture pull request') -cne 'reader-tab-0-2') {
    throw 'The native pull request journey assumed repository order or tab position.'
}

$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ bounds = [pscustomobject]@{} })
$script:observations.Enqueue([pscustomobject]@{ bounds = [pscustomobject]@{ 'account-open' = @(1, 2, 0, 1) } })
$script:observations.Enqueue([pscustomobject]@{ bounds = [pscustomobject]@{ 'account-open' = @(1, 2, 10, 12) } })
$state = Wait-AccountControl 'account-open'
if ($state.bounds.'account-open'[2] -ne 10) {
    throw 'The reconnect driver accepted a missing or zero-area native control.'
}

$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ cached_reader = $null })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = @{ active_item = 43; cached_content_ready = $true } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = @{ active_item = 44; cached_content_ready = $false } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = @{ active_item = 44; cached_content_ready = $true } })
$visited.Clear()
$state = Wait-Reader 'cached content for the expected item' {
    param($reader)
    $visited.Add($reader.active_item)
    $reader.active_item -eq 44 -and $reader.cached_content_ready
} 1
if (!$state.cached_reader.cached_content_ready -or $state.cached_reader.active_item -ne 44 -or
    ($visited -join ',') -cne '43,44,44') {
    throw 'Wait-Reader did not reject missing, mismatched, and loading reader states.'
}

$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ ready = $true })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = $null })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ active_item = 1 } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ pull_request = $null } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ active_item = 1; pull_request = @{ head = 'old' } } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ active_item = 2; pull_request = @{ head = 'current' } } })
$visited.Clear()
$state = Wait-PrReader 'isolated pull request observation' {
    param($pr, $reader)
    $visited.Add($reader.active_item)
    $reader.active_item -eq 2 -and $pr.head -ceq 'current'
}
if ($state.cached_reader.active_item -ne 2 -or ($visited -join ',') -cne '1,2') {
    throw 'Wait-PrReader did not verify its exact nested predicate.'
}

$activityBaseline = [pscustomobject]@{ cached_reader = [pscustomobject]@{
    active_number = 43
    pull_request = [pscustomobject]@{
        merged = $true; head = 'b' * 40; section = 'Files'; files_coverage = 'Partial'; files_on_page = 1
    }
} }
if ((Test-NativeActivityBaselineComparison $null) -or
    (Test-NativeActivityBaselineComparison ([pscustomobject]@{})) -or
    (Test-NativeActivityBaselineComparison $activityBaseline)) {
    throw 'The native baseline accepted an unavailable or partial comparison.'
}
$activityBaseline.cached_reader.pull_request.files_coverage = 'Complete'
if (!(Test-NativeActivityBaselineComparison $activityBaseline)) {
    throw 'The native baseline rejected the fully downloaded comparison for the old merged pull request.'
}
$activityBaseline.cached_reader.active_number = 42
if (Test-NativeActivityBaselineComparison $activityBaseline) {
    throw 'The native baseline accepted a different pull request.'
}

$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{} })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ shared_sync = @{ connection = 'Offline' } } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ shared_sync = @{ connection = 'Reachable' } } })
$script:observations.Enqueue([pscustomobject]@{ cached_reader = [pscustomobject]@{ shared_sync = @{ connection = 'Unreachable' } } })
$states = [System.Collections.Generic.List[string]]::new()
$state = Wait-Sync 'the exact transport outcome' {
    param($sync)
    $states.Add($sync.connection)
    $sync.connection -ceq 'Unreachable'
}
if ($state.cached_reader.shared_sync.connection -cne 'Unreachable' -or
    ($states -join ',') -cne 'Offline,Reachable,Unreachable') {
    throw 'Wait-Sync did not verify its exact nested state predicate.'
}
$sample = Get-ReaderBudget ([double[]](1..20)) 20
if ($sample.sample_count -ne 20 -or $sample.p95_ms -ne 19 -or $sample.status -ne 'Pass' -or
    (Get-ReaderBudget ([double[]](1..20)) 19).status -ne 'Fail') {
    throw 'The Section 16 reader budget did not enforce the strict p95 ceiling.'
}

$AccessProof = $false
$Measure = $false
$ReaderProof = ''
$path = 'C:\isolated profile\observation.json'
if ((Get-ThreadlineArguments $path -OpenReview) -cne '--e2e-report "C:\isolated profile\observation.json" --fixture-s01 --open-review') {
    throw 'The standard native smoke launch arguments changed.'
}
$ReaderProof = 'Offline'
if ((Get-ThreadlineArguments $path) -cne '--e2e-report "C:\isolated profile\observation.json"' -or
    (Get-ThreadlineArguments $path -SeedReader 'small') -cne '--e2e-seed-reader small --e2e-report "C:\isolated profile\observation.json"' -or
    (Get-ThreadlineArguments $path -SeedReader 'section-16') -cne '--e2e-seed-reader section-16 --e2e-report "C:\isolated profile\observation.json"') {
    throw 'The cached reader and seeder receive distinct entry-point arguments.'
}

$fixture = Join-Path ([IO.Path]::GetTempPath()) ('threadline-shared-sqlite-' + [Guid]::NewGuid().ToString('N'))
$null = [IO.Directory]::CreateDirectory($fixture)
try {
    $database = Join-Path $fixture 'threadline.sqlite3'
    $sharing = [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete
    $writer = [IO.File]::Open($database, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, $sharing)
    try {
        $header = [Text.Encoding]::ASCII.GetBytes('SQLite format 3' + [char]0)
        $writer.Write($header, 0, $header.Length)
        $writer.Flush()
        $live = Get-IsolatedSqlite $fixture -Live
        if ($null -ne $live.sha256) { throw 'The live reader computed an unquiesced database digest.' }
    } finally {
        $writer.Dispose()
    }
    $closed = Get-IsolatedSqlite $fixture
    if ($live.creation_time_ticks -ne $closed.creation_time_ticks -or
        $closed.sha256 -cne '3f8434aff012ffcb594ad7527d6042841ce94f499887f86ab828f84a9a275f91') {
        throw 'The Windows SQLite header and clean-quit digest fixture did not match.'
    }
} finally {
    [IO.Directory]::Delete($fixture, $true)
}
. (Join-Path $PSScriptRoot 'windows-issue-writes.ps1')
$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ bounds = [pscustomobject]@{
    'reader-issue-title-0' = @(1, 2, 20, 10)
    'reader-draft-input-0' = @(1, 13, 20, 10)
} })
$script:fixtureText = $null
$script:fixtureWrites = 0
function Set-OwnedFixtureEditValue([IntPtr]$Window, [string]$Id, [string]$Name, [string]$Value) {
    if ($Window -ne 123 -or $Id -cne 'reader-issue-title-0' -or $Name -cne 'Issue title') {
        if ($Window -ne 123 -or !(($Id -ceq 'reader-search' -and $Name -ceq 'Search downloaded content') -or
            ($Id -ceq 'reader-draft-input-0' -and $Name -ceq 'Local Markdown draft'))) {
            throw 'Fixture editing did not target the expected owned field.'
        }
    }
    $script:fixtureWrites++
    $script:fixtureText = $Value
    return $true
}
function Get-OwnedFixtureEditPattern([IntPtr]$Window, [string]$Id, [string]$Name) {
    return [pscustomobject]@{ Current = [pscustomobject]@{ Value = $script:fixtureText } }
}
Type-WriteFixtureField 123 'reader-issue-title-0' 'Cancelled fixture issue title'
Type-WriteFixtureField 123 'reader-issue-title-0' 'Local change to the same issue'
if ($script:fixtureText -cne 'Local change to the same issue') { throw 'The conflict did not edit its known title.' }
Type-WriteFixtureField 123 'reader-draft-input-0' 'Retain the local issue description across the conflict.'
if ($script:fixtureText -cne 'Retain the local issue description across the conflict.') { throw 'The conflict did not edit its known description.' }
$refused = $false
try {
    Type-WriteFixtureField 123 'account-token' 'Cancelled fixture issue title'
} catch {
    $refused = $_.Exception.Message -like '*unknown synthetic field or value*'
}
if (!$refused -or $script:fixtureWrites -ne 3) { throw 'The native edit journey accepted an out-of-scope field.' }


$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ bounds = [pscustomobject]@{ 'reader-search' = @(1, 2, 20, 10) } })
$script:fixtureWrites = 0
for ($index = 0; $index -lt 20; $index++) {
    $item = Set-ReaderPerformanceQuery 123 $index
    if ($item -ne (1000 + 3 * $index) -or $script:fixtureText -cne "Investigate relay behavior $item") {
        throw 'Wrong exact native benchmark search value.'
    }
}
$refused = $false
try { $null = Set-ReaderPerformanceQuery 123 20 } catch {
    $refused = $_.Exception.Message -like '*Unknown Section 16 search*'
}
if (!$refused -or $script:fixtureWrites -ne 20) { throw 'The benchmark wrote an unauthorized search fixture.' }

Write-Output 'Native observation predicate checks passed.'


$script:observations.Clear()
$script:observations.Enqueue([pscustomobject]@{ bounds = [pscustomobject]@{
    'reader-result-0' = @(1, 2, 20, 10)
    'reader-result-1' = @(1, 13, 20, 10)
} })
function Find-OwnedFixtureControl([IntPtr]$Window, [string]$Id, [string]$ExpectedName) {
    $candidate = $serverOrder | Where-Object { $_.Id -ceq $Id -and $_.Name -ceq $ExpectedName }
    if ($null -ne $candidate) { return [pscustomobject]@{ Current = $candidate } }
    return $null
}
$script:fixtureInvocations = 0
function Invoke-OwnedFixtureControl([IntPtr]$Window, [string]$Id, $InputClock, [string]$ExpectedName) {
    if ($Window -ne 123 -or $Id -cne 'reader-result-1' -or $ExpectedName -cne $resultA) {
        throw 'The native PR journey lost its selected identity before activation.'
    }
    $script:fixtureInvocations++
    return $script:fixtureInvocations -eq 2
}
function Add-Evidence([string]$Action, $Observed) {
    if ($Observed.automation_id -cne 'reader-result-1' -or $Observed.eligible_controls -ne 1) {
        throw 'The native PR journey did not record the unique expected target.'
    }
}
Activate-FixturePr 123 'reader-result-' 42 'First fixture pull request' -Result
if ($script:fixtureInvocations -ne 2) {
    throw 'The journey did not retry transient pre-activation unavailability within the wait.'
}

function Measure-ReaderPerformance([IntPtr]$Window, $Samples, $Attribution, [switch]$Physical, [switch]$Concurrent) {
    for ($index = 0; $index -lt 20; $index++) { $Samples.cached_open.Add(100.0) }
    $Samples.cached_tab_switch.Add(90.0)
    $Attribution.cached_tab_switch.Add([ordered]@{ status = 'Correlated' })
    throw 'Native test interruption'
}
$script:report = [ordered]@{ budgets = [ordered]@{ warm_start_ms = @{ status = 'Fail' } } }
$interrupted = $false
try { Invoke-ReaderPerformance 123 -KeepOpen } catch {
    $interrupted = $_.Exception.Message -ceq 'Native test interruption'
}
if (!$interrupted -or $script:report.budgets.warm_start_ms.status -cne 'Fail' -or
    $script:report.budgets.cached_open.p95_ms -ne 100 -or $script:report.budgets.cached_open.status -cne 'Fail' -or
    $script:report.budgets.cached_tab_switch.sample_count -ne 1 -or
    $script:report.budgets.cached_tab_switch.samples_ms[0] -ne 90 -or
    $null -ne $script:report.budgets.cached_tab_switch.p95_ms -or
    $script:report.budgets.cached_tab_switch.status -cne 'Blocked' -or
    $script:report.budgets.local_search.sample_count -ne 0 -or
    $script:report.attribution.cached_tab_switch[0].status -cne 'Correlated') {
    throw 'The interrupted benchmark did not retain completed and partial measurements.'
}
$frequency = [Diagnostics.Stopwatch]::Frequency
$startup = [pscustomobject]@{ startup_ms = 1200; startup_milestones_ms = @{ profile_opened = 500; active_panes_hydrated = 1400 }; first_cached_workspace_paint = [pscustomobject]@{
    qpc = 3 * $frequency; frequency = $frequency; since_app_start_ms = 1500
} }
$timing = Get-LargeStartupTiming $startup $frequency (2 * $frequency) (4 * $frequency)
if ($timing.status -cne 'Correlated' -or $timing.process_start_call_ms -ne 1000 -or
    $timing.app_ready_ms -ne 1200 -or $timing.launch_to_cached_panes_paint_ms -ne 2000 -or
    $timing.app_milestones_ms.profile_opened -ne 500 -or $timing.app_milestones_ms.active_panes_hydrated -ne 1400 -or
    $timing.app_start_to_cached_panes_paint_ms -ne 1500 -or $timing.paint_to_verified_receipt_ms -ne 1000) {
    throw 'Startup attribution did not keep the external endpoint and the app paint distinct.'
}
$timing = Get-LargeStartupTiming $startup $frequency (2 * $frequency) (2 * $frequency)
if ($timing.status -cne 'Unavailable' -or $null -ne $timing.launch_to_cached_panes_paint_ms) {
    throw 'Startup timing accepted an app stamp newer than the external observation.'
}

$scratch = Join-Path ([IO.Path]::GetTempPath()) ('threadline-feed-test-' + [guid]::NewGuid())
$null = [IO.Directory]::CreateDirectory($scratch)
try {
    function Stop-Blocked([string]$Message) { throw $Message }
    $script:AccessPhaseFile = Join-Path $scratch 'access-fixture-phase.txt'
    $held = Join-Path $scratch 'reader-feed-2-held'
    $ack = Join-Path $scratch 'reader-feed-2-painted'
    $served = Join-Path $scratch 'reader-feed-2-served'
    [IO.File]::WriteAllText($held, '')
    $script:app = [pscustomobject]@{ HasExited = $false }
    Wait-ConcurrentFeed 2
    # The runner only publishes served after it observes this exact paint acknowledgment.
    $runspace = [PowerShell]::Create()
    $null = $runspace.AddScript({
        param($ack, $served)
        $clock = [Diagnostics.Stopwatch]::StartNew()
        while (![IO.File]::Exists($ack) -and $clock.Elapsed.TotalSeconds -lt 3) { Start-Sleep -Milliseconds 10 }
        if ([IO.File]::Exists($ack)) { [IO.File]::WriteAllText($served, '') }
    }).AddArgument($ack).AddArgument($served)
    $async = $runspace.BeginInvoke()
    $timing = [ordered]@{ status = 'Correlated'; input = 'UIA InvokePattern' }
    try {
        Complete-ConcurrentFeed 2 $timing
        $null = $runspace.EndInvoke($async)
    } finally { $runspace.Dispose() }
    if ($timing.feed_page -ne 2 -or !$timing.feed_response_written -or ![IO.File]::Exists($ack)) {
        throw 'The driver did not acknowledge the exact overlapped request.'
    }
    $timing.status = 'Unavailable'
    [IO.File]::WriteAllText((Join-Path $scratch 'reader-feed-3-held'), '')
    $refused = $false
    try { Complete-ConcurrentFeed 3 $timing } catch {
        $refused = $_.Exception.Message -like '*could not be correlated*'
    }
    if (!$refused -or [IO.File]::Exists((Join-Path $scratch 'reader-feed-3-painted'))) {
        throw 'The driver accepted a native action without matching paint evidence.'
    }
    [IO.File]::WriteAllText((Join-Path $scratch 'reader-feed-3-expired'), '')
    $expired = $false
    try { Wait-ConcurrentFeed 3 } catch { $expired = $_.Exception.Message -like '*state expired*' }
    if (!$expired) { throw 'A terminal request was mistaken for a held request.' }
    [IO.File]::WriteAllText((Join-Path $scratch 'reader-feed-4-held'), '')
    $script:readCount = 0
    $script:report = [ordered]@{}
    function Read-Observation {
        $script:readCount++
        $id = if ($script:readCount -eq 1) { 7 } else { 9 }
        return [pscustomobject]@{ cached_reader=[pscustomobject]@{
            active_item=9; cached_content_ready=$true
            performance_timing=[pscustomobject]@{frequency=[Diagnostics.Stopwatch]::Frequency
                kind='open'; target=$id; handler_qpc=10; painted_qpc=15 }
        }}
    }
    $inputTiming = @{ method='UIA InvokePattern'; begin=5L; returned=20L }
    $matched = Wait-ReaderPerformanceState 'a fresh paint' {param($r) $r.active_item -eq 9 -and $r.cached_content_ready} 'open' $true 4 $inputTiming
    if ($script:readCount -ne 2 -or $matched.cached_reader.performance_timing.target -ne 9) {
        throw 'An earlier or re-read view supplied the timed paint.'
    }
} finally {
    [IO.Directory]::Delete($scratch, $true)
}

# The standalone concurrent driver starts with no runtime budget map.
$script:report = [ordered]@{ actions = @() }
function Wait-Account { param($Description, $Check) }
function Open-SyncSettings { param($Window) }
function Wait-Sync { param($Description, $Check) }
function Set-AccessPhase { param($Phase) }
function Click-AccountControl { param($Window, $Id) }
function Stop-ThroughUi {}
function Measure-ReaderPerformance([IntPtr]$Window, $Samples, $Attribution, [switch]$Physical, [switch]$Concurrent) {
    if ($Window -ne 123 -or !$Concurrent -or $Physical) { throw 'The native benchmark lost its concurrent mode.' }
    foreach ($metric in $Samples.Keys) {
        for ($index = 0; $index -lt 20; $index++) { $Samples[$metric].Add(50.0) }
    }
}
Invoke-ConcurrentReaderPerformance 123
if (!$script:report.concurrent_checks_passed -or $script:report.budgets.Count -ne 3 -or
    @($script:report.budgets.Values.status | Where-Object { $_ -cne 'Pass' }).Count -ne 0) {
    throw 'The concurrent driver did not populate a new budget map under strict mode.'
}
$missingTiming = Get-ReaderTimingAttribution @{method = 'UIA InvokePattern'; begin = 10; returned = 20} ([pscustomobject]@{ready = $true}) 'search' 1 30
if ($missingTiming.status -cne 'Unavailable') { throw 'A missing reader observation appeared correlated.' }

. (Join-Path $PSScriptRoot 'windows-history.ps1')
$script:historyReads = 0
function Wait-Reader([string]$Description, [scriptblock]$Check) {
    foreach ($reader in @([pscustomobject]@{}, [pscustomobject]@{ history = $null },
        [pscustomobject]@{ history = [pscustomobject]@{ comparison_coverage = 'Missing'; files = 0 } })) {
        $script:historyReads++
        if (& $Check $reader) { return $reader }
    }
    throw $Description
}
$result = Wait-History 'read the completed history observation' {
    param($history) $history.comparison_coverage -ceq 'Missing' -and $history.files -eq 0
}
if ($script:historyReads -ne 3 -or $result.history.files -ne 0) {
    throw 'The history driver accepted an interim observation with no initialized history view.'
}
$script:fixtureQueued = $false
function Wait-ExactFixtureReview([string]$Draft, [string]$Head) {
    if ($Draft -cne 'fixture-draft' -or $Head -cne ('b' * 40)) { throw 'Wrong review scope.' }
    [pscustomobject]@{ cached_reader = [pscustomobject]@{ active_item = 3; outbox = 0
        pull_request = [pscustomobject]@{ review_draft = [pscustomobject]@{
            queued = $script:fixtureQueued; summary_nonempty = $true
        } }
    } }
}
Wait-HistoryUnsubmittedDraft 'fixture-draft' ('b' * 40) 3
$script:fixtureQueued = $true
$refused = $false
try { Wait-HistoryUnsubmittedDraft 'fixture-draft' ('b' * 40) 3 } catch { $refused = $_.Exception.Message -like '*unsubmitted draft*' }
if (!$refused) { throw 'The history driver accepted a submitted review as an unsent draft.' }

. (Join-Path $PSScriptRoot 'windows-pane-positions.ps1')
$script:positionReads = 0
function Read-Observation {
    $script:positionReads++
    $archiveRepo = if ($script:positionReads -eq 1) { 1 } else { 2 }
    [pscustomobject]@{
        bounds = [pscustomobject]@{ 'reader-tab-0-1' = @(0, 0, 12, 12); 'reader-tab-1-0' = @(12, 0, 12, 12) }
        cached_reader = [pscustomobject]@{ tab_counts = @(10, 10); pane_positions = [pscustomobject]@{
            panes = @(
                [pscustomobject]@{ item = 2; repository = $archiveRepo; discussion_anchor = '00000000000000050009'; pull_request = $false }
                [pscustomobject]@{ item = 3; repository = 1; discussion_anchor = $null; pull_request = $true }
            )
        } }
    }
}
$position = Wait-PanePositions 'the exact archive and PR panes' {
    param($left, $right)
    $left.item -eq 2 -and $left.repository -eq 2 -and $left.discussion_anchor -ceq '00000000000000050009' -and
        !$left.pull_request -and $right.item -eq 3 -and $right.repository -eq 1 -and $right.pull_request
}
if ($script:positionReads -ne 3 -or $position.cached_reader.pane_positions.panes[0].repository -ne 2) {
    throw 'The pane predicate did not wait for the exact archive repository beside the original PR.'
}

$script:paneLayoutReads = 0
function Read-Observation {
    $script:paneLayoutReads++
    $title = if ($script:paneLayoutReads -gt 1) { 'a' * 64 } else { $null }
    [pscustomobject]@{
        bounds = [pscustomobject]@{}
        cached_reader = [pscustomobject]@{ offline = $true; signed_out = $true; pane_positions = [pscustomobject]@{
            panes = @([pscustomobject]@{ tab = 'archive' })
            layout = [pscustomobject]@{ settled = $true; strips = @([pscustomobject]@{
                active = 1
                tabs = @([pscustomobject]@{id = 'pr';title_sha256 = $title}, [pscustomobject]@{id = 'archive';title_sha256 = 'b' * 64})
            }) }
        } }
    }
}
$paneLayout = Wait-PaneLayout 'the restarted archive tab and its inactive PR title' {
    param($layout, $panes)
    (Test-PaneLayoutOrder $layout.strips[0] @('pr', 'archive')) -and $layout.strips[0].active -eq 1 -and $panes[0].tab -ceq 'archive'
}
if ($script:paneLayoutReads -ne 3 -or (Test-PaneLayoutOrder $paneLayout.cached_reader.pane_positions.layout.strips[0] @('archive', 'pr'))) {
    throw 'The pane lifecycle predicate accepted missing titles or a reordered strip.'
}

$script:viewportReads = 0
function Read-Observation {
    $script:viewportReads++
    $geometry = @{ 'pr-0-conversation-records' = @(0, 0, 100, 100); 'pr-1-context-content' = @(101, 0, 100, 100) }
    if ($script:viewportReads -gt 1) { $geometry['reader-conversation-0'] = @(0, 0, 100, 100) }
    [pscustomobject]@{
        bounds = [pscustomobject]$geometry
        cached_reader = [pscustomobject]@{
            shared_pr_panes = @(
                [pscustomobject]@{ item = 1; conversation_offset = 0; view = [pscustomobject]@{ section = 'Conversation' } }
                [pscustomobject]@{ item = 1; files_context_offset = 156; view = [pscustomobject]@{ section = 'Files' } }
            )
        }
    }
}
$leftViewport = Wait-SharedPrViewport 1 $false
if ($script:viewportReads -ne 3 -or $leftViewport.id -cne 'reader-conversation-0' -or $leftViewport.field -cne 'conversation_offset') {
    throw 'The shared PR driver selected formal review records or acted before the tracked PR discussion appeared.'
}
$rightViewport = Wait-SharedPrViewport 1 $true
if ($rightViewport.id -cne 'pr-1-context-content' -or $rightViewport.field -cne 'files_context_offset') {
    throw 'The shared PR driver lost the exact Files context after moving its reading position.'
}

$mixedOfflineBefore = [pscustomobject]@{
    fixture_revision = 's12-offline-mixed-outbox-v1'
    account = 1; offline = $true; offline_pins = 1; content_bytes = 1000; discussion_records = 10
    tabs = @(10, 10); drafts = @('issue', 'comment', 'review', 'reply'); pending_dependents = 3
    actions = @(0..3 | ForEach-Object {
        [pscustomobject]@{
            id = "action-$_"; kind = @('Comment', 'EditIssue', 'Review', 'Merge')[$_]; target = @(1, ($_ + 1))
            item = $_ + 1; snapshot_sha256 = "$_"; dependencies = @('action-0')
            state = 'Queued'; cancel_requested = $false; attempts = 0
        }
    })
}
$mixedOfflineAfter = $mixedOfflineBefore | ConvertTo-Json -Depth 8 | ConvertFrom-Json
$mixedOfflineAfter.content_bytes = 250
$mixedOfflineAfter.discussion_records = 1
$mixedOfflineAfter.actions[0].state = 'Cancelled'
$mixedOfflineAfter.actions[0].cancel_requested = $true
Assert-OfflineOutboxCacheResult $mixedOfflineBefore $mixedOfflineAfter
$mixedOfflineAfter.actions[2].snapshot_sha256 = 'changed'
$refused = $false
try { Assert-OfflineOutboxCacheResult $mixedOfflineBefore $mixedOfflineAfter } catch { $refused = $_.Exception.Message -like '*immutable snapshot*' }
if (!$refused) { throw 'The offline Outbox predicate accepted a rewritten submitted review after cache clearing.' }

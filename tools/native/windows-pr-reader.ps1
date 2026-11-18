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
    $id = Wait-For 'the native control for the exact fixture repository and PR number' {
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
        Find-FixturePrChoice $choices $expectedName
    }
    if (!(Invoke-OwnedFixtureControl $Window $id $null $expectedName)) {
        Stop-Blocked 'The verified pull request control is missing a native activation pattern.'
    }
}

function Open-FixturePr([IntPtr]$Window, [int]$Pane, [int]$Number, [string]$Title, [string]$Head, [string]$Path) {
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
        param($pr) $pr.selected_file -ceq $Path -and $pr.patch_change_lines -eq 2
    }
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

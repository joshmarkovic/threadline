function Wait-History([string]$Description, [scriptblock]$HistoryPredicate) {
    Wait-Reader $Description {
        param($reader)
        $property = $reader.PSObject.Properties['history']
        $null -ne $property -and $null -ne $property.Value -and (& $HistoryPredicate $property.Value $reader)
    }
}

function Wait-OwnedHistoryLabel([IntPtr]$Window, [string]$Id, [string]$Name) {
    try {
        Wait-For "the owned visible history label '$Name'" {
            $null -ne (Find-OwnedFixtureControl $Window $Id $Name -AllowDisabled)
        }
    } catch {
        if ($_.Exception.Message -notlike 'Timed out waiting*') { throw }
        $failure = $_
        $observed = [ordered]@{ automation_id = $Id; visible = $false; returned_name = $null; control_type = $null }
        try {
            $node = Find-OwnedFixtureControl $Window $Id -AllowDisabled
            if ($null -ne $node) {
                $observed.visible = $true
                $current = $node.Current
                if ($current.Name -cmatch '^(Pull request comparison|Parent [0-9]+ of |Direct comparison|Showing saved tip |The remote reference was removed|Branch main deleted|Not downloaded)') {
                    $observed.returned_name = $current.Name
                }
                if ($current.ControlType -is [Windows.Automation.ControlType]) {
                    $observed.control_type = $current.ControlType.ProgrammaticName
                }
            }
        } catch {}
        Add-Evidence 'Inspect only the expected owned comparison status control' $observed
        throw $failure
    }
}

function Wait-HistoryUnsubmittedDraft([string]$Draft, [string]$Head, $Item) {
    $review = Wait-ExactFixtureReview $Draft $Head
    if ($review.cached_reader.active_item -ne $Item -or $review.cached_reader.outbox -ne 0 -or
        $review.cached_reader.pull_request.review_draft.queued -or
        !$review.cached_reader.pull_request.review_draft.summary_nonempty) {
        throw 'The original pull request did not retain its saved, unsubmitted draft.'
    }
}

function Invoke-HistoryProof([IntPtr]$Window, [string]$Profile) {
    $head = 'b' * 40
    $firstParent = 'a' * 40
    $secondParent = 'c' * 40
    $separator = ' ' + [char]0x00b7 + ' '
    $arrow = ' ' + [char]0x2192 + ' '
    Wait-SeededFixtureAccount
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    $pr = Open-FixturePr $Window 0 42 'First fixture pull request' $head 'src/first.rs'
    $prItem = $pr.cached_reader.active_item
    Click-AccountControl $Window 'reader-maximize-pane'
    $null = Wait-PrReader 'the immutable PR comparison has the provider merge base' {
        param($pr) $pr.head -ceq $head -and $pr.merge_base -ceq $secondParent
    }
    Wait-OwnedHistoryLabel $Window 'pr-0-comparison-mode' "Pull request comparison${separator}merge base $secondParent${arrow}head $head"
    $draft = Enter-FixtureAnchoredReview $Window $head 'src/first.rs' -AlreadyMaximized
    Wait-HistoryUnsubmittedDraft $draft $head $prItem
    Add-Evidence 'Open the labeled PR merge-base comparison and save an unsent inline draft' ([ordered]@{
        base = $firstParent; merge_base = $secondParent; head = $head; file = 'src/first.rs'; draft = $draft
    })

    Set-AccessPhase 'history'
    Click-AccountControl $Window 'reader-history'
    Click-AccountControl $Window 'history-fetch-refs'
    $null = Wait-History 'the real branch and tag index' { param($history) $history.refs -eq 2 }
    Wait-For 'select the exact owned main branch' {
        Invoke-OwnedFixtureControl $Window 'history-ref-0' $null 'Branch main'
    }
    Wait-For 'verify that the inspected main branch is watched' {
        if ($null -ne (Find-OwnedFixtureControl $Window 'history-monitor-branch' 'Monitoring default branch' -AllowDisabled)) {
            return $true
        }
        if ($null -ne (Find-OwnedFixtureControl $Window 'history-monitor-branch' 'Stop monitoring branch')) {
            return $true
        }
        $null = Invoke-OwnedFixtureControl $Window 'history-monitor-branch' $null 'Monitor this branch'
        return $false
    }
    Click-AccountControl $Window 'history-fetch-older'
    $initial = Wait-History 'one downloaded merge commit and two pending ancestry links' {
        param($history) $history.commits -eq 1 -and $history.coverage -ceq 'Partial' -and $history.unloaded_parents -eq 2
    }
    Click-AccountControl $Window 'history-fetch-older'
    $extended = Wait-History 'expand the prior bounded ancestry with the same immutable tip' {
        param($history)
        $history.repository -eq $initial.cached_reader.history.repository -and
            $history.commits -eq 3 -and $history.coverage -ceq 'Complete' -and $history.unloaded_parents -eq 0
    }
    Add-Evidence 'Read the bounded graph before and after the native older-history action' ([ordered]@{
        initial_rows = $initial.cached_reader.history.commits; initial_boundaries = 2
        expanded_rows = $extended.cached_reader.history.commits; expanded_boundaries = 0
    })

    Click-AccountControl $Window 'history-commit-0'
    $null = Wait-History 'the selected merge commit reports its first parent' {
        param($history) $history.selected -ceq $head -and $history.parent -eq 1 -and $history.comparison_base -ceq $firstParent
    }
    Click-AccountControl $Window 'history-parent-1'
    $null = Wait-History 'the explicit second parent and original merge commit' {
        param($history)
        $history.comparison -ceq 'Parent' -and $history.parent -eq 2 -and $history.comparison_base -ceq $secondParent -and
            $history.comparison_head -ceq $head -and $history.comparison_coverage -ceq 'Missing' -and $history.files -eq 0
    }
    Wait-OwnedHistoryLabel $Window 'history-comparison-mode' "Parent 2 of 2${separator}$secondParent${arrow}$head"
    Wait-OwnedHistoryLabel $Window 'history-comparison-coverage' 'Not downloaded'
    Click-AccountControl $Window 'history-fetch-comparison'
    $null = Wait-History 'the requested second parent returned one exact file patch' {
        param($history) $history.parent -eq 2 -and $history.files -eq 1
    }
    Click-AccountControl $Window 'history-file-0'
    $null = Wait-History 'show the native second-parent file diff' {
        param($history) $history.displayed_file -ceq 'parent-two.txt' -and $history.comparison_base -ceq $secondParent
    }
    Add-Evidence 'Choose the actual second merge parent and open its file changes' ([ordered]@{
        base = $secondParent; head = $head; parent_order = 2; file = 'parent-two.txt'
    })

    Click-AccountControl $Window 'history-compare-1'
    $null = Wait-History 'the explicit two-commit comparison' {
        param($history)
        $history.comparison -ceq 'Pair' -and $history.comparison_base -ceq $firstParent -and $history.comparison_head -ceq $head
    }
    Wait-OwnedHistoryLabel $Window 'history-comparison-mode' "Direct comparison${separator}$firstParent${arrow}$head"
    Click-AccountControl $Window 'history-fetch-comparison'
    $null = Wait-History 'one file for the exact two-commit comparison' {
        param($history) $history.comparison -ceq 'Pair' -and $history.files -eq 1
    }
    Click-AccountControl $Window 'history-file-0'
    $null = Wait-History 'the native direct comparison file' { param($history) $history.displayed_file -ceq 'pair.txt' }
    Click-AccountControl $Window 'history-commit-2'
    $null = Wait-History 'the independent second parent is now selected' {
        param($history) $history.selected -ceq $secondParent
    }
    Click-AccountControl $Window 'history-compare-1'
    $null = Wait-History 'the exact endpoints of a divergent two-commit comparison' {
        param($history)
        $history.comparison -ceq 'Pair' -and $history.comparison_base -ceq $firstParent -and
            $history.comparison_head -ceq $secondParent
    }
    Click-AccountControl $Window 'history-fetch-comparison'
    $null = Wait-History 'the exact trees and available blobs produced one complete endpoint patch' {
        param($history) $history.comparison_coverage -ceq 'Complete' -and $history.files -eq 1
    }
    Click-AccountControl $Window 'history-file-0'
    $null = Wait-History 'the divergent endpoint file is rendered' {
        param($history) $history.displayed_file -ceq 'divergent.txt'
    }
    Add-Evidence 'Compare the two independent commits with exact cached Git objects' ([ordered]@{
        base = $firstParent; head = $secondParent; file = 'divergent.txt'
    })

    Set-AccessPhase 'history-moved'
    Click-AccountControl $Window 'history-fetch-refs'
    Wait-OwnedHistoryLabel $Window 'history-inspected-ref-status' "Showing saved tip $head. Select this branch or tag again to view its new tip $secondParent."
    Set-AccessPhase 'history-deleted'
    Click-AccountControl $Window 'history-fetch-refs'
    Wait-OwnedHistoryLabel $Window 'history-ref-deleted-0' 'Branch main deleted'
    Wait-OwnedHistoryLabel $Window 'history-inspected-ref-status' "The remote reference was removed. Its saved commits at $head remain available."
    $null = Wait-History 'the original commits and pinned comparison still displayed after branch deletion' {
        param($history)
        $history.commits -eq 3 -and $history.coverage -ceq 'Complete' -and $history.selected -ceq $secondParent -and
            $history.comparison_base -ceq $firstParent -and $history.comparison_head -ceq $secondParent -and
            $history.displayed_file -ceq 'divergent.txt'
    }
    Click-AccountControl $Window 'history-back'
    Click-AccountControl $Window 'reader-tab-0-0'
    Wait-HistoryUnsubmittedDraft $draft $head $prItem
    Add-Evidence 'The watched branch moves and disappears while the saved file selection and native draft survive' ([ordered]@{
        reference = 'main'; inspected_tip = $head; updated_tip = $secondParent; deleted = $true
        file = 'divergent.txt'; draft = $draft; queued = $false
    })

    Set-WriteFixtureOffline $Window $true
    $restarted = Restart-Reader $Profile
    Wait-HistoryUnsubmittedDraft $draft $head $prItem
    Click-AccountControl $restarted 'reader-repository-1'
    Click-AccountControl $restarted 'reader-history'
    $null = Wait-History 'the same cached comparison, file, and branch ancestry on offline restart' {
        param($history, $reader)
        $reader.offline -and $history.commits -eq 3 -and $history.selected -ceq $secondParent -and
            $history.comparison -ceq 'Pair' -and $history.comparison_base -ceq $firstParent -and
            $history.comparison_head -ceq $secondParent -and $history.displayed_file -ceq 'divergent.txt'
    }
    Wait-OwnedHistoryLabel $restarted 'history-ref-deleted-0' 'Branch main deleted'
    Add-Evidence 'Preserve the exact two-commit comparison and selected file offline after restart' ([ordered]@{
        base = $firstParent; head = $secondParent; file = 'divergent.txt'; offline = $true
    })
    Stop-ThroughUi
    $script:report.detail = 'Native history verified the three comparison labels and preserved cached commits, the selected divergent file and an unsent anchored PR draft through branch movement, deletion and an offline restart.'
}

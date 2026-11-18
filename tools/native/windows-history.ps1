function Wait-History([string]$Description, [scriptblock]$Check) {
    Wait-Reader $Description {
        param($reader)
        $property = $reader.PSObject.Properties['history']
        $null -ne $property -and $null -ne $property.Value -and (& $Check $property.Value $reader)
    }
}

function Invoke-HistoryProof([IntPtr]$Window, [string]$Profile) {
    $head = 'b' * 40
    $firstParent = 'a' * 40
    $secondParent = 'c' * 40
    Wait-SeededFixtureAccount
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-history'
    Click-AccountControl $Window 'history-fetch-refs'
    $null = Wait-History 'the real branch and tag index' { param($history) $history.refs -eq 2 }
    Click-AccountControl $Window 'history-ref-0'
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
            $history.comparison_head -ceq $head
    }
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
    Click-AccountControl $Window 'history-fetch-comparison'
    $null = Wait-History 'one file for the exact two-commit comparison' {
        param($history) $history.comparison -ceq 'Pair' -and $history.files -eq 1
    }
    Click-AccountControl $Window 'history-file-0'
    $null = Wait-History 'the native direct comparison file' { param($history) $history.displayed_file -ceq 'pair.txt' }
    Set-WriteFixtureOffline $Window $true
    $restarted = Restart-Reader $Profile
    Click-AccountControl $restarted 'reader-repository-1'
    Click-AccountControl $restarted 'reader-history'
    $null = Wait-History 'the same cached comparison, file, and branch ancestry on offline restart' {
        param($history, $reader)
        $reader.offline -and $history.commits -eq 3 -and $history.selected -ceq $head -and
            $history.comparison -ceq 'Pair' -and $history.comparison_base -ceq $firstParent -and
            $history.comparison_head -ceq $head -and $history.displayed_file -ceq 'pair.txt'
    }
    Add-Evidence 'Preserve the exact two-commit comparison and selected file offline after restart' ([ordered]@{
        base = $firstParent; head = $head; file = 'pair.txt'; offline = $true
    })
    Stop-ThroughUi
    $script:report.detail = 'Native history followed the exact immutable continuation, opened a labeled merge parent and restored a separate exact two-commit comparison after an offline restart.'
}

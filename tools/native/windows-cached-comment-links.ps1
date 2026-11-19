function Open-CachedCommentResult([IntPtr]$Window, $Target, [string]$Kind) {
    $bullet = ' ' + [char]0x00b7 + ' '
    $label = "$Kind fixture/relay #$($Target.number)$bullet$($Target.title)"
    Wait-For "the unique cached $Kind search result" {
        $observation = Read-Observation
        if ($null -eq $observation) { return $false }
        $candidates = @(
            foreach ($result in $observation.bounds.PSObject.Properties.Name) {
                if ($result -cmatch '^reader-result-[0-9]+$' -and
                    $null -ne (Find-OwnedFixtureControl $Window $result $label)) {
                    $result
                }
            }
        )
        if ($candidates.Count -gt 1) { throw "Duplicate native $Kind search results." }
        $candidates.Count -eq 1 -and (Invoke-OwnedFixtureControl $Window $candidates[0] $null $label)
    }
    $keys = @($Target.comments | ForEach-Object {
        $number = ([long]$_).ToString('D20', [Globalization.CultureInfo]::InvariantCulture)
        if ($Kind -ceq 'Pull request') { "issue-comment:$number" } else { $number }
    })
    return Wait-Reader "the exact cached $Kind comment selected in the original pane" {
        param($reader)
        $reader.focused_pane -eq 0 -and $reader.active_item -eq $Target.item -and $reader.offline -and
            $null -ne $reader.pane_positions -and $reader.pane_positions.panes[0].discussion_anchor -cin $keys
    }
}

function Invoke-CachedCommentUrl([IntPtr]$Window, [string]$Url) {
    $name = 'GitHub issue or pull request URL'
    Wait-For 'the owned workspace URL editor' {
        Set-OwnedFixtureEditValue $Window 'reader-url' $name $Url
    }
    Wait-For 'the exact synthetic GitHub URL in the workspace field' {
        $pattern = Get-OwnedFixtureEditPattern $Window 'reader-url' $name
        $null -ne $pattern -and $pattern.Current.Value -ceq $Url
    }
    Wait-For 'the owned Open URL action' {
        Invoke-OwnedFixtureControl $Window 'reader-open-url' $null 'Open URL'
    }
}

function Invoke-CachedCommentLinksProof([IntPtr]$Window) {
    $fixture = $script:CachedCommentLinksSeed.comment_links
    $issue = $fixture.issue
    $pull = $fixture.pull_request
    $editor = $fixture.editor
    $null = Wait-Reader 'the two restored offline workspace panes' {
        param($reader)
        $reader.offline -and $reader.pane_count -eq 2 -and $reader.cached_visible_panes -eq 2 -and
            $null -ne $reader.pane_positions -and $reader.pane_positions.panes.Count -eq 2
    }
    Click-AccountControl $Window 'reader-tab-1-0'
    Click-AccountControl $Window 'reader-draft-comment-1'
    $typed = 'Keep this unsent edit in the independent pane.'
    Wait-For 'the owned right-pane Markdown editor' {
        Set-OwnedFixtureEditValue $Window 'reader-draft-input-1' 'Local Markdown draft' $typed
    }
    $old = Wait-Reader 'the durable right-pane editor, original tab, and existing anchor' {
        param($reader)
        $pane = $reader.pane_positions.panes[1]
        $reader.active_item -eq $editor.item -and $reader.active_draft_saved -and
            $pane.tab -ceq $editor.tab -and $pane.discussion_anchor -ceq $editor.anchor -and
            $null -ne $pane.saved_draft_sha256
    }
    $right = $old.cached_reader.pane_positions.panes[1]
    Click-AccountControl $Window 'reader-tab-0-0'
    $search = Invoke-HistoricalLocalSearch $Window $fixture.query
    if ($search.cached_reader.search_results -ne 2) {
        throw 'Two ordinary comments per item must yield one cached issue result and one cached PR result.'
    }
    $issueHit = Open-CachedCommentResult $Window $issue 'Issue'
    foreach ($control in @('pr-0-review-open','pr-0-merge-open')) {
        if ($null -ne (Find-OwnedFixtureControl $Window $control $null -AllowDisabled)) {
            throw 'An ordinary issue exposed a PR-only action.'
        }
    }
    $null = Wait-AccountControl 'reader-draft-comment-0'
    $pullHit = Open-CachedCommentResult $Window $pull 'Pull request'
    $null = Wait-PrReader 'the linked ordinary PR comment opened Conversation' {
        param($pr,$reader)
        $reader.active_item -eq $pull.item -and $pr.section -ceq 'Conversation' -and
            $null -eq $pr.linked_review_comment -and $reader.comment_count -gt 0
    }
    Add-Evidence 'Search four ordinary comments and open exactly two distinct items at downloaded comments' ([ordered]@{
        issue = $issue.number; issue_anchor = $issueHit.cached_reader.pane_positions.panes[0].discussion_anchor
        pull_request = $pull.number; pull_anchor = $pullHit.cached_reader.pane_positions.panes[0].discussion_anchor
        ordinary_results = 2; issue_review_merge_controls_absent = $true
    })

    Invoke-CachedCommentUrl $Window "https://github.com/fixture/relay/issues/$($issue.number)#issuecomment-$($issue.comments[1])"
    $second = ([long]$issue.comments[1]).ToString('D20', [Globalization.CultureInfo]::InvariantCulture)
    $null = Wait-Reader 'the existing issue tab shows the exact pasted ordinary comment' {
        param($reader)
        $reader.active_item -eq $issue.item -and $reader.pane_positions.panes[0].tab -ceq 'comment-links-issue' -and
            $reader.pane_positions.panes[0].discussion_anchor -ceq $second
    }
    Invoke-CachedCommentUrl $Window "https://github.com/fixture/relay/pull/$($pull.number)#discussion_r$($pull.review_reply)"
    $linked = Wait-PrReader 'the exact cached review reply and original file placement' {
        param($pr,$reader)
        $reader.active_item -eq $pull.item -and $pr.section -ceq 'Conversation' -and
            $reader.pane_positions.panes[0].tab -ceq 'comment-links-pull' -and
            $pr.linked_review_comment -eq $pull.review_reply -and
            $pr.revision_id -eq $pull.revision -and $pr.head -ceq $pull.head -and
            $null -ne $pr.linked_review_source -and $pr.linked_review_source.path -ceq $pull.path -and
            $pr.linked_review_source.side -ceq 'Right' -and $pr.linked_review_source.line -eq $pull.line
    }
    Wait-For 'the owned linked-review comment group' {
        $null -ne (Find-OwnedFixtureControl $Window 'pr-0-linked-review-comment' 'Linked review comment' -AllowDisabled)
    }
    $final = $linked.cached_reader
    $later = $final.pane_positions.panes[1]
    if (($final.tab_counts -join ',') -cne '2,1' -or $later.tab -cne $right.tab -or
        $later.item -ne $right.item -or $later.saved_draft_sha256 -cne $right.saved_draft_sha256 -or
        $later.discussion_anchor -cne $right.discussion_anchor -or $later.discussion_offset -ne $right.discussion_offset) {
        throw 'Opening comment links changed the independent tab, editor, or discussion position.'
    }
    Click-AccountControl $Window 'reader-tab-1-0'
    Wait-For 'the untouched native Markdown editor retains its original text' {
        $pattern = Get-OwnedFixtureEditPattern $Window 'reader-draft-input-1' 'Local Markdown draft'
        $null -ne $pattern -and $pattern.Current.Value -ceq $typed
    }
    Add-Evidence 'Open separate cached GitHub comment URLs and preserve the other pane' ([ordered]@{
        ordinary_comment = $issue.comments[1]; review_reply = $pull.review_reply
        source = $final.pull_request.linked_review_source; revision = $pull.revision
        unrelated_tab = $right.tab; unrelated_anchor = $right.discussion_anchor
        unchanged_draft = $true; unchanged_view = $true
    })
    Stop-ThroughUi
    $script:report.detail = 'Offline Windows search opened one issue and one PR without duplicate hits, pasted links kept exact ordinary and review comment identities, and the other pane retained its editor and discussion position.'
}

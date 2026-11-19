function Confirm-FixtureThreadAnnotation([IntPtr]$Window, [long]$Item, [string]$Mode, [string]$Side) {
    $expected = if ($Side -ceq 'Left') {
        @{ line=9; source='Original'; node='PRRT_LEFT'; author='fixture-left'; reply='Downloaded deletion-side reply.' }
    } else {
        @{ line=17; source='Modified'; node='PRRT_RIGHT'; author='fixture-right'; reply='Downloaded right-side reply.' }
    }
    Click-AccountControl $Window 'pr-0-next-unresolved-thread'
    $null = Wait-PrReader "the next unresolved $Side thread in $Mode" {
        param($pr,$reader)
        $reader.active_item -eq $Item -and $reader.active_number -eq 42 -and $pr.diff_mode -ceq $Mode -and
            $pr.head -ceq ('b' * 40) -and $pr.merge_base -ceq ('c' * 40) -and $pr.selected_file -ceq 'src/first.rs' -and
            $null -ne $pr.source_anchor -and $pr.source_anchor.path -ceq 'src/first.rs' -and
            $pr.source_anchor.side -ceq $Side -and $pr.source_anchor.line -eq $expected.line
    }
    $id = 'pr-0-annotation-thread:' + $expected.node
    Wait-For "the visible $Side thread and its downloaded reply in $Mode" {
        $control = Find-OwnedFixtureControl $Window $id $null -AllowDisabled
        try {
            if ($null -eq $control) { return $false }
            $name = $control.Current.Name
            $name.StartsWith("Review on $($expected.source) line $($expected.line). Unresolved") -and
                $name.Contains($expected.author) -and $name.Contains('Reply from fixture-reply') -and
                $name.Contains($expected.reply)
        } catch [Windows.Automation.ElementNotAvailableException] { $false }
    }
    Add-Evidence 'Read the owned inline review group after native unresolved-thread navigation' ([ordered]@{
        mode = $Mode; side = $Side; line = $expected.line; thread = $expected.node; downloaded_reply_visible = $true
    })
}

function Invoke-ReviewThreadPlacementProof([IntPtr]$Window) {
    Wait-SeededFixtureAccount
    Click-AccountControl $Window 'reader-repository-1'
    Click-AccountControl $Window 'reader-fetch-repository'
    $first = Open-FixturePr $Window 0 42 'First fixture pull request' ('b' * 40) 'src/first.rs'
    $item = $first.cached_reader.active_item
    Click-AccountControl $Window 'reader-maximize-pane'
    Click-AccountControl $Window 'pr-0-conversation'
    Click-AccountControl $Window 'pr-0-threads-fetch'
    $null = Wait-PrReader 'all four fixture review threads are downloaded' {
        param($pr,$reader) $reader.active_item -eq $item -and $pr.threads_coverage -ceq 'Complete' -and $pr.threads_on_page -eq 4
    }
    Click-AccountControl $Window 'pr-0-files'
    $open = Wait-PrReader 'the same selected file in the ordinary Files view' {
        param($pr) $pr.section -ceq 'Files' -and $pr.selected_file -ceq 'src/first.rs'
    }
    if (!$open.cached_reader.pull_request.context_collapsed) {
        Click-AccountControl $Window 'pr-0-context-collapse'
    }
    foreach ($mode in @('Unified','Split')) {
        Click-AccountControl $Window ("pr-0-diff-" + $mode.ToLowerInvariant())
        $null = Wait-PrReader "the ordinary $mode view" {
            param($pr) $pr.diff_mode -ceq $mode -and $pr.context_collapsed
        }
        Confirm-FixtureThreadAnnotation $Window $item $mode 'Left'
        Confirm-FixtureThreadAnnotation $Window $item $mode 'Right'
        Wait-For "the $mode fallback for downloaded threads without matching source lines" {
            $label = 'Review threads without a matching downloaded source line are available in Conversation.'
            $null -ne (Find-OwnedFixtureControl $Window 'pr-0-thread-placement' $label -AllowDisabled)
        }
        foreach ($node in @('PRRT_FILE','PRRT_OUTDATED')) {
            if ($null -ne (Find-OwnedFixtureControl $Window ("pr-0-annotation-thread:" + $node) $null -AllowDisabled)) {
                throw "An unplaceable thread appeared inline in $mode."
            }
        }
        Add-Evidence 'Keep file-level and outdated threads in Conversation' ([ordered]@{
            mode = $mode; completed_threads = 4; fallback_visible = $true
        })
    }
    Stop-ThroughUi
    $script:report.detail = 'The Windows Files view navigated to the same exact deletion-side and modified-side threads in Unified and Split, with downloaded replies and a Conversation fallback for file-level and outdated threads.'
}

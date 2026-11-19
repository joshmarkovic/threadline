function Initialize-SelectionObservation {
    $native = Join-Path (Split-Path -Parent $PSScriptRoot) 'native'
    Add-Type -Path (Join-Path $native 'ThreadlineWinInput.cs')
    Add-Type -Path (Join-Path $native 'ThreadlineSidebarPixels.cs') -ReferencedAssemblies System.Drawing
    $null = [ThreadlineWinInput]::SetProcessDpiAwarenessContext([IntPtr](-4))
}

# Sample the installed window's two source rows; keep just their background colors.
function Get-FixtureLineColors {
    $hwnd = $child.MainWindowHandle
    $root = Get-OwnedWindow $child
    if (!$root -or $root.Current.ProcessId -ne $child.Id -or [IntPtr]$root.Current.NativeWindowHandle -ne $hwnd) { return }
    $client = New-Object ThreadlineWinInput+Rect
    $origin = New-Object ThreadlineWinInput+Point
    if (![ThreadlineWinInput]::GetClientRect($hwnd,[ref]$client) -or ![ThreadlineWinInput]::ClientToScreen($hwnd,[ref]$origin)) { return }
    $image = [ThreadlineSidebarPixels]::Capture($hwnd, $child.Id, $client.Right, $client.Bottom)
    try {
        $colors = @()
        foreach ($text in @('old fixture line','new fixture line')) {
            $name = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, $text)
            $nodes = $root.FindAll([Windows.Automation.TreeScope]::Descendants, $name)
            if ($nodes.Count -ne 1 -or $nodes[0].Current.ProcessId -ne $child.Id -or $nodes[0].Current.IsOffscreen) { return }
            $r = $nodes[0].Current.BoundingRectangle
            $x = [int][Math]::Ceiling($r.X-$origin.X)
            $y = [int][Math]::Ceiling($r.Y-$origin.Y)
            $w = [int][Math]::Floor($r.Width)-2
            $h = [int][Math]::Floor($r.Height)-2
            if ($x -lt 0 -or $y -lt 0 -or $w -lt 12 -or $h -lt 4 -or $x+$w -ge $image.Width -or $y+$h -ge $image.Height) { return }
            $hist = @{}
            for ($i=1;$i -le 20;$i++) {
                for ($j=1;$j -le 7;$j++) {
                    $pixel = $image.GetPixel($x+[int]($w*$i/21),$y+[int]($h*$j/8)).ToArgb()
                    $hist[$pixel] = 1 + [int]$hist[$pixel]
                }
            }
            $m = $hist.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First 1
            if ($m.Value -lt 71) { return }
            $colors += $m.Key
        }
        if ($colors[0] -ne $colors[1]) { return [pscustomobject]@{left=$colors[0];right=$colors[1]} }
    } finally { $image.Dispose() }
}
function Read-FixtureLineColors {
    try { return (Get-FixtureLineColors) }
    catch [System.Windows.Automation.ElementNotAvailableException] { return }
    catch [System.Management.Automation.MethodInvocationException] { return }
}
function Wait-FixtureLineColors([scriptblock]$Condition) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $latest = $null
    do {
        $current = Read-FixtureLineColors
        if ($current) {
            $latest = $current
            if (& $Condition $current) { return $current }
        }
        Start-Sleep -Milliseconds 120
    } while ($clock.ElapsedMilliseconds -lt 2400)
    return $latest
}
function Wait-SelectionState([scriptblock]$Test,[string]$Description) {
    $clock=[Diagnostics.Stopwatch]::StartNew()
    while ($clock.ElapsedMilliseconds -lt 16000) {
        $state=Read-Fixture inspect-runtime-selection
        if (& $Test $state) { return $state }
        Start-Sleep -Milliseconds 120
    }
    throw "The installed app did not persist $Description."
}
function Test-InstalledDiffSelection {
    $original=Read-Fixture inspect-runtime-selection
    if ($original.drafts -ne 0 -or $null -ne $original.source_selection -or $original.active -ne 1 -or $original.path -cne 'second.txt' -or
        $original.base -cne ('a'*40) -or $original.head -cne ('b'*40)) { throw 'The fresh comparison is not the expected unselected fixture.' }
    $before=Wait-FixtureLineColors { param($colors) $null -ne $colors }
    Invoke-Control 'pr-0-next-change'
    $pre=Wait-SelectionState { param($s) $s.revision -eq $original.revision -and $s.source_selection -and $s.source_selection.path -ceq 'second.txt' -and
        $s.source_selection.side -ceq 'Right' -and $s.source_selection.line -eq 1 -and $s.source_selection.original_oid -ceq $original.head } 'the modified source line before leaving Files'
    $report.original_selection=$pre.source_selection
    $report.comparison_revision=$pre.revision
    $report.comparison_base=$pre.base
    $report.comparison_head=$pre.head
    $selected=Wait-FixtureLineColors { param($colors) $before -and $colors.left -eq $before.left -and $colors.right -ne $before.right }
    Invoke-Control 'pr-0-commits'
    $null=Wait-SelectionState { param($s) $s.section -ceq 'Commits' -and $s.revision -eq $original.revision -and $s.drafts -eq 0 } 'Commits on the same comparison'
    Invoke-Control 'pr-0-files'
    $null=Wait-Control 'pr-0-review-add-inline' 'Comment on selected source lines'
    $null=Wait-SelectionState { param($s) $s.section -ceq 'Files' -and $s.path -ceq $pre.path -and $s.drafts -eq 0 } 'the original file before composing'
    $restored=Wait-FixtureLineColors { param($colors) $selected -and $colors.left -eq $selected.left -and $colors.right -eq $selected.right }
    $report.live_highlight='Blocked'
    if ($before -and $selected -and $before.left -eq $selected.left -and $before.right -ne $selected.right -and $restored -and $restored.left -eq $selected.left) {
        $report.live_highlight_samples=@($before,$selected,$restored)
        $report.live_highlight = if ($restored.right -eq $selected.right) { 'Pass' } else { 'Fail' }
    }
    Invoke-Control 'pr-0-review-add-inline'
    $label='second.txt '+[char]0x00b7+' modified 1'+[char]0x2013+'1'
    $null=Wait-Control 'pr-0-review-anchor-0' $label
    $saved=Wait-SelectionState { param($s)
        if ($s.revision -ne $pre.revision -or $s.drafts -ne 1 -or !$s.draft -or $s.draft.anchors.Count -ne 1) { return $false }
        $a=$s.draft.anchors[0]
        return $s.draft.id -ceq $s.draft.tab_draft -and $a.path -ceq $pre.path -and $a.source_path -ceq $pre.path -and
            $a.side -ceq 'Right' -and $a.first_line -eq 1 -and $a.last_line -eq 1 -and $a.source_oid -ceq $pre.head
    } 'one new draft bound to the original comparison, path, side and line'
    $report.new_draft=$saved.draft
    $report.visible_anchor=$label
    $report.draft_anchor='Pass'
}

$fixtureTitle = 'Threadline review'
$fixtureBody = 'Open the sample pull request in Threadline.'
$rawView = [Windows.Automation.TreeWalker]::RawViewWalker

function Get-FixtureCondition([string]$Text, [int]$ProcessId) {
    $visible = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::IsOffscreenProperty, $false)
    $owner = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty, $ProcessId)
    $name = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::NameProperty, $Text)
    return [Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]@($visible, $owner, $name))
}

function Get-CombinedFixtureCondition([int]$ProcessId) {
    $conditions = New-Object 'System.Collections.Generic.List[Windows.Automation.Condition]'
    foreach ($separator in @(' ', ', ', '. ', '  ', "`t", "`n", "`r`n", "`n`n", "`r`n`r`n", "`n ", "`r`n ")) {
        $pair = $fixtureTitle + $separator + $fixtureBody
        $variants = @($pair)
        if (Get-Variable fixtureAppDisplayName -ErrorAction SilentlyContinue) {
            $variants += @("$fixtureAppDisplayName, $pair", "${fixtureAppDisplayName}: $pair", "Notification from ${fixtureAppDisplayName}: $pair")
        }
        foreach ($exact in $variants) {
            foreach ($property in @([Windows.Automation.AutomationElement]::NameProperty, [Windows.Automation.AutomationElement]::HelpTextProperty)) {
                $conditions.Add([Windows.Automation.PropertyCondition]::new($property, $exact))
            }
        }
    }
    $pair = [Windows.Automation.OrCondition]::new($conditions.ToArray())
    $visible = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::IsOffscreenProperty, $false)
    $owner = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty, $ProcessId)
    return [Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]@($visible, $owner, $pair))
}

function Get-SignedCenterRoot($Native) {
    if ($null -eq $Native) { return $null }
    $process = Get-Process -Id $Native.ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $process -or $process.SessionId -ne $report.session_id -or !$process.Path) { return $null }
    $identity = "$($process.Id):$($process.StartTime.Ticks)"
    if (!$signedShells.ContainsKey($identity)) {
        $signature = Get-AuthenticodeSignature -LiteralPath $process.Path
        $signedShells[$identity] = $signature.Status -eq 'Valid' -and $null -ne $signature.SignerCertificate -and
            $signature.SignerCertificate.Subject -match '(?:^|,\s*)O=Microsoft Corporation(?:,|$)'
    }
    if (!$signedShells[$identity]) { return $null }
    $root = [Windows.Automation.AutomationElement]::FromHandle($Native.Handle)
    if ($null -eq $root -or $root.Current.ProcessId -ne $Native.ProcessId -or $root.Current.IsOffscreen) { return $null }
    if ($Native.ClassName -ceq 'Windows.UI.NotificationCenterWindow') { return $root }
    $scope = [Windows.Automation.TreeScope]::Subtree
    foreach ($label in @('Notification Center', 'Notification Center Window', 'Notifications and calendar')) {
        if ($null -ne $root.FindFirst($scope, (Get-FixtureCondition $label $Native.ProcessId))) { return $root }
    }
    if ($null -ne $root.FindFirst($scope, (Get-FixtureCondition 'Notifications' $Native.ProcessId)) -and
        $null -ne $root.FindFirst($scope, (Get-FixtureCondition 'Calendar' $Native.ProcessId))) { return $root }
    return $null
}

function Get-CenterFixtureHit($Root, $Native, $Stats) {
    $scope = [Windows.Automation.TreeScope]::Subtree
    $titles = $Root.FindAll($scope, (Get-FixtureCondition $fixtureTitle $Native.ProcessId))
    $bodies = $Root.FindAll($scope, (Get-FixtureCondition $fixtureBody $Native.ProcessId))
    $combined = $Root.FindAll($scope, (Get-CombinedFixtureCondition $Native.ProcessId))
    $Stats.title_matches = [Math]::Max($Stats.title_matches, $titles.Count)
    $Stats.body_matches = [Math]::Max($Stats.body_matches, $bodies.Count)
    $Stats.combined_matches = [Math]::Max($Stats.combined_matches, $combined.Count)
    if ($titles.Count -gt 1 -or $bodies.Count -gt 1 -or $combined.Count -gt 1) { return $null }
    if (Test-FixturePair $titles.Count $bodies.Count) {
        $target = $titles[0]
        $labels = @($titles[0], $bodies[0])
        $card = Get-FixtureCard $target $bodies[0] $Root $Native.ProcessId
        if ($null -ne $card -and $combined.Count -eq 1 -and !(Test-InElement $combined[0] $card $Native.ProcessId) -and
            !(Test-InElement $card $combined[0] $Native.ProcessId)) { return $null }
    } elseif ($combined.Count -eq 1) {
        $target = $combined[0]
        $card = $target
        $labels = @()
    } else { return $null }
    if ($null -eq $card -or (Get-ElementIdentity $card) -ceq (Get-ElementIdentity $Root)) { return $null }
    $card = Get-VerifiedCardWrapper $card $Root $Native.ProcessId
    if ($null -eq $card) {
        $Stats.card_role_misses++
        return $null
    }
    if (!$card.Current.IsEnabled -or $card.Current.IsOffscreen) { return $null }
    $Stats.verified_card_reads++
    if ($Stats.card_structure.Count -eq 0) { $Stats.card_structure = @(Get-FixtureCardStructure $card $Native.ProcessId) }
    $buttonType = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ControlTypeProperty,
        [Windows.Automation.ControlType]::Button)
    $enabled = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::IsEnabledProperty, $true)
    $exactButton = [Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]@(
        (Get-FixtureCondition $fixtureActionLabel $Native.ProcessId), $buttonType, $enabled))
    $buttons = $card.FindAll($scope, $exactButton)
    $Stats.action_matches = [Math]::Max($Stats.action_matches, $buttons.Count)
    if ($buttons.Count -gt 1) { return $null }
    $point = $null
    $pointSource = ''
    $selection = $card
    $kind = 'default card'
    if ($buttons.Count -eq 1) {
        $selection = $buttons[0]
        if (!(Test-ExactFixtureAction $selection $card $Native.ProcessId $fixtureActionLabel)) { return $null }
        $kind = 'Open review button'
        $point = Get-FixturePoint $selection $card $Native
        $pointSource = Get-ElementIdentity $selection
    } else {
        $expands = Test-PatternFlag $card ([Windows.Automation.AutomationElement]::IsExpandCollapsePatternAvailableProperty)
        $toggles = Test-PatternFlag $card ([Windows.Automation.AutomationElement]::IsTogglePatternAvailableProperty)
        if (!(Test-DefaultCardWrapper $card $expands $toggles {
            param($Element)
            $buttonIdentity = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::AutomationIdProperty, 'ExpandButton')
            $owner = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty, [int]$Native.ProcessId)
            $condition = [Windows.Automation.AndCondition]::new([Windows.Automation.Condition[]]@($buttonIdentity, $buttonType, $owner))
            $expansion = $Element.FindAll($scope, $condition)
            return $expansion.Count -eq 1 -and (Test-CardWithAuxiliaryExpand $Element $labels $expansion[0] $Native.ProcessId)
        })) {
            $Stats.default_wrapper_rejections++
            return $null
        }
        if ($expands) { $Stats.auxiliary_expand_reads++ }
        foreach ($label in $labels) {
            if ($expands) { break }
            $point = Get-DefaultFixturePoint $label $card $Native
            if ($null -ne $point) {
                $pointSource = Get-ElementIdentity $label
                $Stats.default_points++
                break
            }
        }
    }
    $canInvoke = Test-PatternFlag $selection ([Windows.Automation.AutomationElement]::IsInvokePatternAvailableProperty)
    if ($kind -ceq 'default card' -and $canInvoke) { $Stats.default_invoke_reads++ }
    return [pscustomobject]@{
        Window = $Native; Identity = (Get-ElementIdentity $card); Selection = $selection
        Kind = $kind; CanInvoke = $canInvoke; Point = $point; PointSource = $pointSource
    }
}

function Test-PatternFlag($Element, $Property) {
    $value = $Element.GetCurrentPropertyValue($Property, $true)
    return $value -is [bool] -and $value
}

function Get-FixtureCardStructure($Card, [uint32]$Owner) {
    $pending = [Collections.Generic.Queue[object]]::new()
    $pending.Enqueue(@{ Node = $Card; Depth = 0 })
    $visited = 0
    while ($pending.Count -gt 0 -and $visited -lt 20) {
        $entry = $pending.Dequeue()
        $visited++
        $node = $entry.Node
        $info = $node.Current
        if ($info.ProcessId -ne $Owner) { continue }
        [pscustomobject]@{
            depth = $entry.Depth; role = $info.ControlType.ProgrammaticName
            class = if ($info.ClassName -cmatch '^[\w.\-]{1,80}$') { $info.ClassName } else { $null }
            automation_id = if ($info.AutomationId -cmatch '^[\w.\-]{1,80}$') { $info.AutomationId } else { $null }
            invoke = Test-PatternFlag $node ([Windows.Automation.AutomationElement]::IsInvokePatternAvailableProperty)
            expandable = Test-PatternFlag $node ([Windows.Automation.AutomationElement]::IsExpandCollapsePatternAvailableProperty)
            toggle = Test-PatternFlag $node ([Windows.Automation.AutomationElement]::IsTogglePatternAvailableProperty)
            enabled = $info.IsEnabled; offscreen = $info.IsOffscreen
        }
        if ($entry.Depth -ge 3) { continue }
        $child = $rawView.GetFirstChild($node)
        while ($null -ne $child -and $pending.Count + $visited -lt 20) {
            $pending.Enqueue(@{ Node = $child; Depth = $entry.Depth + 1 })
            $child = $rawView.GetNextSibling($child)
        }
    }
}

function Test-NotificationCardRole([string]$Role, [string]$Class) {
    if ($Role -cin @('ControlType.ListItem', 'ControlType.Button')) { return $true }
    return $Role -ceq 'ControlType.Group' -and $Class -cmatch '(?:^|\.)(?:Toast|Notification)Card$'
}

function Get-VerifiedCardWrapper($Content, $Root, [uint32]$Owner) {
    $rootIdentity = Get-ElementIdentity $Root
    $ancestor = $Content
    for ($depth = 0; $depth -lt 7 -and $null -ne $ancestor; $depth++) {
        $info = $ancestor.Current
        if ($info.ProcessId -ne $Owner -or (Get-ElementIdentity $ancestor) -ceq $rootIdentity) { return $null }
        $role = $info.ControlType.ProgrammaticName
        if ($role -cin @('ControlType.List', 'ControlType.Pane', 'ControlType.Window')) { return $null }
        if (Test-NotificationCardRole $role $info.ClassName) { return $ancestor }
        $ancestor = $rawView.GetParent($ancestor)
    }
    return $null
}

function Test-ExactFixtureAction($Button, $Card, [uint32]$Owner, [string]$Expected) {
    return $Expected -cmatch '^Open review [0-9a-f]{8}$' -and $Button.Current.Name -ceq $Expected -and
        (Test-InElement $Button $Card $Owner)
}

function Test-DefaultCardWrapper($Card, [bool]$Expands, [bool]$Toggles, [scriptblock]$HasAuxiliaryExpand) {
    $info = $Card.Current
    $dedicated = 'dismiss|expand|collapse|toggle|more(?:options|actions|button|menu)|menu|close|settings'
    if ($Toggles -or $info.ClassName -match $dedicated -or $info.AutomationId -match $dedicated) { return $false }
    return !$Expands -or (& $HasAuxiliaryExpand $Card)
}

function Test-CardWithAuxiliaryExpand($Card, $Labels, $ExpandButton, [uint32]$Owner) {
    $info = $Card.Current
    if ($info.ControlType.ProgrammaticName -cne 'ControlType.ListItem' -or $info.ClassName -cne 'ListViewItem' -or
        $Labels.Count -ne 2 -or (Get-ElementIdentity $ExpandButton) -ceq (Get-ElementIdentity $Card)) { return $false }
    $expand = $ExpandButton.Current
    if ($expand.ControlType.ProgrammaticName -cne 'ControlType.Button' -or $expand.AutomationId -cne 'ExpandButton' -or
        !(Test-WithinFixtureCard $ExpandButton $Card $Owner)) { return $false }
    foreach ($index in 0, 1) {
        $label = $Labels[$index]
        $expectedId = @('Title', 'Content')[$index]
        if ($label.Current.ControlType.ProgrammaticName -cne 'ControlType.Text' -or $label.Current.ClassName -cne 'TextBlock' -or
            $label.Current.AutomationId -cne $expectedId -or !(Test-WithinFixtureCard $label $Card $Owner)) { return $false }
    }
    return $true
}

function Test-WithinFixtureCard($Element, $Card, [uint32]$Owner) {
    if ($Element.Current.ProcessId -ne $Owner) { return $false }
    $cardId = Get-ElementIdentity $Card
    $ancestor = $rawView.GetParent($Element)
    for ($depth = 0; $null -ne $ancestor -and $depth -lt 16; $depth++) {
        $info = $ancestor.Current
        if ($info.ProcessId -ne $Owner) { return $false }
        if ((Get-ElementIdentity $ancestor) -ceq $cardId) { return $true }
        if (Test-NotificationCardRole $info.ControlType.ProgrammaticName $info.ClassName) { return $false }
        $ancestor = $rawView.GetParent($ancestor)
    }
    return $false
}

function Test-DefaultFixtureContent($Label, $Hit, $Card, [uint32]$Owner, [scriptblock]$IsInteractive) {
    if (!(Test-InElement $Label $Card $Owner) -or !(Test-InElement $Hit $Card $Owner)) { return $false }
    if (!(Test-InElement $Label $Hit $Owner) -and !(Test-InElement $Hit $Label $Owner)) { return $false }
    $cardId = Get-ElementIdentity $Card
    foreach ($element in @($Label, $Hit)) {
        $node = $element
        for ($depth = 0; $null -ne $node -and $depth -lt 16; $depth++) {
            if ((Get-ElementIdentity $node) -ceq $cardId) { break }
            $info = $node.Current
            if ($info.ControlType.ProgrammaticName -cnotin @('ControlType.Text', 'ControlType.Group', 'ControlType.Pane', 'ControlType.Custom') -or
                $info.ClassName -match 'dismiss|more(?:options|button|menu)|expand|collapse|close|settings|button' -or
                $info.AutomationId -match 'dismiss|more(?:options|button|menu)|expand|collapse|close|settings|button' -or
                (& $IsInteractive $node)) { return $false }
            $node = $rawView.GetParent($node)
        }
        if ($null -eq $node -or (Get-ElementIdentity $node) -cne $cardId) { return $false }
    }
    return $true
}

function Get-DefaultFixturePoint($Label, $Card, $Window) {
    $bounds = $Label.Current.BoundingRectangle
    if ($bounds.IsEmpty -or $bounds.Width -lt 1 -or $bounds.Height -lt 1) { return $null }
    $point = New-Object ThreadlineWinInput+Point
    $point.X = [int][Math]::Floor($bounds.X + $bounds.Width / 2)
    $point.Y = [int][Math]::Floor($bounds.Y + $bounds.Height / 2)
    if (![ThreadlineWinInput]::BannerOwnsPoint($point, $Window)) { return $null }
    $hit = [Windows.Automation.AutomationElement]::FromPoint([System.Windows.Point]::new($point.X, $point.Y))
    if ($null -eq $hit) { return $null }
    $safe = Test-DefaultFixtureContent $Label $hit $Card $Window.ProcessId {
        param($Element)
        foreach ($property in @(
            [Windows.Automation.AutomationElement]::IsInvokePatternAvailableProperty,
            [Windows.Automation.AutomationElement]::IsTogglePatternAvailableProperty,
            [Windows.Automation.AutomationElement]::IsExpandCollapsePatternAvailableProperty,
            [Windows.Automation.AutomationElement]::IsSelectionItemPatternAvailableProperty)) {
            if (Test-PatternFlag $Element $property) { return $true }
        }
        return $false
    }
    if (!$safe) { return $null }
    return $point
}

function Invoke-FixtureCenterClick([IntPtr]$AppWindow) {
    $signedShells = @{}
    $foreground = [ThreadlineWinInput]::GetForegroundWindow()
    if ($foreground -ne $AppWindow) { Stop-Blocked 'The verified fixture app was not foreground before Win+N.' }
    foreach ($prior in [ThreadlineWinInput]::ShellBannerWindows()) {
        if ($null -ne (Get-SignedCenterRoot $prior)) { Stop-Blocked 'Notification Center was already open. The driver left it in place.' }
    }

    $center = $null
    $stats = [ordered]@{ polls = 0; foreground_shell_reads = 0; verified_center_reads = 0; title_matches = 0; body_matches = 0; combined_matches = 0; card_role_misses = 0; verified_card_reads = 0; action_matches = 0; default_wrapper_rejections = 0; auxiliary_expand_reads = 0; default_points = 0; default_invoke_reads = 0; card_structure = @(); click_attempts = 0; invoke_attempts = 0; elapsed_ms = 0 }
    $clock = [Diagnostics.Stopwatch]::StartNew()
    [ThreadlineWinInput]::Chord(@(0x5b, 0x4e))
    Add-Evidence 'Open Windows Notification Center once with Win+N' $true
    try {
        while ($clock.ElapsedMilliseconds -lt 5200) {
            $stats.polls++
            $native = [ThreadlineWinInput]::ShellWindow([ThreadlineWinInput]::GetForegroundWindow())
            if ($null -ne $native) {
                $stats.foreground_shell_reads++
                try {
                    $root = Get-SignedCenterRoot $native
                    if ($null -ne $root) {
                        $stats.verified_center_reads++
                        if ($null -eq $center) { $center = $native }
                        if (![ThreadlineWinInput]::SameBanner($center, $native)) { return $false }
                        $first = Get-CenterFixtureHit $root $native $stats
                        if ($null -ne $first) {
                            $preferInvoke = $first.Kind -ceq 'default card' -and $first.CanInvoke
                            $pointerPrepared = !$preferInvoke -and $null -ne $first.Point -and [ThreadlineWinInput]::MoveToBanner($first.Point, $first.Window)
                            $checked = $clock.ElapsedMilliseconds
                            $sameNative = [ThreadlineWinInput]::ShellWindow([ThreadlineWinInput]::GetForegroundWindow())
                            if ([ThreadlineWinInput]::SameBanner($native, $sameNative)) {
                                $sameRoot = Get-SignedCenterRoot $sameNative
                                if ($null -ne $sameRoot) {
                                    $second = Get-CenterFixtureHit $sameRoot $sameNative $stats
                                    if ($null -ne $second -and $clock.ElapsedMilliseconds - $checked -le 750 -and
                                        $first.Identity -ceq $second.Identity -and
                                        $first.Kind -ceq $second.Kind -and
                                        (Get-ElementIdentity $first.Selection) -ceq (Get-ElementIdentity $second.Selection)) {
                                        if ($pointerPrepared -and $first.PointSource -ceq $second.PointSource -and (Test-SameFixtureHit $first $second)) {
                                            $stats.click_attempts++
                                            if ([ThreadlineWinInput]::ClickPreparedBanner($second.Point, $second.Window)) {
                                                Add-Evidence 'Physically click the verified fixture target in Notification Center' ([ordered]@{ target = $second.Kind; owner_pid = $native.ProcessId; window_class = $native.ClassName })
                                                return $true
                                            }
                                        }
                                        $pattern = $null
                                        if ($second.CanInvoke -and [ThreadlineWinInput]::GetForegroundWindow() -eq $sameNative.Handle -and
                                            $second.Selection.TryGetCurrentPattern([Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
                                            $stats.invoke_attempts++
                                            try { $pattern.Invoke() }
                                            catch { Stop-Blocked 'Windows UI Automation returned an error invoking the verified fixture. No further input was issued.' }
                                            Add-Evidence 'Invoke the verified fixture target through Windows UI Automation' ([ordered]@{ target = $second.Kind; owner_pid = $native.ProcessId; window_class = $native.ClassName })
                                            return $true
                                        }
                                    }
                                }
                            }
                        }
                    }
                } catch [Windows.Automation.ElementNotAvailableException] { }
                catch [Runtime.InteropServices.COMException] { }
            }
            Start-Sleep -Milliseconds 90
        }
        return $false
    } finally {
        $stats.elapsed_ms = $clock.ElapsedMilliseconds
        Add-Evidence 'Search the verified Notification Center for the exact owned fixture' $stats
        if ($null -ne $center -and [ThreadlineWinInput]::SameBanner($center, [ThreadlineWinInput]::ShellWindow([ThreadlineWinInput]::GetForegroundWindow()))) {
            try {
                [ThreadlineWinInput]::TargetWindow = $center.Handle
                [ThreadlineWinInput]::Chord(@(0x1b))
                Add-Evidence 'Close the test-opened Notification Center through Escape' $true
            } catch {
                Add-Evidence 'Close the test-opened Notification Center through Escape' $false
            } finally { [ThreadlineWinInput]::TargetWindow = $AppWindow }
        }
    }
}

function Get-ElementIdentity($Element) {
    return ($Element.GetRuntimeId() -join '.')
}

function Test-FixturePair([int]$Titles, [int]$Bodies) {
    return $Titles -eq 1 -and $Bodies -eq 1
}

function Test-InElement($Element, $Container, [uint32]$Owner) {
    $identity = Get-ElementIdentity $Container
    $current = $Element
    for ($depth = 0; $null -ne $current -and $depth -lt 16; $depth++) {
        if ($current.Current.ProcessId -ne $Owner) { return $false }
        if ((Get-ElementIdentity $current) -ceq $identity) { return $true }
        $current = $rawView.GetParent($current)
    }
    return $false
}

function Get-FixtureCard($Title, $Body, $Root, [uint32]$Owner) {
    $rootIdentity = Get-ElementIdentity $Root
    $candidate = $Title
    for ($depth = 0; $null -ne $candidate -and $depth -lt 16; $depth++) {
        if ($candidate.Current.ProcessId -ne $Owner) { return $null }
        if (Test-InElement $Body $candidate $Owner) { return $candidate }
        if ((Get-ElementIdentity $candidate) -ceq $rootIdentity) { return $null }
        $candidate = $rawView.GetParent($candidate)
    }
    return $null
}

function Get-FixturePoint($Title, $Card, $Window) {
    $bounds = $Title.Current.BoundingRectangle
    if ($bounds.IsEmpty -or $bounds.Width -lt 1 -or $bounds.Height -lt 1) { return $null }
    $point = New-Object ThreadlineWinInput+Point
    $point.X = [int][Math]::Floor($bounds.X + $bounds.Width / 2)
    $point.Y = [int][Math]::Floor($bounds.Y + $bounds.Height / 2)
    if (![ThreadlineWinInput]::BannerOwnsPoint($point, $Window)) { return $null }
    $hit = [Windows.Automation.AutomationElement]::FromPoint([System.Windows.Point]::new($point.X, $point.Y))
    if ($null -eq $hit -or !(Test-InElement $hit $Card $Window.ProcessId)) { return $null }
    # A child click on a different button, such as Dismiss, does not count as a hit on this fixture.
    $hitID = Get-ElementIdentity $hit
    if ($hitID -cne (Get-ElementIdentity $Card) -and !(Test-InElement $hit $Title $Window.ProcessId)) { return $null }
    return $point
}

function Get-FocusedBannerWindow($Focus) {
    if ($null -eq $Focus) { return $null }
    $process = $Focus.Current.ProcessId
    if ($process -le 0) { return $null }
    $ancestor = $Focus
    for ($depth = 0; $null -ne $ancestor -and $depth -lt 16; $depth++) {
        if ($ancestor.Current.ProcessId -ne $process) { return $null }
        $hwnd = $ancestor.Current.NativeWindowHandle
        if ($hwnd -ne 0) { return [ThreadlineWinInput]::FocusedShellBanner([IntPtr]$hwnd, $process) }
        $ancestor = $rawView.GetParent($ancestor)
    }
    return $null
}

function Find-FixtureBanners($Stats, [switch]$Baseline) {
    $focus = [Windows.Automation.AutomationElement]::FocusedElement
    $focusedWindow = $null
    try { $focusedWindow = Get-FocusedBannerWindow $focus } catch [Windows.Automation.ElementNotAvailableException] { $Stats.stale_elements++ }
    $windows = @([ThreadlineWinInput]::ShellBannerWindows())
    if ($null -ne $focusedWindow -and $focusedWindow.Handle -notin @($windows | ForEach-Object { $_.Handle })) {
        $windows += $focusedWindow
        $Stats.focus_derived_windows++
    }
    foreach ($window in $windows) {
        try {
            $Stats.shell_windows++
            if ($window.ClassName -ceq 'tooltips_class32') {
                $Stats.classic_windows++
                if ([ThreadlineWinInput]::ClassicBannerMatches($window, $fixtureTitle, $fixtureBody)) {
                    $Stats.classic_pairs++
                    $point = New-Object ThreadlineWinInput+Point
                    $point.X = $window.Bounds.Left + [int][Math]::Floor(($window.Bounds.Right - $window.Bounds.Left) / 2)
                    $point.Y = $window.Bounds.Top + [int][Math]::Floor(($window.Bounds.Bottom - $window.Bounds.Top) / 2)
                    if (![ThreadlineWinInput]::BannerOwnsPoint($point, $window)) { $point = $null }
                    [pscustomobject]@{ Window = $window; Identity = 'classic'; Point = $point }
                    continue
                }
            }

            $root = [Windows.Automation.AutomationElement]::FromHandle($window.Handle)
            if ($null -eq $root -or $root.Current.ProcessId -ne $window.ProcessId) { continue }
            $titleCondition = Get-FixtureCondition $fixtureTitle $window.ProcessId
            $bodyCondition = Get-FixtureCondition $fixtureBody $window.ProcessId
            $scope = [Windows.Automation.TreeScope]::Subtree
            $titles = $root.FindAll($scope, $titleCondition)
            $bodies = $root.FindAll($scope, $bodyCondition)
            $combined = $root.FindAll($scope, (Get-CombinedFixtureCondition $window.ProcessId))
            $Stats.title_nodes += $titles.Count
            $Stats.body_nodes += $bodies.Count
            $Stats.combined_nodes += $combined.Count
            if ($combined.Count -gt 1 -or (($titles.Count -gt 1 -or $bodies.Count -gt 1) -and
                ($combined.Count -gt 0 -or ($titles.Count -gt 0 -and $bodies.Count -gt 0)))) {
                $Stats.ambiguous_pairs++
                [pscustomobject]@{ Window = $window; Identity = 'ambiguous'; Point = $null }
                continue
            }

            $card = $null
            $target = $null
            if (Test-FixturePair $titles.Count $bodies.Count) {
                $target = $titles[0]
                $card = Get-FixtureCard $target $bodies[0] $root $window.ProcessId
                if ($combined.Count -eq 1 -and ($null -eq $card -or (!
                    (Test-InElement $combined[0] $card $window.ProcessId) -and !
                    (Test-InElement $card $combined[0] $window.ProcessId)))) {
                    $Stats.ambiguous_pairs++
                    [pscustomobject]@{ Window = $window; Identity = 'ambiguous'; Point = $null }
                    continue
                }
            } elseif ($combined.Count -eq 1) {
                $target = $combined[0]
                $card = $target
            } else { continue }
            $Stats.uia_pairs++
            $point = $null
            if ($null -ne $card -and !$Baseline) {
                $exactToast = $window.ClassName -cin @('Windows.UI.NotificationToastWindow', 'ToastWnd', 'tooltips_class32')
                $focusedCard = $null -ne $focus -and (Test-InElement $focus $card $window.ProcessId)
                if ($focusedCard) { $Stats.focused_pairs++ }
                if ($exactToast -or ($focusedCard -and (Get-ElementIdentity $card) -cne (Get-ElementIdentity $root))) {
                    $point = Get-FixturePoint $target $card $window
                }
            }
            $identity = if ($null -eq $card) { 'unverified' } else { Get-ElementIdentity $card }
            [pscustomobject]@{ Window = $window; Identity = $identity; Point = $point }
        } catch [Windows.Automation.ElementNotAvailableException] {
            $Stats.stale_elements++
        } catch [Runtime.InteropServices.COMException] {
            $Stats.unavailable_elements++
        }
    }
}

function New-FixtureSearchStats {
    return [ordered]@{
        polls = 0; cycles = 0; shell_windows = 0; focus_derived_windows = 0; classic_windows = 0
        title_nodes = 0; body_nodes = 0; combined_nodes = 0
        classic_pairs = 0; uia_pairs = 0; ambiguous_pairs = 0; focused_pairs = 0
        stale_elements = 0; unavailable_elements = 0; expired_checks = 0; click_attempts = 0; elapsed_ms = 0
    }
}

function Test-SameFixtureHit($First, $Second) {
    return $null -ne $First.Point -and $null -ne $Second.Point -and
        $First.Identity -ceq $Second.Identity -and [ThreadlineWinInput]::SameBanner($First.Window, $Second.Window) -and
        $First.Point.X -eq $Second.Point.X -and $First.Point.Y -eq $Second.Point.Y
}

function Initialize-FixtureNotificationProof {
    $state = 0
    $result = [ThreadlineWinInput]::SHQueryUserNotificationState([ref]$state)
    $names = @('Unreported', 'NotPresent', 'Busy', 'FullScreenDirect3D', 'PresentationMode', 'AcceptsNotifications', 'QuietTime', 'WindowsStoreApp')
    $stateName = if ($result -eq 0 -and $state -in (1..7)) { $names[$state] } else { 'Unavailable' }
    Add-Evidence 'Read the Windows user notification state' ([ordered]@{ hresult = $result; state = $state; state_name = $stateName })
    $stats = New-FixtureSearchStats
    $existing = @(Find-FixtureBanners $stats -Baseline)
    Add-Evidence 'Check for a preexisting visible fixture notification' ([ordered]@{ exact_or_ambiguous_matches = $existing.Count; search = $stats })
    if ($existing.Count -ne 0) { Stop-Blocked 'A visible fixture notification already existed before launch of this proof.' }
}

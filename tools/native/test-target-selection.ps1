$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

foreach ($Name in @('windows-smoke.ps1', 'windows-app-toast.ps1', 'windows-notification-focus.ps1')) {
    $parseErrors = $null
    $parsed = [System.Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot $Name), [ref]$null, [ref]$parseErrors)
    if ($parseErrors.Count -ne 0) { throw ($parseErrors -join "`n") }
}
Add-Type -Path (Join-Path $PSScriptRoot 'ThreadlineWinInput.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'ThreadlineToastShortcut.cs')
foreach ($definition in $parsed.FindAll({
    param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
        $node.Name -in @('Test-FixturePair', 'Test-SameFixtureHit', 'Test-InElement', 'Get-FixtureCard', 'Get-ElementIdentity',
            'Test-ExactFixtureAction', 'Get-VerifiedCardWrapper', 'Test-NotificationCardRole', 'Test-DefaultFixtureContent',
            'Test-DefaultCardWrapper', 'Test-CardWithAuxiliaryExpand', 'Test-WithinFixtureCard')
}, $false)) {
    . ([scriptblock]::Create($definition.Extent.Text))
}

function Test-Assert([bool]$Condition, [string]$Description) {
    if (!$Condition) { throw $Description }
}

function New-TestNode([string]$Identity, [int]$Owner, $Parent, [string]$Name = '', [string]$Role = 'ControlType.Custom', [string]$Class = '', [string]$AutomationId = '') {
    $value = [pscustomobject]@{ ID = $Identity; Parent = $Parent; Current = [pscustomobject]@{
        ProcessId = $Owner; Name = $Name; ControlType = [pscustomobject]@{ ProgrammaticName = $Role }; ClassName = $Class; AutomationId = $AutomationId
    } }
    return $value | Add-Member -PassThru ScriptMethod GetRuntimeId { $this.ID }
}

$rawView = [pscustomobject]@{} | Add-Member -PassThru ScriptMethod GetParent { param($Node) $Node.Parent }
$root = New-TestNode root 42 $null
$card = New-TestNode card 42 $root '' 'ControlType.ListItem'
$card2 = New-TestNode card2 42 $root '' 'ControlType.ListItem'
$titleNode = New-TestNode title 42 $card
$bodyNode = New-TestNode body 42 $card
$focusNode = New-TestNode focus 42 $card
$otherNode = New-TestNode other 42 $card2
$wrongOwner = New-TestNode impostor 99 $card
Test-Assert ((Get-FixtureCard $titleNode $bodyNode $root 42).ID -ceq 'card') 'Select the nearest common fixture card.'
Test-Assert ((Get-FixtureCard $titleNode $otherNode $root 42).ID -ceq 'root') 'Separate cards share only the broad root, not a focused card.'
Test-Assert (Test-InElement $focusNode $card 42) 'Accept focus within the exact fixture card.'
Test-Assert (!(Test-InElement $otherNode $card 42)) 'Ignore focus in a sibling card.'
Test-Assert (!(Test-InElement $wrongOwner $card 42)) 'Reject a cross-owner child.'
Test-Assert (Test-FixturePair 1 1) 'Accept a unique title/body pair.'
Test-Assert (!(Test-FixturePair 2 1)) 'Reject duplicate title.'
Test-Assert (!(Test-FixturePair 1 2)) 'Reject duplicate body.'
Test-Assert (!(Test-FixturePair 1 0)) 'Reject a missing body.'

$actionLabel = 'Open review 01234567'
$action = New-TestNode fixtureAction 42 $card $actionLabel
$adjacentGeneric = New-TestNode adjacentGeneric 42 $card2 'Open review'
$adjacentExact = New-TestNode adjacentExact 42 $card2 $actionLabel
$otherOwnerAction = New-TestNode otherOwnerAction 99 $card $actionLabel
$rootAction = New-TestNode rootAction 42 $root $actionLabel
$sharedList = New-TestNode sharedList 42 $root '' 'ControlType.List'
$genericContent = New-TestNode genericContent 42 $sharedList
Test-Assert ((Get-VerifiedCardWrapper $titleNode $root 42).ID -ceq 'card') 'Bind through the nearest actual card role.'
Test-Assert ($null -eq (Get-VerifiedCardWrapper $genericContent $root 42)) 'Reject a generic list as a card.'
Test-Assert (Test-ExactFixtureAction $action $card 42 $actionLabel) 'Accept exactly the unique action in the fixture card.'
Test-Assert (!(Test-ExactFixtureAction $adjacentGeneric $card 42 $actionLabel)) 'Reject another card with a generic Open review action.'
Test-Assert (!(Test-ExactFixtureAction $adjacentExact $card 42 $actionLabel)) 'Reject an identical label in an adjacent card under the same root.'
Test-Assert (!(Test-ExactFixtureAction $rootAction $card 42 $actionLabel)) 'Reject an identical label under the shared root.'
Test-Assert (!(Test-ExactFixtureAction $otherOwnerAction $card 42 $actionLabel)) 'Reject a matching label in a different shell process.'

$textWrapper = New-TestNode textWrapper 42 $card '' 'ControlType.Group' 'TextContainer'
$exactText = New-TestNode exactText 42 $textWrapper 'Threadline review 01234567' 'ControlType.Text'
$dismiss = New-TestNode dismiss 42 $card '' 'ControlType.Button' 'NotificationDismissButton'
$siblingText = New-TestNode siblingText 42 $card '' 'ControlType.Text' 'TextBlock'
$adjacentText = New-TestNode adjacentText 42 $card2 'Threadline review 01234567' 'ControlType.Text'
$expand = New-TestNode expand 42 $card '' 'ControlType.Custom' '' 'ExpandNotification'
$insideExpand = New-TestNode insideExpand 42 $expand 'Threadline review 01234567' 'ControlType.Text'
$otherOwnerText = New-TestNode otherOwnerText 99 $card '' 'ControlType.Text'
$passive = { $false }
Test-Assert (Test-DefaultFixtureContent $exactText $exactText $card 42 $passive) 'Accept the exact title text hit.'
Test-Assert (Test-DefaultFixtureContent $exactText $textWrapper $card 42 $passive) 'Accept the exact title noninteractive wrapper.'
Test-Assert (Test-DefaultFixtureContent $exactText $card $card 42 $passive) 'Accept the verified card hit at its proven text point.'
Test-Assert (!(Test-DefaultFixtureContent $exactText $dismiss $card 42 $passive)) 'Reject a dismiss button in the same card.'
Test-Assert (!(Test-DefaultFixtureContent $exactText $siblingText $card 42 $passive)) 'Reject unrelated text in the same card.'
Test-Assert (!(Test-DefaultFixtureContent $exactText $adjacentText $card 42 $passive)) 'Reject identical text in the next card.'
Test-Assert (!(Test-DefaultFixtureContent $insideExpand $insideExpand $card 42 $passive)) 'Reject fixture text nested under an expand control.'
Test-Assert (!(Test-DefaultFixtureContent $exactText $otherOwnerText $card 42 $passive)) 'Reject a hit from a different process.'
Test-Assert (!(Test-DefaultFixtureContent $exactText $textWrapper $card 42 { param($Node) $Node.ID -eq 'textWrapper' })) 'Reject a text wrapper that advertises an interactive action.'

$expandWrapper = New-TestNode expandWrapper 42 $root '' 'ControlType.Button' '' 'ExpandNotification'
$expandTitle = New-TestNode expandTitle 42 $expandWrapper 'Threadline review 01234567' 'ControlType.Text'
$expandBody = New-TestNode expandBody 42 $expandWrapper 'Open the sample pull request in Threadline.' 'ControlType.Text'
$matchedContent = Get-FixtureCard $expandTitle $expandBody $root 42
$matchedWrapper = Get-VerifiedCardWrapper $matchedContent $root 42
Test-Assert ($matchedWrapper.ID -ceq 'expandWrapper') 'The exact pair can occur inside a dedicated Button wrapper.'
Test-Assert (!(Test-DefaultCardWrapper $matchedWrapper $true $false { $true })) 'Reject default activation on an expand wrapper containing both exact labels.'
Test-Assert (!(Test-DefaultCardWrapper $card $false $true $passive)) 'Reject an otherwise eligible wrapper when it exposes a Toggle pattern.'
Test-Assert (!(Test-DefaultCardWrapper (New-TestNode moreWrapper 42 $root '' 'ControlType.Button' 'MoreOptionsButton') $false $false $passive)) 'Reject a more-options wrapper by class.'
Test-Assert (Test-DefaultCardWrapper $card $false $false $passive) 'Keep default activation available on the actual passive fixture card.'

$observedCard = New-TestNode observedCard 42 $root '' 'ControlType.ListItem' 'ListViewItem'
$observedTitle = New-TestNode observedTitle 42 $observedCard 'Threadline review 01234567' 'ControlType.Text' 'TextBlock' 'Title'
$observedBody = New-TestNode observedBody 42 $observedCard 'Open the sample pull request in Threadline.' 'ControlType.Text' 'TextBlock' 'Content'
$observedExpand = New-TestNode observedExpand 42 $observedCard '' 'ControlType.Button' 'Button' 'ExpandButton'
$observedSettings = New-TestNode observedSettings 42 $observedCard '' 'ControlType.Button' 'Button' 'SettingsButton'
$observedDismiss = New-TestNode observedDismiss 42 $observedCard '' 'ControlType.Button' 'Button' 'DismissButton'
$observedLabels = @($observedTitle, $observedBody)
$adjacentExpand = New-TestNode adjacentExpand 42 $card2 '' 'ControlType.Button' 'Button' 'ExpandButton'
$auxiliary = { param($Candidate) Test-CardWithAuxiliaryExpand $Candidate $observedLabels $observedExpand 42 }
Test-Assert (Test-DefaultCardWrapper $observedCard $true $false $auxiliary) 'Accept the observed default ListViewItem with its distinct auxiliary ExpandButton.'
Test-Assert (!(Test-DefaultCardWrapper $observedExpand $true $false $auxiliary)) 'Do not invoke the dedicated child in place of the parent card.'
Test-Assert (!(Test-DefaultCardWrapper $observedSettings $false $false $passive)) 'Reject the distinct SettingsButton.'
Test-Assert (!(Test-DefaultCardWrapper $observedDismiss $false $false $passive)) 'Reject the distinct DismissButton.'
Test-Assert (!(Test-DefaultCardWrapper $observedCard $true $false $passive)) 'A generic ExpandCollapse pattern alone cannot enable default activation.'
Test-Assert (!(Test-CardWithAuxiliaryExpand $observedCard $observedLabels $adjacentExpand 42)) 'Ignore an identical ExpandButton in an adjacent card.'
Test-Assert (!(Test-CardWithAuxiliaryExpand $observedCard @($observedTitle, $siblingText) $observedExpand 42)) 'Require both exact text labels and their own card scope.'

$window = New-Object ThreadlineWinInput+BannerWindow
$window.Handle = [IntPtr]123
$window.ProcessId = 42
$window.ClassName = 'tooltips_class32'
$sameOwnerDifferentWindow = New-Object ThreadlineWinInput+BannerWindow
$sameOwnerDifferentWindow.Handle = [IntPtr]124
$sameOwnerDifferentWindow.ProcessId = 42
$sameOwnerDifferentWindow.ClassName = 'tooltips_class32'
$point = [pscustomobject]@{ X = 10; Y = 20 }
$first = [pscustomobject]@{ Window = $window; Identity = 'card'; Point = $point }
Test-Assert (Test-SameFixtureHit $first $first) 'Accept a repeated hit.'
Test-Assert (!(Test-SameFixtureHit $first ([pscustomobject]@{ Window = $window; Identity = 'other'; Point = $point }))) 'Reject a replaced card.'
Test-Assert (!(Test-SameFixtureHit $first ([pscustomobject]@{ Window = $sameOwnerDifferentWindow; Identity = 'card'; Point = $point }))) 'Reject a replaced HWND.'
Test-Assert (!(Test-SameFixtureHit $first ([pscustomobject]@{ Window = $window; Identity = 'card'; Point = @{ X = 11; Y = 20 } }))) 'Reject a moved click point.'
Test-Assert (!(Test-SameFixtureHit $first ([pscustomobject]@{ Window = $window; Identity = 'card'; Point = $null }))) 'Reject a vanished click point.'

$title = 'Threadline review'
$body = 'Open the sample pull request in Threadline.'
Test-Assert ([ThreadlineWinInput]::MatchesClassicBannerText("$title`r`n$body", $title, $body)) 'Accept exact classic title and body.'
Test-Assert (![ThreadlineWinInput]::MatchesClassicBannerText($body, $title, $body)) 'Reject a classic body without a title.'
Test-Assert (![ThreadlineWinInput]::MatchesClassicBannerText("$title`n$body`nother", $title, $body)) 'Reject extra text.'
Test-Assert (![ThreadlineWinInput]::MatchesClassicBannerText("$title`n$($body.ToLowerInvariant())", $title, $body)) 'Reject case differences.'
Write-Output 'PASS: PowerShell syntax, C# compilation, and synthetic fixture selection and revalidation.'

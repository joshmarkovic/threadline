function Invoke-SidebarBadgeProof([IntPtr]$Window) {
    $state = Wait-Reader 'the signed-out offline archive and its known Outbox total' {
        param($reader) $reader.offline -and $reader.signed_out -and $reader.outbox -eq 900
    }
    $dpi = [ThreadlineWinInput]::GetDpiForWindow($Window)
    $scale = $state.scale_factor
    if (!(Test-OwnedFixtureWindow $Window) -or $dpi -eq 0 -or [Math]::Abs(96 * $scale - $dpi) -gt 1) {
        Stop-Blocked 'The owned window and its display scale could not be verified.'
    }
    Add-Type -Path (Join-Path $PSScriptRoot 'ThreadlineSidebarPixels.cs') -ReferencedAssemblies System.Drawing
    $client = New-Object ThreadlineWinInput+Rect
    $origin = New-Object ThreadlineWinInput+Point
    if (![ThreadlineWinInput]::GetClientRect($Window, [ref]$client) -or
        ![ThreadlineWinInput]::ClientToScreen($Window, [ref]$origin)) { Stop-Blocked 'The owned client geometry is unavailable.' }
    $rows = foreach ($id in @('reader-review','reader-assigned','reader-outbox')) {
        $button = Find-OwnedFixtureControl $Window $id
        if ($null -eq $button) { Stop-Blocked "The visible owned $id rail button is unavailable." }
        $properties = $button.Current
        if ($properties.Name -notmatch '\. ([0-9]+) (complete cached count|known from partial downloads) for ') {
            throw "The $id button did not expose its cached count and coverage."
        }
        $bounds = $properties.BoundingRectangle
        $rect = [Drawing.Rectangle]::FromLTRB(
            [int][Math]::Ceiling($bounds.Left - $origin.X), [int][Math]::Ceiling($bounds.Top - $origin.Y),
            [int][Math]::Floor($bounds.Right - $origin.X), [int][Math]::Floor($bounds.Bottom - $origin.Y))
        [pscustomobject]@{ id = $id; count = [long]$Matches[1]; coverage = $Matches[2]; rect = $rect }
    }
    if (($rows | Where-Object { $_.id -ceq 'reader-outbox' }).count -ne 900) {
        throw 'The accessible Outbox count differs from the fixture.'
    }
    try { $image = [ThreadlineSidebarPixels]::Capture($Window, $script:app.Id, $client.Right, $client.Bottom) }
    catch { Stop-Blocked 'The Windows backend did not return the owned client image.' }
    try {
        $samples = foreach ($row in $rows) {
            $pixels = [ThreadlineSidebarPixels]::Measure($image, $row.rect)
            [pscustomobject]@{
                id = $row.id; total = $row.count; coverage = $row.coverage
                blue = $pixels.BluePixels; white_digits = $pixels.WhitePixels
                width_px = if ($pixels.BluePixels) { $pixels.Right - $pixels.Left } else { 0 }
                height_px = if ($pixels.BluePixels) { $pixels.Bottom - $pixels.Top } else { 0 }
                right_px = $pixels.Right; right_inset_px = $row.rect.Right - $pixels.Right
                digit_offset_x_px = $pixels.DigitCenterX; digit_offset_y_px = $pixels.DigitCenterY
            }
        }
    } finally { $image.Dispose() }
    $script:report.sidebar = [ordered]@{ dpi = $dpi; scale = $scale; rows = @($samples) }
    if (!($samples | Where-Object { $_.blue -gt 0 })) {
        Stop-Blocked 'PrintWindow did not expose the expected badge color from the owned GPU client.'
    }
    $right = $null
    foreach ($sample in $samples) {
        if ($sample.total -eq 0) {
            if ($sample.blue -ne 0) { throw 'A zero sidebar total painted a badge.' }
            continue
        }
        $expected = [Math]::Max(23, 13 + 7 * $sample.total.ToString().Length) * $scale
        $fill = $sample.blue / ($expected * $expected)
        if ([Math]::Abs($sample.width_px - $expected) -gt 2 -or [Math]::Abs($sample.height_px - $expected) -gt 2 -or
            [Math]::Abs($sample.height_px - $sample.width_px) -gt 1 -or $fill -lt 0.48 -or $fill -gt 0.84 -or
            $sample.white_digits -lt 2 -or [Math]::Abs($sample.digit_offset_x_px) -gt (2 * $scale) -or
            [Math]::Abs($sample.digit_offset_y_px) -gt (3 * $scale) -or
            $sample.right_inset_px -lt 0 -or $sample.right_inset_px -gt (16 * $scale) -or
            ($null -ne $right -and [Math]::Abs($right - $sample.right_px) -gt 1)) {
            throw "The owned $($sample.id) badge did not meet its size, fill, text or alignment requirements."
        }
        $right = $sample.right_px
    }
    Add-Evidence 'Measure cached count badges within the owned window at its reported native DPI' $script:report.sidebar
    Stop-ThroughUi
    $script:report.detail = 'Scoped native sidebar pixels and accessible counts verified.'
}

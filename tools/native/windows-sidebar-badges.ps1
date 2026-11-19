function Read-OwnedBadgeCompositor([IntPtr]$Window,[uint32]$Dpi,$Rows) {
    $helper = $env:THREADLINE_E2E_SIDEBAR_COMPOSITOR
    $pin = $env:THREADLINE_E2E_SIDEBAR_COMPOSITOR_SHA256
    if (!$helper -or $pin -notmatch '^[a-fA-F0-9]{64}$' -or !(Test-Path -LiteralPath $helper -PathType Leaf) -or
        (Get-FileHash -LiteralPath $helper -Algorithm SHA256).Hash -ine $pin) {
        Stop-Blocked 'The GPU client did not provide badge pixels through PrintWindow. Supply the pinned window-only compositor fixture.'
    }
    if (!(Test-OwnedFixtureWindow $Window)) { Stop-Blocked 'The sidebar window changed before compositor capture.' }
    $numbers = @($Window.ToInt64(),$script:app.Id,$Dpi)
    foreach ($row in $Rows) { $numbers += @($row.rect.Left,$row.rect.Top,$row.rect.Right,$row.rect.Bottom) }
    $start = [Diagnostics.ProcessStartInfo]::new()
    $start.FileName = (Get-Item -LiteralPath $helper).FullName
    $start.Arguments = $numbers -join ' '
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $child = [Diagnostics.Process]::Start($start)
    try {
        if (!$child.WaitForExit(12000)) {
            $child.Kill()
            $child.WaitForExit()
            Stop-Blocked 'The fixture compositor did not return a frame in time.'
        }
        $summary = $child.StandardOutput.ReadToEnd()
        $reason = $child.StandardError.ReadToEnd().Trim()
        if ($child.ExitCode -ne 0) { Stop-Blocked ("Window-only compositor unavailable. " + $reason) }
    } finally { $child.Dispose() }
    if (!(Test-OwnedFixtureWindow $Window) -or [ThreadlineWinInput]::GetDpiForWindow($Window) -ne $Dpi) {
        Stop-Blocked 'The fixture window or DPI changed during compositor capture.'
    }
    $measurement = $summary | ConvertFrom-Json
    if ($measurement.dpi -ne $Dpi -or @($measurement.rows).Count -ne 3) {
        throw 'The compositor returned an incomplete sidebar measurement.'
    }
    return $measurement
}

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
    $method = 'PrintWindow'
    $image = $null
    $measurements = @()
    try {
        try { $image = [ThreadlineSidebarPixels]::Capture($Window, $script:app.Id, $client.Right, $client.Bottom) }
        catch { $image = $null }
        if ($image) {
            $measurements = @(foreach ($row in $rows) { [ThreadlineSidebarPixels]::Measure($image, $row.rect) })
        }
    } finally { if ($image) { $image.Dispose() } }
    $printHadBadge = @($measurements | Where-Object { $_.BluePixels -gt 0 }).Count -gt 0
    if (!$printHadBadge) {
        $script:report.sidebar = [ordered]@{ dpi = $dpi; scale = $scale; capture = $method; print_window_badge = $false
            rows = @($rows | ForEach-Object { [pscustomobject]@{ id=$_.id; total=$_.count; coverage=$_.coverage } }) }
        $method = 'Windows Graphics Capture (CreateForWindow)'
        $frame = Read-OwnedBadgeCompositor $Window $dpi $rows
        $measurements = @($frame.rows)
    }
    $samples = @(for ($i=0; $i -lt $rows.Count; $i++) {
            $row = $rows[$i]
            $pixels = $measurements[$i]
            [pscustomobject]@{
                id = $row.id; total = $row.count; coverage = $row.coverage
                blue = $pixels.BluePixels; white_digits = $pixels.WhitePixels
                width_px = if ($pixels.BluePixels) { $pixels.Right - $pixels.Left } else { 0 }
                height_px = if ($pixels.BluePixels) { $pixels.Bottom - $pixels.Top } else { 0 }
                right_px = $pixels.Right; right_inset_px = $row.rect.Right - $pixels.Right
                digit_offset_x_px = $pixels.DigitCenterX; digit_offset_y_px = $pixels.DigitCenterY
            }
    })
    $script:report.sidebar = [ordered]@{ dpi = $dpi; scale = $scale; capture = $method
        print_window_badge = $printHadBadge; rows = $samples }
    if (!$printHadBadge) { $script:report.sidebar.compositor_sha256 = $env:THREADLINE_E2E_SIDEBAR_COMPOSITOR_SHA256 }
    if (!($samples | Where-Object { $_.blue -gt 0 })) {
        Stop-Blocked 'Neither client capture path exposed the expected badge color on the owned window.'
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

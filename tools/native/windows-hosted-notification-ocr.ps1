# Loaded only by the opt-in GitHub-hosted diagnostic. OCR strings and pixels stay in memory.

function Await-HostedOcrOperation($Operation, [type]$ResultType) {
    $task = $script:ocrAsTask.MakeGenericMethod(@($ResultType)).Invoke($null, @($Operation))
    if (!$task.Wait(750)) { throw 'The local WinRT image operation timed out.' }
    return $task.Result
}

function Initialize-HostedOcrEngine {
    Add-Type -AssemblyName System.Runtime.WindowsRuntime, System.Drawing
    $null = [Windows.Graphics.Imaging.BitmapDecoder, Windows.Graphics.Imaging, ContentType = WindowsRuntime]
    $null = [Windows.Graphics.Imaging.SoftwareBitmap, Windows.Graphics.Imaging, ContentType = WindowsRuntime]
    $null = [Windows.Graphics.Imaging.BitmapPixelFormat, Windows.Graphics.Imaging, ContentType = WindowsRuntime]
    $null = [Windows.Graphics.Imaging.BitmapAlphaMode, Windows.Graphics.Imaging, ContentType = WindowsRuntime]
    $null = [Windows.Globalization.Language, Windows.Globalization, ContentType = WindowsRuntime]
    $null = [Windows.Media.Ocr.OcrEngine, Windows.Media.Ocr, ContentType = WindowsRuntime]
    $null = [Windows.Media.Ocr.OcrResult, Windows.Media.Ocr, ContentType = WindowsRuntime]
    $script:ocrAsTask = [System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
        $_.Name -eq 'AsTask' -and $_.IsGenericMethodDefinition -and $_.GetParameters().Count -eq 1 -and
        $_.GetParameters()[0].ParameterType.Name -ceq 'IAsyncOperation`1'
    } | Select-Object -First 1
    $language = [Windows.Globalization.Language]::new('en-US')
    $script:ocrEngine = [Windows.Media.Ocr.OcrEngine]::TryCreateFromLanguage($language)
    if (!$script:ocrAsTask -or !$script:ocrEngine) { throw 'Local Windows en-US OCR is unavailable.' }
}

function Measure-OcrLine($Line) {
    $left = [double]::PositiveInfinity
    $top = [double]::PositiveInfinity
    $right = [double]::NegativeInfinity
    $bottom = [double]::NegativeInfinity
    foreach ($word in $Line.Words) {
        $rect = $word.BoundingRect
        $left = [Math]::Min($left, $rect.X)
        $top = [Math]::Min($top, $rect.Y)
        $right = [Math]::Max($right, $rect.X + $rect.Width)
        $bottom = [Math]::Max($bottom, $rect.Y + $rect.Height)
    }
    return [Drawing.RectangleF]::FromLTRB($left, $top, $right, $bottom)
}

function Find-FixtureTextInBitmap([Drawing.Bitmap]$Bitmap) {
    $memory = [IO.MemoryStream]::new()
    $randomAccess = $null
    $softwareBitmap = $null
    try {
        $Bitmap.Save($memory, [Drawing.Imaging.ImageFormat]::Png)
        $memory.Position = 0
        $randomAccess = [IO.WindowsRuntimeStreamExtensions]::AsRandomAccessStream($memory)
        $decoder = Await-HostedOcrOperation ([Windows.Graphics.Imaging.BitmapDecoder]::CreateAsync($randomAccess)) ([Windows.Graphics.Imaging.BitmapDecoder])
        $softwareBitmap = Await-HostedOcrOperation (
            $decoder.GetSoftwareBitmapAsync([Windows.Graphics.Imaging.BitmapPixelFormat]::Bgra8, [Windows.Graphics.Imaging.BitmapAlphaMode]::Ignore)
        ) ([Windows.Graphics.Imaging.SoftwareBitmap])
        $result = Await-HostedOcrOperation ($script:ocrEngine.RecognizeAsync($softwareBitmap)) ([Windows.Media.Ocr.OcrResult])
        $lines = @($result.Lines | Where-Object { @($_.Words).Count -gt 0 })
        $expectedBody = 'Open the sample pull request in Threadline.'
        $matches = [Collections.Generic.List[object]]::new()
        $fixtureTitleFound = $false
        $fixtureBodyFound = $false

        for ($line = 0; $line -lt $lines.Count; $line++) {
            $text = $lines[$line].Text.Trim()
            if ($text -ceq $expectedBody -or ($line + 1 -lt $lines.Count -and
                ($text + ' ' + $lines[$line + 1].Text.Trim()) -ceq $expectedBody)) {
                $fixtureBodyFound = $true
            }
        }

        for ($index = 0; $index -lt $lines.Count; $index++) {
            if ($lines[$index].Text.Trim() -cne 'Threadline review') { continue }
            $fixtureTitleFound = $true
            $title = Measure-OcrLine $lines[$index]
            $text = ''
            $previous = $title
            for ($line = $index + 1; $line -lt $lines.Count -and $line -le $index + 2; $line++) {
                $body = Measure-OcrLine $lines[$line]
                $gap = $body.Top - $previous.Bottom
                if ($body.Top -le $previous.Top -or $gap -gt 42 -or [Math]::Abs($body.Left - $title.Left) -gt 36) { break }
                $text = ($text + ' ' + $lines[$line].Text.Trim()).Trim()
                if (!$expectedBody.StartsWith($text, [StringComparison]::Ordinal)) { break }
                if ($text -ceq $expectedBody) {
                    $matches.Add(@{ Title = $title; Body = (Measure-OcrLine $lines[$index + 1]) })
                    break
                }
                $previous = $body
            }
        }
        return @{ ExactTitle = $fixtureTitleFound; ExactBody = $fixtureBodyFound; Matches = $matches }
    } finally {
        if ($null -ne $softwareBitmap) { $softwareBitmap.Dispose() }
        if ($null -ne $randomAccess) { $randomAccess.Dispose() }
        $memory.Dispose()
    }
}

function Test-HostedOcrEngine {
    $canvas = [Drawing.Bitmap]::new(500, 220)
    $graphics = [Drawing.Graphics]::FromImage($canvas)
    $font = [Drawing.Font]::new('Segoe UI', 20, [Drawing.GraphicsUnit]::Pixel)
    try {
        $graphics.Clear([Drawing.Color]::White)
        $graphics.DrawString('Threadline review', $font, [Drawing.Brushes]::Black, 12, 8)
        $graphics.DrawString('Open the sample pull request', $font, [Drawing.Brushes]::Black, 12, 40)
        $graphics.DrawString('in Threadline.', $font, [Drawing.Brushes]::Black, 12, 66)
        $positive = Find-FixtureTextInBitmap $canvas
        if (!$positive.ExactBody -or $positive.Matches.Count -ne 1) {
            throw 'Local OCR self-test did not recognize the complete wrapped fixture once.'
        }

        $graphics.DrawString('Threadline review', $font, [Drawing.Brushes]::Black, 12, 118)
        $graphics.DrawString('Open the sample pull request in Threadline.', $font, [Drawing.Brushes]::Black, 12, 150)
        $duplicate = Find-FixtureTextInBitmap $canvas
        if ($duplicate.Matches.Count -ne 2) { throw 'Local OCR self-test did not detect an ambiguous duplicate fixture.' }

        $graphics.Clear([Drawing.Color]::White)
        $graphics.DrawString('Threadline review', $font, [Drawing.Brushes]::Black, 12, 8)
        $graphics.DrawString('Open the sample draft request in Threadline.', $font, [Drawing.Brushes]::Black, 12, 40)
        $nearMiss = Find-FixtureTextInBitmap $canvas
        if (!$nearMiss.ExactTitle -or $nearMiss.ExactBody -or $nearMiss.Matches.Count -ne 0) {
            throw 'Local OCR self-test accepted the wrong body.'
        }
    } finally {
        $font.Dispose()
        $graphics.Dispose()
        $canvas.Dispose()
    }
}

function Read-HostedNotificationCorner($Corner) {
    if (!$script:hostedOcrCaptureEnabled) { throw 'Desktop capture is disabled outside verified hosted replay.' }
    $width = $Corner.Right - $Corner.Left
    $height = $Corner.Bottom - $Corner.Top
    if ($width -le 0 -or $width -gt 512 -or $height -le 0 -or $height -gt 300) { throw 'The notification corner exceeds the diagnostic bounds.' }
    $bitmap = [Drawing.Bitmap]::new($width, $height, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $capturedAt = [Diagnostics.Stopwatch]::GetTimestamp()
        $graphics.CopyFromScreen($Corner.Left, $Corner.Top, 0, 0, $bitmap.Size)
        $result = Find-FixtureTextInBitmap $bitmap
        $result.CapturedAt = $capturedAt
        return $result
    } finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

function Get-HostedToastHit($Corner, $Match) {
    $title = New-Object ThreadlineWinInput+Point
    $title.X = $Corner.Left + [int][Math]::Round($Match.Title.Left + $Match.Title.Width / 2)
    $title.Y = $Corner.Top + [int][Math]::Round($Match.Title.Top + $Match.Title.Height / 2)
    $body = New-Object ThreadlineWinInput+Point
    $body.X = $Corner.Left + [int][Math]::Round($Match.Body.Left + $Match.Body.Width / 2)
    $body.Y = $Corner.Top + [int][Math]::Round($Match.Body.Top + $Match.Body.Height / 2)
    $owner = [ThreadlineWinInput]::FindHostedToast($title)
    $bodyOwner = [ThreadlineWinInput]::FindHostedToast($body)
    if ($null -eq $owner -or $null -eq $bodyOwner -or $owner.Window -ne $bodyOwner.Window -or
        $owner.ProcessId -ne $bodyOwner.ProcessId) { return $null }
    return @{ Owner = $owner; Point = $body }
}

function Invoke-HostedOcrToastClick($Corner) {
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $deadline = [Diagnostics.Stopwatch]::GetTimestamp() + [long](2.2 * [Diagnostics.Stopwatch]::Frequency)
    $polls = 0
    $sawExactPair = $false
    while ([Diagnostics.Stopwatch]::GetTimestamp() -lt $deadline) {
        $polls++
        $first = Read-HostedNotificationCorner $Corner
        if ($first.Matches.Count -gt 1) { Stop-Blocked 'The notification corner contained multiple exact fixture matches.' }
        if ($first.Matches.Count -eq 1) {
            $sawExactPair = $true
            $hit = Get-HostedToastHit $Corner $first.Matches[0]
            if ($null -ne $hit) {
                if ([Diagnostics.Stopwatch]::GetTimestamp() -ge $deadline) { break }
                $second = Read-HostedNotificationCorner $Corner
                if ($second.Matches.Count -ne 1) { Stop-Blocked 'The exact fixture notification did not survive the required reread.' }
                $again = Get-HostedToastHit $Corner $second.Matches[0]
                if ($null -eq $again -or $again.Owner.Window -ne $hit.Owner.Window -or
                    $again.Owner.ProcessId -ne $hit.Owner.ProcessId -or
                    [Math]::Abs($again.Point.X - $hit.Point.X) -gt 3 -or [Math]::Abs($again.Point.Y - $hit.Point.Y) -gt 3) {
                    Stop-Blocked 'Notification contents and the native owning window did not remain stable on reread.'
                }
                if (![ThreadlineWinInput]::ClickHostedToast($again.Point, $again.Owner, $second.CapturedAt, $deadline)) {
                    Stop-Blocked 'The latest notification frame or overall polling deadline expired before input.'
                }
                Add-Evidence 'Re-read and click the uniquely matched synthetic notification in the same native shell window' (
                    [ordered]@{ polls = $polls; window_class = $again.Owner.WindowClass; elapsed_ms = $clock.ElapsedMilliseconds })
                return
            }
        }
        Start-Sleep -Milliseconds 85
    }
    Add-Evidence 'Check the bounded notification corner for a unique synthetic card and verified shell window' (
        [ordered]@{ polls = $polls; exact_fixture_pair = $sawExactPair; elapsed_ms = $clock.ElapsedMilliseconds })
    Stop-Blocked 'Local OCR did not establish a unique fresh notification with a verified native click target.'
}

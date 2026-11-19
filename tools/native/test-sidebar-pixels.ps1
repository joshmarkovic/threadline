[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'ThreadlineSidebarPixels.cs') -ReferencedAssemblies System.Drawing
$image = [Drawing.Bitmap]::new(180, 40)
$graphics = [Drawing.Graphics]::FromImage($image)
$blue = [Drawing.SolidBrush]::new([Drawing.Color]::FromArgb(23,62,112))
try {
    $graphics.Clear([Drawing.Color]::White)
    $bounds = [Drawing.Rectangle]::new(0,0,180,40)
    if ([ThreadlineSidebarPixels]::Measure($image,$bounds).BluePixels -ne 0) { throw 'False positive on a blank row.' }
    $graphics.FillEllipse($blue, 140, 5, 30, 30)
    $graphics.FillRectangle([Drawing.Brushes]::White, 150, 15, 10, 10)
    $sample = [ThreadlineSidebarPixels]::Measure($image,$bounds)
    if ($sample.BluePixels -lt 550 -or $sample.BluePixels -gt 640 -or $sample.WhitePixels -ne 100 -or
        [Math]::Abs($sample.Right-$sample.Left-30) -gt 1 -or [Math]::Abs($sample.Bottom-$sample.Top-30) -gt 1 -or
        [Math]::Abs($sample.DigitCenterX) -gt 1 -or [Math]::Abs($sample.DigitCenterY) -gt 1) {
        throw 'The synthetic badge, centered digit area or extent was measured incorrectly.'
    }
    $graphics.FillRectangle([Drawing.Brushes]::White, 120, 28, 60, 12)
    $short = [ThreadlineSidebarPixels]::Measure($image,$bounds)
    if ($short.Bottom -ne 28 -or $short.Bottom-$short.Top -ge $short.Right-$short.Left) {
        throw 'Clipping the circle did not reduce its measured height.'
    }
    Write-Output 'Blank, whole and clipped synthetic software-drawn badge regions measured.'
} finally { $blue.Dispose(); $graphics.Dispose(); $image.Dispose() }

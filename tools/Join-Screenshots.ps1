<#
.SYNOPSIS
    Tiles several screenshots into one PNG grid (used for docs/images/themes.png).

.DESCRIPTION
    Every image is scaled to the size of the first one and placed left-to-right,
    top-to-bottom, with an optional caption under each tile. Uses System.Drawing,
    so it runs on Windows PowerShell 5.1 and PowerShell 7 on Windows.

.PARAMETER Path
    Input images, in display order.

.PARAMETER OutFile
    The PNG to write.

.PARAMETER Columns
    Tiles per row. Defaults to 2.

.PARAMETER Captions
    Optional caption for each image (same order as -Path).

.PARAMETER TileWidth
    Width of each tile in pixels. Defaults to 800.

.EXAMPLE
    $names = 'default', 'nord', 'monokai', 'catppuccin', 'solarized-dark', 'light'
    .\tools\Join-Screenshots.ps1 -Path ($names | ForEach-Object { "shots\theme-$_.png" }) `
        -Captions $names -OutFile docs\images\themes.png
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string[]]$Path,
    [Parameter(Mandatory)][string]$OutFile,
    [ValidateRange(1, 10)][int]$Columns = 2,
    [string[]]$Captions = @(),
    [ValidateRange(100, 4000)][int]$TileWidth = 800
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$images = foreach ($p in $Path) {
    [System.Drawing.Image]::FromFile((Resolve-Path -LiteralPath $p).ProviderPath)
}
try {
    $tileHeight = [int][Math]::Round($TileWidth * $images[0].Height / $images[0].Width)
    $gap = 16
    $captionHeight = if ($Captions.Count -gt 0) { 36 } else { 0 }
    $rows = [int][Math]::Ceiling($images.Count / $Columns)
    $width = $Columns * $TileWidth + ($Columns + 1) * $gap
    $height = $rows * ($tileHeight + $captionHeight) + ($rows + 1) * $gap

    $canvas = New-Object System.Drawing.Bitmap $width, $height
    $g = [System.Drawing.Graphics]::FromImage($canvas)
    try {
        $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        $g.Clear([System.Drawing.Color]::FromArgb(255, 30, 30, 36))
        $font = New-Object System.Drawing.Font 'Segoe UI', 14
        $brush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 220, 220, 230))
        $format = New-Object System.Drawing.StringFormat
        $format.Alignment = [System.Drawing.StringAlignment]::Center
        $format.LineAlignment = [System.Drawing.StringAlignment]::Center

        for ($i = 0; $i -lt $images.Count; $i++) {
            $col = $i % $Columns
            $row = [Math]::Floor($i / $Columns)
            $x = $gap + $col * ($TileWidth + $gap)
            $y = $gap + $row * ($tileHeight + $captionHeight + $gap)
            $g.DrawImage($images[$i], $x, $y, $TileWidth, $tileHeight)
            if ($i -lt $Captions.Count) {
                $rect = New-Object System.Drawing.RectangleF $x, ($y + $tileHeight), $TileWidth, $captionHeight
                $g.DrawString($Captions[$i], $font, $brush, $rect, $format)
            }
        }
        $font.Dispose()
        $brush.Dispose()
    }
    finally {
        $g.Dispose()
    }

    $outFull = [System.IO.Path]::GetFullPath((Join-Path (Get-Location).ProviderPath $OutFile))
    $outDir = Split-Path -Parent $outFull
    if (-not (Test-Path -LiteralPath $outDir)) {
        New-Item -ItemType Directory -Path $outDir -Force | Out-Null
    }
    $canvas.Save($outFull, [System.Drawing.Imaging.ImageFormat]::Png)
    $canvas.Dispose()
    Write-Host "Wrote $outFull ($width x $height)" -ForegroundColor Green
}
finally {
    foreach ($img in $images) { $img.Dispose() }
}

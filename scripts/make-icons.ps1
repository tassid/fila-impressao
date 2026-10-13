# Regenerates icons/icon.ico, icons/tray.png and icons/tray-busy.png.
# Run from the repo root: powershell -File scripts/make-icons.ps1
Add-Type -AssemblyName System.Drawing

function New-RoundedPath([single]$x, [single]$y, [single]$w, [single]$h, [single]$r) {
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $path.AddArc($x, $y, $d, $d, 180, 90)
    $path.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $path.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $path.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    $path
}

# Draws the printer mark on a 100x100 design grid scaled to $size.
function New-Mark([int]$size, [string]$dot) {
    $bitmap = New-Object System.Drawing.Bitmap $size, $size
    $g = [System.Drawing.Graphics]::FromImage($bitmap)
    $g.SmoothingMode = 'AntiAlias'
    $g.ScaleTransform($size / 100, $size / 100)

    $plate = New-RoundedPath 2 2 96 96 22
    $g.FillPath((New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml('#1C1B1A'))), $plate)

    $paper = New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml('#F4F1EA'))
    $shade = New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml('#1C1B1A'))
    $g.FillPath($paper, (New-RoundedPath 32 16 36 26 4))     # sheet going in
    $g.FillPath($paper, (New-RoundedPath 16 36 68 34 9))     # body
    $g.FillPath($shade, (New-RoundedPath 28 56 44 8 3))      # output slot
    $g.FillPath($paper, (New-RoundedPath 30 60 40 24 4))     # sheet coming out
    $g.FillEllipse((New-Object System.Drawing.SolidBrush ([System.Drawing.ColorTranslator]::FromHtml($dot))), 66, 42, 11, 11)

    $g.Dispose()
    $bitmap
}

function Get-PngBytes($bitmap) {
    $stream = New-Object System.IO.MemoryStream
    $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    $stream.ToArray()
}

$icons = Join-Path (Get-Location) 'icons'
(New-Mark 64 '#19B5E6').Save("$icons\tray.png")
(New-Mark 64 '#F5C518').Save("$icons\tray-busy.png")

# ICO container with PNG-compressed entries.
$sizes = 16, 32, 48, 256
$images = New-Object 'System.Collections.Generic.List[byte[]]'
foreach ($size in $sizes) { $images.Add([byte[]](Get-PngBytes (New-Mark $size '#19B5E6'))) }
$out = New-Object System.IO.MemoryStream
$writer = New-Object System.IO.BinaryWriter $out
$writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $dimension = if ($sizes[$i] -eq 256) { 0 } else { $sizes[$i] }
    $writer.Write([byte]$dimension); $writer.Write([byte]$dimension)
    $writer.Write([byte]0); $writer.Write([byte]0)
    $writer.Write([uint16]1); $writer.Write([uint16]32)
    $writer.Write([uint32]$images[$i].Length); $writer.Write([uint32]$offset)
    $offset += $images[$i].Length
}
foreach ($image in $images) { $writer.Write([byte[]]$image) }
[System.IO.File]::WriteAllBytes("$icons\icon.ico", $out.ToArray())
"icons written to $icons"

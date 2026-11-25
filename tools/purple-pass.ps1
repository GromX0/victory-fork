<#
.SYNOPSIS
  Finds the first render target, in the frame's order, where purple/magenta
  pixels (high red and blue, low green) appear, and the passes that write it.

.DESCRIPTION
  Reads a capture folder of the diagnostics build (VICTORY_CAPTURE with
  VICTORY_DUMP_TARGETS=1 and VICTORY_CAPTURE_FRAMES or VICTORY_CAPTURE_WORLD):

    frame-<n>.png                                  the presented frame
    frame-<n>-<i>-<resource>-<w>x<h>-f<format>.png every target of frame n,
                                                   i in first-use order
    frame-<n>-ops.txt                              the frame's draws,
                                                   dispatches and copies
    frame-<n>-values.txt                           per target: largest value
                                                   and NaN / infinite /
                                                   negative texels per channel

  Each picture is divided into a grid of cells (-Grid across). A pixel is
  purple when red and blue are both at least -MinByte in the picture and
  green, in linear terms, is at most -GreenRatio times the weaker of the two
  (float targets are dumped through a cube root of value / largest value,
  which is undone; 8-bit pictures through gamma 2.2). A cell is purple with
  at least -MinPixels such pixels.

  The purple cells of the presented frame are the ones that count (so a
  normal buffer, purple by nature, is passed over unless the screen is
  purple there too; -NoReference counts every purple cell). Targets are
  taken in first-use order; a target's purple cells that no earlier target
  had are new. The first target with at least -MinCells new cells is named,
  with the ops.txt lines that write it (draws naming it among their targets,
  dispatches with an unordered view of it, copies into it) and their shader
  hashes. A NaN in a channel shows black in a dump, as a lost channel does:
  frame-<n>-values.txt tells them apart, and the first target with NaN or
  infinite green is named too.

  Only targets of the presented frame's aspect and at least a quarter of
  its width count, unless -AllSizes.

  Nothing is written; game pictures stay in the capture folder (outside the
  repository).

.EXAMPLE
  powershell -File tools\purple-pass.ps1 -Folder D:\victory-scratch\captures\purple-1

.EXAMPLE
  # One frame, looser test, every target size, 12 op lines per target:
  powershell -File tools\purple-pass.ps1 -Folder D:\caps -Frame 1234 -GreenRatio 0.7 -AllSizes -ShowOps 12

.EXAMPLE
  # Check the tool itself on a synthetic capture (no game data):
  powershell -File tools\purple-pass.ps1 -SelfTest
#>
param(
    [string]$Folder = '',
    # 0: every frame of the folder with dumped targets.
    [long]$Frame = 0,
    [double]$GreenRatio = 0.5,
    [int]$MinByte = 40,
    [int]$MinPixels = 2,
    [int]$MinCells = 2,
    [int]$Grid = 96,
    [switch]$AllSizes,
    [switch]$NoReference,
    # Lines of ops.txt shown per target named.
    [int]$ShowOps = 8,
    # Also list every target, not only those with purple.
    [switch]$All,
    [switch]$SelfTest
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

if (-not ('PurpleScan' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
public static class PurpleScan {
    // BGRA rows of a picture: the purple pixel count per cell of a gx-by-gy grid.
    public static int[] Cells(byte[] px, int w, int h, int stride, int gx, int gy, bool cube,
                              double greenRatio, int minByte, out int purple) {
        int[] counts = new int[gx * gy];
        double[] lin = new double[256];
        for (int i = 0; i < 256; i++) {
            double t = i / 255.0;
            lin[i] = cube ? t * t * t : Math.Pow(t, 2.2);
        }
        purple = 0;
        for (int y = 0; y < h; y++) {
            int row = y * stride;
            int cy = (int)((long)y * gy / h);
            for (int x = 0; x < w; x++) {
                int at = row + 4 * x;
                int b = px[at], g = px[at + 1], r = px[at + 2];
                if (r < minByte || b < minByte) continue;
                double m = Math.Min(lin[r], lin[b]);
                if (lin[g] <= greenRatio * m) {
                    purple++;
                    counts[cy * gx + (int)((long)x * gx / w)]++;
                }
            }
        }
        return counts;
    }
}
'@
}

# Float formats the dump shows through a cube root (capture.rs `to_rgba`).
$CubeFormats = @(1, 2, 9, 10, 26, 67)

function Read-Cells([string]$Path, [bool]$Cube, [int]$Gx, [int]$Gy) {
    $bmp = New-Object System.Drawing.Bitmap $Path
    try {
        $rect = New-Object System.Drawing.Rectangle 0, 0, $bmp.Width, $bmp.Height
        $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly,
            [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try {
            $bytes = New-Object byte[] ($data.Stride * $bmp.Height)
            [System.Runtime.InteropServices.Marshal]::Copy($data.Scan0, $bytes, 0, $bytes.Length)
            $purple = 0
            $counts = [PurpleScan]::Cells($bytes, $bmp.Width, $bmp.Height, $data.Stride, $Gx, $Gy, $Cube,
                $GreenRatio, $MinByte, [ref]$purple)
        } finally {
            $bmp.UnlockBits($data)
        }
        $cells = New-Object 'System.Collections.Generic.HashSet[int]'
        for ($k = 0; $k -lt $counts.Length; $k++) {
            if ($counts[$k] -ge $MinPixels) { [void]$cells.Add($k) }
        }
        [pscustomobject]@{
            Width = $bmp.Width; Height = $bmp.Height; Cells = $cells; Purple = $purple
            Percent = [math]::Round(100.0 * $purple / [math]::Max(1, $bmp.Width * $bmp.Height), 3)
        }
    } finally {
        $bmp.Dispose()
    }
}

# The ops.txt lines that write `resource` (a 0x... pointer): draws naming it as a target,
# dispatches with an unordered view of it (views print the resource in decimal), copies,
# alias copies and resolves into it.
function Find-Writers([string[]]$Ops, [string]$Resource) {
    $dec = [Convert]::ToUInt64($Resource.Substring(2), 16).ToString()
    $esc = [regex]::Escape($Resource)
    $out = New-Object System.Collections.Generic.List[string]
    foreach ($line in $Ops) {
        $hit = $false
        $t = $line.IndexOf(' targets [')
        if ($t -ge 0) {
            $end = $line.IndexOf('] depth', $t)
            if ($end -lt 0) { $end = $line.Length }
            if ($line.Substring($t, $end - $t) -match ('"' + $esc + ' ')) { $hit = $true }
        }
        if (-not $hit -and $line -match ' dispatch ' -and $line.Contains("Unordered($dec,")) { $hit = $true }
        if (-not $hit -and ($line -match (' to ' + $esc + '(\s|$)') -or $line -match (' into ' + $esc + '(\s|$)'))) { $hit = $true }
        if ($hit) { $out.Add($line) }
    }
    , $out
}

# What an ops.txt line runs: the op, the pipeline's name and the shader hashes after ` | `.
function Describe-Op([string]$Line) {
    $hashes = ([regex]::Matches($Line, '\| ([0-9a-f]{16}):') | ForEach-Object { $_.Groups[1].Value }) -join ','
    $head = $Line
    $cut = $head.IndexOf(' targets [')
    if ($cut -lt 0) { $cut = $head.IndexOf(' | ') }
    if ($cut -gt 0) { $head = $head.Substring(0, $cut) }
    if ($head.Length -gt 160) { $head = $head.Substring(0, 160) + '...' }
    if ($hashes) { "$head  [shaders $hashes]" } else { $head }
}

function Read-Values([string]$Path) {
    $map = @{}
    if (-not (Test-Path -LiteralPath $Path)) { return $map }
    foreach ($line in Get-Content -LiteralPath $Path) {
        $sp = $line.IndexOf(' ')
        if ($sp -lt 0) { continue }
        $name = $line.Substring(0, $sp)
        $v = [pscustomobject]@{ Nan = '-'; Inf = '-'; Negative = '-'; Max = '-'; GreenBad = $false }
        if ($line -match 'nan_rgb \[(\d+), (\d+), (\d+)\]') { $v.Nan = "$($matches[1])/$($matches[2])/$($matches[3])"; if ([int]$matches[2] -gt 0) { $v.GreenBad = $true } }
        if ($line -match 'inf_rgb \[(\d+), (\d+), (\d+)\]') { $v.Inf = "$($matches[1])/$($matches[2])/$($matches[3])"; if ([int]$matches[2] -gt 0) { $v.GreenBad = $true } }
        if ($line -match 'negative_rgb \[(\d+), (\d+), (\d+)\]') { $v.Negative = "$($matches[1])/$($matches[2])/$($matches[3])" }
        if ($line -match 'max_rgb \[([^\]]*)\]') { $v.Max = $matches[1] }
        $map[$name] = $v
    }
    $map
}

function Invoke-Frame([string]$Dir, [long]$N) {
    $pattern = '^frame-' + $N + '-(\d+)-(0x[0-9a-fA-F]+)-(\d+)x(\d+)-f(\d+)\.png$'
    $targets = Get-ChildItem -LiteralPath $Dir -Filter "frame-$N-*.png" |
        Where-Object { $_.Name -match $pattern } |
        ForEach-Object {
            [void]($_.Name -match $pattern)
            [pscustomobject]@{
                Index = [int]$matches[1]; Resource = $matches[2].ToLower(); W = [int]$matches[3]; H = [int]$matches[4]
                Format = [int]$matches[5]; Path = $_.FullName; Name = $_.BaseName
            }
        } | Sort-Object Index
    if (-not $targets) { Write-Host "frame ${N}: no dumped targets"; return $null }
    $refPath = Join-Path $Dir "frame-$N.png"
    $hasRef = (-not $NoReference) -and (Test-Path -LiteralPath $refPath)
    # The screen's shape: the presented frame's, else the widest target's.
    if ($hasRef) {
        $img = [System.Drawing.Image]::FromFile($refPath)
        $refW = $img.Width; $refH = $img.Height; $img.Dispose()
    } else {
        $widest = $targets | Sort-Object { $_.W * $_.H } -Descending | Select-Object -First 1
        $refW = $widest.W; $refH = $widest.H
    }
    $aspect = $refW / [double]$refH
    $gy = [math]::Max(1, [int][math]::Round($Grid / $aspect))
    $screenW = ($targets | Measure-Object -Property W -Maximum).Maximum
    $ref = $null
    if ($hasRef) {
        $ref = Read-Cells $refPath $false $Grid $gy
        Write-Host ("frame {0}: presented frame {1}x{2}, {3}% purple pixels in {4} of {5} cells" -f $N, $ref.Width, $ref.Height, $ref.Percent, $ref.Cells.Count, ($Grid * $gy))
        if ($ref.Cells.Count -eq 0) { Write-Host '  no purple on the screen: every purple cell of a target counts'; $ref = $null }
    } else {
        Write-Host "frame ${N}: no presented frame used: every purple cell of a target counts"
    }
    $values = Read-Values (Join-Path $Dir "frame-$N-values.txt")
    $opsPath = Join-Path $Dir "frame-$N-ops.txt"
    $ops = @()
    if (Test-Path -LiteralPath $opsPath) { $ops = Get-Content -LiteralPath $opsPath } else { Write-Host "  no $opsPath`: passes not named" }
    $seen = New-Object 'System.Collections.Generic.HashSet[int]'
    $first = $null
    $firstGreen = $null
    $rows = New-Object System.Collections.Generic.List[object]
    foreach ($t in $targets) {
        $v = $values[$t.Name]
        if ($v -and $v.GreenBad -and -not $firstGreen) { $firstGreen = $t }
        $screenLike = ([math]::Abs($t.W / [double]$t.H - $aspect) -le 0.03 * $aspect) -and ($t.W * 4 -ge $screenW)
        if (-not $AllSizes -and -not $screenLike) { continue }
        $c = Read-Cells $t.Path ($CubeFormats -contains $t.Format) $Grid $gy
        $counted = New-Object 'System.Collections.Generic.HashSet[int]' -ArgumentList (, [int[]]@($c.Cells))
        if ($ref) { $counted.IntersectWith($ref.Cells) }
        $new = New-Object 'System.Collections.Generic.HashSet[int]' -ArgumentList (, [int[]]@($counted))
        $new.ExceptWith($seen)
        $seen.UnionWith($counted)
        $row = [pscustomobject]@{
            '#' = $t.Index; Resource = $t.Resource; Size = "$($t.W)x$($t.H)"; Fmt = $t.Format
            'Purple%' = $c.Percent; Cells = $counted.Count; New = $new.Count
            'NaN r/g/b' = $(if ($v) { $v.Nan } else { '' }); 'Inf r/g/b' = $(if ($v) { $v.Inf } else { '' })
            'Neg r/g/b' = $(if ($v) { $v.Negative } else { '' })
        }
        if ($All -or $c.Purple -gt 0 -or ($v -and $v.GreenBad)) { $rows.Add($row) }
        if (-not $first -and $new.Count -ge $MinCells) { $first = $t }
    }
    $rows | Format-Table -AutoSize | Out-String -Width 220 | Write-Host
    foreach ($pick in @(@('FIRST TARGET WITH NEW PURPLE', $first), @('FIRST TARGET WITH NaN OR INFINITE GREEN', $firstGreen))) {
        $label = $pick[0]; $t = $pick[1]
        if (-not $t) { Write-Host "  $label`: none"; continue }
        Write-Host ("  {0}: #{1} {2} {3}x{4} f{5}  ({6})" -f $label, $t.Index, $t.Resource, $t.W, $t.H, $t.Format, $t.Path)
        if ($ops.Count -gt 0) {
            $writers = Find-Writers $ops $t.Resource
            Write-Host ("    written by {0} op line(s) of {1}:" -f $writers.Count, (Split-Path $opsPath -Leaf))
            foreach ($w in ($writers | Select-Object -First $ShowOps)) { Write-Host ('      ' + (Describe-Op $w)) }
            if ($writers.Count -gt $ShowOps) { Write-Host "      ... (-ShowOps for more; the last writer is the one whose output was dumped)"; Write-Host ('      last: ' + (Describe-Op $writers[$writers.Count - 1])) }
        }
    }
    $first
}

function New-SelfTestFolder {
    $dir = Join-Path ([System.IO.Path]::GetTempPath()) ("purple-pass-selftest-" + [guid]::NewGuid().ToString('N').Substring(0, 8))
    New-Item -ItemType Directory -Path $dir | Out-Null
    $w = 320; $h = 180
    # (file name, list of (x0, y0, x1, y1, r, g, b) boxes on a grey picture)
    $spot = @(0.60, 0.20, 0.70, 0.30)
    $pictures = @(
        @('frame-7.png', (, ($spot + @(230, 60, 220)))),
        @('frame-7-00-0x1a00-640x360-f10.png', (, @(0.10, 0.60, 0.20, 0.70, 200, 40, 230))), # purple elsewhere (a normal buffer)
        @('frame-7-01-0x1b00-640x360-f10.png', (, ($spot + @(250, 250, 250)))),           # the spot lit, white
        @('frame-7-02-0x1c00-640x360-f10.png', (, ($spot + @(250, 90, 245)))),            # the spot purple: the answer
        @('frame-7-03-0x1d00-640x360-f87.png', (, ($spot + @(240, 70, 230)))),            # still purple later
        @('frame-7-04-0x1e00-64x64-f10.png', (, @(0.0, 0.0, 1.0, 1.0, 255, 0, 255)))        # a small table: not counted
    )
    foreach ($p in $pictures) {
        $bmp = New-Object System.Drawing.Bitmap $w, $h
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.Clear([System.Drawing.Color]::FromArgb(255, 60, 60, 60))
        foreach ($b in $p[1]) {
            $brush = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, $b[4], $b[5], $b[6]))
            $g.FillRectangle($brush, [int]($b[0] * $w), [int]($b[1] * $h), [int](($b[2] - $b[0]) * $w), [int](($b[3] - $b[1]) * $h))
            $brush.Dispose()
        }
        $g.Dispose()
        $bmp.Save((Join-Path $dir $p[0]), [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
    }
    $view = [Convert]::ToUInt64('1c00', 16)
    @(
        '1 list 0x10 draw 3x1 gbuffer targets ["0x1a00 640x360 f10", "0x1b00 640x360 f10"] depth None state [] | 00000000000000aa: u0=0x0',
        ('2 list 0x10 dispatch 40x23x1 light | 00000000000000bb: u0=0x0 [heap 3: Unordered(' + $view + ', 0, None)]'),
        '3 list 0x10 draw 3x1 compose targets ["0x1c00 640x360 f10"] depth None state [] | 00000000000000cc: u0=0x0',
        '4 list 0x10 copy resource 0x1c00 to 0x1d00'
    ) | Set-Content -LiteralPath (Join-Path $dir 'frame-7-ops.txt')
    @(
        'frame-7-02-0x1c00-640x360-f10 high 900 max_rgb [900.0, 0.0, 880.0] nan_rgb [0, 120, 0] inf_rgb [0, 0, 0] negative_rgb [0, 0, 0] texels 230400'
    ) | Set-Content -LiteralPath (Join-Path $dir 'frame-7-values.txt')
    $dir
}

if ($SelfTest) {
    $dir = New-SelfTestFolder
    try {
        $found = Invoke-Frame $dir 7
        if (-not $found -or $found.Index -ne 2) { throw "self-test: expected target #2, got $($found.Index)" }
        $writers = Find-Writers (Get-Content -LiteralPath (Join-Path $dir 'frame-7-ops.txt')) '0x1c00'
        if ($writers.Count -ne 2) { throw "self-test: expected the dispatch and the draw writing 0x1c00, got $($writers.Count)" }
        Write-Host 'self-test passed: target #2 named, with its two writers'
    } finally {
        Remove-Item -LiteralPath $dir -Recurse -Force
    }
    return
}

if (-not $Folder) { throw 'give -Folder (a capture folder), or -SelfTest' }
if (-not (Test-Path -LiteralPath $Folder)) { throw "no folder $Folder" }
$frames = if ($Frame -gt 0) { @($Frame) } else {
    Get-ChildItem -LiteralPath $Folder -Filter 'frame-*-*-0x*.png' |
        Where-Object { $_.Name -match '^frame-(\d+)-\d+-0x' } |
        ForEach-Object { [void]($_.Name -match '^frame-(\d+)-'); [long]$matches[1] } |
        Sort-Object -Unique
}
if (-not $frames) { throw "no dumped targets in $Folder (VICTORY_DUMP_TARGETS=1 with VICTORY_CAPTURE_FRAMES or VICTORY_CAPTURE_WORLD)" }
foreach ($n in $frames) { [void](Invoke-Frame $Folder $n); Write-Host '' }

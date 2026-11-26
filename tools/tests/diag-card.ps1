<#
  Diagnostic: where does the card *actually* open?

  ui-smoke's shots showed a card region hanging below the screen bottom, so
  this script drives the installed island to the Call card exactly like
  ui-smoke does (wheel notch -> read the pin from the log -> click the pill)
  but then dumps every piece of geometry - pill rect, host window rect, host
  region in window-local and screen coordinates, taskbar rect, work area -
  and takes one full-screen capture, so the card's real position on screen
  is on record instead of inferred from a crop.

  The native helpers are pulled verbatim out of ui-smoke.ps1 (one definition,
  no drift). The pin is walked back to Auto at the end, and the log is kept
  under target\soak\diag\.

  .EXAMPLE
  powershell -File tools\tests\diag-card.ps1
#>
[CmdletBinding()]
param(
    [int]$OpenWaitMs = 900
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$out = Join-Path $repoRoot 'target\soak\diag'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$logPath = Join-Path $env:TEMP 'taskbar-island.log'

# ---- shared native helpers (lines 56..243 of ui-smoke.ps1) ----------------
$src = Get-Content -Path (Join-Path $PSScriptRoot 'ui-smoke.ps1')
Invoke-Expression (($src[55..242]) -join "`n")

Add-Type -AssemblyName System.Windows.Forms, System.Drawing

function Save-Screen([string]$Path, [int]$x, [int]$y, [int]$w, [int]$h) {
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

# ---- relaunch the installed copy with logging -----------------------------
$p = Get-Process -Name taskbar-island -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $p) { throw 'taskbar-island is not running - run tools\tests\install-smoke.ps1 first.' }
$exe = $p.Path
Write-Host "installed copy: $exe (pid $($p.Id))"
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 700
Remove-Item -Path $logPath -ErrorAction SilentlyContinue
$env:TBI_LOG = '1'
Start-Process -FilePath $exe

$h = [IntPtr]::Zero
$pill = [IntPtr]::Zero
for ($i = 0; $i -lt 40 -and ($h -eq [IntPtr]::Zero -or $pill -eq [IntPtr]::Zero); $i++) {
    Start-Sleep -Milliseconds 250
    $h = [TbiUi]::FindByClass('TaskbarIsland')
    $pill = [TbiUi]::FindPill('TaskbarIsland')
}
if ($h -eq [IntPtr]::Zero) { throw 'pill window did not come up after relaunch' }
if ($pill -eq [IntPtr]::Zero) { $pill = $h }
Write-Host ("island windows: host {0}, pill {1}" -f $h, $pill)

function Get-Pin {
    if (-not (Test-Path $logPath)) { return $null }
    $lines = @(Get-Content $logPath | Where-Object { $_ -like 'activity.show: *' })
    if ($lines.Count -eq 0) { return $null }
    return ($lines[-1] -replace '^activity\.show: ', '')
}

function Close-Card {
    [TbiUi]::Escape($h)
    Start-Sleep -Milliseconds 450
}

# Walk to Call one notch at a time, verifying each step against the log.
for ($i = 0; $i -lt 8 -and (Get-Pin) -ne 'Some(Call)'; $i++) {
    Close-Card
    $before = @((Get-Content $logPath -ErrorAction SilentlyContinue | Where-Object { $_ -like 'activity.show: *' })).Count
    $rects = [TbiUi]::ScreenRects($pill, $h)
    $b = [TbiUi]::BBox($rects)
    [TbiUi]::PostWheel($h, [int](($b.Left + $b.Right) / 2), [int](($b.Top + $b.Bottom) / 2), 1)
    for ($w = 0; $w -lt 20; $w++) {
        Start-Sleep -Milliseconds 100
        $now = @((Get-Content $logPath -ErrorAction SilentlyContinue | Where-Object { $_ -like 'activity.show: *' })).Count
        if ($now -gt $before) { break }
    }
}
Write-Host "pin: $(Get-Pin)"

# ---- open the card --------------------------------------------------------
Close-Card
$rects = [TbiUi]::ScreenRects($pill, $h)
$b = [TbiUi]::BBox($rects)
$cx = [int](($b.Left + $b.Right) / 2)
$cy = [int](($b.Top + $b.Bottom) / 2)
[TbiUi]::Click($cx, $cy)
Start-Sleep -Milliseconds $OpenWaitMs

# ---- dump geometry --------------------------------------------------------
$hostRect = [TbiUi]::Rect($h)
$pillRect = [TbiUi]::Rect($pill)
$hostLocal = [TbiUi]::RegionRects($h)
$pillLocal = [TbiUi]::RegionRects($pill)
$screenRects = [TbiUi]::ScreenRects($pill, $h)
$screenBox = [TbiUi]::BBox($screenRects)

$lines = @()
$lines += "pin: $(Get-Pin)"
$lines += ("host window rect : ({0},{1})-({2},{3})" -f $hostRect.Left, $hostRect.Top, $hostRect.Right, $hostRect.Bottom)
$lines += ("pill window rect : ({0},{1})-({2},{3})" -f $pillRect.Left, $pillRect.Top, $pillRect.Right, $pillRect.Bottom)
$lines += ("host region (local) rects: $($hostLocal.Count)")
foreach ($r in $hostLocal) { $lines += ("  local  ({0},{1})-({2},{3})" -f $r.Left, $r.Top, $r.Right, $r.Bottom) }
$lines += ("pill region (local) rects: $($pillLocal.Count)")
foreach ($r in $pillLocal) { $lines += ("  local  ({0},{1})-({2},{3})" -f $r.Left, $r.Top, $r.Right, $r.Bottom) }
$lines += ("screen union box : ({0},{1})-({2},{3})" -f $screenBox.Left, $screenBox.Top, $screenBox.Right, $screenBox.Bottom)
$lines += ("virtual screen    : {0}" -f [System.Windows.Forms.SystemInformation]::VirtualScreen)
$lines += ("work area         : {0}" -f [System.Windows.Forms.Screen]::PrimaryScreen.WorkingArea)
$trayRect = [TbiUi]::Rect([TbiUi]::FindByClass('Shell_TrayWnd'))
$lines += ("taskbar tray rect: ({0},{1})-({2},{3})" -f $trayRect.Left, $trayRect.Top, $trayRect.Right, $trayRect.Bottom)

$lines | ForEach-Object { Write-Host $_ }
$lines | Out-File -FilePath (Join-Path $out 'geometry.txt') -Encoding utf8

# Full-screen capture: the crop can lie, the screen cannot.
Save-Screen (Join-Path $out 'fullscreen.png') `
    ([System.Windows.Forms.SystemInformation]::VirtualScreen.X) `
    ([System.Windows.Forms.SystemInformation]::VirtualScreen.Y) `
    ([System.Windows.Forms.SystemInformation]::VirtualScreen.Width) `
    ([System.Windows.Forms.SystemInformation]::VirtualScreen.Height)
Write-Host "saved $(Join-Path $out 'fullscreen.png')"

# ---- restore: card closed, pin back to Auto, log kept --------------------
Close-Card
$pin = Get-Pin
if ($pin -ne 'None') {
    for ($i = 0; $i -lt 6; $i++) {
        Close-Card
        $rects = [TbiUi]::ScreenRects($pill, $h)
        if ($rects.Count -eq 0) { break }
        $b = [TbiUi]::BBox($rects)
        [TbiUi]::PostWheel($h, [int](($b.Left + $b.Right) / 2), [int](($b.Top + $b.Bottom) / 2), 1)
        Start-Sleep -Milliseconds 300
        if ((Get-Pin) -eq 'None') { break }
    }
}
Write-Host "final pin: $(Get-Pin)"
Copy-Item -Path $logPath -Destination (Join-Path $out 'log.txt') -Force -ErrorAction SilentlyContinue
Write-Host "geometry + log in $out"

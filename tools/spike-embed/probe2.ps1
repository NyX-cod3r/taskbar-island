# probe2.ps1 - physical-px shell probe around the spike pill (spike dir only).
# Starts a fresh spike, dumps tray/pill/z-order/overlay state, samples screen and
# PrintWindow pixels, snapshots the real app's pill if it is running, then stops
# the spike. Everything is bounded: spike self-terminates at 120 s anyway.
$ErrorActionPreference = 'Continue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Add-Type -AssemblyName System.Drawing

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class P2 {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, int c);
    [DllImport("user32.dll")] public static extern int GetWindowLongW(IntPtr h, int i);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(PT p);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int m);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
    [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h, IntPtr dc);
    [DllImport("gdi32.dll")] public static extern uint GetPixel(IntPtr dc, int x, int y);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint f);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc p, IntPtr l);
    [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out int v, int s);
    [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
    [StructLayout(LayoutKind.Sequential)] public struct PT { public int X, Y; }
}
'@
# Physical pixels (best effort: fails harmlessly if the process already set it).
[P2]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null

function Get-ClassName([IntPtr]$h) {
    $sb = New-Object System.Text.StringBuilder 256
    [void][P2]::GetClassNameW($h, $sb, 256)
    $sb.ToString()
}
function Get-Rect([IntPtr]$h) {
    $r = New-Object P2+R
    if ([P2]::GetWindowRect($h, [ref]$r)) { return "$($r.L),$($r.T),$($r.Rt),$($r.B)" }
    return '?'
}
function Test-Intersect([string]$a, [string]$b) {
    $A = $a -split ',' | ForEach-Object { [int]$_ }
    $B = $b -split ',' | ForEach-Object { [int]$_ }
    return ($A[0] -lt $B[2]) -and ($B[0] -lt $A[2]) -and ($A[1] -lt $B[3]) -and ($B[1] -lt $A[3])
}
function Get-ScreenPx([int]$x, [int]$y) {
    $dc = [P2]::GetDC([IntPtr]::Zero)
    $c = [P2]::GetPixel($dc, $x, $y)
    [void][P2]::ReleaseDC([IntPtr]::Zero, $dc)
    if ($c -eq 0xFFFFFFFF) { return 'ERR' }
    return ('{0},{1},{2}' -f ($c -band 0xFF), (($c -shr 8) -band 0xFF), (($c -shr 16) -band 0xFF))
}

# ---- start a fresh spike ------------------------------------------------
$exe = Get-ChildItem (Join-Path $here 'target\debug\*.exe') | Select-Object -First 1
if (-not $exe) { Write-Output 'FATAL: no spike exe built'; exit 1 }
$spike = Start-Process $exe.FullName -PassThru
Start-Sleep -Seconds 3

$tray = [P2]::FindWindowW('Shell_TrayWnd', $null)
Write-Output ("tray hwnd={0} rect={1} style=0x{2:X8} ex=0x{3:X8}" -f $tray, (Get-Rect $tray), [P2]::GetWindowLongW($tray, -16), [P2]::GetWindowLongW($tray, -20))
if ($tray -eq [IntPtr]::Zero) { Write-Output 'FATAL: no Shell_TrayWnd'; exit 2 }

# ---- tray children, top to bottom --------------------------------------
$kids = New-Object System.Collections.ArrayList
$cb = [P2+EnumProc]{ param([IntPtr]$h, [IntPtr]$l) [void]$kids.Add($h); return $true }
[void][P2]::EnumChildWindows($tray, $cb, [IntPtr]::Zero)
Write-Output ("tray children (raw enum order, {0}):" -f $kids.Count)
$pill = [IntPtr]::Zero
foreach ($k in $kids) {
    $cls = Get-ClassName $k
    $ex = [P2]::GetWindowLongW($k, -20)
    $vis = [P2]::IsWindowVisible($k)
    $cloaked = 0
    [void][P2]::DwmGetWindowAttribute($k, 14, [ref]$cloaked, 4)
    Write-Output ("  {0} cls={1} rect={2} style=0x{3:X8} ex=0x{4:X8} vis={5} cloak={6}" -f $k, $cls, (Get-Rect $k), [P2]::GetWindowLongW($k, -16), $ex, $vis, $cloaked)
    if ($cls -eq 'TbiSpikeClass') { $pill = $k }
}
# true z order: walk GW_CHILD then GW_HWNDNEXT
$zord = @()
$z = [P2]::GetWindow($tray, 5)   # GW_CHILD
while ($z -ne [IntPtr]::Zero) { $zord += $z; $z = [P2]::GetWindow($z, 2) }  # GW_HWNDNEXT
Write-Output 'z order top->bottom:'
$i = 0
foreach ($z in $zord) { Write-Output ("  [{0}] {1}" -f $i, (Get-ClassName $z)); $i++ }

if ($pill -eq [IntPtr]::Zero) { Write-Output 'FATAL: pill not embedded'; Stop-Process -Id $spike.Id -Force; exit 3 }

# ---- pill ground truth --------------------------------------------------
$pr = Get-Rect $pill
$P = $pr -split ',' | ForEach-Object { [int]$_ }
$cx = [int](($P[0] + $P[2]) / 2); $cy = [int](($P[1] + $P[3]) / 2)
Write-Output ("pill rect={0} center=({1},{2}) screenPx={3}" -f $pr, $cx, $cy, (Get-ScreenPx $cx $cy))
$pt = New-Object P2+PT; $pt.X = $cx; $pt.Y = $cy
$hit = [P2]::WindowFromPoint($pt)
Write-Output ("WindowFromPoint -> {0} cls={1}" -f $hit, (Get-ClassName $hit))

# ---- top-level windows overlapping the pill -----------------------------
$tops = New-Object System.Collections.ArrayList
$cb2 = [P2+EnumProc]{ param([IntPtr]$h, [IntPtr]$l) [void]$tops.Add($h); return $true }
[void][P2]::EnumWindows($cb2, [IntPtr]::Zero)
Write-Output 'top-level windows intersecting the pill rect:'
foreach ($t in $tops) {
    $r = Get-Rect $t
    if ($r -eq '?') { continue }
    if (Test-Intersect $r $pr) {
        $ex = [P2]::GetWindowLongW($t, -20)
        $vis = [P2]::IsWindowVisible($t)
        $pid2 = 0; [void][P2]::GetWindowThreadProcessId($t, [ref]$pid2)
        $pn = '?'; try { $pn = (Get-Process -Id $pid2 -ErrorAction Stop).ProcessName } catch {}
        $flags = @()
        if ($ex -band 0x00000020) { $flags += 'TRANSPARENT' }
        if ($ex -band 0x00080000) { $flags += 'LAYERED' }
        if ($ex -band 0x00200000) { $flags += 'NOREDIRECTION' }
        if ($ex -band 0x00000008) { $flags += 'TOPMOST' }
        if ($ex -band 0x00000040) { $flags += 'TOOLWINDOW' }
        Write-Output ("  {0} cls={1} proc={2} rect={3} style=0x{4:X8} ex=0x{5:X8} vis={6} [{7}]" -f $t, (Get-ClassName $t), $pn, $r, [P2]::GetWindowLongW($t, -16), $ex, $vis, ($flags -join ','))
    }
}

# ---- PrintWindow of the tray (PW_RENDERFULLCONTENT=2) -------------------
try {
    $bmp = New-Object System.Drawing.Bitmap(600, 200)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $g.GetHdc()
    $ok = [P2]::PrintWindow($tray, $hdc, 2)
    $g.ReleaseHdc($hdc)
    $g.Dispose()
    if ($ok) {
        $lx = $cx - $P[0]; $ly = $cy - $P[1]
        if ($lx -ge 0 -and $ly -ge 0 -and $lx -lt 600 -and $ly -lt 200) {
            $c = $bmp.GetPixel($lx, $ly)
            Write-Output ("PrintWindow(tray) ok, px at pill center = {0},{1},{2}" -f $c.R, $c.G, $c.B)
        }
        $bmp.Save((Join-Path $here 'probe2-printwindow.png'), [System.Drawing.Imaging.ImageFormat]::Png)
    } else {
        Write-Output 'PrintWindow(tray) failed'
    }
    $bmp.Dispose()
} catch { Write-Output "PrintWindow error: $_" }

# ---- real app pill, if running -----------------------------------------
$real = Get-Process taskbar-island -ErrorAction SilentlyContinue
if ($real) {
    Write-Output ("taskbar-island running pid={0} started={1}" -f $real.Id, $real.StartTime)
    foreach ($t in $tops) {
        $pid3 = 0; [void][P2]::GetWindowThreadProcessId($t, [ref]$pid3)
        if ($pid3 -eq $real.Id) {
            $r = Get-Rect $t
            $X = $r -split ',' | ForEach-Object { [int]$_ }
            $rx = [int](($X[0] + $X[2]) / 2); $ry = [int](($X[1] + $X[3]) / 2)
            Write-Output ("  real window {0} cls={1} rect={2} vis={3} centerPx={4}" -f $t, (Get-ClassName $t), $r, [P2]::IsWindowVisible($t), (Get-ScreenPx $rx $ry))
        }
    }
    # its child pill (WS_CHILD of the tray) if any
    foreach ($k in $kids) {
        $pid4 = 0; [void][P2]::GetWindowThreadProcessId($k, [ref]$pid4)
        if ($pid4 -eq $real.Id) {
            $r = Get-Rect $k
            $X = $r -split ',' | ForEach-Object { [int]$_ }
            $rx = [int](($X[0] + $X[2]) / 2); $ry = [int](($X[1] + $X[3]) / 2)
            Write-Output ("  real tray-child {0} cls={1} rect={2} vis={3} centerPx={4}" -f $k, (Get-ClassName $k), $r, [P2]::IsWindowVisible($k), (Get-ScreenPx $rx $ry))
        }
    }
} else {
    Write-Output 'taskbar-island: NOT RUNNING'
}

# ---- screenshot bottom-left (physical) ----------------------------------
try {
    $bmp = New-Object System.Drawing.Bitmap(520, 100)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen(0, 1100, 0, 0, $bmp.Size)
    $g.Dispose()
    $bmp.Save((Join-Path $here 'probe2-taskbar.png'), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    Write-Output 'saved probe2-taskbar.png (0,1100 520x100)'
} catch { Write-Output "shot error: $_" }

# ---- stop the spike -----------------------------------------------------
Stop-Process -Id $spike.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500
Copy-Item (Join-Path $here 'log.txt') (Join-Path $here 'log-probe2.txt') -Force -ErrorAction SilentlyContinue
$left = Get-Process | Where-Object { $_.ProcessName -match 'spike' }
Write-Output ("spike stopped; leftovers: {0}" -f ($(if ($left) { ($left | ForEach-Object { $_.ProcessName + ':' + $_.Id }) -join ',' } else { 'none' })))
Write-Output 'PROBE DONE'

# probe3.ps1 - decisive presentation test: with the spike embedded, hide the
# XAML bridge and capture the pill area. If green appears, the pill IS
# presented and the bridge (WS_EX_NOREDIRECTIONBITMAP) paints over it despite
# HWND z-order. If wallpaper appears, the pill's GDI content is not presented
# at all. Bounded: spike self-terminates at 120 s; we stop it here.
$ErrorActionPreference = 'Continue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Add-Type -AssemblyName System.Drawing

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class P3 {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, int c);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int m);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
    [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
    [DllImport("user32.dll")] public static extern bool InvalidateRect(IntPtr h, IntPtr r, bool e);
    [DllImport("user32.dll")] public static extern bool UpdateWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
    [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h, IntPtr dc);
    [DllImport("gdi32.dll")] public static extern uint GetPixel(IntPtr dc, int x, int y);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc p, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
}
'@
[P3]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null

function Get-ClassName([IntPtr]$h) {
    $sb = New-Object System.Text.StringBuilder 256
    [void][P3]::GetClassNameW($h, $sb, 256)
    $sb.ToString()
}
function Get-ScreenPx([int]$x, [int]$y) {
    $dc = [P3]::GetDC([IntPtr]::Zero)
    $c = [P3]::GetPixel($dc, $x, $y)
    [void][P3]::ReleaseDC([IntPtr]::Zero, $dc)
    if ($c -eq 0xFFFFFFFF) { return 'ERR' }
    return ('{0},{1},{2}' -f ($c -band 0xFF), (($c -shr 8) -band 0xFF), (($c -shr 16) -band 0xFF))
}
function Save-Shot([string]$name) {
    $bmp = New-Object System.Drawing.Bitmap(400, 80)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen(0, 1130, 0, 0, $bmp.Size)
    $g.Dispose()
    $bmp.Save((Join-Path $here $name), [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

# ---- start spike --------------------------------------------------------
$exe = Get-ChildItem (Join-Path $here 'target\debug\*.exe') | Select-Object -First 1
if (-not $exe) { Write-Output 'FATAL: no spike exe'; exit 1 }
$spike = Start-Process $exe.FullName -PassThru
Start-Sleep -Seconds 3

$tray = [P3]::FindWindowW('Shell_TrayWnd', $null)
$kids = New-Object System.Collections.ArrayList
$cb = [P3+EnumProc]{ param([IntPtr]$h, [IntPtr]$l) [void]$kids.Add($h); return $true }
[void][P3]::EnumChildWindows($tray, $cb, [IntPtr]::Zero)
$pill = [IntPtr]::Zero; $bridge = [IntPtr]::Zero
foreach ($k in $kids) {
    $cls = Get-ClassName $k
    if ($cls -eq 'TbiSpikeClass') { $pill = $k }
    if ($cls -eq 'Windows.UI.Composition.DesktopWindowContentBridge') { $bridge = $k }
}
if ($pill -eq [IntPtr]::Zero) { Write-Output 'FATAL: no pill'; Stop-Process -Id $spike.Id -Force; exit 3 }
$r = New-Object P3+R
[void][P3]::GetWindowRect($pill, [ref]$r)
$cx = [int](($r.L + $r.Rt) / 2); $cy = [int](($r.T + $r.B) / 2)
Write-Output ("pill rect={0},{1},{2},{3} center=({4},{5})" -f $r.L, $r.T, $r.Rt, $r.B, $cx, $cy)

Write-Output ("BEFORE: screenPx={0} bridge={1}" -f (Get-ScreenPx $cx $cy), $bridge)
Save-Shot 'probe3-before.png'

# force a fresh paint of the pill, re-sample
[void][P3]::InvalidateRect($pill, [IntPtr]::Zero, $true)
[void][P3]::UpdateWindow($pill)
Start-Sleep -Milliseconds 300
Write-Output ("AFTER-REPAINT: screenPx={0}" -f (Get-ScreenPx $cx $cy))
Save-Shot 'probe3-repaint.png'

# hide the bridge, re-sample
if ($bridge -ne [IntPtr]::Zero) {
    [void][P3]::ShowWindow($bridge, 0)   # SW_HIDE
    Start-Sleep -Milliseconds 500
    Write-Output ("BRIDGE-HIDDEN: screenPx={0}" -f (Get-ScreenPx $cx $cy))
    Save-Shot 'probe3-bridgehidden.png'
    [void][P3]::ShowWindow($bridge, 5)   # SW_SHOW
    Start-Sleep -Milliseconds 500
    Write-Output ("BRIDGE-SHOWN: screenPx={0}" -f (Get-ScreenPx $cx $cy))
    Save-Shot 'probe3-bridgeshown.png'
} else {
    Write-Output 'BRIDGE NOT FOUND'
}

# ---- stop spike ---------------------------------------------------------
Stop-Process -Id $spike.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 500
$left = Get-Process | Where-Object { $_.ProcessName -match 'spike' }
Write-Output ("leftovers: {0}" -f ($(if ($left) { ($left | ForEach-Object { $_.ProcessName + ':' + $_.Id }) -join ',' } else { 'none' })))
Write-Output 'PROBE3 DONE'

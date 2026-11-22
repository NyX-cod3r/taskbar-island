# probe4.ps1 - decisive discriminator: does the REAL TaskbarIsland pill
# (D3D11 -> CreateSwapChainForComposition -> DComp visual on a WS_CHILD of
# Shell_TrayWnd) present above the opaque XAML bridge while the spike's plain
# GDI pill does not? Bounded: start app, sample, screenshot, stop app.
$ErrorActionPreference = 'Continue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
Add-Type -AssemblyName System.Drawing

Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class P4 {
    [DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string c, string t);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out R r);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int m);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern IntPtr GetDC(IntPtr h);
    [DllImport("user32.dll")] public static extern int ReleaseDC(IntPtr h, IntPtr dc);
    [DllImport("gdi32.dll")] public static extern uint GetPixel(IntPtr dc, int x, int y);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumProc p, IntPtr l);
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct R { public int L, T, Rt, B; }
}
'@
[P4]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null

function Get-ClassName([IntPtr]$h) {
    $sb = New-Object System.Text.StringBuilder 256
    [void][P4]::GetClassNameW($h, $sb, 256)
    $sb.ToString()
}
function Get-ScreenPx([int]$x, [int]$y) {
    $dc = [P4]::GetDC([IntPtr]::Zero)
    $c = [P4]::GetPixel($dc, $x, $y)
    [void][P4]::ReleaseDC([IntPtr]::Zero, $dc)
    if ($c -eq 0xFFFFFFFF) { return 'ERR' }
    return ('{0},{1},{2}' -f ($c -band 0xFF), (($c -shr 8) -band 0xFF), (($c -shr 16) -band 0xFF))
}

$app = "$env:LOCALAPPDATA\Programs\TaskbarIsland\taskbar-island.exe"
if (-not (Test-Path $app)) { Write-Output "FATAL: $app missing"; exit 1 }
$p = Start-Process $app -PassThru
Start-Sleep -Seconds 4

$tray = [P4]::FindWindowW('Shell_TrayWnd', $null)
$kids = New-Object System.Collections.ArrayList
$cb = [P4+EnumProc]{ param([IntPtr]$h, [IntPtr]$l) [void]$kids.Add($h); return $true }
[void][P4]::EnumChildWindows($tray, $cb, [IntPtr]::Zero)
$found = $false
foreach ($k in $kids) {
    $pid2 = 0; [void][P4]::GetWindowThreadProcessId($k, [ref]$pid2)
    if ($pid2 -eq $p.Id) {
        $found = $true
        $r = New-Object P4+R
        [void][P4]::GetWindowRect($k, [ref]$r)
        $cx = [int](($r.L + $r.Rt) / 2); $cy = [int](($r.T + $r.B) / 2)
        Write-Output ("REAL tray-child {0} cls={1} rect={2},{3},{4},{5} centerPx={6}" -f $k, (Get-ClassName $k), $r.L, $r.T, $r.Rt, $r.B, (Get-ScreenPx $cx $cy))
    }
}
if (-not $found) {
    Write-Output ("REAL app has no tray child; pid={0} alive={1}" -f $p.Id, (-not $p.HasExited))
    # list all its top-level windows
    Add-Type -AssemblyName System.Windows.Forms
    $all = New-Object System.Collections.ArrayList
    # fallback: use Get-Process MainWindowTitle
    $pr = Get-Process -Id $p.Id -ErrorAction SilentlyContinue
    Write-Output ("mainWindow={0} title={1}" -f $pr.MainWindowHandle, $pr.MainWindowTitle)
}

# screenshot the whole taskbar strip
$bmp = New-Object System.Drawing.Bitmap(1920, 70)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen(0, 1135, 0, 0, $bmp.Size)
$g.Dispose()
$bmp.Save((Join-Path $here 'probe4-realapp.png'), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()

Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 800
$left = Get-Process taskbar-island -ErrorAction SilentlyContinue
Write-Output ("app stopped; leftovers={0}" -f ($(if ($left) { ($left | ForEach-Object { $_.Id }) -join ',' } else { 'none' })))
Write-Output 'PROBE4 DONE'

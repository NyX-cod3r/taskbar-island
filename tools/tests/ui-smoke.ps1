<#
  Visual evidence for the card UI.

  Drives the *installed* island and screenshots every card into target\soak\ui\:
  one shot per activity in dark and in light, plus one mid-morph shot while the
  card is springing out of the pill (spec 7).

  How the states are driven - and why they are verified rather than assumed:

    * The wheel only cycles activities while the card is *closed*
      (view::wheel_action -> Ignore while the flyout is open), so every state
      is reached by: close the card -> notch -> click the pill.
    * WM_MOUSEWHEEL is delivered to the *focus* window, not the window under
      the cursor, so synthesized mouse_event notches reach the island only
      when focus happens to sit on our chain - one run silently skipped
      notches and produced two byte-identical shots. The script therefore
      posts WM_MOUSEWHEEL straight to the island HWND (exactly one message,
      one notch) with screen coordinates, the form window.rs expects.
    * The pin is read back from the app's own `activity.show:` log line, so a
      file is named after the activity that is *actually* on screen. Reaching
      a target pin is a loop, not a count: post, read, repeat. The app only
      logs when TBI_LOG is set at launch, so the script relaunches the
      installed copy with it (the log is copied into target\soak\ui\ and the
      temp file removed afterwards).
    * SHOW_ORDER is Auto -> Music -> Call -> Shelf -> Timer -> Clipboard, and
      a positive wheel delta steps *backwards* through it (window.rs
      cycle_activity(-1)); the loop does not care which way it walks.

  Auto with nothing live hides the pill, so that state is photographed with a
  fixed crop of the pill's own rectangle instead of a click (clicking a hidden
  pill would land on whatever taskbar icon sits there).

  The owner's theme is restored in a finally block, so an aborted run cannot
  leave the machine in light mode.

  Everything is ASCII: Windows PowerShell 5.1 reads a BOM-less script as the
  ANSI code page and a non-ASCII byte inside a string literal can end it.

.EXAMPLE
  powershell -File tools\tests\ui-smoke.ps1
#>
[CmdletBinding()]
param(
    [int]$OpenWaitMs = 900,
    [string]$Out = ''
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $Out) { $Out = Join-Path $repoRoot 'target\soak\ui' }
New-Item -ItemType Directory -Force -Path $Out | Out-Null

$logPath = Join-Path $env:TEMP 'taskbar-island.log'

# ---- native helpers --------------------------------------------------------
if (-not ('TbiUi' -as [type])) {
    Add-Type -Namespace Win32 -Name Dpi -MemberDefinition @'
[DllImport("user32.dll")] public static extern bool SetProcessDpiAwarenessContext(IntPtr ctx);
'@
    Add-Type @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public class TbiUi {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr h, EnumProc cb, IntPtr l);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] static extern int GetWindowRgn(IntPtr h, IntPtr rgn);
    [DllImport("gdi32.dll")] static extern IntPtr CreateRectRgn(int l, int t, int r, int b);
    [DllImport("gdi32.dll")] static extern int GetRegionData(IntPtr rgn, int size, IntPtr data);
    [DllImport("gdi32.dll")] static extern bool DeleteObject(IntPtr o);
    [DllImport("user32.dll")] static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] static extern void mouse_event(uint flags, uint dx, uint dy, int data, UIntPtr extra);
    [DllImport("user32.dll")] static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    public struct RECT { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] struct RGNDATAHEADER {
        public uint dwSize, iType, nCount, nRgnSize;
        public RECT rcBound;
    }
    [StructLayout(LayoutKind.Sequential)] struct RGNDATA { public RGNDATAHEADER hdr; }

    static IntPtr Match(string cls, bool topOnly) {
        IntPtr found = IntPtr.Zero;
        EnumProc probe = (h, l) => {
            var sb = new StringBuilder(256);
            GetClassNameW(h, sb, 256);
            if (string.Equals(sb.ToString(), cls, StringComparison.OrdinalIgnoreCase) && IsWindowVisible(h)) {
                found = h; return false;
            }
            return true;
        };
        if (topOnly) EnumWindows(probe, IntPtr.Zero);
        else EnumChildWindows(IntPtr.Zero, probe, IntPtr.Zero);
        return found;
    }

    // The island is a WS_CHILD of Shell_TrayWnd, so the top-level pass can
    // miss it depending on shell state; fall back to the child walk.
    public static IntPtr FindByClass(string cls) {
        IntPtr h = Match(cls, true);
        return h != IntPtr.Zero ? h : Match(cls, false);
    }

    // v0.11 Stage A: the pill is a WS_CHILD of Shell_TrayWnd while the
    // top-level window of the same class is the card host - FindByClass
    // returns the host (whose region is empty while the card is closed).
    // Look for the pill under the taskbar first; fall back to FindByClass
    // when the embed is refused and the pill is top-level again.
    public static IntPtr FindPill(string cls) {
        IntPtr tb = FindWindowW("Shell_TrayWnd", null);
        if (tb != IntPtr.Zero) {
            IntPtr found = IntPtr.Zero;
            EnumProc probe = (h, l) => {
                var sb = new StringBuilder(256);
                GetClassNameW(h, sb, 256);
                if (string.Equals(sb.ToString(), cls, StringComparison.OrdinalIgnoreCase)) {
                    found = h; return false;
                }
                return true;
            };
            EnumChildWindows(tb, probe, IntPtr.Zero);
            if (found != IntPtr.Zero) return found;
        }
        return FindByClass(cls);
    }

    // Region rects of both island windows in *screen* coordinates. Stage A
    // split the surface in two: the pill child always carries the pill's
    // rounded region (click-through), the top-level host carries the card +
    // halo region only while the flyout is open (measured: NULLREGION when
    // closed, 66 rects when open). Either window alone misses half the
    // surface; each rect is shifted by its own window's origin, so the union
    // is correct even if the two windows ever stop sharing one rect.
    public static List<RECT> ScreenRects(IntPtr pill, IntPtr host) {
        var outRects = new List<RECT>();
        AddScreen(outRects, pill);
        if (host != pill) AddScreen(outRects, host);
        return outRects;
    }
    static void AddScreen(List<RECT> outRects, IntPtr hwnd) {
        if (hwnd == IntPtr.Zero) return;
        RECT wr; GetWindowRect(hwnd, out wr);
        foreach (var r in RegionRects(hwnd)) {
            outRects.Add(new RECT {
                Left = wr.Left + r.Left, Top = wr.Top + r.Top,
                Right = wr.Left + r.Right, Bottom = wr.Top + r.Bottom
            });
        }
    }

    public static RECT Rect(IntPtr h) { RECT r; GetWindowRect(h, out r); return r; }

    // The island sets its window region to pill + (flyout + shadow halo), so
    // the region's rectangles are the exact on-screen surfaces. GDI stores the
    // union as one span per scan line; the bounding box of all of them is the
    // crop we want (pill and card together). GetWindowRgn returns coordinates
    // relative to the window's own upper-left.
    public static List<RECT> RegionRects(IntPtr hwnd) {
        var outRects = new List<RECT>();
        IntPtr rgn = CreateRectRgn(0, 0, 0, 0);
        try {
            if (GetWindowRgn(hwnd, rgn) == 0) return outRects;   // NULLREGION
            int size = GetRegionData(rgn, 0, IntPtr.Zero);
            if (size <= 0) return outRects;
            IntPtr buf = Marshal.AllocHGlobal(size);
            try {
                if (GetRegionData(rgn, size, buf) == 0) return outRects;
                int header = Marshal.SizeOf(typeof(RGNDATAHEADER));
                var hdr = (RGNDATAHEADER)Marshal.PtrToStructure(buf, typeof(RGNDATAHEADER));
                int rectSize = Marshal.SizeOf(typeof(RECT));
                for (int i = 0; i < hdr.nCount; i++) {
                    IntPtr p = IntPtr.Add(buf, header + i * rectSize);
                    outRects.Add((RECT)Marshal.PtrToStructure(p, typeof(RECT)));
                }
            } finally { Marshal.FreeHGlobal(buf); }
        } finally { DeleteObject(rgn); }
        return outRects;
    }

    public static RECT BBox(List<RECT> rects) {
        var b = new RECT { Left = int.MaxValue, Top = int.MaxValue, Right = int.MinValue, Bottom = int.MinValue };
        foreach (var r in rects) {
            if (r.Left < b.Left) b.Left = r.Left;
            if (r.Top < b.Top) b.Top = r.Top;
            if (r.Right > b.Right) b.Right = r.Right;
            if (r.Bottom > b.Bottom) b.Bottom = r.Bottom;
        }
        return b;
    }

    public static void Click(int x, int y) {
        SetCursorPos(x, y);
        System.Threading.Thread.Sleep(30);
        mouse_event(0x0002, 0, 0, 0, UIntPtr.Zero);   // LEFTDOWN
        System.Threading.Thread.Sleep(30);
        mouse_event(0x0004, 0, 0, 0, UIntPtr.Zero);   // LEFTUP
    }

    // One WM_MOUSEWHEEL posted straight to the island: exactly one message,
    // one notch, no dependence on which window happens to own the focus.
    // window.rs reads the delta from HIWORD(wParam) and the *screen*
    // coordinates from lParam, then converts them with its own origin/scale.
    public static void PostWheel(IntPtr hwnd, int x, int y, int notches) {
        uint lp = (uint)((y & 0xFFFF) << 16) | (uint)(x & 0xFFFF);
        uint wp = (uint)((notches * 120 & 0xFFFF) << 16);
        PostMessage(hwnd, 0x020A /* WM_MOUSEWHEEL */, (IntPtr)wp, (IntPtr)lp);
    }

    // The card's own wnd_proc reads WM_KEYDOWN directly, so no focus dance is
    // needed: posting the real VK_ESCAPE closes whatever is open.
    public static void Escape(IntPtr hwnd) {
        PostMessage(hwnd, 0x100 /* WM_KEYDOWN */, (IntPtr)0x1B /* VK_ESCAPE */, IntPtr.Zero);
    }

    // The island is a WS_CHILD of Shell_TrayWnd and therefore never receives
    // an HWND_BROADCAST; post the theme change to it directly. The handler is
    // `WM_SETTINGCHANGE | WM_THEMECHANGED` -> refresh_theme().
    public static void ThemeChanged(IntPtr hwnd) {
        PostMessage(hwnd, 0x31A /* WM_THEMECHANGED */, IntPtr.Zero, IntPtr.Zero);
    }

    public static void BroadcastSettingChange() {
        IntPtr r;
        SendMessageTimeout((IntPtr)0xffff, 0x1A, IntPtr.Zero, "ImmersiveColorSet", 0x0002, 3000, out r);
        SendMessageTimeout((IntPtr)0xffff, 0x1A, IntPtr.Zero, "WindowsThemeElement", 0x0002, 3000, out r);
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, string l, uint flags, uint timeout, out IntPtr result);
}
'@
}

# Per-monitor v2 awareness so screen coordinates are physical pixels.
[Win32.Dpi]::SetProcessDpiAwarenessContext([IntPtr](-4)) | Out-Null

Add-Type -AssemblyName System.Windows.Forms, System.Drawing

function Save-Screen([string]$Path, [int]$x, [int]$y, [int]$w, [int]$h) {
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

# ---- relaunch the installed copy with logging ------------------------------
# TBI_LOG is read once at first log call, so the running copy has to be
# restarted for `activity.show:` to exist at all.
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
$script:pill = [IntPtr]::Zero
for ($i = 0; $i -lt 40 -and ($h -eq [IntPtr]::Zero -or $script:pill -eq [IntPtr]::Zero); $i++) {
    Start-Sleep -Milliseconds 250
    $h = [TbiUi]::FindByClass('TaskbarIsland')
    $script:pill = [TbiUi]::FindPill('TaskbarIsland')
}
if ($h -eq [IntPtr]::Zero) { throw 'pill window did not come up after relaunch' }
if ($script:pill -eq [IntPtr]::Zero) { $script:pill = $h }
Write-Host ("island windows: host {0}, pill {1}" -f $h, $script:pill)

# ---- pin state, read back from the app's own log --------------------------
function Get-PinLines {
    if (-not (Test-Path -Path $logPath)) { return @() }
    return @(Get-Content -Path $logPath | Where-Object { $_ -like 'activity.show: *' })
}

function Get-Pin {
    $lines = Get-PinLines
    if ($lines.Count -eq 0) { return $null }
    return ($lines[$lines.Count - 1] -replace '^activity\.show: ', '')
}

# Walk to $target one notch at a time, verifying every step against the log.
# Returns the pin actually reached; the caller names the shot from it.
function Set-Pin([string]$Target) {
    for ($i = 0; $i -lt 8; $i++) {
        $now = Get-Pin
        if ($now -eq $Target) { return $now }
        Close-Card                       # an open card owns the wheel (Ignore)
        $before = (Get-PinLines).Count
        [TbiUi]::PostWheel($h, $script:cx, $script:cy, 1)
        for ($w = 0; $w -lt 20; $w++) {
            Start-Sleep -Milliseconds 100
            if ((Get-PinLines).Count -gt $before) { break }
        }
        $script:notches++
    }
    return (Get-Pin)
}

function Pin-Name([string]$Pin) {
    if ($Pin -eq 'None') { return 'auto' }
    if ($Pin -like '*Music*') { return 'music' }
    if ($Pin -like '*Call*') { return 'call' }
    if ($Pin -like '*Shelf*') { return 'shelf' }
    if ($Pin -like '*Timer*') { return 'timer' }
    if ($Pin -like '*Clipboard*') { return 'clipboard' }
    return 'unknown'
}

# ---- region / geometry -----------------------------------------------------
# The region is stored in *window-local* coordinates (GetWindowRgn is always
# relative to the window's own upper-left), so every span has to be shifted by
# the window's screen origin before it can be clicked or photographed.
function Get-Origin([IntPtr]$hwnd) {
    $r = [TbiUi]::Rect($hwnd)
    return @($r.Left, $r.Top)
}

# $script:pillBox is the region as it is *right now* (null while the pill is
# hidden); $script:lastBox remembers the last non-empty one so a hidden state
# can still be photographed at the right place. Boxes are screen coordinates:
# v0.11 Stage A split the surface across the pill child (its own rounded
# region) and the top-level host (the card + halo region while open), so the
# two windows' regions are unioned by ScreenRects. This refresh only ever
# runs with the card closed, so the box is exactly the pill.
function Refresh-Geometry {
    $rects = [TbiUi]::ScreenRects($script:pill, $h)
    if ($rects.Count -eq 0) { $script:pillBox = $null; return }
    $b = [TbiUi]::BBox($rects)
    $script:pillBox = $b
    $script:lastBox = $b
    $script:cx = [int](($b.Left + $b.Right) / 2)
    $script:cy = [int](($b.Top + $b.Bottom) / 2)
}

$shots = New-Object System.Collections.Generic.List[string]

function Shot([string]$Name, [int]$Pad) {
    $rects = [TbiUi]::ScreenRects($script:pill, $h)
    if ($rects.Count -eq 0) { Write-Warning "  skip $Name (empty region)"; return }
    $b = [TbiUi]::BBox($rects)
    $x = [Math]::Max(0, $b.Left - $Pad)
    $y = [Math]::Max(0, $b.Top - $Pad)
    $w = ($b.Right + $Pad) - $x
    $hh = ($b.Bottom + $Pad) - $y
    $path = Join-Path $Out "$Name.png"
    Save-Screen $path $x $y $w $hh
    $shots.Add($path)
    Write-Host ("  {0}: {1}x{2} at ({3},{4})" -f $Name, $w, $hh, $x, $y)
}

# Auto with nothing live sets an empty region: photograph the pill's own
# rectangle (the last place it was visible) so the absence is on record.
function Shot-Hidden([string]$Name) {
    if (-not $script:lastBox) { Write-Warning "  skip $Name (no known pill rect)"; return }
    $o = $script:origin
    $b = $script:lastBox
    $pad = 40
    $x = [Math]::Max(0, $o[0] + $b.Left - $pad)
    $y = [Math]::Max(0, $o[1] + $b.Top - $pad)
    $w = ($o[0] + $b.Right + $pad) - $x
    $hh = ($o[1] + $b.Bottom + $pad) - $y
    $path = Join-Path $Out "$Name.png"
    Save-Screen $path $x $y $w $hh
    $shots.Add($path)
    Write-Host ("  {0}: {1}x{2} at ({3},{4})  [pill hidden - no live activity]" -f $Name, $w, $hh, $x, $y)
}

function Close-Card {
    [TbiUi]::Escape($h)
    Start-Sleep -Milliseconds 450     # close tween is 260 ms
}

# Open the card for the current pin and photograph it. Always closes first:
# a click during the 260 ms close tween would land on a dead hit map.
function Open-Shot([string]$Name) {
    Close-Card
    Refresh-Geometry
    if (-not $script:pillBox) { Shot-Hidden $Name; return }
    [TbiUi]::Click($script:cx, $script:cy)
    Start-Sleep -Milliseconds $OpenWaitMs
    Shot $Name 40
}

# Auto, Music, Call, Shelf, Timer, Clipboard - the pins in SHOW_ORDER.
$targets = @('None', 'Some(Music)', 'Some(Call)', 'Some(Shelf)', 'Some(Timer)', 'Some(Clipboard)')

$personalize = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
$wasSystem = $null
$wasApps = $null
$script:notches = 0
$script:cx = 0
$script:cy = 0
$script:pillBox = $null
$script:lastBox = $null
$script:origin = @(0, 0)
$labels = @()

try {
    $wasSystem = (Get-ItemProperty -Path $personalize -Name SystemUsesLightTheme -ErrorAction SilentlyContinue).SystemUsesLightTheme
    $wasApps = (Get-ItemProperty -Path $personalize -Name AppsUseLightTheme -ErrorAction SilentlyContinue).AppsUseLightTheme
    Write-Host "theme at start: SystemUsesLightTheme=$wasSystem AppsUseLightTheme=$wasApps"

    # ---- bootstrap: land on a pin that has a visible pill ------------------
    # Coords do not matter while no card is open (wheel_action -> Cycle
    # ignores x/y), and the fresh start pin is Auto, whose pill may be hidden.
    $boot = Set-Pin 'Some(Timer)'
    Write-Host ("boot pin: {0} after {1} notch(es)" -f $boot, $script:notches)
    Start-Sleep -Milliseconds 300
    Refresh-Geometry
    if (-not $script:pillBox) { throw 'pill region is empty after bootstrap' }
    Write-Host ("pill box screen ({0},{1})-({2},{3}) -> centre {4},{5}" -f `
            $script:pillBox.Left, $script:pillBox.Top, `
            $script:pillBox.Right, $script:pillBox.Bottom, $script:cx, $script:cy)

    # ---- spec 7: catch the card mid-morph, half-way out of the pill --------
    Close-Card
    [TbiUi]::Click($script:cx, $script:cy)
    Start-Sleep -Milliseconds 150
    Shot '00-morph-midopen' 60
    Start-Sleep -Milliseconds $OpenWaitMs

    # ---- dark pass ---------------------------------------------------------
    $n = 0
    foreach ($t in $targets) {
        $n++
        $actual = Set-Pin $t
        if ($actual -ne $t) { Write-Warning "  wanted $t, reached $actual" }
        $name = "{0:d2}-{1}-dark" -f $n, (Pin-Name $actual)
        $labels += $name
        Open-Shot $name
    }

    # ---- light theme -------------------------------------------------------
    Close-Card
    Set-ItemProperty -Path $personalize -Name SystemUsesLightTheme -Value 1
    Set-ItemProperty -Path $personalize -Name AppsUseLightTheme -Value 1
    [TbiUi]::BroadcastSettingChange()
    [TbiUi]::ThemeChanged($h)
    Start-Sleep -Milliseconds 1500
    Write-Host 'switched to light'

    $n = 0
    foreach ($t in $targets) {
        $n++
        $actual = Set-Pin $t
        if ($actual -ne $t) { Write-Warning "  wanted $t, reached $actual" }
        $name = "{0:d2}-{1}-light" -f $n, (Pin-Name $actual)
        $labels += $name
        Open-Shot $name
    }
}
finally {
    # ---- restore the owner's theme, whatever happened above ----------------
    Close-Card
    if ($null -ne $wasSystem) {
        Set-ItemProperty -Path $personalize -Name SystemUsesLightTheme -Value $wasSystem
    }
    if ($null -ne $wasApps) {
        Set-ItemProperty -Path $personalize -Name AppsUseLightTheme -Value $wasApps
    }
    [TbiUi]::BroadcastSettingChange()
    [TbiUi]::ThemeChanged($h)
    Start-Sleep -Milliseconds 800
    # Walk back to Auto (SHOW_ORDER has six entries), the state a fresh
    # launch starts in, no matter where an abort left the pin.
    $back = (6 - ($script:notches % 6)) % 6
    for ($i = 0; $i -lt $back; $i++) {
        Close-Card
        [TbiUi]::PostWheel($h, $script:cx, $script:cy, 1)
        Start-Sleep -Milliseconds 300
    }
    # Keep the log as evidence inside the repo, then drop the temp copy.
    $finalPin = Get-Pin
    Get-PinLines | Out-File -FilePath (Join-Path $Out 'pins.log') -Encoding utf8
    Remove-Item -Path $logPath -ErrorAction SilentlyContinue
    Write-Host "theme restored: SystemUsesLightTheme=$wasSystem AppsUseLightTheme=$wasApps"
    Write-Host ("{0} notch(es) posted, final pin {1}" -f $script:notches, $finalPin)
}

Write-Host ''
Write-Host "saved $($shots.Count) screenshots to $Out"
$shots | ForEach-Object { Write-Host "  $_" }

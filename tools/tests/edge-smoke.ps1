<#
  v0.12.0 acceptance (design brief section 6): the four taskbar edges,
  side-edge pause with its one notice card, shell-element clicks, an
  Explorer restart, a fullscreen window, and the TranslucentTB material x
  pill-mode matrix - all driven against the *installed* copy.

  What each phase proves, and how:

    * Edges: `--simulate-edge` is delivered to the running island (a second
      launch posts WM_APP_EDGE_SIMULATE and exits), so every flip travels the
      same 150 ms debounce a real taskbar move takes. Each flip is waited
      out on the app's own `anchor.edge: flipped to ...` log line - the
      script never assumes the layout moved.
    * Open/close x20 per horizontal edge: click the pill, wait for the host
      region to grow (card up), Escape, wait for it to empty (card down).
      A cycle only counts when both waits succeed.
    * Side edges: `island.pause: side taskbar` + the notice region up;
      Escape logs `notice.dismiss` and empties the region (paused keeps
      nothing on screen - the pill is clipped away with it); flipping to the
      *other* side edge raises a fresh notice (one per edge change).
    * Shell elements x20: Start, the clock and quick settings - clicked by
      coordinate (Win11's taskbar is all XAML, there are no child windows to
      find), each verified by the foreground window becoming the shell's own
      `Windows.UI.Core.CoreWindow` panel and returning on Escape. The pill's
      state (visible + non-empty region) is sampled ~14x per cycle: any
      transition is a blink and fails the run.
    * Explorer restart: run while the pill is a WS_CHILD (Taskbar mode
      `embedded`, written to settings.json before a restart), then measure
      Shell_TrayWnd's return to the pill child's reappearance - the section-4
      budget is < 1 s.
    * Fullscreen: a topmost borderless window over the whole screen. The
      pixel under the pill must be the form's colour, i.e. the island does
      not float over fullscreen content (the spike never covered this), and
      the process must stay alive.
    * Matrix: TranslucentTB's own config (RoamingState\settings.json) picks
      clear/blur/acrylic - stop TTB, write, start - and settings.json picks
      auto/embedded, so material x pill-mode runs 6 combos x dark/light with
      a pill-strip screenshot each. Afterwards: Event Viewer 1000/1002 for
      taskbar-island and new CrashDumps files must be zero (section 4).

  Preconditions: installed copy running (install-smoke first), taskbar on the
  bottom edge at 1920x1200 left-aligned (shell click points derive from the
  tray rect), TranslucentTB present for the matrix. Machine state - theme,
  TTB config, island settings, running island - is restored in `finally`,
  including after an aborted run.

  Honest limits (reported as SKIP, not PASS): a natural auto-hide hide ->
  reveal cannot be forced in this environment (three methods failed for
  v0.11 - the bar is configured auto-hide but never tucks), and the DPI
  100/125/150 matrix is not scriptable on the owner's machine.

  Everything is ASCII: Windows PowerShell 5.1 reads a BOM-less script as the
  ANSI code page and a non-ASCII byte inside a string literal can end it.

.EXAMPLE
  powershell -File tools\tests\edge-smoke.ps1
#>
[CmdletBinding()]
param(
    [int]$OpenWaitMs = 900,
    [int]$Cycles = 20,
    [int]$StartX = 34,
    [int]$ClockDx = 23,
    [int]$QsDx = 70,
    [switch]$SkipMatrix
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$out = Join-Path $repoRoot 'target\soak\edge'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$logPath = Join-Path $env:TEMP 'taskbar-island.log'
$personalize = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
$settingsPath = Join-Path $env:APPDATA 'TaskbarIsland\settings.json'
$ttbSettings = Join-Path $env:LOCALAPPDATA 'Packages\28017CharlesMilette.TranslucentTB_v826wp6bftszj\RoamingState\settings.json'

# ---- shared native helpers (ui-smoke.ps1, one definition, no drift) --------
$src = Get-Content -Path (Join-Path $PSScriptRoot 'ui-smoke.ps1')
Invoke-Expression (($src[55..242]) -join "`n")

# Probe additions TbiUi keeps private: visibility, parent, foreground, real
# clicks and a real Escape key event (the shell panels need the latter).
Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public class TbiProbe {
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr FindWindowW(string cls, string title);
    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    public struct RECT { public int Left, Top, Right, Bottom; }
    // Why a Shell_TrayWnd lookup failed: absent, hidden, or off-screen.
    // FindWindowW and EnumWindows+IsWindowVisible do not agree, so report
    // both rather than guessing which one the failure was in.
    public static string TrayDiag() {
        IntPtr fw = FindWindowW("Shell_TrayWnd", null);
        if (fw == IntPtr.Zero) { return "FindWindowW=NONE (window absent)"; }
        RECT r; GetWindowRect(fw, out r);
        return "FindWindowW=0x" + fw.ToInt64().ToString("X")
             + " vis=" + IsWindowVisible(fw)
             + " rect=(" + r.Left + "," + r.Top + ")-(" + r.Right + "," + r.Bottom + ")";
    }
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")]
    public static extern void mouse_event(uint f, uint dx, uint dy, int d, UIntPtr e);
    [DllImport("user32.dll")]
    public static extern void keybd_event(byte vk, byte sc, uint f, UIntPtr e);
    public static void Click(int x, int y) {
        SetCursorPos(x, y); System.Threading.Thread.Sleep(40);
        mouse_event(2, 0, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(40);
        mouse_event(4, 0, 0, 0, UIntPtr.Zero);
    }
    public static void Esc() {
        keybd_event(0x1B, 0, 0, UIntPtr.Zero);
        System.Threading.Thread.Sleep(50);
        keybd_event(0x1B, 0, 2, UIntPtr.Zero);
    }
    public static string Fg() {
        IntPtr h = GetForegroundWindow();
        var c = new StringBuilder(256); GetClassNameW(h, c, 256);
        var t = new StringBuilder(512); GetWindowTextW(h, t, 512);
        return c + " | " + t;
    }
}
'@

function Save-Screen([string]$Path, [int]$x, [int]$y, [int]$w, [int]$h) {
    $bmp = New-Object System.Drawing.Bitmap($w, $h)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($x, $y, 0, 0, $bmp.Size)
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bmp.Dispose()
}

# ---- results ---------------------------------------------------------------
$script:results = New-Object System.Collections.Generic.List[object]
function Report([string]$name, $ok, [string]$detail = '') {
    $script:results.Add([pscustomobject]@{ ok = $ok; name = $name; detail = $detail })
    if ($ok -eq $null) {
        Write-Host "  [SKIP] $name - $detail"
    } elseif ($ok) {
        Write-Host "  [PASS] $name$(if ($detail) { " - $detail" })"
    } else {
        Write-Host "  [FAIL] $name - $detail"
    }
}
function Note([string]$text) { Write-Host "         $text" }

# ---- log tail --------------------------------------------------------------
function Get-Log {
    if (Test-Path -Path $logPath) { return @(Get-Content -Path $logPath) }
    return @()
}
# Wait for a new line (at or after index $since) matching $pattern.
function Wait-Log([string]$pattern, [int]$since, [int]$timeoutMs = 5000) {
    $deadline = (Get-Date).AddMilliseconds($timeoutMs)
    while ((Get-Date) -lt $deadline) {
        $lines = Get-Log
        for ($i = $since; $i -lt $lines.Count; $i++) {
            if ($lines[$i] -match $pattern) { return $true }
        }
        Start-Sleep -Milliseconds 100
    }
    return $false
}

# ---- island windows --------------------------------------------------------
$script:h = [IntPtr]::Zero
$script:pill = [IntPtr]::Zero
function Refresh-Island {
    for ($i = 0; $i -lt 40; $i++) {
        $script:h = [TbiUi]::FindByClass('TaskbarIsland')
        $script:pill = [TbiUi]::FindPill('TaskbarIsland')
        if ($script:h -ne [IntPtr]::Zero -and $script:pill -ne [IntPtr]::Zero) { return $true }
        Start-Sleep -Milliseconds 250
    }
    return $false
}
# One string per state: visibility + whether the region paints anything.
# While paused the region is the notice (or empty after dismissing it), so
# this single value tracks every on-screen transition of the island.
function Pill-State {
    $v = [TbiProbe]::IsWindowVisible($script:pill)
    $r = [TbiUi]::RegionRects($script:pill).Count -gt 0
    return "$v/$r"
}
function Close-Card {
    [TbiUi]::Escape($script:h)
    Start-Sleep -Milliseconds 450
}
function Get-HostRects { return [TbiUi]::RegionRects($script:h) }

# ---- pins (wheel -> log-verified, ui-smoke's walk) -------------------------
function Get-Pin {
    $lines = @((Get-Log) | Where-Object { $_ -like 'activity.show: *' })
    if ($lines.Count -eq 0) { return $null }
    return ($lines[-1] -replace '^activity\.show: ', '')
}
# The wheel is hit-tested against the pill rect (window.rs converts the
# posted screen point into surface space and calls pill.contains), so it has
# to land on the pill itself - ui-smoke posts the pill's centre for the
# same reason.
function Get-PillCenter {
    $rects = [TbiUi]::ScreenRects($script:pill, $script:h)
    if ($rects.Count -eq 0) { return $null }
    $b = [TbiUi]::BBox($rects)
    return @([int](($b.Left + $b.Right) / 2), [int](($b.Top + $b.Bottom) / 2))
}
function Set-Pin([string]$Target) {
    for ($i = 0; $i -lt 8; $i++) {
        if ((Get-Pin) -eq $Target) { return $true }
        Close-Card
        # wheel() only gates on the flyout being closed (window.rs
        # wheel_action), so any point works while the card is shut; the pill
        # centre is used when the region has one, and (0,0) otherwise - an
        # idle Auto state has an empty region and must still be walkable.
        $c = Get-PillCenter
        $wx = 0
        $wy = 0
        if ($c) { $wx = $c[0]; $wy = $c[1] }
        $before = @((Get-Log) | Where-Object { $_ -like 'activity.show: *' }).Count
        [TbiUi]::PostWheel($script:h, $wx, $wy, 1)
        for ($w = 0; $w -lt 20; $w++) {
            Start-Sleep -Milliseconds 100
            $now = @((Get-Log) | Where-Object { $_ -like 'activity.show: *' }).Count
            if ($now -gt $before) { break }
        }
    }
    return ((Get-Pin) -eq $Target)
}

# ---- edge flips ------------------------------------------------------------
# Deliver --simulate-edge to the running island; wait for the app's own
# commit line. Returns $true only when the debounce pipeline reported it.
function Invoke-Edge([string]$edge) {
    $before = (Get-Log).Count
    $psi = Start-Process -FilePath $exe -ArgumentList @('--simulate-edge', $edge) `
        -PassThru -WindowStyle Hidden
    [void]$psi.WaitForExit(10000)
    $delivered = Wait-Log 'anchor\.edge: simulate' $before 4000
    $flipped = Wait-Log ("anchor\.edge: flipped to " + (Get-Culture).TextInfo.ToTitleCase($edge)) $before 6000
    Start-Sleep -Milliseconds 350      # the 200 ms slide settles
    return ($delivered -and $flipped)
}

# ---- open/close cycles -----------------------------------------------------
# "Open" is the island's *union box growing*, not the host region becoming
# non-empty. The card adds ~500 x 240 px of surface (above the pill on a
# bottom bar, below it on a top bar), so the box grows in either orientation.
# The region-count test ui-smoke uses only works while the pill is a
# WS_CHILD: in safe mode the pill is top-level and FindPill resolves to the
# *same* window as the host, whose region always carries the pill and never
# reads empty - which is exactly why the first acceptance run scored 0/20 on
# every horizontal edge while the embedded matrix combos scored 2/2.
function Get-UnionBox {
    $rects = [TbiUi]::ScreenRects($script:pill, $script:h)
    if ($rects.Count -eq 0) { return $null }
    return [TbiUi]::BBox($rects)
}
function Test-OneCycle {
    Close-Card
    $closed = Get-UnionBox
    if (-not $closed) { return $false }
    $cw = $closed.Right - $closed.Left
    $ch = $closed.Bottom - $closed.Top
    [TbiUi]::Click([int](($closed.Left + $closed.Right) / 2), [int](($closed.Top + $closed.Bottom) / 2))
    $opened = $false
    for ($w = 0; $w -lt 30; $w++) {
        Start-Sleep -Milliseconds 100
        $b = Get-UnionBox
        if ($b -and ((($b.Bottom - $b.Top) -ge ($ch + 80)) -or (($b.Right - $b.Left) -ge ($cw + 80)))) {
            $opened = $true
            break
        }
    }
    if (-not $opened) { return $false }
    [TbiUi]::Escape($script:h)
    for ($w = $w; $w -lt 55; $w++) {
        Start-Sleep -Milliseconds 100
        $b = Get-UnionBox
        if ($b -and (($b.Bottom - $b.Top) -lt ($ch + 40)) -and (($b.Right - $b.Left) -lt ($cw + 40))) {
            return $true
        }
    }
    return $false
}
function Test-OpenClose([string]$label, [int]$n) {
    $ok = 0
    for ($i = 0; $i -lt $n; $i++) {
        if (Test-OneCycle) { $ok++ }
    }
    Report "$label open/close x$n" ($ok -eq $n) "$ok/$n cycles"
    return $ok
}

# ---- shell elements --------------------------------------------------------
# Click one shell element xN. Open = the foreground window becomes the
# shell's own panel (Win11 hosts Start/clock/quick settings as CoreWindow
# popups); closed = foreground returns. The pill is sampled throughout:
# any state transition is a blink.
function Test-ShellElement([string]$name, [int]$x, [int]$y) {
    $baselineFg = [TbiProbe]::Fg()
    $lastState = Pill-State
    $transitions = 0
    $opened = 0
    $closed = 0
    for ($i = 1; $i -le $Cycles; $i++) {
        [TbiProbe]::Click($x, $y)
        $sawOpen = $false
        for ($w = 0; $w -lt 10; $w++) {
            Start-Sleep -Milliseconds 70
            $s = Pill-State
            if ($s -ne $lastState) { $transitions++; $lastState = $s }
            $fg = [TbiProbe]::Fg()
            if ($fg -ne $baselineFg -and $fg -like 'Windows.UI.Core.CoreWindow*') { $sawOpen = $true }
        }
        [TbiProbe]::Esc()
        $sawReturn = $false
        for ($w = 0; $w -lt 8; $w++) {
            Start-Sleep -Milliseconds 70
            $s = Pill-State
            if ($s -ne $lastState) { $transitions++; $lastState = $s }
            $fg = [TbiProbe]::Fg()
            if ($fg -eq $baselineFg -or $fg -notlike 'Windows.UI.Core.CoreWindow*') { $sawReturn = $true }
        }
        if ($sawOpen) { $opened++ } else { [TbiProbe]::Esc(); Start-Sleep -Milliseconds 250 }
        if ($sawReturn) { $closed++ }
    }
    Report "$name opens/closes x$Cycles" ($opened -eq $Cycles -and $closed -eq $Cycles) `
        "opened $opened/$Cycles, closed $closed/$Cycles"
    Report "$name zero pill blinks" ($transitions -eq 0) "$transitions state transitions"
}

# ---- island lifecycle ------------------------------------------------------
function Stop-Island {
    Stop-Process -Name taskbar-island -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 700
}
function Start-Island {
    param([string[]]$ArgumentList = @())
    $env:TBI_LOG = '1'
    if ($ArgumentList.Count -gt 0) {
        Start-Process -FilePath $exe -ArgumentList $ArgumentList | Out-Null
    } else {
        Start-Process -FilePath $exe | Out-Null
    }
    if (-not (Refresh-Island)) { throw 'island windows did not come up' }
}
function Restart-Island {
    param([string[]]$ArgumentList = @())
    Stop-Island
    Start-Island -ArgumentList $ArgumentList
}
# Taskbar mode is read from settings.json at startup (window.rs EMBED_MODE),
# so a file edit + restart is the tray menu's equivalent for automation.
function Set-IslandMode([string]$mode) {
    $raw = Get-Content -Path $settingsPath -Raw
    if ($raw -match '"taskbarMode"') {
        $raw = $raw -replace '"taskbarMode"\s*:\s*"[a-z]+"', ('"taskbarMode": "' + $mode + '"')
    } else {
        $raw = $raw -replace '(\r?\n})$', (',' + "`r`n" + '  "taskbarMode": "' + $mode + '"' + '$1')
        if ($raw -notmatch '"taskbarMode"') { $raw = $raw -replace '}$', ('  "taskbarMode": "' + $mode + '"}' ) }
    }
    Set-Content -Path $settingsPath -Value $raw -NoNewline -Encoding ASCII
    $script:settingsChanged = $true
}

# ---- transcript state ------------------------------------------------------
$script:start = Get-Date
$script:themeChanged = $false
$script:ttbChanged = $false
$script:settingsChanged = $false
$wasSystem = $null
$wasApps = $null
$ttbBackup = Join-Path $out 'ttb-settings.backup.json'
$settingsBackup = Join-Path $out 'settings.backup.json'
$script:exitNote = ''

try {
    # ---- precondition: installed copy --------------------------------------
    $p = Get-Process -Name taskbar-island -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $p) { throw 'taskbar-island is not running - run tools\tests\install-smoke.ps1 first.' }
    $exe = $p.Path
    Write-Host "installed copy: $exe (pid $($p.Id))"

    if (-not (Test-Path -Path $ttbSettings)) { throw "TranslucentTB config not found at $ttbSettings" }
    Copy-Item -Path $ttbSettings -Destination $ttbBackup -Force
    Copy-Item -Path $settingsPath -Destination $settingsBackup -Force

    # ---- prep: relaunch with the log, pin, wait out the safe-mode toast ----
    Stop-Island
    Remove-Item -Path $logPath -ErrorAction SilentlyContinue
    Start-Island
    Write-Host "island windows: host $script:h, pill $script:pill"

    # The safe-mode tray balloon is a real toast; it takes the top-right of
    # the screen for a few seconds and must be gone before any clicking.
    Start-Sleep -Milliseconds 12000

    $baseline = (Get-Log) | Where-Object { $_ -match 'anchor\.edge: raw=' } | Select-Object -First 1
    if ($baseline -match '-> Bottom') {
        Report 'real taskbar edge reads Bottom (uEdge, not the return value)' $true ($baseline.Trim())
    } else {
        Report 'real taskbar edge reads Bottom (uEdge, not the return value)' $false `
            "expected Bottom, log says: $baseline"
        throw 'this script needs the real taskbar on the bottom edge'
    }

    if (-not (Set-Pin 'Some(Timer)')) { throw 'could not pin Timer for a visible pill' }
    Close-Card
    Start-Sleep -Milliseconds 300

    # ---- phase 1: four edges ----------------------------------------------
    Write-Host ''
    Write-Host 'phase 1: edge flips + open/close + pause/resume'
    $flips = 0

    # Baseline (real bottom edge, no flip needed): 20x open/close.
    Test-OpenClose 'bottom edge (real bar)' $Cycles | Out-Null

    if (Invoke-Edge 'top') { $flips++ }
    $topFlip = (Get-Log) -match 'anchor\.edge: flipped to Top'
    Report 'flip bottom -> top commits through the 150 ms debounce' ($topFlip.Count -ge 1)
    Test-OpenClose 'top edge' $Cycles | Out-Null

    # Side edge: pause + one notice card.
    $before = (Get-Log).Count
    if (Invoke-Edge 'left') { $flips++ }
    Report 'flip to left edge delivered + committed' ((Wait-Log 'island\.pause: side taskbar' $before 4000))
    $noticeUp = $false
    for ($w = 0; $w -lt 30; $w++) {
        Start-Sleep -Milliseconds 100
        if ((Get-HostRects).Count -gt 0) { $noticeUp = $true; break }
    }
    Report 'left edge: notice card on screen' $noticeUp 'host region carries the notice'
    $pauseLines = @((Get-Log) | Where-Object { $_ -like 'island.pause: *' }).Count
    Report 'left edge: paused exactly once' ($pauseLines -eq 1) "$pauseLines island.pause line(s)"

    # Esc drops the notice; the island stays paused with nothing on screen.
    $before = (Get-Log).Count
    [TbiUi]::Escape($script:h)
    $dismissed = Wait-Log 'notice\.dismiss' $before 3000
    Start-Sleep -Milliseconds 300
    $emptyWhilePaused = ((Get-HostRects).Count -eq 0)
    Report 'notice dismissible, pause keeps nothing on screen' `
        ($dismissed -and $emptyWhilePaused) "dismiss=$dismissed region-empty=$emptyWhilePaused"

    # The other side edge raises a fresh notice (one per edge change).
    $before = (Get-Log).Count
    if (Invoke-Edge 'right') { $flips++ }
    $rightFlip = Wait-Log 'anchor\.edge: flipped to Right' $before 4000
    $reNotice = $false
    for ($w = 0; $w -lt 30; $w++) {
        Start-Sleep -Milliseconds 100
        if ((Get-HostRects).Count -gt 0) { $reNotice = $true; break }
    }
    Report 'right edge: fresh notice per edge change' ($rightFlip -and $reNotice)
    $pauseLines = @((Get-Log) | Where-Object { $_ -like 'island.pause: *' }).Count
    Report 'still one pause line across both side edges' ($pauseLines -eq 1) "$pauseLines island.pause line(s)"

    # Back to horizontal: auto-resume, notice gone, 20x open/close again.
    $before = (Get-Log).Count
    if (Invoke-Edge 'bottom') { $flips++ }
    $resumed = Wait-Log 'island\.resume: top/bottom taskbar' $before 4000
    Start-Sleep -Milliseconds 300
    $pillBack = ((Get-HostRects).Count -gt 0)
    Report 'bottom edge: auto-resume, pill back, notice gone' ($resumed -and $pillBack)
    Report 'every edge flip committed through the 150 ms debounce' ($flips -eq 4) "$flips/4 flips"
    Test-OpenClose 'bottom edge (after resume)' $Cycles | Out-Null

    # ---- phase 2: shell elements ------------------------------------------
    Write-Host ''
    Write-Host "phase 2: $Cycles clicks x each shell element, pill sampled throughout"
    $tray = [TbiUi]::FindByClass('Shell_TrayWnd')
    $tr = [TbiUi]::Rect($tray)
    $taskbarY = [int](($tr.Top + $tr.Bottom) / 2)
    $clockX = $tr.Right - $ClockDx
    $qsX = $tr.Right - $QsDx
    Note "taskbar y=$taskbarY, Start x=$StartX, clock x=$clockX, quick settings x=$qsX"
    Test-ShellElement 'Start menu' $StartX $taskbarY
    Test-ShellElement 'clock (notification center)' $clockX $taskbarY
    Test-ShellElement 'quick settings' $qsX $taskbarY

    # ---- phase 3: explorer restart (needs a WS_CHILD pill) -----------------
    Write-Host ''
    Write-Host 'phase 3: Explorer restart re-attach (embedded mode)'
    # A shell-flyout cycle earlier in the run can wedge the taskbar's window
    # list: FindWindowW still returns Shell_TrayWnd (the island's own lookup
    # is immune, so geometry and the pill stay correct) but EnumWindows no
    # longer visits it, so SetParent fails with error 87 and every embed
    # attempt is refused while the taskbar looks otherwise healthy. The only
    # proven fix is restarting Explorer, so heal before embedding rather than
    # reporting three refused attempts as a product failure.
    $enumTray = [TbiUi]::FindByClass('Shell_TrayWnd')
    $findTray = [TbiProbe]::FindWindowW('Shell_TrayWnd', $null)
    if ($enumTray -eq [IntPtr]::Zero -and $findTray -ne [IntPtr]::Zero) {
        Note 'wedged taskbar: EnumWindows misses Shell_TrayWnd; restarting Explorer to heal'
        Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
        $tHeal = Get-Date
        while ((Get-Date) -lt $tHeal.AddSeconds(30)) {
            if ([TbiUi]::FindByClass('Shell_TrayWnd') -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 150
        }
        # Let the fresh bar settle before the embed race starts.
        Start-Sleep -Seconds 2
        Note ('healed: ' + [TbiProbe]::TrayDiag())
    }
    # The embed is refused once per process when it loses the startup race,
    # and EMBED_REFUSED then blocks the 2 s guard for the whole session - the
    # same one-shot refusal AGENTS.md records for 0.11.0 (10/10 clean restarts
    # in isolation, so this is a flake, not a deterministic break). A refused
    # attempt therefore gets a fresh process rather than a longer wait.
    # Read the embed state, with enough context to tell "the pill is not a
    # child" apart from "the taskbar window itself could not be found".
    function Get-EmbedState {
        $tray = [IntPtr]::Zero
        for ($w = 0; $w -lt 20 -and $tray -eq [IntPtr]::Zero; $w++) {
            $tray = [TbiUi]::FindByClass('Shell_TrayWnd')
            if ($tray -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 150 }
        }
        $pill = [TbiUi]::FindPill('TaskbarIsland')
        $par = [IntPtr]::Zero
        if ($pill -ne [IntPtr]::Zero) { $par = [TbiProbe]::GetParent($pill) }
        $ok = ($pill -ne [IntPtr]::Zero -and $tray -ne [IntPtr]::Zero -and $par -eq $tray)
        $detail = "pill=$pill parent=$par tray=$tray [$([TbiProbe]::TrayDiag())]"
        return @{ ok = $ok; detail = $detail }
    }

    $embedded = $false
    $attempts = 0
    $lastDetail = 'not attempted'
    for ($a = 1; $a -le 3 -and -not $embedded; $a++) {
        $attempts = $a
        Set-IslandMode 'embedded'
        $since = (Get-Log).Count
        Restart-Island
        # The embed decision is logged during startup; wait for it rather than
        # reading a window list that may predate the SetParent.
        Wait-Log 'pill\.embed' $since 8000 | Out-Null
        $state = Get-EmbedState
        $embedded = $state.ok
        $lastDetail = $state.detail
        if ($embedded) { break }

        # Section 4's own retry: the tray's Taskbar mode choice posts
        # WM_APP_PILL_MODE (WM_APP + 57, wparam = TaskbarMode::code(),
        # Embedded = 1) and apply_taskbar_mode clears the one-shot
        # EMBED_REFUSED before rebuilding the pill - a fresh process is not
        # the only way out of a refused embed.
        Note "attempt $a did not embed ($lastDetail); re-applying Taskbar mode"
        $since2 = (Get-Log).Count
        [TbiProbe]::PostMessage($script:h, 0x8039, [IntPtr]1, [IntPtr]::Zero)
        Wait-Log 'pill\.mode|pill\.embed' $since2 6000 | Out-Null
        Start-Sleep -Milliseconds 600
        $state = Get-EmbedState
        $embedded = $state.ok
        $lastDetail = $state.detail
        if (-not $embedded) { Note "retry after Taskbar mode did not embed ($lastDetail)" }
    }
    Report 'Taskbar mode Always embedded: pill is a WS_CHILD' $embedded `
        "$lastDetail after $attempts attempt(s)"
    if (-not $embedded) {
        $embedLines = @((Get-Log) | Where-Object { $_ -match 'pill\.embed' })
        if ($embedLines.Count -gt 0) { Note ('last pill.embed line: ' + $embedLines[-1]) }
    }
    if (-not (Set-Pin 'Some(Timer)')) { Note 'pin walk failed after the embed restart (continuing)' }
    Close-Card

    if ($embedded) {
        $islandPid = (Get-Process -Name taskbar-island | Select-Object -First 1).Id
        Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
        $tDown = Get-Date
        $trayUp = [IntPtr]::Zero
        while ((Get-Date) -lt $tDown.AddSeconds(30)) {
            $trayUp = [TbiUi]::FindByClass('Shell_TrayWnd')
            if ($trayUp -ne [IntPtr]::Zero) { break }
            Start-Sleep -Milliseconds 50
        }
        $tTray = Get-Date
        $reattached = $false
        $reattachMs = -1
        while ((Get-Date) -lt $tTray.AddSeconds(5)) {
            $child = [TbiUi]::FindPill('TaskbarIsland')
            if ($child -ne [IntPtr]::Zero -and [TbiProbe]::GetParent($child) -eq $trayUp) {
                $reattached = $true
                $reattachMs = [int]((Get-Date) - $tTray).TotalMilliseconds
                break
            }
            Start-Sleep -Milliseconds 50
        }
        Report 'TaskbarCreated re-attach < 1 s' ($reattached -and $reattachMs -lt 1000) `
            "pill child under the new taskbar after $reattachMs ms"
        $alive = (Get-Process -Name taskbar-island -ErrorAction SilentlyContinue) -ne $null
        $samePid = $false
        try { $samePid = (Get-Process -Name taskbar-island -ErrorAction Stop).Id -eq $islandPid } catch { }
        Report 'island survived the Explorer restart' ($alive -and $samePid)
        Start-Sleep -Milliseconds 1500
        if (-not (Refresh-Island)) { Note 'window handles changed after restart' }
    } else {
        Report 'TaskbarCreated re-attach < 1 s' $null 'skipped: pill did not embed'
        Report 'island survived the Explorer restart' $null 'skipped: pill did not embed'
    }

    # ---- phase 4: fullscreen ----------------------------------------------
    Write-Host ''
    Write-Host 'phase 4: fullscreen window over the whole screen'
    Close-Card
    $pillBox = $null
    $rects = [TbiUi]::ScreenRects($script:pill, $script:h)
    if ($rects.Count -gt 0) { $pillBox = [TbiUi]::BBox($rects) }
    if (-not $pillBox) { Restart-Island; if (Set-Pin 'Some(Timer)') { Close-Card } }
    $rects = [TbiUi]::ScreenRects($script:pill, $script:h)
    if ($rects.Count -gt 0) { $pillBox = [TbiUi]::BBox($rects) }

    $form = New-Object System.Windows.Forms.Form
    $form.FormBorderStyle = 'None'
    $form.StartPosition = 'Manual'
    $form.Bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $form.TopMost = $true
    $form.ShowInTaskbar = $false
    $form.BackColor = [System.Drawing.Color]::White
    $form.Show()
    Start-Sleep -Milliseconds 2500
    $aliveDuring = $null -ne (Get-Process -Name taskbar-island -ErrorAction SilentlyContinue)
    $shot = Join-Path $out 'fullscreen.png'
    $sb = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    Save-Screen $shot $sb.X $sb.Y $sb.Width $sb.Height
    $pixelOk = $null
    if ($pillBox) {
        $cx = [int](($pillBox.Left + $pillBox.Right) / 2)
        $cy = [int](($pillBox.Top + $pillBox.Bottom) / 2)
        $bmp = New-Object System.Drawing.Bitmap(1, 1)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.CopyFromScreen($cx, $cy, 0, 0, (New-Object System.Drawing.Size(1, 1)))
        $c = $bmp.GetPixel(0, 0)
        $lum = ([int]$c.R + [int]$c.G + [int]$c.B) / 3
        $pixelOk = ($lum -gt 200)   # white form on top; a dark pill would float over it
        Note "pixel under pill centre ($cx,$cy) = rgb($($c.R),$($c.G),$($c.B))"
        $g.Dispose(); $bmp.Dispose()
    }
    $form.Close(); $form.Dispose()
    Start-Sleep -Milliseconds 800
    Report 'fullscreen: island stayed alive' ([bool]$aliveDuring)
    Report 'fullscreen: island does not float over the content' ([bool]$pixelOk) `
        'pill-centre pixel is the white form, not the pill'
    $rects = [TbiUi]::ScreenRects($script:pill, $script:h)
    Report 'fullscreen: island back afterwards' ($rects.Count -gt 0)
    $panics = @((Get-Log) | Where-Object { $_ -match 'panic|RefCell' }).Count
    Report 'no panic in the log so far' ($panics -eq 0) "$panics panic line(s)"

    # ---- phase 5: TranslucentTB material x pill mode -----------------------
    if ($SkipMatrix) {
        Write-Host ''
        Report 'TranslucentTB material x mode matrix' $null 'skipped by -SkipMatrix'
    } else {
        Write-Host ''
        Write-Host 'phase 5: TranslucentTB material x pill-mode matrix (6 combos, dark + light)'
        $ttbApp = Get-StartApps | Where-Object { $_.Name -like '*Translucent*' } | Select-Object -First 1
        if (-not $ttbApp) {
            Report 'TranslucentTB matrix' $null 'TranslucentTB not in StartApps'
        } else {
            $ttbAumid = $ttbApp.AppID
            $wasSystem = (Get-ItemProperty -Path $personalize -Name SystemUsesLightTheme -ErrorAction SilentlyContinue).SystemUsesLightTheme
            $wasApps = (Get-ItemProperty -Path $personalize -Name AppsUseLightTheme -ErrorAction SilentlyContinue).AppsUseLightTheme
            $themeChanged = $true

            function Set-TtbAccent([string]$accent) {
                # Stop first: TTB rewrites its config on the way out and
                # would clobber a live edit.
                Stop-Process -Name TranslucentTB -Force -ErrorAction SilentlyContinue
                Start-Sleep -Milliseconds 800
                $raw = Get-Content -Path $ttbSettings -Raw
                $raw = $raw -replace '"accent":\s*"[a-z]+"', ('"accent": "' + $accent + '"')
                Set-Content -Path $ttbSettings -Value $raw -NoNewline -Encoding UTF8
                $script:ttbChanged = $true
                $before = (Get-Log).Count
                Start-Process -FilePath "shell:AppsFolder\$ttbAumid" | Out-Null
                $deadline = (Get-Date).AddSeconds(20)
                while ((Get-Date) -lt $deadline) {
                    if (Get-Process -Name TranslucentTB -ErrorAction SilentlyContinue) { break }
                    Start-Sleep -Milliseconds 250
                }
                # A live process is not enough: an `auto` combo decides from
                # the island's own coexist watcher, so wait until the *island*
                # acknowledges TTB. The first matrix combo restarted before
                # that landed and honestly (but wrongly for the test) embedded
                # under a tool it had not yet seen - the clear/auto failure in
                # the first acceptance run.
                if (-not (Wait-Log 'coexist\.watch: TranslucentTB started' $before 12000)) {
                    Note 'TranslucentTB did not report back to the island watcher within 12 s'
                }
                Start-Sleep -Milliseconds 1500    # the taskbar restyles
            }
            function Set-Theme([int]$sys, [int]$apps) {
                Set-ItemProperty -Path $personalize -Name SystemUsesLightTheme -Value $sys
                Set-ItemProperty -Path $personalize -Name AppsUseLightTheme -Value $apps
                [TbiUi]::BroadcastSettingChange()
                [TbiUi]::ThemeChanged($script:h)
                Start-Sleep -Milliseconds 500
            }

            $combos = @(
                @{ material = 'clear';    mode = 'auto' },
                @{ material = 'clear';    mode = 'embedded' },
                @{ material = 'blur';     mode = 'auto' },
                @{ material = 'blur';     mode = 'embedded' },
                @{ material = 'acrylic';  mode = 'auto' },
                @{ material = 'acrylic';  mode = 'embedded' }
            )
            $currentMaterial = $null
            foreach ($combo in $combos) {
                if ($combo.material -ne $currentMaterial) {
                    Set-TtbAccent $combo.material
                    $currentMaterial = $combo.material
                }
                Set-IslandMode $combo.mode
                $since = (Get-Log).Count
                Restart-Island
                # Wait for the startup decision (safe-mode scan or the embed)
                # so the assertion below cannot race the SetParent.
                Wait-Log 'coexist\.scan|coexist: safe mode|pill\.embed' $since 8000 | Out-Null
                if (-not (Set-Pin 'Some(Timer)')) { Note "pin walk failed for $($combo.material)/$($combo.mode)" }
                Close-Card

                # The pill mode must be what was asked for. Shell_TrayWnd is
                # polled: one EnumWindows pass can miss it.
                $trayM = [IntPtr]::Zero
                for ($w = 0; $w -lt 20 -and $trayM -eq [IntPtr]::Zero; $w++) {
                    $trayM = [TbiUi]::FindByClass('Shell_TrayWnd')
                    if ($trayM -eq [IntPtr]::Zero) { Start-Sleep -Milliseconds 150 }
                }
                $pillM = [TbiUi]::FindPill('TaskbarIsland')
                $isChild = ($pillM -ne [IntPtr]::Zero -and $trayM -ne [IntPtr]::Zero -and
                    [TbiProbe]::GetParent($pillM) -eq $trayM)
                $wantChild = ($combo.mode -eq 'embedded')
                Report "$($combo.material)/$($combo.mode): pill mode as configured" ($isChild -eq $wantChild) `
                    "child=$isChild want=$wantChild"

                # Exercise the card twice - this is the crash matrix path.
                $ok = 0
                for ($i = 0; $i -lt 2; $i++) { if (Test-OneCycle) { $ok++ } }
                Report "$($combo.material)/$($combo.mode): card open/close" ($ok -eq 2) "$ok/2"

                # Dark shot of the pill strip; light shot on the auto combos.
                $strip = [TbiUi]::ScreenRects($script:pill, $script:h)
                if ($strip.Count -gt 0) {
                    $b = [TbiUi]::BBox($strip)
                    $x = [Math]::Max(0, $b.Left - 260)
                    $w = [Math]::Min(1920, $b.Right + 60) - $x
                    Save-Screen (Join-Path $out ("mat-{0}-{1}-dark.png" -f $combo.material, $combo.mode)) `
                        $x ([Math]::Max(0, $b.Top - 30)) $w ([Math]::Min(1200, $b.Bottom + 30) - [Math]::Max(0, $b.Top - 30))
                } else {
                    Report "$($combo.material)/$($combo.mode): pill visible for shot" $false 'empty region'
                }

                if ($combo.mode -eq 'auto') {
                    Set-Theme 1 1
                    $strip = [TbiUi]::ScreenRects($script:pill, $script:h)
                    if ($strip.Count -gt 0) {
                        $b = [TbiUi]::BBox($strip)
                        $x = [Math]::Max(0, $b.Left - 260)
                        $w = [Math]::Min(1920, $b.Right + 60) - $x
                        Save-Screen (Join-Path $out ("mat-{0}-{1}-light.png" -f $combo.material, $combo.mode)) `
                            $x ([Math]::Max(0, $b.Top - 30)) $w ([Math]::Min(1200, $b.Bottom + 30) - [Math]::Max(0, $b.Top - 30))
                    }
                    Set-Theme 0 0
                }

                $mAlive = $null -ne (Get-Process -Name taskbar-island -ErrorAction SilentlyContinue)
                Report "$($combo.material)/$($combo.mode): process alive" ([bool]$mAlive)
            }
        }
    }

    # ---- phase 6: crash evidence ------------------------------------------
    Write-Host ''
    Write-Host 'phase 6: Event Viewer 1000/1002 + CrashDumps since run start'
    $faults = @(Get-WinEvent -FilterHashtable @{ LogName = 'Application'; StartTime = $script:start } `
        -ErrorAction SilentlyContinue |
        Where-Object { ($_.Id -eq 1000 -or $_.Id -eq 1002) -and $_.Message -match 'taskbar-island' })
    Report 'zero Event Viewer 1000/1002 for taskbar-island' ($faults.Count -eq 0) "$($faults.Count) event(s)"
    $dumps = @(Get-ChildItem -Path (Join-Path $env:LOCALAPPDATA 'CrashDumps') -File -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTime -gt $script:start -and $_.Name -match 'taskbar-island' })
    Report 'zero new taskbar-island crash dumps' ($dumps.Count -eq 0) "$($dumps.Count) file(s)"
    $panics = @((Get-Log) | Where-Object { $_ -match 'panic|RefCell' }).Count
    Report 'no panic in the whole run log' ($panics -eq 0) "$panics panic line(s)"

    # ---- honest skips ------------------------------------------------------
    Report 'auto-hide natural hide -> reveal' $null `
        'environment cannot tuck the bar (three methods failed for v0.11); pill sampler = 0 blinks above'
    Report 'DPI 100/125/150 x light/dark matrix' $null `
        'changing system DPI is not scriptable on the owner machine - manual before 1.0.0'
}
catch {
    Report 'script aborted' $false $_.Message
}
finally {
    Write-Host ''
    Write-Host 'restoring machine state...'
    if ($themeChanged -and $wasSystem -ne $null) {
        Set-ItemProperty -Path $personalize -Name SystemUsesLightTheme -Value $wasSystem
        Set-ItemProperty -Path $personalize -Name AppsUseLightTheme -Value $wasApps
        [TbiUi]::BroadcastSettingChange()
        Note "theme restored ($wasSystem/$wasApps)"
    }
    if ($settingsChanged -and (Test-Path -Path $settingsBackup)) {
        Copy-Item -Path $settingsBackup -Destination $settingsPath -Force
        Note 'island settings.json restored'
    }
    if ($ttbChanged -and (Test-Path -Path $ttbBackup)) {
        Stop-Process -Name TranslucentTB -Force -ErrorAction SilentlyContinue
        Start-Sleep -Milliseconds 800
        Copy-Item -Path $ttbBackup -Destination $ttbSettings -Force
        $ttbApp = Get-StartApps | Where-Object { $_.Name -like '*Translucent*' } | Select-Object -First 1
        if ($ttbApp) { Start-Process -FilePath ("shell:AppsFolder\" + $ttbApp.AppID) | Out-Null }
        Note 'TranslucentTB config restored + restarted'
    }
    # Leave the island exactly as found: installed copy, real edge, Auto mode.
    Stop-Island
    if ($exe) {
        $env:TBI_LOG = '1'
        Start-Process -FilePath $exe | Out-Null
        Start-Sleep -Milliseconds 1500
    }
    Copy-Item -Path $logPath -Destination (Join-Path $out 'log.txt') -Force -ErrorAction SilentlyContinue
    Remove-Item -Path $logPath -ErrorAction SilentlyContinue
    Note 'island relaunched clean (real edge, Taskbar mode Auto)'
}

# ---- summary ---------------------------------------------------------------
Write-Host ''
$pass = @($script:results | Where-Object { $_.ok -eq $true }).Count
$fail = @($script:results | Where-Object { $_.ok -eq $false }).Count
$skip = @($script:results | Where-Object { $_.ok -eq $null }).Count
Write-Host ("{0} passed, {1} failed, {2} skipped - evidence in {3}" -f $pass, $fail, $skip, $out)
if ($fail -gt 0) {
    Write-Host 'failed checks:'
    $script:results | Where-Object { $_.ok -eq $false } | ForEach-Object { Write-Host "  - $($_.name): $($_.detail)" }
    exit 1
}
exit 0

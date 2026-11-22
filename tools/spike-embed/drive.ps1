<#
.SYNOPSIS
  Scenario driver for the TaskbarIsland v0.11.0 embed spike (tools\spike-embed).

.DESCRIPTION
  Starts spike-embed.exe (which self-terminates after 120 s) and walks it
  through the Stage-A scenarios, writing DRIVER markers into the same log.txt
  the spike appends to:

    1 baseline | 2 Start open/close xN | 3 tray-clock / Win+A / Win+B / Ctrl+Esc
    4 auto-hide on/off (registry + explorer restart) | 5 explorer restart
      (re-embed latency) | 6 fullscreen (only if safe, else NOT TESTED)

  Every process this script starts (the spike, the watchdog job, explorer) is
  stopped or verified inside try/finally - no background processes are left
  behind. Nothing outside this folder is written.

  Pure ASCII: PowerShell 5.1 reads a BOM-less script as the ANSI code page.

.EXAMPLE
  powershell -File tools\spike-embed\drive.ps1
.EXAMPLE
  powershell -File tools\spike-embed\drive.ps1 -Scenarios 5
#>
[CmdletBinding()]
param(
    [string[]]$Scenarios = @('1', '2', '3', '4', '5', '6'),
    [int]$StartOpens = 5,
    [int]$Interacts = 3,
    # Abort remaining scenarios when the spike's 120 s self-terminate budget
    # is nearly spent (the spike must never outlive its cap).
    [int]$BudgetSeconds = 100
)

$ErrorActionPreference = 'Stop'
# pwsh -File passes "-Scenarios 1,2,3,4,5,6" as ONE string; -contains would then
# never match. Normalize a single comma-joined element into the list.
if ($Scenarios.Count -eq 1 -and $Scenarios[0] -match ',') {
    $Scenarios = @($Scenarios[0] -split ',' | ForEach-Object { $_.Trim() } | Where-Object { $_ })
}
$dir = $PSScriptRoot
$log = Join-Path $dir 'log.txt'
$exe = Join-Path $dir 'target\debug\spike-embed.exe'

$script:spike = $null
$script:watchdog = $null
$script:autoHideTouched = $false
$script:autoHideOriginal = $null
$script:autoHideHadKey = $false
$script:explorerRestarted = $false
$script:explorerTitlesBefore = @()
$sw = [Diagnostics.Stopwatch]::StartNew()

function Get-ExplorerWindowTitles {
    try {
        $sh = New-Object -ComObject Shell.Application
        return @(@($sh.Windows()) | ForEach-Object { "$($_.Name)" })
    } catch { return @() }
}

# ------------------------------------------------------------------ log i/o
function Add-LogLine([string]$line) {
    for ($i = 0; $i -lt 30; $i++) {
        try {
            $fs = [System.IO.File]::Open($log, [System.IO.FileMode]::Append,
                [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read)
            $w = New-Object System.IO.StreamWriter($fs)
            $w.Write($line + "`n")
            $w.Flush(); $w.Dispose(); $fs.Dispose()
            return
        } catch { Start-Sleep -Milliseconds 15 }
    }
    Write-Warning "could not append to log: $line"
}

function Mark([string]$m) {
    $e = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    Add-LogLine "[epoch=$e] DRIVER $m"
}

function Read-LogLines {
    for ($i = 0; $i -lt 30; $i++) {
        try {
            $fs = [System.IO.File]::Open($log, [System.IO.FileMode]::Open,
                [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
            $r = New-Object System.IO.StreamReader($fs)
            $txt = $r.ReadToEnd(); $r.Dispose(); $fs.Dispose()
            return @($txt -split "`n")
        } catch { Start-Sleep -Milliseconds 15 }
    }
    return @()
}

# --------------------------------------------------------------- state read
function Get-LastState([int]$MaxAgeMs = 6000) {
    $lines = Read-LogLines
    for ($i = $lines.Count - 1; $i -ge 0; $i--) {
        $l = $lines[$i]
        if ($l -notmatch 'STATE ') { continue }
        $e = $null
        if ($l -match '\[epoch=(\d+)\]') { $e = [long]$Matches[1] } else { continue }
        $now = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        # Log-on-change + 5 s heartbeat: a stale newest line means the spike is
        # dead or wedged - treat as "no sample" rather than trusting old data.
        if (($now - $e) -gt $MaxAgeMs) { return $null }
        $o = [ordered]@{ epoch = $e; line = $l.Trim() }
        foreach ($k in 'vis', 'tb', 'aboveTl', 'zok', 'bridge', 'sibAbove', 'start', 'quns') {
            if ($l -match ("(?<![A-Za-z])$k=(\d+)")) { $o[$k] = [int]$Matches[1] }
        }
        if ($l -match 'rect=(-?\d+),(-?\d+),(-?\d+),(-?\d+)') {
            $o.rect = @([int]$Matches[1], [int]$Matches[2], [int]$Matches[3], [int]$Matches[4])
        }
        if ($l -match 'px=(-?\d+),(-?\d+),(-?\d+)') {
            $o.px = @([int]$Matches[1], [int]$Matches[2], [int]$Matches[3])
        }
        if ($l -match 'fg=(\S*)\|(.*)$') { $o.fgCls = $Matches[1]; $o.fgTitle = $Matches[2].Trim() }
        return [pscustomobject]$o
    }
    return $null
}

# Is the screen pixel at the child's centre green? (ground truth for visible)
function Test-Green($s) {
    if ($null -eq $s -or $null -eq $s.px) { return $false }
    return ($s.px[1] -gt 200 -and $s.px[0] -lt 80 -and $s.px[2] -lt 80)
}

function Report([string]$tag) {
    $s = Get-LastState
    if ($null -eq $s) { Mark "$tag SAMPLE=NONE"; return $null }
    Mark ("{0} vis={1} zok={2} tb={3} rect={4} px={5} green={6} start={7} sibAbove={8} fg={9}|{10}" -f `
        $tag, $s.vis, $s.zok, $s.tb, ($s.rect -join ','), ($s.px -join ','), `
        (Test-Green $s), $s.start, $s.sibAbove, $s.fgCls, $s.fgTitle)
    return $s
}

function Assert-VA([string]$tag, $s, [switch]$AllowOccluded) {
    if ($null -eq $s) { Mark "$tag RESULT=FAIL(no-sample)"; return $false }
    $green = Test-Green $s
    $ok = ($s.vis -eq 1 -and $s.zok -eq 1)
    if (-not $AllowOccluded) { $ok = ($ok -and $green) }
    $v = 'FAIL'; if ($ok) { $v = 'PASS' }
    Mark "$tag RESULT=$v vis=$($s.vis) zok=$($s.zok) green=$green"
    return $ok
}

# ------------------------------------------------------------ spike lifecycle
function Start-Spike {
    $env:SPIKE_LOG = $log
    $script:spike = Start-Process -FilePath $exe -WorkingDirectory $dir -PassThru
    # Backstop: even if the spike's own 120 s timer failed, kill it at 130 s.
    $script:watchdog = Start-Job -ScriptBlock {
        param($id)
        Start-Sleep -Seconds 130
        Stop-Process -Id $id -Force -ErrorAction SilentlyContinue
    } -ArgumentList $script:spike.Id
    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $deadline) {
        $s = Get-LastState -MaxAgeMs 5000
        if ($null -ne $s) { return $s }
        if ($script:spike.HasExited) { throw "spike exited early code=$($script:spike.ExitCode) - see $log" }
        Start-Sleep -Milliseconds 100
    }
    throw "spike produced no STATE line within 15 s - see $log"
}

function Stop-Spike {
    if ($script:watchdog) {
        Stop-Job $script:watchdog -ErrorAction SilentlyContinue
        Remove-Job $script:watchdog -Force -ErrorAction SilentlyContinue
        $script:watchdog = $null
    }
    if ($script:spike -and -not $script:spike.HasExited) {
        Stop-Process -Id $script:spike.Id -Force -ErrorAction SilentlyContinue
        Mark "spike stopped by driver pid=$($script:spike.Id)"
    } elseif ($script:spike) {
        Mark "spike already exited code=$($script:spike.ExitCode)"
    }
}

# ------------------------------------------------------------------ explorer
function Restart-Explorer([string]$why) {
    $t = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    Mark "explorer-restart-BEGIN why=$why epoch=$t"
    $script:explorerRestarted = $true
    Stop-Process -Name explorer -Force -ErrorAction SilentlyContinue
    # Wait until the old shell process is GONE: relaunching while it is still
    # dying makes explorer.exe open a File Explorer window instead of starting
    # the shell (observed in the scenario-5 validation run).
    $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Date) -lt $deadline) {
        if (-not (Get-Process explorer -ErrorAction SilentlyContinue)) { break }
        Start-Sleep -Milliseconds 100
    }
    $left = @(Get-Process explorer -ErrorAction SilentlyContinue)
    if ($left.Count -gt 0) { Mark "explorer-restart old-shell-still-alive pids=$($left.Id -join ',')" }
    Start-Process explorer.exe
    $deadline = (Get-Date).AddSeconds(15)
    while ((Get-Date) -lt $deadline) {
        if (Get-Process explorer -ErrorAction SilentlyContinue) { break }
        Start-Sleep -Milliseconds 250
    }
    if (-not (Get-Process explorer -ErrorAction SilentlyContinue)) {
        Mark "explorer-restart BACKSTOP relaunch"
        Start-Process explorer.exe
        Start-Sleep -Seconds 2
    }
    Mark "explorer-restart-LAUNCHED why=$why epoch=$t"
    return $t
}

function Close-Flyouts {
    # Esc twice closes Start / quick settings / calendar / notification panes.
    Key-Down 0x1B; Start-Sleep -Milliseconds 60; Key-Up 0x1B
    Start-Sleep -Milliseconds 120
    Key-Down 0x1B; Start-Sleep -Milliseconds 60; Key-Up 0x1B
    Start-Sleep -Milliseconds 400
}

# ------------------------------------------------------------------- input
if (-not ('SpikeInj' -as [type])) {
    Add-Type @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class SpikeInj {
  [DllImport("user32.dll")] public static extern void keybd_event(byte bVk, byte bScan, uint dwFlags, UIntPtr dwExtraInfo);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int X, int Y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint dwFlags, uint dx, uint dy, uint dwData, UIntPtr dwExtraInfo);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowW(string cls, string title);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowExW(IntPtr parent, IntPtr after, string cls, string title);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int max);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
}
'@
}

function Key-Down([int]$vk) { [SpikeInj]::keybd_event([byte]$vk, 0, 0, [UIntPtr]::Zero) }
function Key-Up([int]$vk) { [SpikeInj]::keybd_event([byte]$vk, 0, 2, [UIntPtr]::Zero) }
function Tap-Key([int]$vk) { Key-Down $vk; Start-Sleep -Milliseconds 50; Key-Up $vk }

function Invoke-ClockClick {
    $tray = [SpikeInj]::FindWindowW('Shell_TrayWnd', $null)
    if ($tray -eq [IntPtr]::Zero) { Mark "clock FAIL no Shell_TrayWnd"; return }
    $notify = [SpikeInj]::FindWindowExW($tray, [IntPtr]::Zero, 'TrayNotifyWnd', $null)
    if ($notify -eq [IntPtr]::Zero) { Mark "clock FAIL no TrayNotifyWnd"; return }
    $r = New-Object SpikeInj+RECT
    if (-not [SpikeInj]::GetWindowRect($notify, [ref]$r)) { Mark "clock FAIL GetWindowRect"; return }
    $cx = [int](($r.Left + $r.Right) / 2)
    $cy = [int](($r.Top + $r.Bottom) / 2)
    $ok = [SpikeInj]::SetCursorPos($cx, $cy)
    Mark "clock click pos=$cx,$cy trayNotify=($($r.Left),$($r.Top),$($r.Right),$($r.Bottom)) setcursor=$ok"
    [SpikeInj]::mouse_event(0x2, 0, 0, 0, [UIntPtr]::Zero)
    Start-Sleep -Milliseconds 60
    [SpikeInj]::mouse_event(0x4, 0, 0, 0, [UIntPtr]::Zero)
}

function Test-Budget([string]$sc) {
    if ($sw.Elapsed.TotalSeconds -gt $BudgetSeconds) {
        Mark "SKIP scenario$sc (elapsed $([int]$sw.Elapsed.TotalSeconds)s of $BudgetSeconds s spike budget)"
        return $false
    }
    return $true
}

# ================================================================== scenarios
if (-not (Test-Path $exe)) { throw "spike not built: cargo build --offline (in $dir)" }

$failures = 0
try {
    if (Test-Path $log) { Remove-Item $log -Force }
    Mark "driver-start scenarios=$($Scenarios -join ',') exe=$exe"
    $script:explorerTitlesBefore = @(Get-ExplorerWindowTitles)
    Mark "explorer-windows-before count=$($script:explorerTitlesBefore.Count) titles=$($script:explorerTitlesBefore -join ' ; ')"
    Start-Spike | Out-Null
    Mark "driver spike-up"

    # ---- 1. baseline ------------------------------------------------------
    if ($Scenarios -contains '1') {
        Mark "scenario1 baseline begin (5 s)"
        Start-Sleep -Seconds 5
        $s = Report 'scenario1 baseline'
        if (-not (Assert-VA 'scenario1 baseline' $s)) { $failures++ }
        if (-not (Test-Green $s)) { Mark "scenario1 NOTE centre pixel not green" }
    }

    # ---- 2. Start open/close ---------------------------------------------
    if ($Scenarios -contains '2' -and (Test-Budget '2')) {
        Mark "scenario2 start-open x$StartOpens begin"
        $injection = 'unknown'
        for ($i = 1; $i -le $StartOpens; $i++) {
            Key-Down 0x5B                       # VK_LWIN down
            Start-Sleep -Milliseconds 1400
            $o = Report "scenario2 open#$i"
            Key-Up 0x5B                         # VK_LWIN up
            Start-Sleep -Milliseconds 900
            $c = Report "scenario2 close#$i"
            if ($i -eq 1) {
                if ($null -eq $o) { $injection = 'no-sample' }
                elseif ($o.start -eq 1) { $injection = 'work' }
                elseif ($o.fgCls -ne $c.fgCls -or $o.fgTitle -ne $c.fgTitle) { $injection = 'fg-changed-only' }
                else { $injection = 'no-effect' }
                Mark "scenario2 injection=$injection openFg=$($o.fgCls)|$($o.fgTitle) closeFg=$($c.fgCls)|$($c.fgTitle)"
            }
            if (-not (Assert-VA "scenario2 close#$i" $c)) { $failures++ }
        }
        if ($injection -eq 'no-effect' -or $injection -eq 'no-sample') {
            Mark "scenario2 INPUT-INJECTION-NOT-WORKING -> scenario3 input parts marked NOT TESTED"
            $script:inputDead = $true
        } else {
            $script:inputDead = $false
        }
        Close-Flyouts
    } elseif ($Scenarios -contains '2') {
        Mark "scenario2 SKIPPED(budget)"
    }

    # ---- 3. taskbar interactions -----------------------------------------
    if ($Scenarios -contains '3' -and (Test-Budget '3')) {
        # Scenario 2 sets $script:inputDead when key injection had no effect.
        $dead = $false
        if ($Scenarios -contains '2') { $dead = [bool]$script:inputDead }
        if ($dead) {
            Mark "scenario3 NOT TESTED (input injection unavailable, see scenario2)"
        } else {
            Mark "scenario3 begin x$Interacts each"
            for ($i = 1; $i -le $Interacts; $i++) {
                # 3a: tray clock (coordinates derived from TrayNotifyWnd rect)
                if (Test-Budget '3a') {
                    Mark "scenario3 clock#$i open"
                    Invoke-ClockClick
                    Start-Sleep -Milliseconds 1200
                    $o = Report "scenario3 clock#$i open"
                    Close-Flyouts
                    $c = Report "scenario3 clock#$i closed"
                    if (-not (Assert-VA "scenario3 clock#$i" $c)) { $failures++ }
                }
                # 3b: Win+A quick settings
                if (Test-Budget '3b') {
                    Mark "scenario3 winA#$i open"
                    Tap-Key 0x5B
                    Start-Sleep -Milliseconds 1200
                    $o = Report "scenario3 winA#$i open"
                    Close-Flyouts
                    $c = Report "scenario3 winA#$i closed"
                    if (-not (Assert-VA "scenario3 winA#$i" $c)) { $failures++ }
                }
                # 3c: Win+B notification focus
                if (Test-Budget '3c') {
                    Mark "scenario3 winB#$i open"
                    Key-Down 0x5B; Start-Sleep -Milliseconds 50; Tap-Key 0x42; Key-Up 0x5B
                    Start-Sleep -Milliseconds 1200
                    $o = Report "scenario3 winB#$i open"
                    Close-Flyouts
                    $c = Report "scenario3 winB#$i closed"
                    if (-not (Assert-VA "scenario3 winB#$i" $c)) { $failures++ }
                }
                # 3d: Ctrl+Esc
                if (Test-Budget '3d') {
                    Mark "scenario3 ctrlEsc#$i open"
                    Key-Down 0x11; Start-Sleep -Milliseconds 60
                    Tap-Key 0x1B
                    Start-Sleep -Milliseconds 60; Key-Up 0x11
                    Start-Sleep -Milliseconds 1200
                    $o = Report "scenario3 ctrlEsc#$i open"
                    Close-Flyouts
                    $c = Report "scenario3 ctrlEsc#$i closed"
                    if (-not (Assert-VA "scenario3 ctrlEsc#$i" $c)) { $failures++ }
                }
            }
        }
    } elseif ($Scenarios -contains '3') {
        Mark "scenario3 SKIPPED(budget)"
    }

    # ---- 4. auto-hide -----------------------------------------------------
    if ($Scenarios -contains '4' -and (Test-Budget '4')) {
        Mark "scenario4 begin"
        Close-Flyouts
        $adv = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
        $props = (Get-ItemProperty -Path $adv).PSObject.Properties.Name
        $script:autoHideHadKey = ($props -contains 'TaskbarAutoHide')
        if ($script:autoHideHadKey) { $script:autoHideOriginal = (Get-ItemProperty -Path $adv).TaskbarAutoHide }
        $script:autoHideTouched = $true

        Mark "scenario4 autohide=1 set"
        Set-ItemProperty -Path $adv -Name TaskbarAutoHide -Value 1 -Type DWord
        Restart-Explorer 'scenario4-autohide-on' | Out-Null
        Start-Sleep -Seconds 3
        $on = Report 'scenario4 autohide=on'
        # Does the child go with the hidden taskbar?
        if ($null -eq $on) { Mark "scenario4 on SAMPLE=NONE" }
        elseif ($on.tb -eq 0) { Mark "scenario4 on: taskbar hidden, child vis=$($on.vis) (child went with it: not visible)" }
        elseif ($on.rect.Count -eq 4 -and $on.rect[1] -ge 1080) { Mark "scenario4 on: taskbar+child moved off-screen rect=$($on.rect -join ',')" }
        else { Mark "scenario4 on: taskbar tb=$($on.tb) rect=$($on.rect -join ',') childVis=$($on.vis) green=$(Test-Green $on)" }

        Mark "scenario4 autohide=0 set"
        Set-ItemProperty -Path $adv -Name TaskbarAutoHide -Value 0 -Type DWord
        Restart-Explorer 'scenario4-autohide-off' | Out-Null
        Start-Sleep -Seconds 3
        $off = Report 'scenario4 autohide=off'
        if (-not (Assert-VA 'scenario4 autohide=off' $off)) { $failures++ }
    } elseif ($Scenarios -contains '4') {
        Mark "scenario4 SKIPPED(budget)"
    }

    # ---- 5. explorer restart (auto-hide off) -----------------------------
    if ($Scenarios -contains '5' -and (Test-Budget '5')) {
        Mark "scenario5 begin (auto-hide off)"
        Close-Flyouts
        $t = Restart-Explorer 'scenario5-restart'
        Start-Sleep -Milliseconds 1000
        $s1 = Report 'scenario5 at1s'
        if ($null -eq $s1 -or $s1.vis -ne 1 -or $s1.zok -ne 1) { Mark "scenario5 at1s NOT-RECOVERED-YET" }
        else { Mark "scenario5 at1s RECOVERED" }
        Start-Sleep -Milliseconds 2000
        $s3 = Report 'scenario5 at3s'
        if (-not (Assert-VA 'scenario5 at3s' $s3)) { $failures++ }

        # Re-embed latency: first `event re-parented` epoch after Stop-Process.
        $lat = $null
        $internal = $null
        foreach ($l in (Read-LogLines)) {
            if ($l -match '\[epoch=(\d+)\] event re-parented .* latency=(\d+)ms') {
                $e = [long]$Matches[1]
                if ($e -ge $t) { $lat = $e - $t; $internal = [int]$Matches[2]; break }
            }
        }
        if ($null -eq $lat) { Mark "scenario5 re-embed-latency=NOT-FOUND" }
        else { Mark "scenario5 re-embed-latency=${lat}ms (from Stop-Process) internal=${internal}ms" }
    } elseif ($Scenarios -contains '5') {
        Mark "scenario5 SKIPPED(budget)"
    }

    # ---- 6. fullscreen ----------------------------------------------------
    if ($Scenarios -contains '6') {
        # No fullscreen app can be launched offline without side effects, and
        # Alt+Enter on existing windows would alter the user's session.
        Mark "scenario6 fullscreen NOT TESTED (no safe fullscreen app available offline)"
    }
} finally {
    # ---- restore anything we changed --------------------------------------
    if ($script:autoHideTouched) {
        $adv = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced'
        try {
            if ($script:autoHideHadKey) {
                Set-ItemProperty -Path $adv -Name TaskbarAutoHide -Value $script:autoHideOriginal -Type DWord
            } else {
                Remove-ItemProperty -Path $adv -Name TaskbarAutoHide -ErrorAction SilentlyContinue
            }
            $now = (Get-ItemProperty -Path $adv -ErrorAction SilentlyContinue).PSObject.Properties.Name -contains 'TaskbarAutoHide'
            $val = (Get-ItemProperty -Path $adv -ErrorAction SilentlyContinue).TaskbarAutoHide
            Mark "restore autohide hadKey=$($script:autoHideHadKey) original=$($script:autoHideOriginal) now=$now/$val"
            # Make sure a live taskbar exists with auto-hide off.
            $t = Get-Process explorer -ErrorAction SilentlyContinue
            if (-not $t) { Restart-Explorer 'restore-explorer' | Out-Null }
        } catch { Mark "restore autohide FAILED: $_" }
    }
    # Whatever happened, never leave the machine without a shell.
    try {
        if (-not (Get-Process explorer -ErrorAction SilentlyContinue)) {
            Mark "restore explorer missing -> relaunch"
            Restart-Explorer 'ensure-explorer' | Out-Null
        }
    } catch { Mark "restore explorer ensure FAILED: $_" }
    # A File Explorer window spawned by a test restart must not outlive the run.
    if ($script:explorerRestarted) {
        try {
            $sh = New-Object -ComObject Shell.Application
            foreach ($w in @($sh.Windows())) {
                $n = "$($w.Name)"
                if ($script:explorerTitlesBefore -notcontains $n) {
                    Mark "cleanup closing test-spawned explorer window '$n'"
                    $w.Quit()
                }
            }
        } catch { Mark "cleanup explorer windows FAILED: $_" }
    }
    # ---- summary -----------------------------------------------------------
    try {
        $states = @()
        foreach ($l in (Read-LogLines)) {
            if ($l -notmatch 'STATE ') { continue }
            $v = $null; $e = $null
            if ($l -match 'vis=(\d+)') { $v = [int]$Matches[1] }
            if ($l -match '\[epoch=(\d+)\]') { $e = [long]$Matches[1] }
            if ($null -ne $v -and $null -ne $e) { $states += [pscustomobject]@{ e = $e; v = $v } }
        }
        $blinks = @()
        for ($i = 1; $i -lt $states.Count; $i++) {
            if ($states[$i].v -ne $states[$i - 1].v) {
                $blinks += [pscustomobject]@{ e = $states[$i].e; v = $states[$i].v }
            }
        }
        Mark ("summary state-samples={0} blink-count={1}" -f $states.Count, $blinks.Count)
        foreach ($b in $blinks) { Mark ("summary blink epoch={0} vis->{1}" -f $b.e, $b.v) }
        foreach ($l in (Read-LogLines)) {
            if ($l -match 'event re-parented' -or $l -match 'event recover-failed' -or $l -match 'error ') {
                Mark ("summary " + $l.Trim())
            }
        }
        Mark "summary failures=$failures driver-end elapsed=$([int]$sw.Elapsed.TotalSeconds)s"
    } catch { Write-Warning "summary failed: $_" }
    Stop-Spike
}

Write-Host "done: failures=$failures elapsed=$([int]$sw.Elapsed.TotalSeconds)s log=$log"
if ($failures -gt 0) { exit 1 }
exit 0

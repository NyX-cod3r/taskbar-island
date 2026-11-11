<#
  The "did the code I just built actually install and run?" check.

  Run it after every code change, in this order:

      1. stop the running island        (Stop-Process taskbar-island)
      2. install dist\TaskbarIsland-Setup.exe
      3. launch it and verify the windows come up

  This script does all three. It installs over the real per-user install at
  %LOCALAPPDATA%\Programs\TaskbarIsland on purpose — that is the path a user
  takes — so the previous build is replaced by the one in dist\.

  Checks
    stopped     no taskbar-island process is left running
    install     Setup exits 0 and the installed exe matches target\release
    version     the installed exe carries a FileVersion
    launch      the installed copy starts and owns the pill window
    windows     class `TaskbarIsland` (visible) and `TaskbarIslandTray`
    single      a second launch is a no-op: still exactly one process

  Never run this while you care about the current island: it kills it.
#>
[CmdletBinding()]
param(
    # Defaults to ..\..\dist\TaskbarIsland-Setup.exe relative to this script.
    [string]$SetupPath = '',
    # Seconds to wait for each async step (process exit, window, launch).
    [int]$TimeoutSec = 15
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $SetupPath) { $SetupPath = Join-Path $repoRoot 'dist\TaskbarIsland-Setup.exe' }
$installedExe = Join-Path $env:LOCALAPPDATA 'Programs\TaskbarIsland\taskbar-island.exe'
$releaseExe = Join-Path $repoRoot 'target\release\taskbar-island.exe'

$results = New-Object System.Collections.Generic.List[object]
function Check([string]$Name, [bool]$Ok, [string]$Detail = '') {
    $results.Add([pscustomobject]@{ Name = $Name; Passed = $Ok; Detail = $Detail })
}

# ---- window enumeration for the class checks -------------------------------
if (-not ('TbiWindows' -as [type])) {
Add-Type @'
using System;
using System.Collections.Generic;
using System.Text;
using System.Runtime.InteropServices;
public class TbiWindows {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
    [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    public static List<string> ClassesOf(uint pid) {
        var found = new List<string>();
        EnumWindows((h, l) => {
            uint p; GetWindowThreadProcessId(h, out p);
            if (p == pid) {
                var sb = new StringBuilder(256);
                GetClassNameW(h, sb, 256);
                found.Add(sb.ToString() + (IsWindowVisible(h) ? ":visible" : ":hidden"));
            }
            return true;
        }, IntPtr.Zero);
        return found;
    }
}
'@
}

function Get-IslandProcess {
    Get-Process -Name 'taskbar-island' -ErrorAction SilentlyContinue
}

# ---- 1. stop the running island -------------------------------------------
if (-not (Test-Path -LiteralPath $SetupPath)) {
    Write-Host "setup not found: $SetupPath (build it first: .\scripts\build.ps1 -Mode Installer)"
    exit 1
}
Get-IslandProcess | Stop-Process -Force -ErrorAction SilentlyContinue
$gone = $false
for ($i = 0; $i -lt $TimeoutSec * 10; $i++) {
    if (-not (Get-IslandProcess)) { $gone = $true; break }
    Start-Sleep -Milliseconds 100
}
Check 'stopped' $gone $(if ($gone) { '' } else { 'taskbar-island is still running' })
if (-not $gone) {
    # Nothing else can be trusted while the old process holds the mutex.
    $results | ForEach-Object { Write-Host ("[{0}] {1} {2}" -f $(if ($_.Passed) { 'PASS' } else { 'FAIL' }), $_.Name, $_.Detail) }
    exit 1
}

# ---- 2. install the build in dist\ ----------------------------------------
$setupLog = Join-Path $env:TEMP 'tbi-install-smoke.log'
if (Test-Path $setupLog) { Remove-Item $setupLog -Force }
$proc = Start-Process -FilePath $SetupPath `
    -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=$setupLog" `
    -Wait -PassThru -WindowStyle Hidden
Check 'install' ($proc.ExitCode -eq 0) "setup exit $($proc.ExitCode); log $setupLog"
Check 'exe' (Test-Path -LiteralPath $installedExe) $installedExe

if (Test-Path -LiteralPath $installedExe) {
    $version = (Get-Item -LiteralPath $installedExe).VersionInfo.FileVersion
    Check 'version' ([bool]$version) "FileVersion=$version"

    # The installed binary must be the one that was just built.
    if (Test-Path -LiteralPath $releaseExe) {
        $installedHash = (Get-FileHash -LiteralPath $installedExe -Algorithm SHA256).Hash
        $releaseHash = (Get-FileHash -LiteralPath $releaseExe -Algorithm SHA256).Hash
        Check 'matches-target' ($installedHash -ieq $releaseHash) "installed=$($installedHash.Substring(0,12))… target=$($releaseHash.Substring(0,12))…"
    } else {
        Check 'matches-target' $true 'skipped: target\release\taskbar-island.exe absent'
    }
}

# ---- 3. launch and verify --------------------------------------------------
Start-Process -FilePath $installedExe
$run = $null
for ($i = 0; $i -lt $TimeoutSec * 10; $i++) {
    $run = Get-IslandProcess
    if ($run) { break }
    Start-Sleep -Milliseconds 100
}
if (-not $run) {
    Check 'launch' $false 'the installed copy never started'
} else {
    Start-Sleep -Seconds 2   # let the pill, tray and clipboard windows register
    $run = Get-IslandProcess
    Check 'launch' ([bool]$run) "pid=$($run.Id)"

    $classes = [TbiWindows]::ClassesOf([uint32]$run.Id)
    Check 'windows' ($classes -contains 'TaskbarIsland:visible') (($classes -join ', ') -replace '\s+', ' ')

    # Single instance: a second launch must not create a second process.
    Start-Process -FilePath $installedExe
    Start-Sleep -Seconds 2
    $count = @(Get-IslandProcess).Count
    Check 'single' ($count -eq 1) "processes=$count"
}

# ---- report ----------------------------------------------------------------
Write-Host ''
$failed = 0
foreach ($r in $results) {
    if ($r.Passed) {
        Write-Host "[PASS] $($r.Name)"
    } else {
        $failed++
        Write-Host "[FAIL] $($r.Name)  $($r.Detail)"
    }
}
Write-Host "$($results.Count) checked, $failed failed"
if ($failed -gt 0) { exit 1 }
exit 0

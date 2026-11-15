<#
.SYNOPSIS
  Long-run resource soak for the installed TaskbarIsland.

.DESCRIPTION
  Samples the running `taskbar-island` process once per interval and writes a
  CSV (timestamp, working set, private bytes, handles, threads, CPU seconds
  consumed during the interval, responsive). Nothing in the app is touched:
  this measures the resting, event-driven cost - a leak shows up as a trend in
  handles or private bytes across the rows.

  The CSV lands in target\soak\ so it is covered by .gitignore and never by a
  commit; paste the numbers into docs/testing.md.

  Pure ASCII on purpose: Windows PowerShell 5.1 reads a BOM-less script as the
  ANSI code page, and a non-ASCII byte inside a string literal can terminate it.

.EXAMPLE
  powershell -File tools\soak.ps1 -Minutes 60
#>
[CmdletBinding()]
param(
    [int]$Minutes = 60,
    [int]$IntervalSeconds = 60,
    [string]$Out = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root 'target\soak'
if (-not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir | Out-Null }
if (-not $Out) {
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $ver = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
    $Out = Join-Path $dir ("soak-$ver-$stamp.csv")
}

function Get-Island {
    Get-Process -Name taskbar-island -ErrorAction SilentlyContinue | Select-Object -First 1
}

$proc = Get-Island
if (-not $proc) { throw 'taskbar-island is not running - run tools\tests\install-smoke.ps1 first.' }

$rows = New-Object System.Collections.Generic.List[object]
$interval = [double]$IntervalSeconds
$samples = [int][Math]::Ceiling(($Minutes * 60.0) / $interval)
$prevCpu = $proc.CPU
$startTime = Get-Date

Write-Host "soak: $samples samples x $interval s = $([int]($samples * $interval)) s, csv $Out"

for ($i = 1; $i -le $samples; $i++) {
    Start-Sleep -Seconds $IntervalSeconds
    $p = Get-Island
    if (-not $p) {
        Write-Warning "sample ${i}: process gone - stopping."
        break
    }
    # CPU can reset if the process was replaced; treat that as a restart.
    $cpuDelta = $p.CPU - $prevCpu
    if ($cpuDelta -lt 0) { $cpuDelta = 0.0 }
    $prevCpu = $p.CPU
    $row = [pscustomobject]@{
        Sample       = $i
        Time         = (Get-Date -Format 'HH:mm:ss')
        WorkingSetMB = [Math]::Round($p.WorkingSet64 / 1MB, 2)
        PrivateMB    = [Math]::Round($p.PrivateMemorySize64 / 1MB, 2)
        Handles      = $p.HandleCount
        Threads      = $p.Threads.Count
        CpuSec       = [Math]::Round($cpuDelta, 3)
        Responding   = $p.Responding
        Pid          = $p.Id
    }
    $rows.Add($row)
    $line = '  [{0,3}/{1}] ws={2} MB private={3} MB handles={4} threads={5} cpu={6} s responding={7}' -f `
        $i, $samples, $row.WorkingSetMB, $row.PrivateMB, $row.Handles, $row.Threads, $row.CpuSec, $row.Responding
    Write-Host $line
}

$rows | Export-Csv -Path $Out -NoTypeInformation -Encoding UTF8

$first = $rows | Select-Object -First 1
$last = $rows | Select-Object -Last 1
$unresponsive = @($rows | Where-Object { -not $_.Responding }).Count
$span = [Math]::Round(((Get-Date) - $startTime).TotalMinutes, 1)

Write-Host ''
Write-Host "soak done: $($rows.Count) samples over $span min -> $Out"
if ($first -and $last) {
    $wsDelta = [Math]::Round($last.WorkingSetMB - $first.WorkingSetMB, 2)
    $pvDelta = [Math]::Round($last.PrivateMB - $first.PrivateMB, 2)
    $hDelta = $last.Handles - $first.Handles
    $tDelta = $last.Threads - $first.Threads
    Write-Host ('  working set {0} -> {1} MB ({2})' -f $first.WorkingSetMB, $last.WorkingSetMB, $wsDelta)
    Write-Host ('  private     {0} -> {1} MB ({2})' -f $first.PrivateMB, $last.PrivateMB, $pvDelta)
    Write-Host ('  handles     {0} -> {1} ({2})' -f $first.Handles, $last.Handles, $hDelta)
    Write-Host ('  threads     {0} -> {1} ({2})' -f $first.Threads, $last.Threads, $tDelta)
    $cpuTotal = ($rows | Measure-Object -Property CpuSec -Sum).Sum
    $pct = [Math]::Round(100.0 * $cpuTotal / ($span * 60.0), 2)
    Write-Host ('  cpu total   {0} s over {1} min ({2} percent of one core)' -f $cpuTotal, $span, $pct)
    Write-Host "  unresponsive samples: $unresponsive"
}

if ($unresponsive -gt 0) { exit 1 }

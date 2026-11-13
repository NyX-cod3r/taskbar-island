<#
  Keeps dist\ down to a single version: the installer and the update payload
  that tools\release.ps1 publishes.

  The installer's name is fixed (TaskbarIsland-Setup.exe) so the download URL
  never changes; the version lives inside the installer (AppVersion,
  AppVerName, VersionInfoVersion). Older builds wrote
  TaskbarIsland-Setup-<version>.exe instead and nothing removed them, so dist\
  collected one installer per version. This deletes every build artifact that
  is not the current installer or the current update payload.

  Left alone: manifest.txt (rewritten by release.ps1 before each publish) and
  update-smoke.exe (the self-update test harness, which tools\tests\
  update-smoke.ps1 expects to find already compiled).

  Examples
    .\tools\clean-dist.ps1                # use Cargo.toml's version
    .\tools\clean-dist.ps1 -Version 0.7.0
#>
[CmdletBinding()]
param(
    # Defaults to `version =` in Cargo.toml (the single source of truth).
    [string]$Version = ''
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$distDir = Join-Path $repoRoot 'dist'

if (-not $Version) {
    $cargoToml = Join-Path $repoRoot 'Cargo.toml'
    $text = Get-Content -LiteralPath $cargoToml -Raw
    if ($text -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
        throw "Cannot find 'version =' in $cargoToml"
    }
    $Version = $Matches[1]
}
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version '$Version' is not x.y.z."
}

if (-not (Test-Path -LiteralPath $distDir -PathType Container)) {
    return
}

# Only these are build outputs; everything else in dist\ is a manifest or
# a test artifact and must survive. The payload keeps its version because
# manifest.txt points at it by exact name. `TaskbarIsland-Converters-Setup.exe`
# (the optional engines pack) matches none of these patterns and is kept too.
$setupName = 'TaskbarIsland-Setup.exe'
$payloadName = "taskbar-island-$Version.exe"
$removed = 0
foreach ($file in Get-ChildItem -LiteralPath $distDir -File) {
    # `TaskbarIsland-Setup*` also catches the legacy versioned names.
    $isSetup = $file.Name -like 'TaskbarIsland-Setup*.exe'
    $isPayload = $file.Name -like 'taskbar-island-*.exe'
    if (($isSetup -and $file.Name -ne $setupName) -or ($isPayload -and $file.Name -ne $payloadName)) {
        Write-Host "  removing stale $($file.Name)"
        Remove-Item -LiteralPath $file.FullName -Force
        $removed++
    }
}
if ($removed -eq 0) {
    Write-Host "  dist\ holds only $setupName and $payloadName"
}

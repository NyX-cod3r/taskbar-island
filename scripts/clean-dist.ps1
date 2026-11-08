<#
  Keeps dist\ down to a single version: the installer and the update payload
  that scripts\release.ps1 publishes.

  The installer's name is fixed (TaskbarIsland-Setup.exe) so the download URL
  never changes; the version lives inside the installer (AppVersion,
  AppVerName, VersionInfoVersion). Older builds wrote
  TaskbarIsland-Setup-<version>.exe instead and nothing removed them, so dist\
  collected one installer per version. This deletes every build artifact that
  is not the current installer or the current update payload.

  Left alone: manifest.txt (rewritten by release.ps1 before each publish) and
  update-smoke.exe (the self-update test harness, which scripts\tests\
  update-smoke.ps1 expects to find already compiled).

  Examples
    .\scripts\clean-dist.ps1                # use installer.iss's version
    .\scripts\clean-dist.ps1 -Version 0.7.0
#>
[CmdletBinding()]
param(
    # Defaults to MyAppVersion in scripts\installer.iss.
    [string]$Version = ''
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$distDir = Join-Path $repoRoot 'dist'

if (-not $Version) {
    $installerIss = Join-Path $PSScriptRoot 'installer.iss'
    $text = Get-Content -LiteralPath $installerIss -Raw
    if ($text -notmatch '#define\s+MyAppVersion\s+"([^"]+)"') {
        throw "Cannot find '#define MyAppVersion' in $installerIss"
    }
    $Version = $Matches[1]
}
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version '$Version' is not x.y.z."
}

if (-not (Test-Path -LiteralPath $distDir -PathType Container)) {
    return
}

# Only these two are build outputs; everything else in dist\ is a manifest or
# a test artifact and must survive. The payload keeps its version because
# manifest.txt points at it by exact name.
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

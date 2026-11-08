<#
  Publishes a TaskbarIsland release that the installer's self-update can consume.

  It builds, packages and uploads three assets to one GitHub release:

    TaskbarIsland-Setup.exe          what a new user downloads (name never changes)
    taskbar-island-<ver>.exe        what an existing install downloads on update
    manifest.txt                    what the installer reads first

  The manifest is what makes the whole thing work, so its shape is fixed:

    version=<x.y.z>
    url=https://github.com/<repo>/releases/download/v<ver>/taskbar-island-<ver>.exe
    sha256=<64 hex digits of the payload, lower case>

  scripts\update.iss fetches releases/latest/download/manifest.txt, so the
  release must be published as the latest (never a draft or a prerelease).

  Examples
    .\scripts\release.ps1 -DryRun                 # build + manifest, no upload
    $env:GITHUB_TOKEN = <token>
    .\scripts\release.ps1                         # build and publish
    .\scripts\release.ps1 -SkipBuild -Version 0.7.0
#>
[CmdletBinding()]
param(
    # Defaults to MyAppVersion in scripts\installer.iss.
    [string]$Version = '',
    [string]$Repo = 'NyX-cod3r/taskbar-island',
    [string]$Token = $env:GITHUB_TOKEN,
    # Build and package only: write dist\manifest.txt, then stop.
    [switch]$DryRun,
    # Package an already-built tree: skips cargo + ISCC.
    [switch]$SkipBuild,
    # Fail instead of warning when Cargo.toml / app.rc disagree with the installer.
    [switch]$Strict,
    [string]$Notes = ''
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$installerIss = Join-Path $PSScriptRoot 'installer.iss'
$cargoToml = Join-Path $repoRoot 'Cargo.toml'
$appRc = Join-Path $repoRoot 'assets\app.rc'
$distDir = Join-Path $repoRoot 'dist'
$releaseExe = Join-Path $repoRoot 'target\release\taskbar-island.exe'

function Read-InstallerVersion {
    $line = Select-String -LiteralPath $installerIss -Pattern '^#define MyAppVersion\s+"([^"]+)"' | Select-Object -First 1
    if (-not $line) { throw "Cannot find '#define MyAppVersion' in $installerIss" }
    return $line.Matches[0].Groups[1].Value
}

if (-not $Version) { $Version = Read-InstallerVersion }
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version '$Version' is not x.y.z - scripts\update.iss compares versions numerically."
}

# The version baked into the shipped exe should match the installer's.
$mismatches = @()
$cargoLine = Select-String -LiteralPath $cargoToml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if ($cargoLine -and $cargoLine.Matches[0].Groups[1].Value -ne $Version) {
    $mismatches += "Cargo.toml $($cargoLine.Matches[0].Groups[1].Value)"
}
# FILEVERSION 0,7,0,0 -> 0.7.0. The revision part is required by the resource
# format and has no counterpart in Cargo.toml / MyAppVersion, so compare the
# first three parts only.
$rcLine = Select-String -LiteralPath $appRc -Pattern '^\s*FILEVERSION\s+([0-9]+(?:,[0-9]+)+)' | Select-Object -First 1
if ($rcLine) {
    $rcRaw = $rcLine.Matches[0].Groups[1].Value
    $rcParts = $rcRaw -split ','
    if ($rcParts.Count -gt 3) { $rcParts = $rcParts[0..2] }
    $rcVersion = $rcParts -join '.'
    if ($rcVersion -ne $Version) { $mismatches += "assets\app.rc $rcRaw" }
}
if ($mismatches) {
    $msg = "Version mismatch with $Version : $($mismatches -join ', ')"
    if ($Strict) { throw $msg } else { Write-Warning "$msg (use -Strict to fail)" }
}

Write-Host "Releasing $Repo $Version"

# --- build ---------------------------------------------------------------
if (-not $DryRun -and -not $SkipBuild) {
    & (Join-Path $PSScriptRoot 'build.ps1') -Mode Installer
    if ($LASTEXITCODE -ne 0) { throw "build.ps1 Installer failed with $LASTEXITCODE" }
}

$setupExe = Join-Path $distDir 'TaskbarIsland-Setup.exe'
$payloadName = "taskbar-island-$Version.exe"
$payloadPath = Join-Path $distDir $payloadName
$manifestPath = Join-Path $distDir 'manifest.txt'

if (-not (Test-Path -LiteralPath $setupExe)) {
    throw "Missing $setupExe - run without -SkipBuild/-DryRun first."
}
if (-not (Test-Path -LiteralPath $releaseExe)) {
    throw "Missing $releaseExe - run 'cargo build --release' first."
}

# The updater must end up with exactly the bits a fresh install would write.
Copy-Item -LiteralPath $releaseExe -Destination $payloadPath -Force
$payloadSha = (Get-FileHash -LiteralPath $payloadPath -Algorithm SHA256).Hash.ToLowerInvariant()
$payloadUrl = "https://github.com/$Repo/releases/download/v$Version/$payloadName"

$manifestText = @(
    "version=$Version"
    "url=$payloadUrl"
    "sha256=$payloadSha"
) -join "`r`n"
Set-Content -LiteralPath $manifestPath -Value $manifestText -Encoding ascii

# Fail here rather than publish a manifest scripts\update.iss cannot read.
$check = @{}
foreach ($line in (Get-Content -LiteralPath $manifestPath)) {
    $i = $line.IndexOf('=')
    if ($i -ge 1) { $check[$line.Substring(0, $i)] = $line.Substring($i + 1) }
}
foreach ($key in 'version', 'url', 'sha256') {
    if (-not $check.ContainsKey($key)) { throw "manifest.txt is missing '$key'" }
}
if ($check.version -ne $Version) { throw "manifest version $($check.version) != $Version" }
if ($check.sha256 -notmatch '^[0-9a-f]{64}$') { throw "manifest sha256 is not 64 lower-case hex digits" }

$setupSize = [math]::Round((Get-Item -LiteralPath $setupExe).Length / 1MB, 1)
$payloadSize = [math]::Round((Get-Item -LiteralPath $payloadPath).Length / 1MB, 1)
Write-Host "  setup    $setupExe ($setupSize MB)"
Write-Host "  payload  $payloadPath ($payloadSize MB)"
Write-Host "  sha256   $payloadSha"
Write-Host "  manifest $manifestPath"

if ($DryRun) {
    Write-Host '-DryRun: stopping before any network access.'
    return
}
if (-not $Token) {
    throw 'GITHUB_TOKEN is not set. Export it, or pass -DryRun to package only.'
}

# Covers -SkipBuild, where build.ps1 never ran to sweep for itself.
& (Join-Path $PSScriptRoot 'clean-dist.ps1') -Version $Version

# --- release -------------------------------------------------------------
$api = 'https://api.github.com'
$headers = @{
    Authorization         = "Bearer $Token"
    Accept                = 'application/vnd.github+json'
    'X-GitHub-Api-Version' = '2022-11-28'
    'User-Agent'          = 'taskbar-island-release'
}
$tag = "v$Version"
$notes = if ($Notes) { $Notes } else { "TaskbarIsland $Version" }

$release = try {
    Invoke-RestMethod -Uri "$api/repos/$Repo/releases/tags/$tag" -Headers $headers -Method Get
} catch {
    if ($_.Exception.Response.StatusCode.value__ -ne 404) { throw }
    $null
}

if (-not $release) {
    Write-Host "Creating release $tag ..."
    $release = Invoke-RestMethod -Uri "$api/repos/$Repo/releases" -Headers $headers -Method Post -ContentType 'application/json' -Body (@{
        tag_name  = $tag
        name      = "TaskbarIsland $Version"
        body      = $notes
        draft     = $false
        prerelease = $false
    } | ConvertTo-Json)
} elseif ($release.draft -or $release.prerelease) {
    throw "Release $tag exists but is a draft/prerelease. scripts\update.iss only follows the latest stable release."
}

# --- assets --------------------------------------------------------------
foreach ($asset in @($release.assets)) {
    if ($asset.name -in @($payloadName, 'manifest.txt', (Split-Path -Leaf $setupExe))) {
        Write-Host "Removing old asset $($asset.name)"
        Invoke-RestMethod -Uri "$api/repos/$Repo/releases/assets/$($asset.id)" -Headers $headers -Method Delete | Out-Null
    }
}

$uploadBase = $release.upload_url -replace '\{\?name,label\}$', ''
function Publish-Asset {
    param([string]$Path, [string]$Name)
    Write-Host "Uploading $Name ..."
    Invoke-RestMethod -Uri "$($uploadBase)?name=$Name" -Headers $headers -Method Post `
        -ContentType 'application/octet-stream' -InFile $Path | Out-Null
}

Publish-Asset -Path $manifestPath -Name 'manifest.txt'
Publish-Asset -Path $payloadPath -Name $payloadName
Publish-Asset -Path $setupExe -Name (Split-Path -Leaf $setupExe)

Write-Host ''
Write-Host "Published $Repo $tag"
Write-Host "  https://github.com/$Repo/releases/latest"
Write-Host "  update check: https://github.com/$Repo/releases/latest/download/manifest.txt"

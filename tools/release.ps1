<#
  Publishes a TaskbarIsland release that the installer's self-update can consume.

  It builds, packages and uploads the assets a GitHub release needs:

    TaskbarIsland-Setup.exe          what a new user downloads (name never changes)
    taskbar-island-<ver>.exe         what an existing install downloads on update
    manifest.txt                     what the installer reads first
    TaskbarIsland-Setup.exe.sha256   the hash of the setup (written by CI)

  The manifest is what makes the whole thing work, so its shape is fixed:

    version=<x.y.z>
    url=https://github.com/<repo>/releases/download/v<ver>/taskbar-island-<ver>.exe
    sha256=<64 hex digits of the payload, lower case>

  installer\update.iss fetches releases/latest/download/manifest.txt, so the
  release must end up published as the latest (never a prerelease).

  DRAFTS: .github/workflows/release.yml creates the draft and puts the setup
  plus its SHA-256 on it. This script then attaches the payload and manifest
  and NEVER replaces an asset CI already uploaded (that setup may be the
  signed one) and NEVER publishes - publishing is the owner's manual step
  (docs/release-process.md).

  Examples
    .\tools\release.ps1 -DryRun                 # build + manifest, no upload
    $env:GITHUB_TOKEN = <token>
    .\tools\release.ps1                         # build and attach to the release
    .\tools\release.ps1 -SkipBuild -Version 0.7.0
#>
[CmdletBinding()]
param(
    # Defaults to `version =` in Cargo.toml (the single source of truth).
    [string]$Version = '',
    [string]$Repo = 'NyX-cod3r/taskbar-island',
    [string]$Token = $env:GITHUB_TOKEN,
    # Build and package only: write dist\manifest.txt, then stop.
    [switch]$DryRun,
    # Package an already-built tree: skips cargo + ISCC.
    [switch]$SkipBuild,
    # Fail instead of warning when the built exe disagrees with Cargo.toml.
    [switch]$Strict,
    [string]$Notes = ''
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
$cargoToml = Join-Path $repoRoot 'Cargo.toml'
$distDir = Join-Path $repoRoot 'dist'
$releaseExe = Join-Path $repoRoot 'target\release\taskbar-island.exe'

function Read-CargoValue([string]$Key) {
    $line = Select-String -LiteralPath $cargoToml -Pattern "^$([regex]::Escape($Key))\s*=\s*`"([^`"]+)`"" | Select-Object -First 1
    if (-not $line) { throw "Cannot find '$Key =' in $cargoToml" }
    return $line.Matches[0].Groups[1].Value
}

if (-not $Version) { $Version = Read-CargoValue 'version' }
if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "Version '$Version' is not x.y.z - installer\update.iss compares versions numerically."
}
$ProductName = Read-CargoValue 'product-name'

# The shipped exe must carry Cargo.toml's identity in its VERSIONINFO: that is
# what Explorer, Apps & Features and the signing policy show. app.rc is a
# template now, so the exe itself is the thing to compare.
$mismatches = @()
if (Test-Path -LiteralPath $releaseExe) {
    $info = (Get-Item -LiteralPath $releaseExe).VersionInfo
    if ($info.FileVersion -notlike "$Version*") { $mismatches += "exe FileVersion $($info.FileVersion)" }
    if ($info.ProductName -ne $ProductName) { $mismatches += "exe ProductName $($info.ProductName)" }
} else {
    Write-Warning "Missing $releaseExe - skipping the VERSIONINFO check."
}
if ($mismatches) {
    $msg = "Identity mismatch with $Version / $ProductName : $($mismatches -join ', ')"
    if ($Strict) { throw $msg } else { Write-Warning "$msg (use -Strict to fail)" }
}

Write-Host "Releasing $Repo $Version"

# --- build ---------------------------------------------------------------
if (-not $DryRun -and -not $SkipBuild) {
    & (Join-Path $PSScriptRoot 'build.ps1') -Mode Installer
    if ($LASTEXITCODE -ne 0) { throw "build.ps1 Installer failed with $LASTEXITCODE" }
}

# The setup is only needed when this script has to upload one (a published
# release, or a local rehearsal without CI); the payload and manifest are
# always required.
$setupExe = Join-Path $distDir 'TaskbarIsland-Setup.exe'
$payloadName = "taskbar-island-$Version.exe"
$payloadPath = Join-Path $distDir $payloadName
$manifestPath = Join-Path $distDir 'manifest.txt'

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

# Fail here rather than publish a manifest installer\update.iss cannot read.
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

$payloadSize = [math]::Round((Get-Item -LiteralPath $payloadPath).Length / 1MB, 1)
Write-Host "  payload  $payloadPath ($payloadSize MB)"
if (Test-Path -LiteralPath $setupExe) {
    $setupSize = [math]::Round((Get-Item -LiteralPath $setupExe).Length / 1MB, 1)
    Write-Host "  setup    $setupExe ($setupSize MB)"
}
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
    Authorization          = "Bearer $Token"
    Accept                = 'application/vnd.github+json'
    'X-GitHub-Api-Version' = '2022-11-28'
    'User-Agent'          = 'taskbar-island-release'
}
$tag = "v$Version"
$notes = if ($Notes) { $Notes } else { "TaskbarIsland $Version" }

# Drafts are invisible to /releases/tags/<tag> (that endpoint only answers for
# published releases), so list them and match by tag name instead.
$release = try {
    Invoke-RestMethod -Uri "$api/repos/$Repo/releases?per_page=100" -Headers $headers -Method Get |
        Where-Object { $_.tag_name -eq $tag } | Select-Object -First 1
} catch {
    throw
}

if (-not $release) {
    # No CI draft (a tag pushed without the workflow, or a local rehearsal):
    # create it as a DRAFT so nothing goes public by accident. Publishing is
    # the owner's manual step - see docs/release-process.md.
    Write-Host "Creating DRAFT release $tag ..."
    $release = Invoke-RestMethod -Uri "$api/repos/$Repo/releases" -Headers $headers -Method Post -ContentType 'application/json' -Body (@{
        tag_name   = $tag
        name       = "TaskbarIsland $Version"
        body       = $notes
        draft      = $true
        prerelease = $false
    } | ConvertTo-Json)
}
$isDraft = [bool]$release.draft
if ($release.prerelease) {
    throw "Release $tag is a prerelease. installer\update.iss only follows the latest stable release."
}

# --- assets --------------------------------------------------------------
$setupName = Split-Path -Leaf $setupExe
$existing = @($release.assets | ForEach-Object { $_.name })

# The setup belongs to whoever built it (CI, or the signed artifact), so a
# draft's copy is never replaced here. Only the payload and the manifest are
# ours to regenerate.
$owned = @($payloadName, 'manifest.txt')
if (-not $isDraft) { $owned += $setupName }
foreach ($asset in @($release.assets)) {
    if ($asset.name -in $owned) {
        Write-Host "Removing old asset $($asset.name)"
        Invoke-RestMethod -Uri "$api/repos/$Repo/releases/assets/$($asset.id)" -Headers $headers -Method Delete | Out-Null
        $existing = @($existing | Where-Object { $_ -ne $asset.name })
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
if ($isDraft) {
    if ($existing -contains $setupName) {
        Write-Host "Keeping the setup CI already put on the draft: $setupName"
    } else {
        Write-Warning "The draft has no $setupName; CI normally uploads it (see docs/release-process.md)."
    }
} else {
    if (Test-Path -LiteralPath $setupExe) {
        Publish-Asset -Path $setupExe -Name $setupName
    } else {
        Write-Warning "No local $setupName to upload; the release keeps whatever it already has."
    }
}

Write-Host ''
if ($isDraft) {
    Write-Host "Draft $Repo $tag is ready, but NOT published."
    Write-Host "  Review it, then publish it yourself (docs/release-process.md):"
    Write-Host "  https://github.com/$Repo/releases/edit/$tag"
    Write-Host "  Self-update only starts working once the release is published."
} else {
    Write-Host "Published $Repo $tag"
    Write-Host "  https://github.com/$Repo/releases/latest"
    Write-Host "  update check: https://github.com/$Repo/releases/latest/download/manifest.txt"
}

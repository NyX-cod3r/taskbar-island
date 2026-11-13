[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release', 'Test', 'Installer', 'Converters')]
    [string]$Mode = 'Debug',
    # Folder containing ffmpeg\bin, pandoc\ and tesseract\ for the Converters
    # pack build. Defaults to $env:TBI_ENGINES_DIR, then %LOCALAPPDATA%\Filee\engines.
    [string]$EnginesDir = ''
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot

# --- toolchain --------------------------------------------------------------
# The local WinLibs install leads PATH: the legacy C:\MinGW dlltool on this PC
# fails to load, and Rust calls dlltool directly for windows-rs import
# libraries, so the healthy toolchain must precede it for cargo's whole process
# tree. On any other machine (CI) the WinLibs folder simply does not exist and
# whatever gcc/windres is already on PATH is used instead.
$winLibsBin = 'C:\Users\satya\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin'
if (Test-Path -LiteralPath (Join-Path $winLibsBin 'dlltool.exe')) {
    $env:PATH = "$winLibsBin;$env:PATH"
} elseif (-not (Get-Command gcc -ErrorAction SilentlyContinue)) {
    throw "No MinGW toolchain found: expected WinLibs at $winLibsBin, or gcc/windres on PATH."
}

# --- Cargo.toml: single source of the product identity ----------------------
# The exe's VERSIONINFO (build.rs) and the installer metadata (/DMyAppName,
# /DMyAppVersion below) are both generated from these two values, so nothing
# else in the repo may carry a literal version or product name.
function Read-CargoTomlValue([string]$Key) {
    $line = Select-String -LiteralPath (Join-Path $repoRoot 'Cargo.toml') `
        -Pattern "^$([regex]::Escape($Key))\s*=\s*`"([^`"]+)`"" | Select-Object -First 1
    if (-not $line) { throw "Cannot find '$Key =' in Cargo.toml" }
    return $line.Matches[0].Groups[1].Value
}
$MyAppName = Read-CargoTomlValue 'product-name'
$MyAppVersion = Read-CargoTomlValue 'version'
if ($MyAppVersion -notmatch '^\d+\.\d+\.\d+$') {
    throw "Cargo.toml version '$MyAppVersion' is not x.y.z - installer\update.iss compares versions numerically."
}

# --- toolchain manifest conflict (binutils bug 34362) -----------------------
# GCC 16 toolchains ship lib\default-manifest.o and link it into every
# executable. It carries an RT_MANIFEST just like ours, so ld aborts its .rsrc
# merge with ".rsrc merge failure: multiple non-default manifests" - the link
# may then fail outright or keep the wrong manifest (sourceware bug 34362,
# winlibs_mingw#299). The toolchain author's documented workaround is to take
# that file out of the way: rename it (never delete), once, and Assert-ExeManifest
# below proves the shipped exe still carries our own manifest.
function Repair-DefaultManifestConflict {
    $gcc = Get-Command gcc -ErrorAction SilentlyContinue
    if (-not $gcc) { return }
    $search = & $gcc.Source -print-search-dirs 2>$null | Out-String
    if ($search -notmatch '(?m)^libraries:\s*(.+)$') { return }
    foreach ($entry in ($Matches[1] -split ';')) {
        $dir = $entry.Trim().TrimStart('=')
        if (-not $dir) { continue }
        $path = Join-Path $dir 'default-manifest.o'
        if (-not (Test-Path -LiteralPath $path)) { continue }
        if (Test-Path -LiteralPath "$path.disabled-by-taskbar-island") { continue }
        Rename-Item -LiteralPath $path -NewName 'default-manifest.o.disabled-by-taskbar-island'
        Write-Host "toolchain: moved default-manifest.o aside (sourceware bug 34362): $path"
        Write-Host '  restore it with: Rename-Item default-manifest.o.disabled-by-taskbar-island default-manifest.o'
    }
}
Repair-DefaultManifestConflict

# --- artifact gate: the shipped exe must carry OUR manifest ------------------
# The <msix> element is what gives the running process its sparse-package
# identity (spec 3 Source B). A link that silently swapped in the toolchain's
# default manifest would still produce a working-looking exe, so every release
# build fails here instead.
function Assert-ExeManifest([string]$ExePath) {
    if (-not (Test-Path -LiteralPath $ExePath)) {
        throw "expected the built executable at $ExePath"
    }
    $bytes = [System.IO.File]::ReadAllBytes($ExePath)
    $text = [System.Text.Encoding]::UTF8.GetString($bytes)
    if ($text -notmatch '<assembly[ >]') {
        $text = [System.Text.Encoding]::Unicode.GetString($bytes)
    }
    $roots = ([regex]::Matches($text, '<assembly[ >]')).Count
    if ($roots -ne 1) {
        throw "$ExePath carries $roots <assembly> roots (expected exactly 1) - toolchain manifest conflict, sourceware bug 34362"
    }
    if ($text -notmatch 'urn:schemas-microsoft-com:msix\.v1') {
        throw "$ExePath does not carry the <msix> fusion manifest (sparse package identity, spec 3 Source B)"
    }
    Write-Host "manifest gate: $ExePath carries exactly one <assembly> root including <msix>"
}

# Where cargo put the executable. CI sets CARGO_BUILD_TARGET, so the artifact
# lands under target\<triple>\release\ instead of target\release\.
$ExeRelPath = if ($env:CARGO_BUILD_TARGET) {
    "target\$env:CARGO_BUILD_TARGET\release\taskbar-island.exe"
} else {
    'target\release\taskbar-island.exe'
}

function Find-ISCC {
    $isccPath = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    if (-not $isccPath) {
        $knownPaths = @(
            "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
            "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
            "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
        )
        $isccPath = $knownPaths | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    }
    if (-not $isccPath) {
        throw 'Inno Setup 6 was not found. Install it, then rerun: .\tools\build.ps1 -Mode Installer'
    }
    return $isccPath
}

# The engine payload lives only in the optional converters pack (spec 1:
# core installer vs. converters pack), never in the signed core installer.
function Resolve-EnginesDir {
    $dir = $EnginesDir
    if (-not $dir) { $dir = $env:TBI_ENGINES_DIR }
    if (-not $dir) { $dir = Join-Path $env:LOCALAPPDATA 'Filee\engines' }
    if (-not (Test-Path -LiteralPath $dir -PathType Container)) {
        throw "Engine payload folder not found: $dir. Pass -EnginesDir <dir> or set TBI_ENGINES_DIR."
    }
    return (Resolve-Path -LiteralPath $dir).Path
}

switch ($Mode) {
    'Debug'   { cargo build --bin taskbar-island }
    'Release' {
        cargo build --release --bin taskbar-island
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed with $LASTEXITCODE" }
        Assert-ExeManifest (Join-Path $repoRoot $ExeRelPath)
    }
    'Test'    { cargo test --bin taskbar-island }
    'Installer' {
        # The core installer: only TaskbarIsland's own files (exe + license
        # notices). No engine payload - that is the converters pack, and only
        # our own binaries may end up in what gets code-signed (docs/signing.md).
        cargo build --release --bin taskbar-island
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed with $LASTEXITCODE" }
        Assert-ExeManifest (Join-Path $repoRoot $ExeRelPath)
        $isccPath = Find-ISCC
        & $isccPath "/DMyAppVersion=$MyAppVersion" "/DMyAppName=$MyAppName" `
            "/DAppExePath=..\$ExeRelPath" (Join-Path $PSScriptRoot '..\installer\installer.iss')
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed with exit code $LASTEXITCODE" }
        # One installer per version, never cleaned up: drop the older ones so
        # dist\ only ever holds what was just built.
        & (Join-Path $PSScriptRoot 'clean-dist.ps1') -Version $MyAppVersion
    }
    'Converters' {
        # The optional pack with the third-party engines. Not signed under our
        # name (docs/signing.md); the core installer builds without it.
        $engineRoot = Resolve-EnginesDir
        & (Join-Path $PSScriptRoot 'engine-includes.ps1') -EngineRoot $engineRoot -OutDir (Join-Path $PSScriptRoot 'generated')
        if ($LASTEXITCODE -ne 0) { throw 'engine-includes.ps1 failed' }
        $isccPath = Find-ISCC
        & $isccPath "/DEngineRoot=$engineRoot" "/DMyAppVersion=$MyAppVersion" "/DMyAppName=$MyAppName" `
            (Join-Path $PSScriptRoot '..\installer\converters.iss')
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed with exit code $LASTEXITCODE" }
        & (Join-Path $PSScriptRoot 'clean-dist.ps1') -Version $MyAppVersion
    }
}

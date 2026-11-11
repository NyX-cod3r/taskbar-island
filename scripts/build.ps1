[CmdletBinding()]
param(
    [ValidateSet('Debug', 'Release', 'Test', 'Installer')]
    [string]$Mode = 'Debug',
    # Folder containing ffmpeg\bin, pandoc\ and tesseract\ for the Installer
    # build. Defaults to $env:TBI_ENGINES_DIR, then %LOCALAPPDATA%\Filee\engines.
    [string]$EnginesDir = ''
)

$winLibsBin = 'C:\Users\satya\AppData\Local\Microsoft\WinGet\Packages\BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\mingw64\bin'
if (-not (Test-Path -LiteralPath (Join-Path $winLibsBin 'dlltool.exe'))) {
    throw "Working WinLibs toolchain not found at: $winLibsBin"
}

# The legacy C:\MinGW dlltool on this PC fails to load. Rust calls dlltool
# directly for windows-rs import libraries, so the healthy toolchain must be
# first on PATH for the duration of Cargo's process tree.
$env:PATH = "$winLibsBin;$env:PATH"

switch ($Mode) {
    'Debug'   { cargo build --bin taskbar-island }
    'Release' { cargo build --release --bin taskbar-island }
    'Test'    { cargo test --bin taskbar-island }
    'Installer' {
        cargo build --release --bin taskbar-island
        if (-not $EnginesDir) { $EnginesDir = $env:TBI_ENGINES_DIR }
        if (-not $EnginesDir) { $EnginesDir = Join-Path $env:LOCALAPPDATA 'Filee\engines' }
        if (-not (Test-Path -LiteralPath $EnginesDir -PathType Container)) {
            throw "Engine payload folder not found: $EnginesDir. Pass -EnginesDir <dir> or set TBI_ENGINES_DIR."
        }
        $engineRoot = (Resolve-Path -LiteralPath $EnginesDir).Path
        & (Join-Path $PSScriptRoot 'engine-includes.ps1') -EngineRoot $engineRoot -OutDir (Join-Path $PSScriptRoot 'generated')
        if ($LASTEXITCODE -ne 0) { throw 'engine-includes.ps1 failed' }
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
            throw 'Inno Setup 6 was not found. Install it, then rerun: .\scripts\build.ps1 Installer'
        }
        & $isccPath "/DEngineRoot=$engineRoot" (Join-Path $PSScriptRoot 'installer.iss')
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed with exit code $LASTEXITCODE" }
        # One installer per version, never cleaned up: drop the older ones so
        # dist\ only ever holds what was just built.
        & (Join-Path $PSScriptRoot 'clean-dist.ps1')
    }
}

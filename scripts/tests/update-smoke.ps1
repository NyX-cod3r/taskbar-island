<#
  Runtime test for the TaskbarIsland installer's self-update (scripts\update.iss).

  It builds nothing: compile the harness first, then run this.

      ISCC.exe /DUpdateManifestUrl=http://127.0.0.1:18742/manifest.txt `
               scripts\tests\update-smoke.iss
      powershell -File scripts\tests\update-smoke.ps1

  The harness installs into throwaway folders under its own AppId, so the real
  TaskbarIsland install, its registry entries and a running app are untouched.

  Scenarios
    offline    nothing listening on the manifest URL   -> embedded build
    update     manifest newer than the bundled version -> downloaded payload
    uptodate   manifest equals the bundled version     -> embedded build
    badhash    manifest points at the wrong digest     -> embedded build
    noupdate   /NOUPDATE on the command line           -> embedded build
#>
[CmdletBinding()]
param(
    # Must match the port baked into update-smoke.exe by the ISCC command above.
    [int]$Port = 18742,
    [string]$WorkDir = ''
)

$ErrorActionPreference = 'Stop'

$testsRoot = $PSScriptRoot
$repoRoot = Split-Path -Parent (Split-Path -Parent $testsRoot)
$setupExe = Join-Path $repoRoot 'dist\update-smoke.exe'
$serverPy = Join-Path $testsRoot 'update-server.py'
$embeddedText = (Get-Content -LiteralPath (Join-Path $testsRoot 'embedded.txt') -Raw).Trim()

if (-not $WorkDir) { $WorkDir = Join-Path $env:LOCALAPPDATA 'Temp\opencode\tbi-smoke' }
$baseUrl = "http://127.0.0.1:$Port"
$manifestUrl = "$baseUrl/manifest.txt"

if (-not (Test-Path -LiteralPath $setupExe)) {
    throw "update-smoke.exe not found. Compile it first with /DUpdateManifestUrl=$manifestUrl"
}

if (Test-Path -LiteralPath $WorkDir) { Remove-Item -LiteralPath $WorkDir -Recurse -Force }
New-Item -ItemType Directory -Path $WorkDir -Force | Out-Null

# The "new" application file the manifest points at.
$payloadFile = Join-Path $WorkDir 'payload.txt'
Set-Content -LiteralPath $payloadFile -Value 'UPDATED-BUILD' -NoNewline -Encoding ascii
$payloadSha = (Get-FileHash -LiteralPath $payloadFile -Algorithm SHA256).Hash.ToLowerInvariant()

function Write-Manifest {
    param([string]$Path, [string]$Version, [string]$Sha256)
    $text = @(
        "version=$Version"
        "url=$baseUrl/payload.txt"
        "sha256=$Sha256"
    ) -join "`r`n"
    Set-Content -LiteralPath $Path -Value $text -Encoding ascii
}

$manNew = Join-Path $WorkDir 'manifest-new.txt'   # 9.9.9, correct digest
$manSame = Join-Path $WorkDir 'manifest-same.txt' # equals the bundled version
$manBad = Join-Path $WorkDir 'manifest-bad.txt'   # 9.9.9, wrong digest
Write-Manifest $manNew '9.9.9' $payloadSha
Write-Manifest $manSame '1.0.0' $payloadSha
Write-Manifest $manBad '9.9.9' ('0' * 64)

$results = New-Object System.Collections.Generic.List[object]

function Invoke-Scenario {
    param(
        [string]$Name,
        [string]$Manifest,   # $null => no server running
        [string[]]$ExtraArgs = @(),
        [string]$Expected
    )

    $installDir = Join-Path $WorkDir ("install-" + $Name)
    if (Test-Path -LiteralPath $installDir) { Remove-Item -LiteralPath $installDir -Recurse -Force }
    if ($Manifest) { Copy-Item -LiteralPath $Manifest -Destination (Join-Path $WorkDir 'manifest.txt') -Force }

    $setupLog = Join-Path $WorkDir ("setup-{0}.log" -f $Name)
    $argList = @(
        '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'
        "/DIR=$installDir", "/LOG=$setupLog"
    ) + $ExtraArgs

    $proc = Start-Process -FilePath $setupExe -ArgumentList $argList -Wait -PassThru -WindowStyle Hidden

    $installed = Join-Path $installDir 'app.txt'
    if (Test-Path -LiteralPath $installed) {
        $actual = (Get-Content -LiteralPath $installed -Raw).Trim()
    } else {
        $actual = '<missing>'
    }

    $ok = ($actual -eq $Expected)
    $note = if ($ok) { '' } else {
        $tail = if (Test-Path -LiteralPath $setupLog) {
            (Get-Content -LiteralPath $setupLog | Select-String -Pattern 'update:') -join ' | '
        } else { '<no setup log>' }
        "setup exit $($proc.ExitCode); got '$actual', want '$Expected'; $tail"
    }
    $results.Add([pscustomobject]@{ Name = $Name; Passed = $ok; Detail = $note })

    # Leave no residue: uninstall, then remove whatever is left.
    $unins = Join-Path $installDir 'unins000.exe'
    if (Test-Path -LiteralPath $unins) {
        Start-Process -FilePath $unins -ArgumentList @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART') -Wait -WindowStyle Hidden | Out-Null
    }
    if (Test-Path -LiteralPath $installDir) {
        Remove-Item -LiteralPath $installDir -Recurse -Force -ErrorAction SilentlyContinue
    }
    $leftover = Get-ChildItem -LiteralPath $WorkDir -Filter 'TaskbarIslandUpdateSmoke*' -ErrorAction SilentlyContinue
    if ($leftover) {
        $results.Add([pscustomobject]@{ Name = "$Name-residue"; Passed = $false; Detail = ($leftover.Name -join ', ') })
    }
}

$serverProc = $null
try {
    # 1. Nothing listening: the manifest fetch must fail and Setup must fall
    #    back to the payload embedded in the harness.
    Invoke-Scenario -Name 'offline' -Manifest $null -Expected $embeddedText

    # 2..5 with the local server up.
    Copy-Item -LiteralPath $manNew -Destination (Join-Path $WorkDir 'manifest.txt') -Force
    $serverErr = Join-Path $WorkDir 'server.err'
    $serverOut = Join-Path $WorkDir 'server.out'
    $python = (Get-Command python).Source
    $serverProc = Start-Process -FilePath $python -ArgumentList @($serverPy, $WorkDir, $Port) `
        -PassThru -WindowStyle Hidden -RedirectStandardOutput $serverOut -RedirectStandardError $serverErr

    $ready = $false
    for ($i = 0; $i -lt 50 -and -not $ready; $i++) {
        try {
            $r = Invoke-WebRequest -Uri $manifestUrl -UseBasicParsing -TimeoutSec 2
            if ($r.StatusCode -eq 200) { $ready = $true }
        } catch { Start-Sleep -Milliseconds 200 }
    }
    if (-not $ready) { throw "local manifest server never came up; see $serverErr" }

    Invoke-Scenario -Name 'update' -Manifest $manNew -Expected 'UPDATED-BUILD'
    Invoke-Scenario -Name 'uptodate' -Manifest $manSame -Expected $embeddedText
    Invoke-Scenario -Name 'badhash' -Manifest $manBad -Expected $embeddedText
    Invoke-Scenario -Name 'noupdate' -Manifest $manNew -ExtraArgs @('/NOUPDATE') -Expected $embeddedText
}
finally {
    if ($serverProc -and -not $serverProc.HasExited) {
        Stop-Process -Id $serverProc.Id -Force -ErrorAction SilentlyContinue
    }
}

Write-Host ''
$failedCount = 0
foreach ($r in $results) {
    if ($r.Passed) {
        Write-Host "[PASS] $($r.Name)"
    } else {
        $failedCount++
        Write-Host "[FAIL] $($r.Name)  $($r.Detail)"
    }
}
$total = $results.Count
Write-Host "$total checked, $failedCount failed"
if ($failedCount -gt 0) { exit 1 }
exit 0

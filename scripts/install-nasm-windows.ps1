#Requires -Version 5.1
<#
    .SYNOPSIS
        Downloads NASM into a project-local directory and exports it on PATH.

    .DESCRIPTION
        rav1e's `asm` feature compiles hand-tuned AV1 SIMD via nasm. This
        script grabs the official Windows build from nasm.us, extracts it to
        $env:NASM_HOME (defaults to <repo>\.tooling\nasm), and adds the
        directory to PATH for the current process and any later step in a
        GitHub Actions job.

    .NOTES
        - Idempotent: if nasm.exe is already on PATH or already present in the
          target directory, the script skips download.
        - Local installs land under .tooling/ which is .gitignored.
        - End users should prefer `winget install -e --id NASM.NASM` instead;
          this script exists so CI runners and the `mise run setup` task work
          without admin access or third-party JS actions.
#>

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$nasmVersion = '2.16.03'
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
$installRoot = if ($env:NASM_HOME) { $env:NASM_HOME } else { Join-Path $repoRoot '.tooling\nasm' }
$archiveName = "nasm-$nasmVersion-win64.zip"
$archivePath = Join-Path $installRoot $archiveName
$nasmExe = Join-Path $installRoot 'nasm.exe'

function Add-PathDirectory([string]$Directory) {
    if (-not (Test-Path $Directory)) { return }
    $resolved = (Resolve-Path $Directory).Path
    if ($env:Path -notlike "*${resolved}*") {
        $env:Path = "$resolved;$env:Path"
    }
    if ($env:GITHUB_PATH) {
        Add-Content -Path $env:GITHUB_PATH -Value $resolved
    }
}

if (Get-Command nasm -ErrorAction SilentlyContinue) {
    Write-Host "nasm already on PATH: $((Get-Command nasm).Source)"
    exit 0
}

if (Test-Path $nasmExe) {
    Write-Host "nasm already installed at $installRoot"
    Add-PathDirectory $installRoot
    exit 0
}

New-Item -ItemType Directory -Force -Path $installRoot | Out-Null

$downloadUrl = "https://www.nasm.us/pub/nasm/releasebuilds/$nasmVersion/win64/$archiveName"
Write-Host "Downloading $downloadUrl"
Invoke-WebRequest -UseBasicParsing -Uri $downloadUrl -OutFile $archivePath

Write-Host "Extracting to $installRoot"
$extractDir = Join-Path $installRoot '_extract'
if (Test-Path $extractDir) { Remove-Item -Recurse -Force $extractDir }
Expand-Archive -Path $archivePath -DestinationPath $extractDir

$inner = Get-ChildItem -Path $extractDir -Directory | Select-Object -First 1
if (-not $inner) {
    throw "NASM archive layout changed; could not find inner directory in $extractDir"
}
Get-ChildItem -Path $inner.FullName | Move-Item -Destination $installRoot -Force
Remove-Item -Recurse -Force $extractDir
Remove-Item -Force $archivePath

if (-not (Test-Path $nasmExe)) {
    throw "nasm.exe missing after extraction"
}

Add-PathDirectory $installRoot
Write-Host "nasm installed: $(& $nasmExe -v)"

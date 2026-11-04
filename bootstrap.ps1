<#
.SYNOPSIS
Sets up a Windows machine to build with Buck2.

.DESCRIPTION
Installs DotSlash into .tools, fetches the Buck2 release that toolchains\buck2
pins, and provisions the pinned toolchain. Linux and macOS use bootstrap.sh.

bin\buck2.cmd runs this with -Buck2Only on first use, which stops after Buck2.
Provisioning needs an elevated shell, because the Visual Studio and Windows SDK
installers do. Re-run it after the toolchain pin moves; it skips whatever is in
place.

Run it with: powershell -ExecutionPolicy Bypass -File bootstrap.ps1
#>
param(
    [switch]$Buck2Only,
    # Where the pinned toolchain archives are downloaded and cached. Kept out
    # of the repository so a clean checkout does not download several GB again.
    [string]$OfflineRoot = (Join-Path $env:LOCALAPPDATA "wows-toolkit-buck-offline")
)

$ErrorActionPreference = "Stop"

$DotSlashVersion = "v0.5.9"
$Assets = @{
    "AMD64" = @{
        Name = "dotslash-windows.$DotSlashVersion.tar.gz"
        Sha256 = "c925840a9bf9105a187cd120278cb023c51dac0cecb3234855152422751b7b41"
    }
    "ARM64" = @{
        Name = "dotslash-windows-arm64.$DotSlashVersion.tar.gz"
        Sha256 = "43d49d3472808738993ccf95a36dda8cc2869996d9a397bb02bde1e39b16146b"
    }
}

$Root = $PSScriptRoot
$Tools = Join-Path $Root ".tools"
$Bin = Join-Path $Tools "bin"
$DotSlash = Join-Path $Bin "dotslash.exe"
$Stamp = Join-Path $Bin "dotslash.sha256"
# The Windows curl and tar: Git's GNU tar reads a drive letter as a host name.
$Curl = Join-Path $env:SystemRoot "System32\curl.exe"
$Tar = Join-Path $env:SystemRoot "System32\tar.exe"

function Install-DotSlash($Asset) {
    foreach ($tool in $Curl, $Tar) {
        if (-not (Test-Path $tool)) {
            throw "$tool is required to download DotSlash."
        }
    }

    New-Item -ItemType Directory -Force -Path $Bin | Out-Null
    $scratch = Join-Path $Tools ("download-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $scratch | Out-Null
    try {
        Write-Host "Downloading DotSlash $DotSlashVersion"
        $archive = Join-Path $scratch $Asset.Name
        & $Curl -fsSL --retry 3 -o $archive "https://github.com/facebook/dotslash/releases/download/$DotSlashVersion/$($Asset.Name)"
        if ($LASTEXITCODE -ne 0) {
            throw "curl could not download $($Asset.Name)."
        }

        $actual = (Get-FileHash -Algorithm SHA256 $archive).Hash.ToLowerInvariant()
        if ($actual -ne $Asset.Sha256) {
            throw "$($Asset.Name) has SHA-256 $actual, not $($Asset.Sha256)."
        }

        & $Tar -xzf $archive -C $scratch dotslash.exe
        if ($LASTEXITCODE -ne 0) {
            throw "tar could not extract $($Asset.Name)."
        }

        try {
            Move-Item -Force -Path (Join-Path $scratch "dotslash.exe") -Destination $DotSlash
        }
        catch {
            # Another bootstrap installed this release first and is running it,
            # and Windows does not replace a running executable.
            $current = if (Test-Path $Stamp) { (Get-Content -Path $Stamp -TotalCount 1).Trim() }
            if ($current -ne $Asset.Sha256) {
                throw
            }

            return
        }

        Set-Content -Encoding ascii -Path $Stamp -Value $Asset.Sha256
    }
    finally {
        Remove-Item -Recurse -Force -Path $scratch
    }
}

function Write-MissingRequired($Tool, $Reason) {
    if (-not $script:RequiredReported) {
        Write-Host "Missing required host tools:"
        $script:RequiredReported = $true
    }

    $script:RequiredMissing = $true
    Write-Host "  ${Tool}: $Reason"
}

function Write-MissingOptional($Tool, $Reason) {
    if (-not $script:OptionalReported) {
        Write-Host "Optional capabilities unavailable:"
        $script:OptionalReported = $true
    }

    Write-Host "  ${Tool}: $Reason"
}

function Test-HostTools {
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        Write-MissingRequired "git" "required. Buck2 fetches git dependencies with it."
    }

    $fileSystem = Get-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem" -Name LongPathsEnabled -ErrorAction SilentlyContinue
    if (-not $fileSystem -or $fileSystem.LongPathsEnabled -ne 1) {
        Write-MissingRequired "long paths" "buck-out paths exceed 260 characters. As administrator, set LongPathsEnabled to 1 under HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem."
    }

    $elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
    if (-not $elevated) {
        Write-MissingRequired "elevation" "provisioning runs the Visual Studio and Windows SDK installers, which need an elevated shell."
    }

    if (-not (Get-Command reindeer -ErrorAction SilentlyContinue)) {
        Write-MissingOptional "reindeer" "regenerating third-party/rust/BUCK after a dependency change needs it. Run mise run install-reindeer."
    }
}

# 32-bit PowerShell on a 64-bit system reports x86 here and the system's
# architecture in PROCESSOR_ARCHITEW6432.
$architecture = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
$asset = $Assets[$architecture]
if (-not $asset) {
    throw "No DotSlash release for processor architecture $architecture."
}

$installed = if (Test-Path $Stamp) { (Get-Content -Path $Stamp -TotalCount 1).Trim() }
if (-not (Test-Path $DotSlash) -or $installed -ne $asset.Sha256) {
    Install-DotSlash $asset
}

Write-Host "Fetching Buck2"
$env:DOTSLASH_CACHE = Join-Path $Tools "dotslash"
& $DotSlash (Join-Path $Root "toolchains\buck2") --version
if ($LASTEXITCODE -ne 0) {
    throw "Buck2 does not run."
}

if ($Buck2Only) {
    exit 0
}

Test-HostTools
if ($script:RequiredMissing) {
    throw "Install the required host tools above and run bootstrap.ps1 again."
}

Write-Host "Provisioning the toolchain (caching archives in $OfflineRoot)"
& (Join-Path $Root "toolchains\windows\provision-toolchain.ps1") -OfflineRoot $OfflineRoot
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

Write-Host "Run bin\buck2 in place of buck2, or add $Root\bin to PATH. Then:"
Write-Host "  bin\buck2 build //:wows_toolkit"

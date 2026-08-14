#!/usr/bin/env pwsh
# One-time setup for building with Buck on Windows.
#
# Downloads and installs the pinned toolchain, then the pinned crate sources.
# Everything after this is offline: no Buck action downloads anything.
#
# Needs an elevated shell, because the Visual Studio and Windows SDK installers
# do. macOS and Linux: run setup.sh instead.
param(
    # Where the pinned archives are downloaded and cached. Keep it out of the
    # repo so a clean checkout does not re-download several GB.
    [string]$OfflineRoot = (Join-Path $env:LOCALAPPDATA "wows-toolkit-buck-offline")
)

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

$elevated = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $elevated) {
    Write-Error "Run this from an elevated shell: the Visual Studio and Windows SDK installers require it."
}

Write-Host "==> Toolchain (caching archives in $OfflineRoot)"
./toolchains/windows/provision-toolchain.ps1 -OfflineRoot $OfflineRoot

Write-Host ""
Write-Host "Setup complete. Build with:"
Write-Host "  buck2 build //:wows_toolkit"

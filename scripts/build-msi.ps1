#!/usr/bin/env pwsh
# Build the unsigned MSI declared by Buck.

param()

$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

$target = "toolchains//windows:wows_toolkit_msi"
$output = & buck2 build --show-output --target-platforms toolchains//windows:windows_x86_64_msvc -c native_build.mode=release $target
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$msiPath = ($output | Select-String -Pattern 'wows-toolkit-.*-unsigned\.msi$' | Select-Object -Last 1).ToString().Split(" ")[-1]
if (-not $msiPath) {
    throw "Buck did not report an MSI output for $target."
}

Write-Host "Unsigned MSI: $msiPath"

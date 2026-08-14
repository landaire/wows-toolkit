#!/usr/bin/env pwsh
# Build the unsigned MSI declared by Buck.

param()

$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

# The provisioned Buck2, not whatever is on PATH: the vendored prelude only
# loads under the release it was expanded from. Its path comes from the manifest
# so the pinned version lives in exactly one place.
$manifest = Get-Content -Raw "toolchains/windows/toolchain-manifest.json" | ConvertFrom-Json
$buck2 = Join-Path "toolchains/windows/offline/installed" $manifest.tools.buck2.path
if (-not (Test-Path -LiteralPath $buck2 -PathType Leaf)) {
    throw "Missing $buck2. Run toolchains/windows/provision-toolchain.ps1 first."
}

$target = "toolchains//windows:wows_toolkit_msi"
$output = & $buck2 build --show-output --target-platforms toolchains//windows:windows_x86_64_msvc -c native_build.mode=release $target
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$msiPath = ($output | Select-String -Pattern 'wows-toolkit-.*-unsigned\.msi$' | Select-Object -Last 1).ToString().Split(" ")[-1]
if (-not $msiPath) {
    throw "Buck did not report an MSI output for $target."
}

Write-Host "Unsigned MSI: $msiPath"

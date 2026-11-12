#!/usr/bin/env pwsh
# Build an unsigned MSI installer for local testing.
# Prerequisites: .NET SDK (for `dotnet tool`), cargo.
#
# Usage:
#   ./scripts/build-msi.ps1            # build release + MSI
#   ./scripts/build-msi.ps1 -SkipBuild # MSI only (assumes cargo build --release already ran)

param(
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
Set-Location (Split-Path $PSScriptRoot -Parent)

# Ensure WiX is installed
$wixCmd = Get-Command wix -ErrorAction SilentlyContinue
if (-not $wixCmd) {
    Write-Host "Installing WiX v6..."
    dotnet tool install --global wix --version 6.0.2
    # Refresh PATH so the current session picks it up.
    $env:PATH = [Environment]::GetEnvironmentVariable("PATH", "User") + ";" + $env:PATH
}

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"' |
    Select-Object -First 1).Matches.Groups[1].Value
$msiVersion = ($version -split '-', 2)[0]
$package = if ($version.Contains('-')) { 'wows-toolkit-gpui' } else { 'wows_toolkit' }
$binaryName = "$package.exe"
Write-Host "Version: $version ($package)"

if (-not $SkipBuild) {
    Write-Host "Building release binary..."
    cargo build --release --target x86_64-pc-windows-msvc -p $package
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

# Generate installer banner
Add-Type -AssemblyName System.Drawing

$png = [System.Drawing.Image]::FromFile("$PWD\assets\wows_toolkit.png")

# Banner image (493x58) - icon on the right, white background
$banner = New-Object System.Drawing.Bitmap 493, 58
$g = [System.Drawing.Graphics]::FromImage($banner)
$g.Clear([System.Drawing.Color]::White)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.DrawImage($png, (493 - 58), 0, 58, 58)
$g.Dispose()

$bmp24 = $banner.Clone(
    [System.Drawing.Rectangle]::new(0, 0, $banner.Width, $banner.Height),
    [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
$bmp24.Save("$PWD\wix\banner.bmp", [System.Drawing.Imaging.ImageFormat]::Bmp)
$bmp24.Dispose()
$banner.Dispose()
$png.Dispose()

# Generate license RTF from plaintext LICENSE
$licenseText = (Get-Content -Raw "$PWD\LICENSE").Replace("`r`n", "`n").Replace("`n", "\par`n")
$rtf = "{\rtf1\ansi\deff0{\fonttbl{\f0\fswiss Segoe UI;}}\viewkind4\uc1\pard\f0\fs20 \b MIT License\b0\par\par $licenseText}"
Set-Content -Path "$PWD\wix\license.rtf" -Value $rtf -Encoding ASCII

# Cargo uses the package name for each frontend's executable.
$binary = "target\x86_64-pc-windows-msvc\release\$binaryName"
if (-not (Test-Path -LiteralPath $binary)) {
    $binary = "target\release\$binaryName"
}
if (-not (Test-Path -LiteralPath $binary)) {
    throw "Release executable is missing: $binary"
}
$outDir = "target\wix"
$binDir = Join-Path $outDir 'payload'
New-Item -ItemType Directory -Path $binDir -Force | Out-Null
Copy-Item -LiteralPath $binary -Destination (Join-Path $binDir 'wows_toolkit.exe') -Force
$msiPath = "$outDir\wows-toolkit-v${version}-windows-x86_64.msi"
Write-Host "Building MSI: $msiPath"

# Ensure WiX extensions are installed (idempotent).
wix extension add WixToolset.UI.wixext/6.0.2 2>$null
wix extension add WixToolset.Util.wixext/6.0.2 2>$null

wix build wix\main.wxs `
    -d BinDir=$binDir `
    -d Version=$msiVersion `
    -ext WixToolset.UI.wixext `
    -ext WixToolset.Util.wixext `
    -o $msiPath

if ($LASTEXITCODE -ne 0) {
    Write-Error "WiX build failed"
    exit $LASTEXITCODE
}

Write-Host ""
Write-Host "MSI built: $msiPath" -ForegroundColor Green

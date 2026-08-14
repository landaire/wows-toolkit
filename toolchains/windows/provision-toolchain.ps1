param(
    [Parameter(Mandatory = $true)]
    [string]$OfflineRoot,
    [string]$InstallRoot = (Join-Path $PSScriptRoot "offline/installed"),
    [string]$ManifestPath = (Join-Path $PSScriptRoot "toolchain-manifest.json"),
    [string]$BuckConfigPath = (Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) ".buckconfig.local")
)

$ErrorActionPreference = "Stop"
$verifier = Join-Path $PSScriptRoot "verify-toolchain.ps1"

function Invoke-MsiAdministrativeInstall([string]$Archive, [string]$Destination, [string]$Name) {
    $process = Start-Process -FilePath (Join-Path $env:SystemRoot "System32\\msiexec.exe") -ArgumentList @("/a", $Archive, "/qn", "TARGETDIR=$Destination") -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "$Name administrative installation failed with exit code $($process.ExitCode)."
    }
}

$manifest = Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json -AsHashtable

# Fetching is the only step allowed to reach the network. Everything downstream,
# including every Buck action, runs against the resulting offline tree. Existing
# files are left alone; verify-toolchain rejects any that do not hash correctly.
foreach ($archive in $manifest.archives) {
    $destination = Join-Path $OfflineRoot $archive.path
    if (Test-Path -LiteralPath $destination -PathType Leaf) {
        continue
    }
    New-Item -ItemType Directory -Force -Path (Split-Path $destination -Parent) | Out-Null
    Write-Host "Downloading $($archive.name)"
    Invoke-WebRequest -Uri $archive.url -OutFile $destination -UseBasicParsing
}

$vs = $manifest.archives | Where-Object { $_.name -eq "visual_studio_build_tools" } | Select-Object -First 1
$vsLayout = Join-Path $OfflineRoot $vs.layout_path

# --layout mirrors the selected components locally so the install itself can run
# with --noWeb. Without it the installer would fetch payloads at install time.
$layoutMarker = Join-Path $vsLayout "Layout.json"
if (-not (Test-Path -LiteralPath $layoutMarker -PathType Leaf)) {
    $layoutArguments = @("--layout", $vsLayout, "--lang", "en-US", "--quiet", "--wait", "--norestart")
    foreach ($component in $vs.component_ids) {
        $layoutArguments += @("--add", $component)
    }
    $layoutProcess = Start-Process -FilePath (Join-Path $OfflineRoot $vs.path) -ArgumentList $layoutArguments -Wait -PassThru
    if ($layoutProcess.ExitCode -ne 0) {
        throw "Visual Studio layout creation failed with exit code $($layoutProcess.ExitCode)."
    }
}

& $verifier -OfflineRoot $OfflineRoot -InstallRoot $InstallRoot -ManifestPath $ManifestPath -ArchivesOnly
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null

$vsArguments = @("--noWeb", "--quiet", "--wait", "--norestart", "--installPath", (Join-Path $InstallRoot $vs.install_path), "--includeRecommended")
foreach ($component in $vs.component_ids) {
    $vsArguments += @("--add", $component)
}
$vsProcess = Start-Process -FilePath (Join-Path $vsLayout "vs_BuildTools.exe") -WorkingDirectory $vsLayout -ArgumentList $vsArguments -Wait -PassThru
if ($vsProcess.ExitCode -ne 0) {
    throw "Visual Studio offline installation failed with exit code $($vsProcess.ExitCode)."
}

$sdk = $manifest.archives | Where-Object { $_.name -eq "windows_sdk" } | Select-Object -First 1
$sdkImage = Join-Path $OfflineRoot $sdk.path
$mountedImage = Mount-DiskImage -ImagePath $sdkImage -PassThru
try {
    $sdkVolume = $mountedImage | Get-Volume
    $sdkSetup = Join-Path "$($sdkVolume.DriveLetter):" "WinSDKSetup.exe"
    $sdkProcess = Start-Process -FilePath $sdkSetup -ArgumentList @("/quiet", "/norestart", "/features", "+", "/installpath", (Join-Path $InstallRoot $sdk.install_path)) -Wait -PassThru
    if ($sdkProcess.ExitCode -ne 0) {
        throw "Windows SDK offline installation failed with exit code $($sdkProcess.ExitCode)."
    }
}
finally {
    Dismount-DiskImage -ImagePath $sdkImage
}

$rust = $manifest.archives | Where-Object { $_.name -eq "rust_msvc" } | Select-Object -First 1
Invoke-MsiAdministrativeInstall (Join-Path $OfflineRoot $rust.path) (Join-Path $InstallRoot $rust.install_path) "Rust MSVC"

$nasm = $manifest.archives | Where-Object { $_.name -eq "nasm" } | Select-Object -First 1
Expand-Archive -LiteralPath (Join-Path $OfflineRoot $nasm.path) -DestinationPath (Join-Path $InstallRoot $nasm.install_path) -Force

$wix = $manifest.archives | Where-Object { $_.name -eq "wix" } | Select-Object -First 1
Invoke-MsiAdministrativeInstall (Join-Path $OfflineRoot $wix.path) (Join-Path $InstallRoot $wix.install_path) "WiX"

$wixExtensions = $manifest.archives | Where-Object { $_.name -eq "wix_extensions" } | Select-Object -First 1
Expand-Archive -LiteralPath (Join-Path $OfflineRoot $wixExtensions.path) -DestinationPath (Join-Path $InstallRoot $wixExtensions.install_path) -Force

$zstdArchive = $manifest.archives | Where-Object { $_.name -eq "zstd" } | Select-Object -First 1
Expand-Archive -LiteralPath (Join-Path $OfflineRoot $zstdArchive.path) -DestinationPath (Join-Path $InstallRoot $zstdArchive.install_path) -Force

# Buck2 itself is part of the pinned boundary: the vendored prelude only loads
# under the release it was expanded from.
$buck2 = $manifest.archives | Where-Object { $_.name -eq "buck2" } | Select-Object -First 1
$buck2Dir = Join-Path $InstallRoot $buck2.install_path
New-Item -ItemType Directory -Force -Path $buck2Dir | Out-Null
$zstd = Join-Path $InstallRoot $manifest.tools.zstd.path
& $zstd -d -f (Join-Path $OfflineRoot $buck2.path) -o (Join-Path $buck2Dir "buck2.exe")
if ($LASTEXITCODE -ne 0) {
    throw "Decompressing the pinned Buck2 binary failed with exit code $LASTEXITCODE."
}

& $verifier -OfflineRoot $OfflineRoot -InstallRoot $InstallRoot -ManifestPath $ManifestPath -BuckConfigPath $BuckConfigPath
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

Write-Host "Provisioned offline Windows toolchain tree at $InstallRoot."

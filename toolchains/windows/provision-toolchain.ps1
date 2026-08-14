param(
    [Parameter(Mandatory = $true)]
    [string]$OfflineRoot,
    [string]$InstallRoot = (Join-Path $PSScriptRoot "offline/installed"),
    [string]$ManifestPath = (Join-Path $PSScriptRoot "toolchain-manifest.json"),
    [string]$BuckConfigPath = ""
)

$ErrorActionPreference = "Stop"
$verifier = Join-Path $PSScriptRoot "verify-toolchain.ps1"

function Invoke-MsiAdministrativeInstall([string]$Archive, [string]$Destination, [string]$Name) {
    $process = Start-Process -FilePath (Join-Path $env:SystemRoot "System32\\msiexec.exe") -ArgumentList @("/a", $Archive, "/qn", "TARGETDIR=$Destination") -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        throw "$Name administrative installation failed with exit code $($process.ExitCode)."
    }
}

& $verifier -OfflineRoot $OfflineRoot -InstallRoot $InstallRoot -ManifestPath $ManifestPath -ArchivesOnly
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

$manifest = Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json -AsHashtable
New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null

$vs = $manifest.archives | Where-Object { $_.name -eq "visual_studio_build_tools" } | Select-Object -First 1
$vsBootstrapper = Join-Path $OfflineRoot $vs.path
$vsArguments = @("--noWeb", "--quiet", "--wait", "--norestart", "--installPath", (Join-Path $InstallRoot $vs.install_path), "--includeRecommended")
foreach ($component in $vs.component_ids) {
    $vsArguments += @("--add", $component)
}
$vsProcess = Start-Process -FilePath $vsBootstrapper -WorkingDirectory (Join-Path $OfflineRoot $vs.layout_path) -ArgumentList $vsArguments -Wait -PassThru
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

& $verifier -OfflineRoot $OfflineRoot -InstallRoot $InstallRoot -ManifestPath $ManifestPath
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

Write-Host "Provisioned offline Windows toolchain tree at $InstallRoot."

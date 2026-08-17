#Requires -Version 7

param(
    [Parameter(Mandatory = $true)]
    [string]$OfflineRoot,
    [string]$InstallRoot = (Join-Path $PSScriptRoot "offline/installed"),
    [string]$ManifestPath = (Join-Path $PSScriptRoot "toolchain-manifest.json"),
    [string]$BuckConfigPath = (Join-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) ".buckconfig.local")
)

$ErrorActionPreference = "Stop"
$verifier = Join-Path $PSScriptRoot "verify-toolchain.ps1"

function Show-VsInstallerLog([string]$Context) {
    # The bootstrapper reports nothing useful through its exit code; its own log
    # is the only place that says what happened.
    Write-Host "--- $Context : newest Visual Studio installer log ---"
    Get-ChildItem "$env:TEMP\dd_*.log" -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 2 |
        ForEach-Object {
            Write-Host "=== $($_.Name) ==="
            Get-Content $_.FullName -Tail 40 | ForEach-Object { Write-Host $_ }
        }
}

function Wait-VsInstaller([int]$TimeoutMinutes = 20) {
    # Not vs_installershell: it is the GUI shell, and CI images that ship Visual
    # Studio keep one resident, so waiting for it to exit never returns. The
    # deadline is a backstop for anything else that lingers.
    $deadline = (Get-Date).AddMinutes($TimeoutMinutes)
    while ((Get-Date) -lt $deadline) {
        $running = @(Get-Process -Name "vs_installer", "vs_setup", "vs_bootstrapper" -ErrorAction SilentlyContinue)
        if ($running.Count -eq 0) {
            return
        }
        Start-Sleep -Seconds 5
    }
    $names = ((Get-Process -Name "vs_installer", "vs_setup", "vs_bootstrapper" -ErrorAction SilentlyContinue) | ForEach-Object { $_.ProcessName }) -join ", "
    Write-Host "Gave up after $TimeoutMinutes minutes waiting for: $names"
}

function Wait-MsiIdle([int]$TimeoutMinutes = 10) {
    # Windows Installer serialises transactions behind a global mutex, so an
    # administrative install started while another one holds it blocks forever.
    $deadline = (Get-Date).AddMinutes($TimeoutMinutes)
    while ((Get-Date) -lt $deadline) {
        if (@(Get-Process msiexec -ErrorAction SilentlyContinue).Count -eq 0) {
            return
        }
        Start-Sleep -Seconds 5
    }
    Write-Host "Windows Installer still busy after $TimeoutMinutes minutes; continuing anyway."
}

function Invoke-MsiAdministrativeInstall([string]$Archive, [string]$Destination, [string]$Name, [int]$TimeoutMinutes = 15) {
    Wait-MsiIdle
    $msiLog = Join-Path $env:TEMP ("msi-" + [IO.Path]::GetFileNameWithoutExtension($Archive) + ".log")
    $process = Start-Process -FilePath (Join-Path $env:SystemRoot "System32\msiexec.exe") -ArgumentList @("/a", $Archive, "/qn", "/L*v", $msiLog, "TARGETDIR=$Destination") -PassThru
    if (-not $process.WaitForExit($TimeoutMinutes * 60 * 1000)) {
        $process | Stop-Process -Force -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $msiLog) {
            Write-Host "--- tail of $msiLog ---"
            Get-Content $msiLog -Tail 25 | ForEach-Object { Write-Host $_ }
        }
        throw "$Name administrative install hung for $TimeoutMinutes minutes and was killed."
    }
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
        if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -eq $archive.sha256) {
            continue
        }
        Write-Host "Refetching $($archive.name): on-disk copy does not match its pin"
        Remove-Item -LiteralPath $destination -Force
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

    # --wait returns when the launcher exits, not when the mirror is complete,
    # and the child may not have spawned yet when it does. Rerunning --layout
    # resumes it, so drive it to completion and let --verify be the judge.
    $attempt = 0
    while ($true) {
        $attempt++
        Wait-VsInstaller
        $layoutVerify = Start-Process -FilePath (Join-Path $OfflineRoot $vs.path) `
            -ArgumentList @("--layout", $vsLayout, "--verify", "--quiet", "--wait") -Wait -PassThru
        $size = [math]::Round((((Get-ChildItem -LiteralPath $vsLayout -Recurse -File -ErrorAction SilentlyContinue) | Measure-Object Length -Sum).Sum / 1GB), 2)
        Write-Host "Layout attempt ${attempt}: verify exit $($layoutVerify.ExitCode), $size GB mirrored"
        if ($layoutVerify.ExitCode -eq 0 -and $size -gt 0.5) {
            break
        }
        if ($layoutVerify.ExitCode -eq 0) {
            Show-VsInstallerLog "layout verified but only $size GB present"
        }
        if ($attempt -ge 10) {
            $size = [math]::Round((((Get-ChildItem -LiteralPath $vsLayout -Recurse -File -ErrorAction SilentlyContinue) | Measure-Object Length -Sum).Sum / 1GB), 2)
            $count = (Get-ChildItem -LiteralPath $vsLayout -Recurse -File -ErrorAction SilentlyContinue).Count
            $free = [math]::Round(((Get-PSDrive (Split-Path $OfflineRoot -Qualifier).TrimEnd(":")).Free / 1GB), 2)
            throw "Visual Studio layout still incomplete after $attempt attempts (verify exit $($layoutVerify.ExitCode)); layout $size GB across $count files; $free GB free on $(Split-Path $OfflineRoot -Qualifier)"
        }
        Start-Sleep -Seconds 10
        $resume = Start-Process -FilePath (Join-Path $OfflineRoot $vs.path) -ArgumentList $layoutArguments -Wait -PassThru
        if ($resume.ExitCode -ne 0) {
            throw "Visual Studio layout resume failed with exit code $($resume.ExitCode) on attempt $attempt."
        }
    }
}

& $verifier -OfflineRoot $OfflineRoot -InstallRoot $InstallRoot -ManifestPath $ManifestPath -ArchivesOnly
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

# Visual Studio refuses to install into a long root and reports only "the root
# installation path is too long" behind a generic exit code, so check first.
$vsInstallPath = Join-Path $InstallRoot $vs.install_path
if ($vsInstallPath.Length -gt 64) {
    throw "Visual Studio install path is $($vsInstallPath.Length) characters and it must be at most 64: $vsInstallPath. Pass a shorter -InstallRoot."
}

New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null

$sdk = $manifest.archives | Where-Object { $_.name -eq "windows_sdk" } | Select-Object -First 1
$sdkTarget = Join-Path $InstallRoot $sdk.install_path
$sdkVersionDir = "10.0.26100.0"

# The SDK bundle refuses to place a second copy of a version the machine already
# has and exits 1001, and CI images ship exactly the version pinned here. When
# that is the case, link the pinned version in rather than fighting it; the
# verifier still checks the tool versions behind the link.
$systemSdk = Join-Path ${env:ProgramFiles(x86)} ("Windows Kits" + [IO.Path]::DirectorySeparatorChar + "10")
$systemHeaders = Join-Path $systemSdk "Include\$sdkVersionDir"
if ((Test-Path -LiteralPath $systemHeaders -PathType Container) -and -not (Test-Path -LiteralPath $sdkTarget)) {
    Write-Host "Using the Windows SDK already installed at $systemSdk"
    New-Item -ItemType Directory -Force -Path (Split-Path $sdkTarget -Parent) | Out-Null
    New-Item -ItemType Junction -Path $sdkTarget -Target $systemSdk | Out-Null
} elseif (-not (Test-Path -LiteralPath $sdkTarget)) {
    $sdkImage = Join-Path $OfflineRoot $sdk.path
    $mountedImage = Mount-DiskImage -ImagePath $sdkImage -PassThru
    try {
        $sdkVolume = $mountedImage | Get-Volume
        $sdkSetup = Join-Path "$($sdkVolume.DriveLetter):" "WinSDKSetup.exe"
        $sdkProcess = Start-Process -FilePath $sdkSetup -ArgumentList @(
            "/quiet", "/norestart", "/installpath", $sdkTarget,
            "/features", "OptionId.WindowsSoftwareDevelopmentKit", "OptionId.DesktopCPPx64"
        ) -Wait -PassThru
        if ($sdkProcess.ExitCode -ne 0) {
            throw "Windows SDK offline installation failed with exit code $($sdkProcess.ExitCode)."
        }
    }
    finally {
        Dismount-DiskImage -ImagePath $sdkImage | Out-Null
    }
}

$vsArguments = @("--noWeb", "--quiet", "--wait", "--norestart", "--installPath", (Join-Path $InstallRoot $vs.install_path), "--includeRecommended")
foreach ($component in $vs.component_ids) {
    $vsArguments += @("--add", $component)
}
$vsProcess = Start-Process -FilePath (Join-Path $vsLayout "vs_BuildTools.exe") -WorkingDirectory $vsLayout -ArgumentList $vsArguments -Wait -PassThru
if ($vsProcess.ExitCode -ne 0) {
    Show-VsInstallerLog "install failed"
    $free = [math]::Round(((Get-PSDrive (Split-Path $InstallRoot -Qualifier).TrimEnd(":")).Free / 1GB), 2)
    throw "Visual Studio offline installation failed with exit code $($vsProcess.ExitCode); $free GB free on $(Split-Path $InstallRoot -Qualifier)"
}
Wait-VsInstaller

$rust = $manifest.archives | Where-Object { $_.name -eq "rust_msvc" } | Select-Object -First 1
Invoke-MsiAdministrativeInstall (Join-Path $OfflineRoot $rust.path) (Join-Path $InstallRoot $rust.install_path) "Rust MSVC"

$nasm = $manifest.archives | Where-Object { $_.name -eq "nasm" } | Select-Object -First 1
Expand-Archive -LiteralPath (Join-Path $OfflineRoot $nasm.path) -DestinationPath (Join-Path $InstallRoot $nasm.install_path) -Force

$wix = $manifest.archives | Where-Object { $_.name -eq "wix" } | Select-Object -First 1
Invoke-MsiAdministrativeInstall (Join-Path $OfflineRoot $wix.path) (Join-Path $InstallRoot $wix.install_path) "WiX"

$wixExtensions = $manifest.archives | Where-Object { $_.name -eq "wix_extensions" } | Select-Object -First 1
Expand-Archive -LiteralPath (Join-Path $OfflineRoot $wixExtensions.path) -DestinationPath (Join-Path $InstallRoot $wixExtensions.install_path) -Force

$extensionRoot = Join-Path $InstallRoot $wixExtensions.install_path
foreach ($name in @("WixToolset.UI.wixext", "WixToolset.Util.wixext")) {
    $package = Join-Path $extensionRoot "$name.$($wixExtensions.version).nupkg"
    if (-not (Test-Path -LiteralPath $package -PathType Leaf)) {
        throw "Missing WiX extension package: $package"
    }
    # Expand-Archive goes by extension, so the package needs a .zip name first.
    $staged = Join-Path $env:TEMP "$name.zip"
    Copy-Item -LiteralPath $package -Destination $staged -Force
    Expand-Archive -LiteralPath $staged -DestinationPath (Join-Path $extensionRoot $name) -Force
    Remove-Item -LiteralPath $staged -Force
}

$zstdArchive = $manifest.archives | Where-Object { $_.name -eq "zstd" } | Select-Object -First 1
Expand-Archive -LiteralPath (Join-Path $OfflineRoot $zstdArchive.path) -DestinationPath (Join-Path $InstallRoot $zstdArchive.install_path) -Force

$python = $manifest.archives | Where-Object { $_.name -eq "python" } | Select-Object -First 1
$pythonDir = Join-Path $InstallRoot $python.install_path
Expand-Archive -LiteralPath (Join-Path $OfflineRoot $python.path) -DestinationPath $pythonDir -Force

# The embeddable distribution ships a ._pth file that makes sys.path exactly its
# own contents, so a script cannot import a module sitting next to it. The
# prelude's C++ and Rust tools are multi-module, so they need the default rules.
Get-ChildItem -LiteralPath $pythonDir -Filter "*._pth" | Remove-Item -Force

# Buck2 itself is part of the pinned boundary: the vendored prelude only loads
# under the release it was expanded from.
$buck2 = $manifest.archives | Where-Object { $_.name -eq "buck2" } | Select-Object -First 1
$buck2Dir = Join-Path $InstallRoot $buck2.install_path
New-Item -ItemType Directory -Force -Path $buck2Dir | Out-Null
# A running daemon holds the pinned binary open, and the decompress below would
# otherwise fail with a bare permission error that names no cause.
$runningBuck2 = @(Get-Process buck2 -ErrorAction SilentlyContinue)
if ($runningBuck2.Count -gt 0) {
    Write-Host "Stopping $($runningBuck2.Count) running buck2 process(es) holding the pinned binary open."
    $runningBuck2 | Stop-Process -Force
    Start-Sleep -Seconds 2
}

$zstd = Join-Path $InstallRoot $manifest.tools.zstd.path
& $zstd -d -f (Join-Path $OfflineRoot $buck2.path) -o (Join-Path $buck2Dir "buck2.exe")
if ($LASTEXITCODE -ne 0) {
    throw "Decompressing the pinned Buck2 binary failed with exit code $LASTEXITCODE."
}

& $verifier -OfflineRoot $OfflineRoot -InstallRoot $InstallRoot -ManifestPath $ManifestPath -BuckConfigPath $BuckConfigPath
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}

# Crate sources are not committed; fetch them against Cargo.lock's checksums.
$repo = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
& (Join-Path $InstallRoot $manifest.tools.python.path) (Join-Path $repo "scripts/fetch-buck-deps.py")
if ($LASTEXITCODE -ne 0) {
    throw "Fetching crate sources failed with exit code $LASTEXITCODE."
}

Write-Host "Provisioned offline Windows toolchain tree at $InstallRoot."

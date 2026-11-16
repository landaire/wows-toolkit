#Requires -Version 7

param(
    [Parameter(Mandatory = $true)]
    [string]$OfflineRoot,
    [string]$InstallRoot = (Join-Path $PSScriptRoot "offline/installed"),
    [string]$ManifestPath = (Join-Path $PSScriptRoot "toolchain-manifest.json"),
    [string]$BuckConfigPath = "",
    [switch]$ArchivesOnly
)

$ErrorActionPreference = "Stop"

function Fail([string]$Message) {
    [Console]::Error.WriteLine($Message)
    exit 1
}

function Get-RequiredPath([string]$Path, [string]$Description) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        # Braces are required: `$Description:` parses as a scope qualifier.
        Fail "Missing ${Description}: $Path"
    }
    return $Path
}

function Test-ArchiveHash($Archive) {
    $path = Join-Path $OfflineRoot $Archive.path
    Get-RequiredPath $path "archive $($Archive.name)" | Out-Null
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
    if ($actual -ne $Archive.sha256.ToLowerInvariant()) {
        Fail "Archive hash mismatch: $($Archive.name). Expected $($Archive.sha256), got $actual."
    }
}

function Test-FileVersion($ToolName, $Tool) {
    $path = Join-Path $InstallRoot $Tool.path
    Get-RequiredPath $path "tool $ToolName" | Out-Null
    if ($Tool.ContainsKey("version")) {
        $actual = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($path).FileVersion
        if ($actual -ne $Tool.version) {
            Fail "Tool version mismatch: $ToolName. Expected $($Tool.version), got $actual."
        }
    }
    if ($Tool.ContainsKey("version_output")) {
        $actual = (& $path --version 2>&1 | Out-String).Trim()
        if ($actual -notmatch [regex]::Escape($Tool.version_output)) {
            Fail "Tool version mismatch: $ToolName. Expected output containing $($Tool.version_output), got $actual."
        }
    }
}

if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
    Fail "Missing toolchain manifest: $ManifestPath"
}

$manifest = Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json -AsHashtable
foreach ($archive in $manifest.archives) {
    Test-ArchiveHash $archive
}

# vs_BuildTools.exe resolves its payload from Microsoft's channel manifest, so
# its pinned hash does not pin the toolset version. Take whatever it installed.
$msvcToolsets = Join-Path $InstallRoot "VisualStudio/BuildTools/VC/Tools/MSVC"
if (Test-Path -LiteralPath $msvcToolsets -PathType Container) {
    $toolsets = @(Get-ChildItem -LiteralPath $msvcToolsets -Directory)
    if ($toolsets.Count -ne 1) {
        Fail "Expected exactly one MSVC toolset under $msvcToolsets, found $($toolsets.Count)."
    }
    foreach ($entry in @($manifest.tools.Keys)) {
        $manifest.tools[$entry].path = $manifest.tools[$entry].path.Replace("{msvc}", $toolsets[0].Name)
    }
}

$vsArchive = $manifest.archives | Where-Object { $_.name -eq "visual_studio_build_tools" } | Select-Object -First 1
$vsLayout = Join-Path $OfflineRoot $vsArchive.layout_path
if (-not (Test-Path -LiteralPath $vsLayout -PathType Container)) {
    Fail "Missing Visual Studio offline layout: $vsLayout"
}

if ($ArchivesOnly) {
    Write-Host "Verified $($manifest.archives.Count) offline archive hashes."
    exit 0
}

$vsVerify = Start-Process -FilePath (Join-Path $vsLayout "vs_BuildTools.exe") `
    -ArgumentList @("--layout", $vsLayout, "--verify", "--quiet", "--wait") -Wait -PassThru
if ($vsVerify.ExitCode -ne 0) {
    Fail "Visual Studio offline layout verification failed with exit code $($vsVerify.ExitCode)."
}

foreach ($entry in $manifest.tools.GetEnumerator()) {
    Test-FileVersion $entry.Key $entry.Value
}

foreach ($name in @("include", "lib", "ucrt_lib", "um_lib")) {
    $path = Join-Path $InstallRoot $manifest.sdk[$name]
    if (-not (Test-Path -LiteralPath $path -PathType Container)) {
        Fail "Missing Windows SDK $name directory: $path"
    }
}

if ($BuckConfigPath) {
    # Buck reads these instead of resolving any tool through PATH. This is the
    # Windows counterpart of scripts/refresh-buck-toolchain.nu.
    $installed = (Resolve-Path -LiteralPath $InstallRoot).Path
    $tool = { param($name) Join-Path $installed $manifest.tools[$name].path }

    # Derive the MSVC root from cl.exe rather than pinning the version twice:
    # <msvc>/bin/Hostx64/x64/cl.exe
    $msvcRoot = Split-Path (Split-Path (Split-Path (Split-Path (& $tool "cl") -Parent) -Parent) -Parent) -Parent
    $sdkInclude = Join-Path $installed $manifest.sdk.include

    $include = @(
        (Join-Path $msvcRoot "include")
        (Join-Path $sdkInclude "ucrt")
        (Join-Path $sdkInclude "um")
        (Join-Path $sdkInclude "shared")
    ) -join ";"
    $libPaths = @(
        (Join-Path $msvcRoot "lib\x64")
        (Join-Path $installed $manifest.sdk.ucrt_lib)
        (Join-Path $installed $manifest.sdk.um_lib)
    ) -join ";"

    # cvtres ships with MSVC, beside cl, not with the SDK tools beside rc.
    $cvtres = Join-Path (Split-Path (& $tool "cl") -Parent) "cvtres.exe"

    @(
        "[hermetic_tools]"
        "ar = $(& $tool "lib")"
        "cc = $(& $tool "cl")"
        "cxx = $(& $tool "cl")"
        "link = $(& $tool "link")"
        "ml64 = $(& $tool "ml64")"
        # ring hardcodes clang when targeting wasm32 and cc-rs has no MSVC
        # fallback there, so the Visual Studio Clang component is required.
        "clang = $(& $tool "clang")"
        "llvm_ar = $(& $tool "llvm_ar")"
        "rc = $(& $tool "rc")"
        "cvtres = $cvtres"
        "midl = $(& $tool "midl")"
        "rustc = $(& $tool "rustc")"
        "rustdoc = $(& $tool "rustdoc")"
        "clippy_driver = $(& $tool "clippy_driver")"
        "rustfmt = $(& $tool "rustfmt")"
        "nasm = $(& $tool "nasm")"
        "python = $(& $tool "python")"
        "wix = $(& $tool "wix")"
        "wix_ui_extension = $(& $tool "wix_ui_extension")"
        "wix_util_extension = $(& $tool "wix_util_extension")"
        "include = $include"
        "lib_paths = $libPaths"
        ""
    ) -join "`n" | ForEach-Object {
        # Not Set-Content -Encoding utf8: Windows PowerShell 5.1 writes a BOM,
        # and Buck2 rejects the file with a parse error on the first section.
        [System.IO.File]::WriteAllText($BuckConfigPath, $_, (New-Object System.Text.UTF8Encoding($false)))
    }
    Write-Host "Wrote $BuckConfigPath."
}

Write-Host "Verified offline Windows toolchain at $InstallRoot."

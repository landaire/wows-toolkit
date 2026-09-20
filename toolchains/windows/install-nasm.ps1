#Requires -Version 5.1

<#
.SYNOPSIS
Installs the pinned NASM and records its location in .buckconfig.local.

.DESCRIPTION
Repairs the `nasm` entry of an existing machine-local .buckconfig.local. The
platform bootstrap (setup.ps1, or toolchains/windows/provision-toolchain.ps1)
provisions NASM and writes the entire file; this script covers the case where
that single entry is absent or names a file that no longer exists. It
terminates with an error when no bootstrap has produced a configuration.

NASM is taken from the manifest entry the bootstrap uses and is verified
against the same SHA-256, so a Cargo build and a Buck build assemble rav1e's
SIMD kernels with the same assembler.

The installation directory is .tooling/nasm rather than a location under
%TEMP%. An entry naming a file that Windows has since deleted is not reported
until rav1e's build script fails, several minutes into a build.

The script makes no changes once the pinned version is installed. Specify
-Force to reinstall.
#>

[CmdletBinding()]
param(
    [string]$RepoRoot = (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent),
    [switch]$Force
)

$ErrorActionPreference = "Stop"

$manifestPath = Join-Path $PSScriptRoot "toolchain-manifest.json"
$installRoot = Join-Path $RepoRoot ".tooling\nasm"
$nasmExe = Join-Path $installRoot "nasm.exe"
$buckConfig = Join-Path $RepoRoot ".buckconfig.local"
$bootstrapHint = "Run the platform bootstrap first: .\setup.ps1, or toolchains\windows\provision-toolchain.ps1."

function Read-BuckConfigLines {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) { return @() }
    $text = [System.IO.File]::ReadAllText($Path)
    if ($text.Length -eq 0) { return @() }
    $lines = $text -split "`r?`n"
    # A trailing newline produces a final empty element. Discarding it prevents
    # each rewrite from appending another blank line. The one-element case is
    # handled separately because 0..-1 yields two elements.
    if ($lines.Length -gt 1 -and $lines[-1] -eq "") {
        $lines = $lines[0..($lines.Length - 2)]
    } elseif ($lines.Length -eq 1 -and $lines[0] -eq "") {
        $lines = @()
    }
    return $lines
}

function Get-SectionRanges {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][AllowEmptyString()][string[]]$Lines,
        [Parameter(Mandatory = $true)][string]$Section
    )

    $header = "[$Section]"
    $ranges = @()
    for ($i = 0; $i -lt $Lines.Length; $i++) {
        if ($Lines[$i].Trim() -ne $header) { continue }
        $end = $Lines.Length
        for ($j = $i + 1; $j -lt $Lines.Length; $j++) {
            if ($Lines[$j].Trim() -match '^\[.+\]$') {
                $end = $j
                break
            }
        }
        $ranges += @{ Start = $i; End = $end }
    }
    return $ranges
}

function Find-BuckConfigKeyIndex {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][AllowEmptyString()][string[]]$Lines,
        [Parameter(Mandatory = $true)][string]$Section,
        [Parameter(Mandatory = $true)][string]$Key
    )

    # Buck2 merges repeated sections and applies the last definition of a key.
    # The effective line is therefore the final match across every section of
    # that name.
    $found = -1
    foreach ($range in @(Get-SectionRanges -Lines $Lines -Section $Section)) {
        for ($i = $range.Start + 1; $i -lt $range.End; $i++) {
            if ($Lines[$i] -match "^\s*$([regex]::Escape($Key))\s*=") { $found = $i }
        }
    }
    return $found
}

function Get-BuckConfigValue {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][AllowEmptyString()][string[]]$Lines,
        [Parameter(Mandatory = $true)][string]$Section,
        [Parameter(Mandatory = $true)][string]$Key
    )

    $index = Find-BuckConfigKeyIndex -Lines $Lines -Section $Section -Key $Key
    if ($index -lt 0) { return $null }
    if ($Lines[$index] -match "^\s*$([regex]::Escape($Key))\s*=\s*(.*)$") {
        return $Matches[1].Trim()
    }
    return $null
}

function Set-BuckConfigValue {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Section,
        [Parameter(Mandatory = $true)][string]$Key,
        [Parameter(Mandatory = $true)][string]$Value
    )

    $lines = @(Read-BuckConfigLines -Path $Path)
    $keyIndex = Find-BuckConfigKeyIndex -Lines $lines -Section $Section -Key $Key
    if ($keyIndex -ge 0) {
        $lines[$keyIndex] = "$Key = $Value"
    } else {
        $ranges = @(Get-SectionRanges -Lines $lines -Section $Section)
        if ($ranges.Length -eq 0) {
            $lines = @($lines) + @("[$Section]", "$Key = $Value")
        } else {
            $last = $ranges[$ranges.Length - 1]
            $insertAt = $last.End
            # Place the key within its section rather than after the blank line
            # that separates it from the next section.
            while ($insertAt -gt $last.Start + 1 -and $lines[$insertAt - 1].Trim() -eq "") {
                $insertAt--
            }
            $head = $lines[0..($insertAt - 1)]
            $tail = @()
            if ($insertAt -lt $lines.Length) { $tail = $lines[$insertAt..($lines.Length - 1)] }
            $lines = @($head) + @("$Key = $Value") + @($tail)
        }
    }

    # Not Set-Content -Encoding utf8: Windows PowerShell 5.1 writes a BOM, and
    # Buck2 rejects the file with a parse error on the first section.
    $text = ($lines -join "`n") + "`n"
    [System.IO.File]::WriteAllText($Path, $text, (New-Object System.Text.UTF8Encoding($false)))
}

function Get-InstalledNasmVersion {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) { return $null }
    try {
        $reported = (& $Path -v) -join " "
    } catch {
        return $null
    }
    if ($reported -match 'NASM version (\S+)') { return $Matches[1] }
    return $null
}

$archive = (Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json).archives |
    Where-Object { $_.name -eq "nasm" } |
    Select-Object -First 1
if (-not $archive) {
    throw "$manifestPath has no archive named 'nasm'."
}
foreach ($field in @("url", "sha256", "version")) {
    if ([string]::IsNullOrWhiteSpace($archive.$field)) {
        throw "The 'nasm' archive in $manifestPath has no $field."
    }
}

# Writing a single nasm entry into a file the bootstrap did not produce moves
# the failure to the next absent tool, where it appears to be an unrelated
# defect.
$existing = @(Read-BuckConfigLines -Path $buckConfig)
if ($existing.Length -eq 0) {
    throw "$buckConfig does not exist. $bootstrapHint"
}
if (-not (Get-BuckConfigValue -Lines $existing -Section "hermetic_tools" -Key "cc")) {
    throw "$buckConfig has no [hermetic_tools] cc, so it did not come from the bootstrap. $bootstrapHint"
}

$installed = Get-InstalledNasmVersion -Path $nasmExe
if (-not $Force -and $installed -eq $archive.version) {
    Write-Host "NASM $installed is already installed at $installRoot. No download required."
} else {
    if ($installed) {
        Write-Host "Replacing NASM $installed at $installRoot with the pinned version $($archive.version)."
    }

    $staging = Join-Path ([System.IO.Path]::GetTempPath()) ("nasm-" + [Guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force -Path $staging | Out-Null
    try {
        $zip = Join-Path $staging "nasm.zip"
        Write-Host "Downloading NASM $($archive.version)."
        # Windows PowerShell 5.1 negotiates TLS 1.0 by default, which nasm.us
        # rejects.
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -Uri $archive.url -OutFile $zip -UseBasicParsing

        $actual = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $archive.sha256.ToLowerInvariant()) {
            throw "Archive hash mismatch for nasm. Expected $($archive.sha256), got $actual."
        }

        $unpacked = Join-Path $staging "x"
        Expand-Archive -LiteralPath $zip -DestinationPath $unpacked -Force

        # The archive places its contents under nasm-<version>/. Locating the
        # executable rather than assuming that structure prevents a change to
        # the archive from installing nothing.
        $found = Get-ChildItem -Path $unpacked -Recurse -Filter "nasm.exe" -File | Select-Object -First 1
        if (-not $found) {
            throw "nasm.exe is not present in $($archive.url); the archive structure has changed."
        }

        New-Item -ItemType Directory -Force -Path $installRoot | Out-Null
        foreach ($name in @("nasm.exe", "ndisasm.exe", "LICENSE")) {
            $from = Join-Path $found.DirectoryName $name
            if (-not (Test-Path -LiteralPath $from)) { continue }
            $to = Join-Path $installRoot $name
            Copy-Item -LiteralPath $from -Destination $to -Force
            Set-ItemProperty -LiteralPath $to -Name IsReadOnly -Value $false
        }
    } finally {
        Remove-Item -Recurse -Force -LiteralPath $staging -ErrorAction SilentlyContinue
    }

    $installed = Get-InstalledNasmVersion -Path $nasmExe
    if ($installed -ne $archive.version) {
        throw "Expected NASM $($archive.version) at $nasmExe, found '$installed'."
    }
    Write-Host "Installed NASM $installed to $installRoot."
}

Set-BuckConfigValue -Path $buckConfig -Section "hermetic_tools" -Key "nasm" -Value $nasmExe
Write-Host "Recorded [hermetic_tools] nasm = $nasmExe in $buckConfig."

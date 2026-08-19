<#
.SYNOPSIS
Builds the CLI tools through Buck and packages them into the release archive.

.DESCRIPTION
The set of tools, and the name each ships under, come from
build-support/release-tools.json rather than from whatever Buck happened to
emit. The archive is verified against that list before the script exits, so a
renamed or dropped tool fails the build instead of quietly changing the
release.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Buck2,
    # Release tag or branch name, with slashes already replaced.
    [Parameter(Mandatory = $true)][string]$Slug,
    [string]$TargetPlatform
)

$ErrorActionPreference = "Stop"

# "//:wowsunpack" on a command line and "root//:wowsunpack" in buck2's JSON are
# the same target; compare the part after the cell.
function Get-TargetKey {
    param([string]$Label)
    $index = $Label.IndexOf("//")
    if ($index -lt 0) { return $Label }
    return $Label.Substring($index + 2)
}

$spec = Get-Content -Raw build-support/release-tools.json | ConvertFrom-Json
$targets = @($spec.tools | ForEach-Object { $_.target })

$buildArgs = @("build", "-c", "native_build.mode=release")
if ($TargetPlatform) { $buildArgs += @("--target-platforms", $TargetPlatform) }
$buildArgs += "--show-json-output"
$buildArgs += $targets

$json = & $Buck2 @buildArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$outputs = @{}
foreach ($property in ($json | Out-String | ConvertFrom-Json).PSObject.Properties) {
    $outputs[(Get-TargetKey $property.Name)] = $property.Value
}

New-Item -ItemType Directory -Force artifacts | Out-Null
Get-ChildItem artifacts | Remove-Item -Recurse -Force

$expected = @()
foreach ($tool in $spec.tools) {
    $key = Get-TargetKey $tool.target
    if (-not $outputs.ContainsKey($key)) {
        throw "buck2 reported no output for $($tool.target)."
    }
    $name = $tool.ship + [System.IO.Path]::GetExtension($outputs[$key])
    Copy-Item $outputs[$key] (Join-Path "artifacts" $name) -Force
    $expected += $name
}
foreach ($extra in $spec.extra_files) {
    Copy-Item $extra artifacts/ -Force
    $expected += [System.IO.Path]::GetFileName($extra)
}
# Buck outputs are read-only, which Compress-Archive carries into the zip.
Get-ChildItem artifacts | ForEach-Object { $_.IsReadOnly = $false }

$zip = "wows_toolkit_tools_${Slug}_win64.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path artifacts/* -DestinationPath $zip

Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead((Resolve-Path $zip).Path)
try {
    $actual = @($archive.Entries | ForEach-Object { $_.FullName })
} finally {
    $archive.Dispose()
}

$missing = @($expected | Where-Object { $actual -notcontains $_ })
$unexpected = @($actual | Where-Object { $expected -notcontains $_ })
if ($missing.Count -gt 0 -or $unexpected.Count -gt 0) {
    if ($missing.Count -gt 0) { Write-Host "Missing from ${zip}: $($missing -join ', ')" }
    if ($unexpected.Count -gt 0) { Write-Host "Unexpected in ${zip}: $($unexpected -join ', ')" }
    throw "$zip does not match build-support/release-tools.json."
}

Write-Host "$zip contains: $($actual -join ', ')"
exit 0

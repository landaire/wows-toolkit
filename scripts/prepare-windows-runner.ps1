<#
.SYNOPSIS
Prepares a Windows CI runner for a Buck build and copies the checkout to a short path.

.DESCRIPTION
Three things have to be true before buck2 can build this workspace on a hosted
Windows runner:

  * Defender must not hold freshly written artifacts. buck2's materialiser
    replaces files it has just written, and real-time protection keeps a handle
    open long enough that the replace fails with "Access is denied (os error 5)".
  * Long paths must be enabled. buck-out paths carry a configuration hash, a
    doubled target name and a content hash.
  * The tree must live somewhere short. buck2 canonicalises its project root, so
    a junction resolves back to the long workspace path and link.exe still hits
    MAX_PATH; the tree is copied instead.

Every step is verified. A silently ignored exclusion produces a corrupt release
rather than a failed build, so this script fails loudly instead.
#>
[CmdletBinding()]
param(
    # Checkout to copy from, normally $env:GITHUB_WORKSPACE.
    [Parameter(Mandatory = $true)][string]$Source,
    # Short path to copy to, normally C:\w.
    [Parameter(Mandatory = $true)][string]$Destination,
    # Additional paths to exclude from Defender, e.g. the toolchain install root.
    [string[]]$AlsoExclude = @()
)

$ErrorActionPreference = "Stop"

function Test-DefenderRealtime {
    if (-not (Get-Command Get-MpComputerStatus -ErrorAction SilentlyContinue)) {
        return $false
    }
    try {
        return [bool](Get-MpComputerStatus).RealTimeProtectionEnabled
    } catch {
        # Some images expose the cmdlets but refuse to report status. Assume
        # protection is on so a missing exclusion is still treated as fatal.
        return $true
    }
}

function Set-DefenderExclusion {
    param([string[]]$Path)

    if (-not (Test-DefenderRealtime)) {
        Write-Host "Defender real-time protection is off; exclusions not needed."
        return
    }
    if (-not (Get-Command Add-MpPreference -ErrorAction SilentlyContinue)) {
        throw "Defender real-time protection is on but Add-MpPreference is unavailable, so buck2's materialiser cannot be protected from it."
    }

    foreach ($item in $Path) {
        try {
            Add-MpPreference -ExclusionPath $item
        } catch {
            # Report but keep going: the read-back below is what decides.
            Write-Host "::warning::Add-MpPreference failed for ${item}: $($_.Exception.Message)"
        }
    }

    $applied = @()
    try {
        $applied = @((Get-MpPreference).ExclusionPath) | Where-Object { $null -ne $_ }
    } catch {
        throw "Defender exclusions could not be read back: $($_.Exception.Message)"
    }

    $missing = @()
    foreach ($item in $Path) {
        $want = $item.TrimEnd('\')
        $found = $false
        foreach ($have in $applied) {
            if ($have.TrimEnd('\') -ieq $want) { $found = $true; break }
        }
        if (-not $found) { $missing += $item }
    }
    if ($missing.Count -gt 0) {
        throw "Defender exclusions did not apply: $($missing -join ', '). buck2's materialiser fails intermittently with 'Access is denied (os error 5)' while real-time protection holds freshly written artifacts."
    }
    Write-Host "Defender exclusions active: $($Path -join ', ')"
}

function Enable-LongPath {
    $key = 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem'
    Set-ItemProperty -Path $key -Name LongPathsEnabled -Value 1
    $value = (Get-ItemProperty -Path $key -Name LongPathsEnabled).LongPathsEnabled
    if ($value -ne 1) {
        throw "LongPathsEnabled is $value after being set to 1."
    }
    Write-Host "Long paths enabled."
}

$exclusions = @($Destination) + $AlsoExclude
if ($env:RUNNER_TEMP) { $exclusions += $env:RUNNER_TEMP }
Set-DefenderExclusion -Path $exclusions

Enable-LongPath

robocopy $Source $Destination /E /NFL /NDL /NJH /NJS /NP | Out-Null
# robocopy uses exit codes 0-7 for success variants; 8 and above are failures.
if ($LASTEXITCODE -ge 8) {
    throw "robocopy $Source -> $Destination failed with exit code $LASTEXITCODE."
}
if (-not (Test-Path (Join-Path $Destination '.buckroot'))) {
    throw "$Destination does not contain .buckroot after the copy, so it is not a usable buck2 project root."
}
Write-Host "Checkout copied to $Destination."

exit 0

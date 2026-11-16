$ErrorActionPreference = "Stop"

$repositoryRoot = Split-Path (Split-Path (Split-Path $PSScriptRoot -Parent) -Parent) -Parent
$manifestSource = Join-Path $repositoryRoot "toolchains/windows/toolchain-manifest.json"
$verifier = Join-Path $repositoryRoot "toolchains/windows/verify-toolchain.ps1"
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("wows-toolchain-hash-mismatch-" + [guid]::NewGuid())

try {
    New-Item -ItemType Directory -Force -Path $temporaryRoot | Out-Null
    $manifest = Get-Content -Raw -Path $manifestSource | ConvertFrom-Json -AsHashtable
    $manifest.archives[0].sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
    $manifestPath = Join-Path $temporaryRoot "toolchain-manifest.json"
    $manifest | ConvertTo-Json -Depth 8 | Set-Content -NoNewline -Encoding utf8 -Path $manifestPath

    $archivePath = Join-Path $temporaryRoot $manifest.archives[0].path
    New-Item -ItemType Directory -Force -Path (Split-Path $archivePath -Parent) | Out-Null
    [System.IO.File]::WriteAllBytes($archivePath, [byte[]](1, 2, 3, 4))

    $result = & $verifier -OfflineRoot $temporaryRoot -ManifestPath $manifestPath -ArchivesOnly 2>&1
    if ($LASTEXITCODE -eq 0) {
        throw "verify-toolchain.ps1 accepted a substituted archive hash."
    }

    $diagnostic = $result | Out-String
    if ($diagnostic -notmatch "Archive hash mismatch: visual_studio_build_tools") {
        throw "Expected the visual_studio_build_tools hash diagnostic, got: $diagnostic"
    }

    Write-Host "PASS: substituted archive hash was rejected before Buck starts."
}
finally {
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $temporaryRoot
}

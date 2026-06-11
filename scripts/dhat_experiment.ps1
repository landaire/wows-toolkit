# Diagnostic harness: launch the instrumented dhat build, watch markers + memory,
# detect whether the profiler drop completes and dhat-heap.json appears, then kill.
param(
    [int]$Secs = 22,
    [int]$Trim = 16,
    [string]$DoExit = "0",
    [int]$WatchSecs = 100
)

$root = "G:\dev\wows-toolkit"
$exe = "$root\target\profiling\wows_toolkit.exe"
$markers = "$root\dhat-markers.log"
$heap = "$root\dhat-heap.json"
foreach ($f in @($markers, $heap)) { if (Test-Path $f) { Remove-Item $f -Force } }

$env:DHAT_RUN_SECS = "$Secs"
$env:DHAT_TRIM = "$Trim"
$env:DHAT_EXIT = "$DoExit"

Write-Host "Launch: RUN_SECS=$Secs TRIM=$Trim EXIT=$DoExit"
$p = Start-Process -FilePath $exe -WorkingDirectory $root -PassThru
$seenAfterDrop = $false
$iters = [int]($WatchSecs / 1.5)
for ($i = 0; $i -lt $iters; $i++) {
    Start-Sleep -Milliseconds 1500
    if (-not $p.HasExited) { try { $p.Refresh() } catch {} }
    $mem = if ($p.HasExited) { "exited" } else { "{0} MiB" -f [math]::Round($p.PrivateMemorySize64/1MB) }
    $mk = if (Test-Path $markers) { ((Get-Content $markers) -join ",") } else { "(none)" }
    $hp = if (Test-Path $heap) { "{0:N0} KiB" -f ((Get-Item $heap).Length/1KB) } else { "-" }
    Write-Host ("  t={0,4:N0}s  priv={1,-10}  heap.json={2,-10}  markers=[{3}]" -f ($i*1.5), $mem, $hp, $mk)
    if ((Test-Path $heap) -and ($mk -match "after_drop")) { $seenAfterDrop = $true; Write-Host "  -> drop completed + heap written"; break }
    if ($p.HasExited) { break }
}
if (-not $p.HasExited) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
Write-Host ""
if (Test-Path $heap) { Write-Host ("RESULT: dhat-heap.json = {0:N0} KiB" -f ((Get-Item $heap).Length/1KB)) }
else { Write-Host "RESULT: no dhat-heap.json" }
Write-Host ("markers: " + $(if (Test-Path $markers) { (Get-Content $markers) -join " -> " } else { "none" }))

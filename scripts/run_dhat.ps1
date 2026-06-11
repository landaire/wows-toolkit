# Launch the dhat-instrumented build, sample OS memory while it runs, and wait
# for it to self-close (DHAT_RUN_SECS) so dhat-heap.json is written.
param(
    [int]$Secs = 30,
    [string]$Exe = "G:\dev\wows-toolkit\target\profiling\wows_toolkit.exe"
)

$env:DHAT_RUN_SECS = "$Secs"
$heap = "G:\dev\wows-toolkit\dhat-heap.json"
if (Test-Path $heap) { Remove-Item $heap -Force }

Write-Host "Launching $Exe (auto-close after $Secs s of app time)..."
$p = Start-Process -FilePath $Exe -WorkingDirectory "G:\dev\wows-toolkit" -PassThru

$peakWS = 0; $peakPriv = 0
while (-not $p.HasExited) {
    Start-Sleep -Milliseconds 1500
    try { $p.Refresh() } catch { break }
    if ($p.HasExited) { break }
    $ws = $p.WorkingSet64
    $priv = $p.PrivateMemorySize64
    if ($ws -gt $peakWS) { $peakWS = $ws }
    if ($priv -gt $peakPriv) { $peakPriv = $priv }
    $t = "{0:N0}" -f ([math]::Round($ws / 1MB))
    $tp = "{0:N0}" -f ([math]::Round($priv / 1MB))
    Write-Host ("  WorkingSet={0} MiB  PrivateBytes={1} MiB" -f $t, $tp)
}

Write-Host ""
Write-Host ("PEAK WorkingSet   = {0:N0} MiB" -f ([math]::Round($peakWS / 1MB)))
Write-Host ("PEAK PrivateBytes = {0:N0} MiB" -f ([math]::Round($peakPriv / 1MB)))
if (Test-Path $heap) {
    $sz = (Get-Item $heap).Length
    Write-Host ("dhat-heap.json written: {0:N0} KiB" -f ([math]::Round($sz / 1KB)))
} else {
    Write-Host "WARNING: dhat-heap.json was NOT written (process may have been killed before clean exit)."
}

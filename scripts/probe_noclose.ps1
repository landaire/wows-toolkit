# Launch the dhat exe with the auto-close disabled (timer set far in the future)
# and sample memory for ~50s to see whether it plateaus or runs away on its own.
$env:DHAT_RUN_SECS = "999999"
$exe = "G:\dev\wows-toolkit\target\profiling\wows_toolkit.exe"
$p = Start-Process -FilePath $exe -WorkingDirectory "G:\dev\wows-toolkit" -PassThru
Write-Host "pid=$($p.Id) launched, sampling ~50s with NO close command..."
for ($i = 0; $i -lt 34; $i++) {
    Start-Sleep -Milliseconds 1500
    try { $p.Refresh() } catch { break }
    if ($p.HasExited) { Write-Host "exited early"; break }
    $ws = [math]::Round($p.WorkingSet64 / 1MB)
    $pv = [math]::Round($p.PrivateMemorySize64 / 1MB)
    Write-Host ("  t={0,4:N0}s  WS={1,5} MiB  Priv={2,5} MiB" -f ($i * 1.5), $ws, $pv)
}
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
Write-Host "killed"

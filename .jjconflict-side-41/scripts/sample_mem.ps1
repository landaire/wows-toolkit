# Launch an exe, sample OS memory to steady state, report steady + peak, then kill.
# Used to A/B the global allocator (system heap vs mimalloc) on the normal build.
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [int]$Secs = 36,
    [string]$Label = "run"
)
$root = "G:\dev\wows-toolkit"
$p = Start-Process -FilePath $Exe -WorkingDirectory $root -PassThru
$ws = @(); $pv = @()
$iters = [int]($Secs / 1.5)
for ($i = 0; $i -lt $iters; $i++) {
    Start-Sleep -Milliseconds 1500
    if ($p.HasExited) { break }
    try { $p.Refresh() } catch { break }
    $w = [math]::Round($p.WorkingSet64 / 1MB)
    $v = [math]::Round($p.PrivateMemorySize64 / 1MB)
    $ws += $w; $pv += $v
    Write-Host ("  [{0}] t={1,4:N0}s WS={2,5} Priv={3,5} MiB" -f $Label, ($i * 1.5), $w, $v)
}
Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
# Steady state = median of the last third of samples.
function Median($a) { $s = $a | Sort-Object; $n = $s.Count; if ($n -eq 0) { return 0 }; if ($n % 2) { $s[[int](($n-1)/2)] } else { [math]::Round(($s[$n/2-1] + $s[$n/2]) / 2) } }
$tail = [int]($ws.Count / 3)
$wsTail = $ws[-$tail..-1]; $pvTail = $pv[-$tail..-1]
Write-Host ""
Write-Host ("RESULT [{0}]  steady WS={1} MiB  steady Priv={2} MiB  peak WS={3} MiB  peak Priv={4} MiB" -f `
    $Label, (Median $wsTail), (Median $pvTail), ($ws | Measure-Object -Maximum).Maximum, ($pv | Measure-Object -Maximum).Maximum)

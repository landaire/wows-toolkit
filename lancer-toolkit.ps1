# =====================================================================
#  Lanceur-patcheur WoWs Toolkit (version team : webhook + filtre Clan Wars)
# ---------------------------------------------------------------------
#  But : garantir que NOTRE build modifié est toujours celui qui se lance,
#  même si la mise à jour officielle a écrasé l'exe dans AppData.
#
#  À chaque lancement :
#   1. Si notre build custom diffère de celui déployé -> on le redéploie.
#   2. On informe si une nouvelle version officielle (upstream) existe.
#   3. On lance l'application.
# =====================================================================

$ErrorActionPreference = 'Stop'

$Repo   = 'C:\Games\WoWs-Toolkit'
$Build  = Join-Path $Repo 'target\release\wows_toolkit.exe'
$Deploy = Join-Path $env:LOCALAPPDATA 'WoWs Toolkit\wows_toolkit.exe'

Write-Host '=== Lanceur WoWs Toolkit (build team) ===' -ForegroundColor Cyan

# --- 1. Redéploiement de notre build si besoin ----------------------
if (-not (Test-Path $Build)) {
    Write-Host "[!] Build custom introuvable : $Build" -ForegroundColor Yellow
    Write-Host "    Demande à Claude de recompiler (cargo build --release --bin wows_toolkit)." -ForegroundColor Yellow
} else {
    $needCopy = $true
    if (Test-Path $Deploy) {
        $s = Get-Item $Build
        $d = Get-Item $Deploy
        # On recopie si taille différente OU si le déployé est plus ancien (= écrasé par la maj officielle)
        if (($s.Length -eq $d.Length) -and ($d.LastWriteTime -ge $s.LastWriteTime)) {
            $needCopy = $false
        }
    }
    if ($needCopy) {
        Write-Host '[*] Notre build a été écrasé ou est plus récent -> redéploiement...' -ForegroundColor Green
        $deployDir = Split-Path $Deploy
        if (-not (Test-Path $deployDir)) { New-Item -ItemType Directory -Force -Path $deployDir | Out-Null }
        Copy-Item $Build $Deploy -Force
        Write-Host '[OK] Build team restauré dans AppData.' -ForegroundColor Green
    } else {
        Write-Host '[OK] Build team déjà en place.' -ForegroundColor Green
    }
}

# --- 2. Vérification informative d'une nouvelle version upstream -----
try {
    Push-Location $Repo
    git fetch --quiet origin --tags 2>$null
    $localBase = (git describe --tags --abbrev=0 2>$null)
    $remoteTip = (git ls-remote --tags origin 2>$null | Select-String -Pattern 'refs/tags/v[0-9]' | ForEach-Object { ($_ -split 'refs/tags/')[1] } | Sort-Object -Descending | Select-Object -First 1)
    if ($remoteTip -and $localBase -and ($remoteTip -ne $localBase)) {
        Write-Host ''
        Write-Host "[i] Nouvelle version officielle disponible : $remoteTip (ta base : $localBase)" -ForegroundColor Magenta
        Write-Host "    -> Ne l'installe PAS depuis l'appli. Demande à Claude une mise à niveau" -ForegroundColor Magenta
        Write-Host "       (rebase de nos features sur $remoteTip) pour garder webhook + Clan Wars." -ForegroundColor Magenta
        Write-Host ''
    }
    Pop-Location
} catch {
    # Vérif version non critique : on continue le lancement quoi qu'il arrive.
    if ((Get-Location).Path -ne $Repo) { } else { Pop-Location -ErrorAction SilentlyContinue }
}

# --- 3. Lancement ----------------------------------------------------
if (Test-Path $Deploy) {
    Write-Host '[*] Lancement de WoWs Toolkit...' -ForegroundColor Cyan
    Start-Process -FilePath $Deploy
} else {
    Write-Host "[X] Exe introuvable : $Deploy" -ForegroundColor Red
    Read-Host 'Appuie sur Entrée pour fermer'
}

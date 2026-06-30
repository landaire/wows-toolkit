//! Installateur autonome WoWs Toolkit (version team : webhook + filtre Clan Wars).
//! Embarque l'exe modifie et l'installe au lancement (espace utilisateur, sans UAC).

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// L'exe modifie est embarque dans cet installeur a la compilation.
const TOOLKIT: &[u8] = include_bytes!(r"C:\Games\WoWs-Toolkit\target\release\wows_toolkit.exe");

fn main() {
    println!("  ====================================================");
    println!("    Installation de WoWs Toolkit - version team");
    println!("    (envoi auto Discord + filtre Clan Wars)");
    println!("  ====================================================\n");

    // Dossier de destination : %LOCALAPPDATA%\WoWs Toolkit
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    if local.is_empty() {
        eprintln!("  [ERREUR] Variable LOCALAPPDATA introuvable.");
        pause();
        std::process::exit(1);
    }
    let dest_dir = PathBuf::from(&local).join("WoWs Toolkit");
    let dest_exe = dest_dir.join("wows_toolkit.exe");

    // Fermer l'app si elle tourne (sinon fichier verrouille)
    let _ = Command::new("taskkill")
        .args(["/IM", "wows_toolkit.exe", "/F"])
        .output();
    std::thread::sleep(std::time::Duration::from_millis(1200));

    // Creer le dossier + ecrire l'exe
    if let Err(e) = std::fs::create_dir_all(&dest_dir) {
        eprintln!("  [ERREUR] Creation du dossier impossible : {e}");
        pause();
        std::process::exit(1);
    }
    if let Err(e) = std::fs::write(&dest_exe, TOOLKIT) {
        eprintln!("  [ERREUR] Ecriture de l'application impossible : {e}");
        eprintln!("  Ferme WoWs Toolkit s'il est ouvert puis relance l'installeur.");
        pause();
        std::process::exit(1);
    }
    println!("  [OK] Application installee dans :");
    println!("       {}", dest_dir.display());

    // Raccourci sur le bureau via PowerShell (WScript.Shell)
    let ps = format!(
        "$w=New-Object -ComObject WScript.Shell; \
         $sc=$w.CreateShortcut([Environment]::GetFolderPath('Desktop')+'\\WoWs Toolkit (team).lnk'); \
         $sc.TargetPath='{}'; $sc.WorkingDirectory='{}'; $sc.Save()",
        dest_exe.display(),
        dest_dir.display()
    );
    let shortcut_ok = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if shortcut_ok {
        println!("  [OK] Raccourci cree sur le bureau : \"WoWs Toolkit (team)\"");
    } else {
        println!("  [i] Raccourci non cree (lance l'exe depuis le dossier ci-dessus).");
    }

    println!("\n  ----------------------------------------------------");
    println!("   Termine ! Lance le jeu via le raccourci du bureau.");
    println!("   IMPORTANT : si l'appli propose une mise a jour,");
    println!("   REFUSE-LA (sinon la version team est ecrasee).");
    println!("  ----------------------------------------------------\n");

    pause();
}

/// Attend l'appui sur Entree (utile quand lance par double-clic).
fn pause() {
    print!("  Appuie sur Entree pour fermer...");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
}

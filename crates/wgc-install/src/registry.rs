//! Windows-registry install discovery.

use crate::GameInstall;

#[cfg(windows)]
pub fn discover_registry_installs() -> Vec<GameInstall> {
    // Wargaming writes an uninstall entry with an InstallLocation value.
    // Read the HKLM uninstall keys and keep any whose install dir looks like
    // a WoWs install.
    let mut out = Vec::new();
    let uninstall = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    let Ok(root) = windows_registry::LOCAL_MACHINE.open(uninstall) else {
        return out;
    };
    let Ok(names) = root.keys() else {
        return out;
    };
    for name in names {
        let Ok(sub) = root.open(&name) else { continue };
        let Ok(display) = sub.get_string("DisplayName") else { continue };
        if !display.contains("World of Warships") {
            continue;
        }
        let Ok(location) = sub.get_string("InstallLocation") else { continue };
        if location.is_empty() {
            continue;
        }
        let path = std::path::PathBuf::from(location);
        if crate::version::is_probably_wows(&path) {
            out.push(GameInstall { source: crate::InstallSource::Manual, ..GameInstall::manual(path) });
        }
    }
    out
}

#[cfg(not(windows))]
pub fn discover_registry_installs() -> Vec<GameInstall> {
    Vec::new()
}

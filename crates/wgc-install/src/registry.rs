//! Windows-registry install discovery.

use crate::GameInstall;

#[cfg(windows)]
pub fn discover_registry_installs() -> Vec<GameInstall> {
    // Wargaming writes an uninstall entry with an InstallLocation value.
    // Read the HKLM uninstall keys and keep any whose install dir looks like
    // a WoWs install.
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::enums::KEY_READ;

    let mut out = Vec::new();
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let uninstall = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
    let Ok(root) = hklm.open_subkey_with_flags(uninstall, KEY_READ) else {
        return out;
    };
    for name in root.enum_keys().flatten() {
        let Ok(sub) = root.open_subkey_with_flags(&name, KEY_READ) else { continue };
        let display: String = sub.get_value("DisplayName").unwrap_or_default();
        if !display.contains("World of Warships") {
            continue;
        }
        let location: String = sub.get_value("InstallLocation").unwrap_or_default();
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

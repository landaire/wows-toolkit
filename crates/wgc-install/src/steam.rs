//! Steam install discovery, read-only, via steamroom.

use std::path::Path;
use std::path::PathBuf;

use steamroom::depot::AppId;
use steamroom::types::key_value::KvValue;
use steamroom::types::key_value::parse_text_kv;

use crate::GameInstall;
use crate::InstallSource;

/// World of Warships on Steam.
pub const WOWS_STEAM_APP_ID: AppId = AppId(552990);

/// Library paths listed in `<steam_dir>/steamapps/libraryfolders.vdf`.
pub fn libraries_from_root(steam_dir: &Path) -> Vec<PathBuf> {
    let vdf = steam_dir.join("steamapps").join("libraryfolders.vdf");
    let Ok(text) = std::fs::read_to_string(&vdf) else {
        return Vec::new();
    };
    let Ok(kv) = parse_text_kv(&text) else {
        return Vec::new();
    };
    // parse_text_kv returns the top-level node itself (kv.key == "libraryfolders"),
    // not a wrapper containing it, so its children are the library entries.
    let KvValue::Children(ref folders) = kv.value else {
        return Vec::new();
    };
    folders.values().filter_map(|entry| entry.get("path").and_then(|p| p.as_str()).map(PathBuf::from)).collect()
}

/// Resolve an app's install dir within a library, or `None` if not installed.
pub fn install_dir_from_library(library: &Path, app_id: u32) -> Option<PathBuf> {
    let acf = library.join("steamapps").join(format!("appmanifest_{app_id}.acf"));
    let text = std::fs::read_to_string(&acf).ok()?;
    let kv = parse_text_kv(&text).ok()?;
    // kv is already the "AppState" node itself; no wrapper to unwrap.
    let installdir = kv.get("installdir").and_then(|v| v.as_str())?;
    let path = library.join("steamapps").join("common").join(installdir);
    path.is_dir().then_some(path)
}

/// Discover a Steam-installed WoWs, if present. Read-only: the resulting
/// install is launch-only and never patched.
pub fn discover_steam_installs() -> Vec<GameInstall> {
    let Some(steam_dir) = steamroom_client::steam_creds::steam_dir() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for library in libraries_from_root(&steam_dir) {
        if let Some(path) = install_dir_from_library(&library, WOWS_STEAM_APP_ID.0) {
            out.push(GameInstall {
                path,
                version: None,
                build: None,
                source: InstallSource::Steam { app_id: WOWS_STEAM_APP_ID, library },
                product_id: None,
                auto_update: false,
            });
        }
    }
    out
}

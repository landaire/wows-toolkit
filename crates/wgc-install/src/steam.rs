//! Steam install discovery, read-only, via steamroom.

use std::path::Path;
use std::path::PathBuf;

use steamroom::depot::AppId;
use steamroom::types::key_value::KvValue;
use steamroom::types::key_value::parse_text_kv;

use crate::AppManifestError;
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

/// The appmanifest Steam keeps for `app_id` in `library`.
pub fn app_manifest_path(library: &Path, app_id: AppId) -> PathBuf {
    library.join("steamapps").join(format!("appmanifest_{}.acf", app_id.0))
}

/// Steam's `StateFlags` bit set for an installed app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AppStateFlags(u32);

impl AppStateFlags {
    pub const FULLY_INSTALLED: AppStateFlags = AppStateFlags(4);
    /// Set while Steam runs the game; it changes no files.
    pub const APP_RUNNING: AppStateFlags = AppStateFlags(64);

    pub fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    pub fn bits(self) -> u32 {
        self.0
    }

    pub fn contains(self, other: AppStateFlags) -> bool {
        self.0 & other.0 == other.0
    }
}

/// What an appmanifest says about the app's install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppManifest {
    pub state_flags: AppStateFlags,
    /// The result code of Steam's last update. Absent in manifests of apps
    /// Steam has not updated.
    pub update_result: Option<u32>,
    /// The game language picked in Steam, such as `english`.
    pub language: Option<String>,
}

/// Why Steam may still change an install's files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingUpdate {
    /// The install is not fully installed, or an update or validation is
    /// due or running.
    State { flags: AppStateFlags },
    /// The last update failed, and Steam retries it.
    UpdateFailed { result: u32 },
}

impl AppManifest {
    pub fn read(library: &Path, app_id: AppId) -> Result<Self, AppManifestError> {
        let path = app_manifest_path(library, app_id);
        let text =
            std::fs::read_to_string(&path).map_err(|source| AppManifestError::Read { path: path.clone(), source })?;

        Self::parse(&text).map_err(|kind| kind.at(path))
    }

    pub fn parse(text: &str) -> Result<Self, crate::AppManifestErrorKind> {
        use crate::AppManifestErrorKind;

        let kv = parse_text_kv(text).map_err(|_| AppManifestErrorKind::Syntax)?;
        let number = |field: &'static str| -> Result<Option<u32>, AppManifestErrorKind> {
            let Some(value) = kv.get(field).and_then(|value| value.as_str()) else {
                return Ok(None);
            };
            value.parse().map(Some).map_err(|_| AppManifestErrorKind::BadNumber { field, value: value.to_owned() })
        };

        let state_flags = number("StateFlags")?.ok_or(AppManifestErrorKind::MissingStateFlags)?;
        let update_result = number("UpdateResult")?;
        let language = kv
            .get("UserConfig")
            .and_then(|config| config.get("language"))
            .and_then(|language| language.as_str())
            .filter(|language| !language.is_empty())
            .map(str::to_owned);

        Ok(Self { state_flags: AppStateFlags(state_flags), update_result, language })
    }

    /// Why Steam may change the install's files, if it may. Files must not
    /// change under a mounted overlay, so a launch over the install waits.
    pub fn pending_update(&self) -> Option<PendingUpdate> {
        let settled = AppStateFlags::FULLY_INSTALLED.0 | AppStateFlags::APP_RUNNING.0;
        if !self.state_flags.contains(AppStateFlags::FULLY_INSTALLED) || self.state_flags.0 & !settled != 0 {
            return Some(PendingUpdate::State { flags: self.state_flags });
        }

        match self.update_result {
            Some(result) if result != 0 => Some(PendingUpdate::UpdateFailed { result }),
            _ => None,
        }
    }
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

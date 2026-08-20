//! Parse WGC on-disk state (preferences.xml, apps map, data dir).

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

use crate::GameInstall;
use crate::InstallError;
use crate::InstallSource;

#[derive(Debug, Deserialize)]
struct Preferences {
    application: Application,
}

#[derive(Debug, Deserialize)]
struct Application {
    games_manager: GamesManager,
}

#[derive(Debug, Deserialize)]
struct GamesManager {
    games: Games,
}

#[derive(Debug, Deserialize, Default)]
struct Games {
    #[serde(default)]
    game: Vec<Game>,
}

#[derive(Debug, Deserialize)]
struct Game {
    working_dir: String,
    #[serde(default)]
    auto_update: bool,
}

/// Parse a WGC `preferences.xml` into one install per configured game.
pub fn parse_preferences(xml: &str) -> Result<Vec<GameInstall>, InstallError> {
    let prefs: Preferences = quick_xml::de::from_str(xml).map_err(|e| InstallError::Xml { detail: e.to_string() })?;
    Ok(prefs
        .application
        .games_manager
        .games
        .game
        .into_iter()
        .map(|g| {
            let working_dir = PathBuf::from(g.working_dir);
            GameInstall {
                path: working_dir.clone(),
                version: None,
                build: None,
                source: InstallSource::Wgc { working_dir },
                product_id: None,
                auto_update: g.auto_update,
            }
        })
        .collect())
}

/// The default WGC data directory, when one is knowable for this OS.
pub fn default_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let program_data = std::env::var_os("ProgramData")?;
        Some(PathBuf::from(program_data).join("Wargaming.net").join("GameCenter"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Discover WGC installs from the resolved data directory.
///
/// An absent default dir yields an empty list (WGC simply is not installed).
/// An explicit override that does not exist is an error, because the caller
/// asserted it should be there.
pub fn discover_wgc_installs(data_dir_override: Option<&Path>) -> Result<Vec<GameInstall>, InstallError> {
    let data_dir = match data_dir_override {
        Some(dir) => {
            if !dir.exists() {
                return Err(InstallError::WgcDataDirMissing);
            }
            dir.to_path_buf()
        }
        None => match default_data_dir() {
            Some(dir) if dir.exists() => dir,
            _ => return Ok(Vec::new()),
        },
    };

    let prefs_path = data_dir.join("preferences.xml");
    let xml = match std::fs::read_to_string(&prefs_path) {
        Ok(xml) => xml,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(InstallError::Io { detail: e.to_string() }),
    };
    parse_preferences(&xml)
}

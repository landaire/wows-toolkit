//! Parse WGC on-disk state (preferences.xml, apps map, data dir).

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

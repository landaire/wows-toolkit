//! Parse a WGC-managed install's on-disk `game_info.xml`.
//!
//! WGC writes this file into each game's install directory. It records the
//! game id, the installed part versions, and the base URL of the
//! update/content service (WGUS) that serves catalog and download requests.

use serde::Deserialize;

use crate::error::CatalogError;
use crate::models::BuildVersion;
use crate::models::PartName;

/// Parsed contents of a `game_info.xml` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameInfo {
    /// Kept as the raw wire string, not `wgc_core::ProductId`: `ProductId`'s
    /// `Realm` only accepts `WW` (see `wgc-core`'s design scope), but real
    /// installs report other realms (e.g. `WOWS.PT.PRODUCTION`), which would
    /// make parsing fail on legitimate installs.
    pub game_id: String,
    pub update_base_url: url::Url,
    pub localization: String,
    pub installed_parts: Vec<PartVersion>,
}

/// Installed vs. available version of one game part (client, locale, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartVersion {
    pub name: PartName,
    pub installed: BuildVersion,
    pub available: BuildVersion,
}

// Real on-disk game_info.xml nests everything under a `<game>` element inside
// `<protocol>`; the fields are not direct children of `<protocol>`.
#[derive(Debug, Deserialize)]
#[serde(rename = "protocol")]
struct ProtocolXml {
    game: GameXml,
}

#[derive(Debug, Deserialize)]
struct GameXml {
    id: String,
    localization: String,
    update_urls: UpdateUrlsXml,
    #[serde(default)]
    part_versions: PartVersionsXml,
}

#[derive(Debug, Deserialize)]
struct UpdateUrlsXml {
    value: String,
}

#[derive(Debug, Deserialize, Default)]
struct PartVersionsXml {
    #[serde(default, rename = "version")]
    version: Vec<PartVersionXml>,
}

#[derive(Debug, Deserialize)]
struct PartVersionXml {
    #[serde(rename = "@name")]
    name: String,
    #[serde(rename = "@installed")]
    installed: String,
    #[serde(rename = "@available")]
    available: String,
}

impl GameInfo {
    /// A game with nothing installed yet: `installed_parts` is empty, so
    /// `check_updates` will report every part `NotInstalled` and populate
    /// `downloads` for a full install. `check_updates` always asks WGUS for
    /// the full newest chain (`CurrentPartVersions::full`, every part's
    /// current version sent as `"0"`) regardless of what is installed, so no
    /// extra signaling is needed here beyond an empty `installed_parts`.
    pub fn for_fresh_install(game_id: String, update_base_url: url::Url, localization: String) -> GameInfo {
        GameInfo { game_id, update_base_url, localization, installed_parts: Vec::new() }
    }
}

/// Parse a `game_info.xml` document into a [`GameInfo`].
pub fn parse_game_info(xml: &str) -> Result<GameInfo, CatalogError> {
    let parsed: ProtocolXml =
        quick_xml::de::from_str(xml).map_err(|e| CatalogError::GameInfo { detail: e.to_string() })?;

    let game = parsed.game;
    let update_base_url = url::Url::parse(&game.update_urls.value)
        .map_err(|e| CatalogError::GameInfoUrl { value: game.update_urls.value.clone(), detail: e.to_string() })?;

    Ok(GameInfo {
        game_id: game.id,
        update_base_url,
        localization: game.localization,
        installed_parts: game
            .part_versions
            .version
            .into_iter()
            .map(|v| PartVersion {
                name: PartName::parse(&v.name),
                installed: BuildVersion::new(v.installed),
                available: BuildVersion::new(v.available),
            })
            .collect(),
    })
}

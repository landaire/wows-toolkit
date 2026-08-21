//! Built-in registry of WoWS game descriptors: the `game_id` + WGUS
//! `update_base_url` pairs needed to build a fresh [`GameInfo`] without an
//! on-disk `game_info.xml` (i.e. before the game is installed).
//!
//! These two entries are reverse-engineered from the user's own WGC client
//! traffic (EU account, real `patches_chain` request logs and a PT install),
//! not from any public source. Both are the EU realm's WGUS hosts; WGUS
//! hosts for other realms (NA/Asia/RU) are not yet known here. A future
//! wgcps product-discovery step (querying WG's product-list service) is
//! expected to supply those instead of hand-added entries.

use crate::error::CatalogError;
use crate::game_info::GameInfo;

/// Which WG account realm a [`KnownGame`]'s WGUS host was observed on. A
/// local, minimal stand-in for `wgc-auth::AccountRealm`: this crate must not
/// depend on `wgc-auth` (catalog access is unauthenticated and should stay
/// that way), so the two crates keep separate realm enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountRealmHint {
    Eu,
    Na,
    Asia,
    Ru,
}

/// One known, installable WG game descriptor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownGame {
    pub game_id: &'static str,
    pub display_name: &'static str,
    pub update_base_url: &'static str,
    pub client_type: &'static str,
    pub localization: &'static str,
    pub realm: AccountRealmHint,
}

impl KnownGame {
    /// Build a fresh [`GameInfo`] (empty `installed_parts`) for this game,
    /// ready to pass to `CatalogClient::check_updates`.
    pub fn to_fresh_game_info(&self) -> Result<GameInfo, CatalogError> {
        let update_base_url = url::Url::parse(self.update_base_url)
            .map_err(|source| CatalogError::GameInfoUrl { value: self.update_base_url.to_string(), source })?;
        Ok(GameInfo::for_fresh_install(self.game_id.to_string(), update_base_url, self.localization.to_string()))
    }
}

const KNOWN_GAMES: &[KnownGame] = &[
    KnownGame {
        game_id: "WOWS.WW.PRODUCTION",
        display_name: "World of Warships",
        update_base_url: "https://wgus-eu.wargaming.net",
        client_type: "high",
        localization: "EN",
        realm: AccountRealmHint::Eu,
    },
    KnownGame {
        game_id: "WOWS.PT.PRODUCTION",
        display_name: "World of Warships (Public Test)",
        update_base_url: "https://wgus-wowspt.worldofwarships.eu",
        client_type: "high",
        localization: "EN",
        realm: AccountRealmHint::Eu,
    },
];

/// The built-in registry of known game descriptors.
pub fn known_games() -> &'static [KnownGame] {
    KNOWN_GAMES
}

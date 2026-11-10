//! Names a search result's ship from the game data of the build the match was
//! recorded on, in the current language.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::PoisonError;

use wows_replays::types::GameParamId;
use wows_toolkit_config::index::rows::MatchHit;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_viewmodel::search;

use crate::replay_inspector::constants::BuildNumber;

/// A ship as one build's game data knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ShipKey {
    pub build: BuildNumber,
    pub ship: GameParamId,
}

impl ShipKey {
    /// The recording player's ship and the build that names it, when the
    /// index stored both.
    pub fn of(hit: &MatchHit) -> Option<Self> {
        Some(Self { build: BuildNumber(hit.version_build?), ship: hit.self_ship_id? })
    }
}

/// How far a lookup may go for a build's game data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildReach {
    /// Only a build already loaded: a page of results can span years of
    /// builds, and loading each to name a column costs more than the names.
    Loaded,
    /// Loads the build when it is not loaded.
    Load,
}

/// Names ships in place of the game data, for tests with no game install.
pub type ShipNamer = fn(ShipKey, BuildReach) -> Option<String>;

static NAMER: Mutex<Option<ShipNamer>> = Mutex::new(None);

/// The names the builds already loaded give each of `keys`. Blocking.
pub fn loaded_names(keys: impl IntoIterator<Item = ShipKey>) -> HashMap<ShipKey, String> {
    let source = NameSource::current();
    let mut names = HashMap::new();

    for key in keys {
        if names.contains_key(&key) {
            continue;
        }
        if let Some(name) = source.name(key, BuildReach::Loaded) {
            names.insert(key, name);
        }
    }

    names
}

/// The name `key`'s build gives its ship, loading the build when it is not
/// loaded. Blocking: a build load reads the whole build.
pub fn load_name(key: ShipKey) -> Option<String> {
    NameSource::current().name(key, BuildReach::Load)
}

/// Makes the lookups use `namer` instead of the game data; `None` restores
/// the game data.
#[cfg(feature = "test-support")]
pub fn set_namer(namer: Option<ShipNamer>) {
    *NAMER.lock().unwrap_or_else(PoisonError::into_inner) = namer;
}

/// Where the names come from for one lookup.
enum NameSource {
    Test(ShipNamer),
    GameData(Option<GameDataCache>),
}

impl NameSource {
    fn current() -> Self {
        let namer = *NAMER.lock().unwrap_or_else(PoisonError::into_inner);
        match namer {
            Some(namer) => Self::Test(namer),
            None => Self::GameData(crate::game_data::cache()),
        }
    }

    fn name(&self, key: ShipKey, reach: BuildReach) -> Option<String> {
        let cache = match self {
            Self::Test(namer) => return namer(key, reach),
            Self::GameData(cache) => cache.as_ref()?,
        };

        let loaded = match reach {
            BuildReach::Loaded => cache.loaded_build(key.build.0)?,
            BuildReach::Load => match cache.get_or_load_build_for(key.build.0, None) {
                Ok(loaded) => loaded,
                Err(error) => {
                    // A build that is not installed is the ordinary case; the
                    // row keeps the name the index stored.
                    log::debug!("search: build {} could not name ship {}: {error}", key.build, key.ship);
                    return None;
                }
            },
        };

        search::try_resolve_ship_name(key.ship, Some(loaded.provider()))
    }
}

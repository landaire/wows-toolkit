//! Game art and names for the Stats tab, read from the loaded build: each
//! achievement's icon and name, and each ship's nation flag and class icon,
//! as the GPUI tab shows them.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;

use base64::Engine as _;
use wows_replays::types::GameParamId;
use wows_toolkit_replay_data::load::LoadedGameData;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_assets::ShipIconState;
use wowsunpack::game_params::translations::translate_achievement_description;
use wowsunpack::game_params::translations::translate_achievement_name;
use wowsunpack::game_params::types::GameParamProvider;

use crate::replay_inspector::constants::BuildNumber;

/// An asset read from one build.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ArtKey {
    build: BuildNumber,
    asset: String,
}

/// `data:` URLs already read, by build and asset. `None` records an asset
/// the build does not have, so it is not looked for again.
static URLS: LazyLock<Mutex<HashMap<ArtKey, Option<Arc<str>>>>> = LazyLock::new(Mutex::default);

/// The loaded build's art and names.
pub(crate) struct GameArt {
    loaded: Arc<LoadedGameData>,
}

impl GameArt {
    /// The newest loaded build's art, `None` before any build has loaded.
    pub(crate) fn current() -> Option<Self> {
        crate::game_data::newest_loaded().map(|loaded| Self { loaded })
    }

    /// An achievement's icon as a PNG `data:` URL.
    pub(crate) fn achievement_icon(&self, icon_key: &str) -> Option<Arc<str>> {
        self.url(format!("achievement:{icon_key}"), "image/png", GuiAsset::Achievement(icon_key))
    }

    /// An achievement's name in the build's language.
    pub(crate) fn achievement_name(&self, icon_key: &str) -> Option<String> {
        translate_achievement_name(icon_key, self.loaded.provider().as_ref() as &dyn ResourceLoader)
    }

    pub(crate) fn achievement_description(&self, icon_key: &str) -> Option<String> {
        translate_achievement_description(icon_key, self.loaded.provider().as_ref() as &dyn ResourceLoader)
    }

    /// The ship's nation flag as a PNG `data:` URL.
    pub(crate) fn nation_flag(&self, ship: GameParamId) -> Option<Arc<str>> {
        let param = GameParamProvider::game_param_by_id(self.loaded.provider().as_ref(), ship)?;
        let nation = param.nation().to_owned();
        self.url(format!("nation:{nation}"), "image/png", GuiAsset::NationFlag(&nation))
    }

    /// The ship's class icon as an SVG `data:` URL. The view tints it and
    /// turns it upright, as the GPUI table does.
    pub(crate) fn class_icon(&self, ship: GameParamId) -> Option<Arc<str>> {
        let param = GameParamProvider::game_param_by_id(self.loaded.provider().as_ref(), ship)?;
        let species = *param.species()?.known()?;
        self.url(format!("class:{species:?}"), "image/svg+xml", GuiAsset::ShipClassIcon { species, state: ShipIconState::Alive })
    }

    fn url(&self, asset_key: String, mime: &str, asset: GuiAsset<'_>) -> Option<Arc<str>> {
        let cache_key = ArtKey { build: BuildNumber(self.loaded.build()), asset: asset_key };
        if let Some(known) = URLS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&cache_key) {
            return known.clone();
        }

        let url = asset
            .read(self.loaded.vfs(), None)
            .map(|bytes| Arc::<str>::from(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))));
        URLS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(cache_key, url.clone());
        url
    }
}

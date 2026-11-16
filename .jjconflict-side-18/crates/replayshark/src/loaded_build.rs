use std::sync::Arc;

use wows_replays::context::GameDataContext;
use wows_replays::context::GameDataContextError;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::rpc::entitydefs::EntitySpec;

/// One build's game data, loaded in full before any replay is read.
///
/// It answers for every version asked. The caller chose this build for the
/// replay, which may be another region's build of the same game version or
/// the nearest build when the replay's own is not installed.
pub struct LoadedBuild {
    provider: Arc<GameMetadataProvider>,
    constants: Arc<GameConstants>,
}

impl LoadedBuild {
    pub fn new(provider: GameMetadataProvider, constants: GameConstants) -> Self {
        Self { provider: Arc::new(provider), constants: Arc::new(constants) }
    }

    pub fn provider(&self) -> &GameMetadataProvider {
        &self.provider
    }

    pub fn constants(&self) -> &GameConstants {
        &self.constants
    }
}

impl GameDataContext for LoadedBuild {
    fn entity_specs(&self, _version: &Version) -> Result<Arc<Vec<EntitySpec>>, GameDataContextError> {
        Ok(self.provider.entity_specs_arc())
    }

    fn game_constants(&self, _version: &Version) -> Result<Arc<GameConstants>, GameDataContextError> {
        Ok(Arc::clone(&self.constants))
    }

    fn metadata_provider(&self, _version: &Version) -> Result<Arc<GameMetadataProvider>, GameDataContextError> {
        Ok(Arc::clone(&self.provider))
    }
}

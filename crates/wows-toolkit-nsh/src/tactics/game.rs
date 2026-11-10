use wows_replays::game_constants::GameConstants;
use wows_toolkit_replay_data::load::LoadedGameData;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::vfs::VfsPath;

/// The loaded build a board draws with: map art and measurements from its VFS,
/// names and ship ranges from its params, and the constants a replay walk
/// reads layouts with.
pub trait BoardGame: Send + Sync {
    fn vfs(&self) -> &VfsPath;
    fn metadata(&self) -> &GameMetadataProvider;
    fn constants(&self) -> &GameConstants;
    /// The version ship ranges are read at. `None` draws no range circles,
    /// since they are gated on it.
    fn version(&self) -> Option<Version>;
}

impl BoardGame for LoadedGameData {
    fn vfs(&self) -> &VfsPath {
        LoadedGameData::vfs(self)
    }

    fn metadata(&self) -> &GameMetadataProvider {
        self.provider()
    }

    fn constants(&self) -> &GameConstants {
        self.base_constants()
    }

    fn version(&self) -> Option<Version> {
        LoadedGameData::version(self)
    }
}

use wows_toolkit_viewmodel::tactics::PresetError;

/// Why a tactics board command could not be carried out.
#[derive(Debug, thiserror::Error)]
pub enum TacticsError {
    #[error("Give the board a name first")]
    PresetNeedsName,
    #[error("Set the board on a map first")]
    PresetNeedsMap,
    #[error("The board could not be saved: {source}")]
    PresetSave {
        name: String,
        #[source]
        source: PresetError,
    },
    #[error("The board could not be opened: {source}")]
    PresetLoad {
        name: String,
        #[source]
        source: PresetError,
    },
    #[error("The board could not be dropped: {source}")]
    PresetDelete {
        name: String,
        #[source]
        source: PresetError,
    },
    #[error("World of Warships directory is not set")]
    NoReplaysDir,
    #[error("Load a game build first")]
    NoGameData,
    #[error("there is no map {value:?} to set the board on")]
    UnknownMap { value: String },
    #[error("the map has no mode {value:?}")]
    UnknownMode { value: String },
    #[error("there is no ship match {index}")]
    UnknownShip { index: u32 },
    #[error("there is no ink {index}")]
    UnknownInk { index: u32 },
}

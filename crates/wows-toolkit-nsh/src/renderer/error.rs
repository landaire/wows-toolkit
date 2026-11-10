use rust_i18n::t;
use wows_replays::types::EntityId;
use wows_toolkit_replay_data::minimap::PreviewError;
use wows_toolkit_replay_data::playback::ExportError;

use crate::renderer::ids::RendererId;

/// Why a renderer command could not be carried out.
#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    #[error("there is no open renderer {id}")]
    UnknownRenderer { id: RendererId },
    #[error("there is no layer {id:?}")]
    UnknownLayer { id: String },
    #[error("there is no codec {value:?}")]
    UnknownCodec { value: String },
    #[error("there is no ink {index}")]
    UnknownInk { index: u32 },
    #[error("there is no speed {index}")]
    UnknownSpeed { index: u32 },
    #[error("there is no ship match {index}")]
    UnknownShip { index: u32 },
    #[error("there is no ship species {short:?}")]
    UnknownSpecies { short: String },
    #[error("the build does not know the ship {param_index:?}")]
    UnknownShipParam { param_index: String },
    #[error("there is no event kind {index}")]
    UnknownKind { index: u32 },
    #[error("the file dialog could not be shown: {reason}")]
    Dialog { reason: String },
    #[error("the replay is still being read")]
    NotReady,
    #[error("no temporary file could be made for the clipboard")]
    NoTemporaryFile,
    #[error("a batch render is already running")]
    BatchRunning,
    #[error("the session has no window {replay_id}")]
    UnknownWindow { replay_id: String },
    #[error("there is no replay to render")]
    NothingToRender,
    #[error("there is no ship with entity {entity_id:?} in this battle")]
    UnknownEntity { entity_id: String },
    #[error("ship {entity_id} took no recorded hits, so there is no armor to show")]
    NoArmorToShow { entity_id: EntityId },
    #[error("the settings database could not be read, so there is no game data to read the replay with")]
    NoGameData,
}

/// Why a replay's battle could not be read for drawing.
#[derive(Debug, thiserror::Error)]
pub enum ReplayLoadError {
    #[error(transparent)]
    Preview(#[from] PreviewError),
    #[error("{}", RendererError::NoGameData)]
    NoGameData,
}

/// Why an export did not reach where it was going.
#[derive(Debug, thiserror::Error)]
pub enum ExportFailure {
    /// Said in the reader's language: the reader picked the path.
    #[error("{}", t!("ui.replay.renderer.export_path_unusable"))]
    PathUnusable,
    #[error(transparent)]
    Encode(ExportError),
    #[error(transparent)]
    Clipboard(#[from] arboard::Error),
}

impl From<ExportError> for ExportFailure {
    fn from(err: ExportError) -> Self {
        match err {
            ExportError::PathUnusable { .. } => Self::PathUnusable,
            err => Self::Encode(err),
        }
    }
}

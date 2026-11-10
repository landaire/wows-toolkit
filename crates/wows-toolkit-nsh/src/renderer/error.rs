/// Why a renderer command could not be carried out.
#[derive(Debug, thiserror::Error)]
pub enum RendererError {
    #[error("there is no open renderer {id}")]
    UnknownRenderer { id: u32 },
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
    #[error("no World of Warships directory is set, so there is no game data to read the replay with")]
    NoGameData,
}

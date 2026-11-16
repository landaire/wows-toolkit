use wows_minimap_renderer::error::TrackFileError;

/// Why the clip could not be shown.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// `reason` is the browser's own description; nothing reads it but a person.
    #[error("{url} could not be fetched: {reason}")]
    Fetch { url: String, reason: String },
    #[error("{url} answered HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("the track file is unreadable")]
    TrackFile(#[from] TrackFileError),
    #[error("the track file holds no frames")]
    NoFrames,
    #[error("the map art is not a readable PNG")]
    MapPng(#[from] png::DecodingError),
    #[error("the map art is too large to decode")]
    MapTooLarge,
    #[error("the map art is {color_type:?}, not RGB or RGBA")]
    MapColorType { color_type: png::ColorType },
}

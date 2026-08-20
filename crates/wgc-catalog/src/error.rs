use thiserror::Error;

/// Errors from parsing WGC/WGUS XML documents.
#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("failed to parse game_info.xml: {detail}")]
    GameInfo { detail: String },
    #[error("game_info.xml update_urls value {value:?} is not a valid URL: {detail}")]
    GameInfoUrl { value: String, detail: String },
    #[error("failed to parse metadata response: {detail}")]
    Metadata { detail: String },
    #[error("failed to parse patches_chain response: {detail}")]
    PatchesChain { detail: String },
}

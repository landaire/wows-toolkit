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

    #[error("failed to build HTTP client: {source}")]
    ClientBuild {
        #[source]
        source: reqwest::Error,
    },
    #[error("could not build request URL from base {base:?}: {detail}")]
    BadUrl { base: String, detail: String },
    #[error("request to {url} failed: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{url} returned HTTP {status}: {body}")]
    Http { url: String, status: u16, body: String },
    #[error("{url} returned an empty response body")]
    EmptyResponse { url: String },
    #[error("response body from {url} is not valid UTF-8: {source}")]
    Utf8 {
        url: String,
        #[source]
        source: std::string::FromUtf8Error,
    },
    #[error("patches_chain response for game {game_id:?} has no {chain_type} chain")]
    NoChain { game_id: String, chain_type: &'static str },
}

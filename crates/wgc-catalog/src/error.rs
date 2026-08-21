use thiserror::Error;

/// Errors from parsing WGC/WGUS XML documents.
#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("failed to parse game_info.xml: {source}")]
    GameInfo {
        #[source]
        source: quick_xml::DeError,
    },
    #[error("game_info.xml update_urls value {value:?} is not a valid URL: {source}")]
    GameInfoUrl {
        value: String,
        #[source]
        source: url::ParseError,
    },
    #[error("failed to parse metadata response: {source}")]
    Metadata {
        #[source]
        source: quick_xml::DeError,
    },
    #[error("failed to parse patches_chain response: {source}")]
    PatchesChain {
        #[source]
        source: quick_xml::DeError,
    },

    #[error("failed to build HTTP client: {source}")]
    ClientBuild {
        #[source]
        source: reqwest::Error,
    },
    #[error("could not build request URL from base {base:?}: {source}")]
    BadUrl {
        base: String,
        #[source]
        source: url::ParseError,
    },
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

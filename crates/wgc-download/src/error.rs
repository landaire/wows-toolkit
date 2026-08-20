use thiserror::Error;

/// Errors from parsing a WG `.torrent` (bencode) manifest.
#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("failed to parse torrent bencode: {detail}")]
    Bencode { detail: String },
    #[error("failed to parse torrent comment JSON: {detail}")]
    Json { detail: String },
    #[error("malformed torrent: {detail}")]
    BadTorrent { detail: String },
}

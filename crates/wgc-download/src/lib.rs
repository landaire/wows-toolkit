//! Parses WG `.torrent` (bencode) manifests for WGC-managed game builds.
//!
//! `torrent` turns a `.torrent` byte slice into a typed [`TorrentMeta`].
//! Webseed-HTTP download and sha1 verification against the parsed manifest
//! are a later milestone (P2-2); this crate has no network code yet.

pub mod error;
pub mod torrent;

pub use error::DownloadError;
pub use torrent::TorrentFile;
pub use torrent::TorrentMeta;
pub use torrent::parse_torrent;

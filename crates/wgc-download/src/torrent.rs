//! Parse a WG `.torrent` (bencode) manifest into a typed [`TorrentMeta`].
//!
//! WG's torrents are standard BitTorrent v1 multi-file dicts with one
//! non-standard extension: each `info.files[]` entry carries a raw 20-byte
//! `sha1` alongside the usual `path`/`length` (verified against a real
//! `wows_<ver>_na` build torrent; see tests). BitTorrent piece-alignment
//! padding entries (`attr` containing `p`, BEP47) are dropped: they carry no
//! `sha1`, are not real WG content, and do not exist on the download CDN.
//!
//! The `comment` field is not free text here: WG stuffs a JSON object
//! `{"unpacked_patches_size": {...}, "diffs_result_size": {...}}` into it,
//! giving the unpacked size of each `.dspkg` file (`diffs_result_size` is
//! not modeled; it is delta-patch bookkeeping, out of scope here). A
//! missing `comment` is a legitimate, documented case, not an error: it
//! yields an empty `unpacked_sizes` map rather than failing the parse.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::DownloadError;

/// BEP47 padding-file attribute flag. Padding entries align real files on
/// piece boundaries; they are not downloadable WG content.
const PADDING_ATTR: char = 'p';

/// Parsed contents of a WG `.torrent` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentMeta {
    pub name: String,
    pub piece_length: u64,
    /// Sum of `files[].length`, excluding padding entries.
    pub total_length: u64,
    pub files: Vec<TorrentFile>,
    /// From the `comment` JSON's `unpacked_patches_size` map: `.dspkg` file
    /// name -> unpacked byte size. Empty if `comment` is absent.
    pub unpacked_sizes: BTreeMap<String, u64>,
}

/// One real (non-padding) file listed in `info.files[]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentFile {
    pub path: Vec<String>,
    pub length: u64,
    /// WG's non-standard per-file extension, hex-encoded. `None` if the
    /// entry carries no `sha1` (should not occur for real, non-padding WG
    /// files; padding entries are already filtered out before this type is
    /// built).
    pub sha1: Option<String>,
}

impl TorrentFile {
    /// `path` segments joined by `/`, e.g. `"build/client.dspkg"`.
    pub fn relative_path(&self) -> String {
        self.path.join("/")
    }
}

#[derive(Debug, Deserialize)]
struct TorrentWire {
    #[serde(default)]
    comment: Option<String>,
    info: InfoWire,
}

#[derive(Debug, Deserialize)]
struct InfoWire {
    name: String,
    #[serde(rename = "piece length")]
    piece_length: u64,
    #[serde(default)]
    files: Vec<FileWire>,
}

#[derive(Debug, Deserialize)]
struct FileWire {
    length: u64,
    path: Vec<String>,
    #[serde(default)]
    sha1: Option<serde_bytes::ByteBuf>,
    #[serde(default)]
    attr: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CommentJson {
    #[serde(default)]
    unpacked_patches_size: BTreeMap<String, u64>,
}

/// Parse a `.torrent` file's raw bytes into a [`TorrentMeta`].
pub fn parse_torrent(bytes: &[u8]) -> Result<TorrentMeta, DownloadError> {
    let wire: TorrentWire = serde_bencode::from_bytes(bytes)?;

    let unpacked_sizes = match wire.comment {
        // Genuinely absent, not malformed: not every torrent carries this
        // WG extension, and callers can proceed with no size hints.
        None => BTreeMap::new(),
        Some(comment) => {
            let parsed: CommentJson = serde_json::from_str(&comment)?;
            parsed.unpacked_patches_size
        }
    };

    let files: Vec<TorrentFile> = wire
        .info
        .files
        .into_iter()
        .filter(|f| !f.attr.as_deref().is_some_and(|attr| attr.contains(PADDING_ATTR)))
        .map(|f| TorrentFile {
            path: f.path,
            length: f.length,
            sha1: f.sha1.map(|raw| data_encoding::HEXLOWER.encode(&raw)),
        })
        .collect();

    if files.is_empty() {
        return Err(DownloadError::BadTorrent {
            detail: "torrent has no downloadable files (files list is empty, or only padding entries)".to_string(),
        });
    }

    let total_length = files.iter().map(|f| f.length).sum();

    Ok(TorrentMeta { name: wire.info.name, piece_length: wire.info.piece_length, total_length, files, unpacked_sizes })
}

use std::path::PathBuf;

use thiserror::Error;

/// Errors from parsing a WG `.torrent` (bencode) manifest, and from the
/// webseed-HTTP downloader that fetches the files it describes.
#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("failed to parse torrent bencode: {0}")]
    Bencode(#[from] serde_bencode::Error),
    #[error("failed to parse torrent comment JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("malformed torrent: {detail}")]
    BadTorrent { detail: String },
    /// Never issued for a Steam-managed install: Steam owns that directory
    /// and patches it itself, so we refuse before making any request.
    #[error("refusing to download into a Steam-managed install")]
    SteamInstallRefused,
    /// The webseed responded, but not with a success status.
    #[error("HTTP {status} fetching {url}")]
    Http { url: String, status: reqwest::StatusCode },
    /// The request itself failed (DNS, connect, TLS, mid-stream transport
    /// error), so there is no HTTP status to report.
    #[error("request to {url} failed: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{operation} {path}: {source}")]
    Io {
        /// A short verb phrase describing what was attempted, e.g. "creating
        /// directory" or "opening". Not a stringified error - a fixed label
        /// that keeps the message informative alongside `source`.
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// A downloaded (or previously-staged) file's sha1 does not match the
    /// torrent manifest. Never treated as a soft warning: the caller must
    /// see this and can retry, but the mismatched bytes are never accepted
    /// as if they were correct.
    #[error("hash mismatch for {file}: expected {expected}, got {got}")]
    HashMismatch { file: String, expected: String, got: String },
    /// The torrent's file entry carries no sha1 (should not occur for real,
    /// non-padding WG files; see `torrent::parse_torrent`), so there is
    /// nothing to verify the download against. Refused rather than
    /// downloading unverified bytes.
    #[error("torrent entry {file} has no sha1 checksum to verify against")]
    MissingSha1 { file: String },
    #[error("short read downloading {file}: expected {expected} bytes, got {got} bytes")]
    ShortRead { file: String, expected: u64, got: u64 },
    /// A torrent file entry's path contains a segment that could escape
    /// `staging_dir` (empty, `.`, `..`, an absolute-path indicator, a
    /// Windows drive/prefix, or any multi-component segment). Refused
    /// before any filesystem write is attempted.
    #[error("unsafe path in torrent manifest: {path}")]
    UnsafePath { path: String },
    /// A `.dspkg`'s file name (the last path component) is not valid UTF-8,
    /// so it cannot be checked against the delta-package naming convention
    /// or reported in progress/error messages.
    #[error("dspkg path has no valid UTF-8 file name: {path}")]
    InvalidDspkgFileName { path: PathBuf },
    /// A `.dspkg` failed to 7z-extract: corrupt archive, unsupported codec,
    /// or an I/O failure while writing an entry. Never silently ignored -
    /// the caller must see this so the run is not reported as successful
    /// while a package sits unextracted.
    #[error("failed to unpack {file}: {source}")]
    Unpack {
        file: String,
        #[source]
        source: sevenz_rust2::Error,
    },
    /// The blocking task running [`crate::unpack::unpack_dspkg`] panicked
    /// (as opposed to `Unpack`, where the archive extraction itself failed).
    #[error("unpack task for {file} panicked: {source}")]
    UnpackTaskPanicked {
        file: String,
        #[source]
        source: tokio::task::JoinError,
    },
    /// `file`'s name has the delta-package shape (two WG version tokens
    /// before the part segment). Delta packages are rsync-based diffs, not
    /// directly 7z-extractable to real files; we only install FULL
    /// packages for now.
    #[error("delta package unpacking is not supported yet: {file}")]
    DeltaUnsupported { file: String },
    /// The caller's `DownloadControl::cancel` flag was set while a file was
    /// in flight. Checked at the start of `download_one` and inside its
    /// chunk loop, so a multi-GB torrent aborts promptly rather than only
    /// between files.
    #[error("download cancelled")]
    Cancelled,
    /// The caller's `DownloadControl::pause` flag was set while a file was
    /// in flight. Unlike `Cancelled`, any file already sha1-verified stays
    /// staged on disk, so a resumed run skips it.
    #[error("download paused")]
    Paused,
}

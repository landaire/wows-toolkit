use thiserror::Error;

/// Errors from parsing a WG `.torrent` (bencode) manifest, and from the
/// webseed-HTTP downloader that fetches the files it describes.
#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("failed to parse torrent bencode: {detail}")]
    Bencode { detail: String },
    #[error("failed to parse torrent comment JSON: {detail}")]
    Json { detail: String },
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
    #[error("request to {url} failed: {detail}")]
    Request { url: String, detail: String },
    #[error("I/O error: {detail}")]
    Io { detail: String },
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
    /// A `.dspkg` failed to 7z-extract: corrupt archive, unsupported codec,
    /// or an I/O failure while writing an entry. Never silently ignored -
    /// the caller must see this so the run is not reported as successful
    /// while a package sits unextracted.
    #[error("failed to unpack {file}: {detail}")]
    Unpack { file: String, detail: String },
    /// `file`'s name has the delta-package shape (two WG version tokens
    /// before the part segment). Delta packages are rsync-based diffs, not
    /// directly 7z-extractable to real files; we only install FULL
    /// packages for now.
    #[error("delta package unpacking is not supported yet: {file}")]
    DeltaUnsupported { file: String },
}

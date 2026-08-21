//! Orchestrates a full build install: webseed-download every real file of
//! a torrent into staging (verified, see [`crate::webseed::download`]),
//! then 7z-extract each FULL `.dspkg` into the install directory. DELTA
//! packages are not extracted - they are recorded as skipped, not treated
//! as a run failure, since a build's non-delta parts can still complete.

use std::path::PathBuf;

use crate::error::DownloadError;
use crate::torrent::TorrentFile;
use crate::torrent::TorrentMeta;
use crate::unpack::is_delta_package;
use crate::unpack::unpack_dspkg;
use crate::webseed::DownloadProgress;
use crate::webseed::DownloadRequest;
use crate::webseed::{
    self,
};

/// A request to download and install one torrent's build.
pub struct InstallRequest {
    /// Where downloaded `.dspkg` files are verified and held before
    /// extraction. Distinct from `install_dir` so a failed or partial
    /// unpack never leaves a half-extracted archive's leftovers mixed
    /// into the staged download set.
    pub staging_dir: PathBuf,
    /// Where FULL packages are 7z-extracted to become the playable game.
    pub install_dir: PathBuf,
    pub torrent: TorrentMeta,
    /// Webseed base URL from the WGUS `web_seeds` element. Ends with `/`.
    pub web_seed_base: String,
    /// Steam owns a Steam install's directory and patches it itself; set
    /// this from the install's `InstallSource` so `download_and_install`
    /// can refuse before making any request.
    pub is_steam_install: bool,
}

/// Progress across both phases `download_and_install` drives: streaming a
/// `.dspkg` down from the webseed, then 7z-extracting a FULL one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallProgress {
    Downloading(DownloadProgress),
    Unpacking { file: String },
    Unpacked { file: String, extracted_files: usize },
}

/// Result of a `download_and_install` run: every FULL package's real files
/// were downloaded, verified, and extracted; every DELTA package present
/// in the torrent was downloaded and verified but left un-extracted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallOutcome {
    /// Every file path written by extracting a FULL `.dspkg`.
    pub extracted_files: Vec<PathBuf>,
    /// `.dspkg` file names skipped because they are DELTA packages
    /// (downloaded and verified, but not extracted - see
    /// [`DownloadError::DeltaUnsupported`]).
    pub skipped_deltas: Vec<String>,
}

/// Download every real file of `req.torrent` into `req.staging_dir`
/// (verified against its manifest sha1, same as
/// [`crate::webseed::download`]), then 7z-extract each FULL `.dspkg` into
/// `req.install_dir`. A DELTA `.dspkg` is downloaded and verified like any
/// other file but is recorded in the outcome's `skipped_deltas` rather
/// than extracted or treated as a run failure - deltas are rsync-based
/// diffs, not directly 7z-extractable, and are not yet supported.
///
/// Unpacking runs on a blocking thread pool via [`tokio::task::spawn_blocking`]
/// since `sevenz-rust2` extraction is synchronous I/O.
pub async fn download_and_install(
    client: &reqwest::Client,
    req: &InstallRequest,
    progress: impl Fn(InstallProgress),
) -> Result<InstallOutcome, DownloadError> {
    if req.is_steam_install {
        return Err(DownloadError::SteamInstallRefused);
    }

    let download_req = DownloadRequest {
        staging_dir: req.staging_dir.clone(),
        torrent: req.torrent.clone(),
        web_seed_base: req.web_seed_base.clone(),
        is_steam_install: false,
    };
    webseed::download(client, &download_req, |p| progress(InstallProgress::Downloading(p))).await?;

    let mut outcome = InstallOutcome::default();
    let real_files: Vec<&TorrentFile> =
        req.torrent.files.iter().filter(|f| !f.relative_path().starts_with(webseed::PADDING_FILE_PREFIX)).collect();

    for file in real_files {
        let relative_path = file.relative_path();
        // Every real WG torrent entry observed so far is a `.dspkg`; a
        // non-`.dspkg` entry has nothing to unpack and is left staged as
        // downloaded, matching download()'s existing contract.
        if !relative_path.ends_with(".dspkg") {
            continue;
        }

        if is_delta_package(&relative_path) {
            outcome.skipped_deltas.push(relative_path);
            continue;
        }

        progress(InstallProgress::Unpacking { file: relative_path.clone() });

        let staged_path = webseed::safe_join(&req.staging_dir, &file.path)?;
        let install_dir = req.install_dir.clone();
        let extracted =
            tokio::task::spawn_blocking(move || unpack_dspkg(&staged_path, &install_dir)).await.map_err(|e| {
                DownloadError::Unpack { file: relative_path.clone(), detail: format!("unpack task panicked: {e}") }
            })??;

        progress(InstallProgress::Unpacked { file: relative_path.clone(), extracted_files: extracted.len() });
        outcome.extracted_files.extend(extracted);
    }

    Ok(outcome)
}

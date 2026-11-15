use std::path::PathBuf;

use thiserror::Error;

use crate::accounts::AccountStatus;
use crate::game::GameBuild;
use crate::settings::AccountKey;

/// Why a download or install could not start.
#[derive(Debug, Error)]
pub enum LauncherError {
    #[error("no update check result for this install yet")]
    NoCatalogResult,
    #[error("no version check result for this game yet")]
    NoVersionCheck,
    #[error("nothing to download: the catalog reports no parts to fetch")]
    NothingToDownload,
    #[error("catalog response has no web seed base to download from")]
    NoWebSeed,
    #[error("catalog response has no torrent URLs to download from")]
    NoTorrentUrls,
    #[error("cannot build download client: {0}")]
    HttpClient(#[from] reqwest::Error),
    #[error("nothing to resume for this download")]
    NothingToResume,
}

/// Why Play could not start the game.
#[derive(Debug, Error)]
pub enum PlayError {
    #[error("the game is already running")]
    GameRunning,
    #[error("this account is not saved")]
    UnknownAccount { key: AccountKey },
    #[error("sign in to this account first")]
    NotSignedIn { key: AccountKey, status: AccountStatus },
    #[error("no game install is set for this account")]
    NoInstall { key: AccountKey },
    #[error("{path} is being updated")]
    InstallBusy { path: PathBuf },
    #[error("the game settings are being saved; try again in a moment")]
    SavingGameSettings,
    #[error(transparent)]
    Launch(#[from] wgc_launch::LaunchError),
}

/// Why a replay could not be opened in the game.
#[derive(Debug, Error)]
pub enum ReplayLaunchError {
    #[error("the game is already running; close it to watch a replay")]
    GameRunning,
    #[error("no install has game build {build}")]
    NoInstallForBuild { build: GameBuild },
    #[error("no game install is set")]
    NoInstall,
    #[error("the game settings are being saved; try again in a moment")]
    SavingGameSettings,
    #[error(transparent)]
    Launch(#[from] wgc_launch::LaunchError),
}

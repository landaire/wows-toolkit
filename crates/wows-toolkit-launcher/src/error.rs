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
    #[error("checking whether the game is still running; try again in a moment")]
    CheckingForGame,
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
    #[error("the app has no data directory to keep the game runtime in")]
    NoDataDir,
}

/// Why a launch that Play or a replay started did not reach the game. Reported after the call
/// returned, once the background work fails.
#[derive(Debug, Error)]
pub enum LaunchFailure {
    #[error("could not start the game: {0}")]
    Lock(#[from] wgc_launch::SessionLockError),
    #[error(transparent)]
    Runner(#[from] wgc_launch::RunnerError),
    #[error("could not create the Wine prefix: {0}")]
    Prefix(#[from] wgc_launch::PrefixInitError),
    #[error("could not prepare the game install: {0}")]
    View(#[from] wgc_launch::ViewError),
    #[error("signing in through the launcher needs a Wine prefix; this runner has none")]
    NoWinePrefix,
    #[error("could not set up the Game Center relay: {0}")]
    RelayPrefix(#[from] wgc_ipc::PrefixError),
    #[error("could not start the Game Center relay: {0}")]
    Relay(#[from] wgc_ipc::IpcError),
    #[error("could not start the game: {0}")]
    Spawn(#[from] wgc_launch::SpawnError),
    #[error("the launch task stopped: {0}")]
    Task(#[from] tokio::task::JoinError),
}

/// Why a replay could not be opened in the game.
#[derive(Debug, Error)]
pub enum ReplayLaunchError {
    #[error("the game is already running; close it to watch a replay")]
    GameRunning,
    #[error("checking whether the game is still running; try again in a moment")]
    CheckingForGame,
    #[error("no install has game build {build}")]
    NoInstallForBuild { build: GameBuild },
    #[error("no game install is set")]
    NoInstall,
    #[error("the game settings are being saved; try again in a moment")]
    SavingGameSettings,
    #[error("the app has no data directory to keep the game runtime in")]
    NoDataDir,
}

/// Why the game could not be started outside the launcher.
#[derive(Debug, Error)]
pub enum NativeReplayError {
    #[error("game executable not found at {path}")]
    ExeMissing { path: PathBuf },
    #[error("could not start {path}: {source}")]
    Spawn {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Why a new build did not get the replay settings of an older one.
#[derive(Debug, Error)]
pub enum ReplayCarryError {
    #[error("could not list the builds of the install: {source}")]
    ListBuilds {
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Read(#[from] wows_game_settings::ReadError),
    #[error(transparent)]
    Save(#[from] wows_game_settings::SaveError),
    #[error(transparent)]
    GameRunning(#[from] wows_game_settings::GameRunning),
}

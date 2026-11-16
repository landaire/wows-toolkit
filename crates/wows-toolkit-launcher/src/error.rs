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
    #[error("{path} plays through Steam")]
    PlaysThroughSteam { path: PathBuf },
    #[error("{path} is not a Steam install")]
    NotSteamInstall { path: PathBuf },
    #[error(transparent)]
    LaunchDelta(#[from] LaunchDeltaError),
    #[error("the Steam client was not found; install Steam to play this install")]
    SteamNotFound,
}

/// Why a Steam install cannot run with the WGC layer now.
#[derive(Debug, Error)]
pub enum LaunchDeltaError {
    #[error("{path} is not a Steam install")]
    NotSteamInstall { path: PathBuf },
    #[error("Steam has an update pending for this install; let Steam finish it, then play")]
    SteamUpdatePending { path: PathBuf, update: wgc_install::steam::PendingUpdate },
    #[error(transparent)]
    SteamManifest(#[from] wgc_install::AppManifestError),
    #[error("could not read the game builds of {path}: {source}")]
    ReadBuilds {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} has no game build")]
    NoGameBuild { path: PathBuf },
    #[error("the Game Center files for game build {build} are not downloaded yet")]
    WgcLayerNotReady { build: GameBuild },
    #[error("Steam changed {path} from game build {expected} to {found} during the launch; try again")]
    BuildChanged { path: PathBuf, expected: GameBuild, found: GameBuild },
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
    #[error("could not lay the Game Center files over the install: {0}")]
    WgcLayer(#[from] WgcLayerError),
    #[error("could not start Steam: {0}")]
    SteamLaunch(std::io::Error),
    #[error(transparent)]
    LaunchDelta(#[from] LaunchDeltaError),
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
    #[error(transparent)]
    LaunchDelta(#[from] LaunchDeltaError),
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

/// Why the WGC layer for a Steam install could not be found or built.
#[derive(Debug, Error)]
pub enum WgcLayerError {
    #[error("could not ask WG's update service for the Game Center files: {0}")]
    Catalog(wgc_catalog::error::CatalogError),
    #[error("WG's update service lists no locale package for World of Warships")]
    NoLocalePart,
    #[error("WG's update service lists no client build for World of Warships")]
    NoClientPart,
    #[error("WG offers Game Center files for build {offered}, but this install is build {build}")]
    NoLayerForBuild { build: GameBuild, offered: GameBuild },
    #[error("the locale package has no web seed to download from")]
    NoWebSeed,
    #[error("the locale package has no torrent to download from")]
    NoTorrent,
    #[error("the locale torrent does not list the package WG's update service names")]
    NoLocalePackage,
    #[error("the locale package has no {path}")]
    MissingPackageFile { path: PathBuf },
    #[error("cannot build the download client: {0}")]
    Client(reqwest::Error),
    #[error("could not download the Game Center files: {0}")]
    Download(#[from] wgc_download::DownloadError),
    #[error("the locale package has no files for the offered build {offered}")]
    PackageMissesBuild { offered: GameBuild, composed: Vec<GameBuild> },
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the layer task stopped: {0}")]
    Task(tokio::task::JoinError),
}

impl WgcLayerError {
    /// Attaches `path` to an I/O error.
    pub(crate) fn io(path: &std::path::Path) -> impl FnOnce(std::io::Error) -> WgcLayerError + '_ {
        move |source| WgcLayerError::Io { path: path.to_path_buf(), source }
    }
}

/// Why part of a dev build's launch diagnostics capture failed.
#[cfg(all(debug_assertions, target_os = "linux", target_arch = "x86_64"))]
#[derive(Debug, Error)]
pub(crate) enum CaptureError {
    #[error("the system clock is before 1970: {source}")]
    Clock {
        #[source]
        source: std::time::SystemTimeError,
    },
    #[error("could not create {path}: {source}")]
    CreateFolder {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not create {path}: {source}")]
    CreateFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the install has no {path}")]
    NoLog { path: PathBuf },
    #[error("could not list the game session's processes: {source}")]
    Members {
        #[source]
        source: std::io::Error,
    },
    #[error("the game session has no {exe} process")]
    NoProcess { exe: &'static str },
    #[error("could not start thread {name}: {source}")]
    Thread {
        name: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("a capture thread panicked")]
    WorkerPanicked,
}

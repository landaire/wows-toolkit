//! What the views show of the launcher's state, as values a memo can compare, so a view is rebuilt
//! only when what it shows changes.

use std::num::NonZeroU64;
use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use wgc_catalog::known_games::KnownGame;
use wgc_install::GameInstall;
use wows_toolkit_launcher::CatalogCheckState;
use wows_toolkit_launcher::CatalogSummary;
use wows_toolkit_launcher::DownloadPhase;
use wows_toolkit_launcher::DownloadState;
use wows_toolkit_launcher::Launcher;

/// Which form of the account section shows.
#[derive(Clone, Debug, PartialEq)]
pub enum AccountMode {
    SignedIn(SessionView),
    /// A sign-in waits on a two-factor code.
    TwoFactor,
    /// WGNI asked for a CAPTCHA this launcher cannot show.
    Captcha,
    SignedOut,
}

impl AccountMode {
    pub fn of(launcher: &Launcher, now: SystemTime) -> Self {
        if let Some(session) = launcher.session() {
            let expiry = match session.expiry {
                wgc_auth::TokenExpiry::KnownAt(at) => match at.duration_since(now) {
                    Ok(remaining) => Expiry::In(wows_toolkit_launcher::format_duration(remaining)),
                    Err(_) => Expiry::Passed,
                },
                wgc_auth::TokenExpiry::Unknown => Expiry::Unknown,
            };
            return Self::SignedIn(SessionView {
                nickname: session.nickname.as_str().to_owned(),
                region: wows_toolkit_launcher::realm_label(session.realm),
                expiry,
                persisted: launcher.session_persisted(),
            });
        }

        if launcher.two_factor_pending() {
            Self::TwoFactor
        } else if launcher.captcha_blocked() {
            Self::Captcha
        } else {
            Self::SignedOut
        }
    }
}

/// The signed-in account.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionView {
    pub nickname: String,
    pub region: &'static str,
    pub expiry: Expiry,
    /// False when the session lives only in memory.
    pub persisted: bool,
}

/// When the session's token expires.
#[derive(Clone, Debug, PartialEq)]
pub enum Expiry {
    /// After this long, already formatted.
    In(String),
    Passed,
    Unknown,
}

/// One install's header, which keys its row.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InstallView {
    /// Where the row sits, which numbers its controls.
    pub slot: usize,
    pub path: PathBuf,
    pub source: &'static str,
    /// Steam owns the content, so the launcher neither checks nor downloads it.
    pub read_only: bool,
    pub version: Option<InstallVersion>,
    /// Whether the install has content on disk, which is where a download's folder picker starts.
    pub has_content: bool,
}

impl InstallView {
    pub fn of(slot: usize, install: &GameInstall) -> Self {
        let read_only = install.source.is_read_only_content();
        let version = match &install.version {
            Some(version) => {
                Some(InstallVersion::Known(format!("{}.{}.{}", version.major, version.minor, version.patch)))
            }
            None if read_only => Some(InstallVersion::ManagedBySteam),
            None => None,
        };

        Self {
            slot,
            path: install.path.clone(),
            source: install.source.label(),
            read_only,
            version,
            has_content: install.version.is_some(),
        }
    }

    /// Every install the launcher found, in its order.
    pub fn all(launcher: &Launcher) -> Vec<Self> {
        launcher.installs().iter().enumerate().map(|(slot, install)| Self::of(slot, install)).collect()
    }
}

/// What an install's header says about its version.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum InstallVersion {
    Known(String),
    ManagedBySteam,
}

/// What an install's catalog check found.
#[derive(Clone, Debug, PartialEq)]
pub enum CatalogView {
    Checking,
    Found {
        /// `"<part> <from> -> <to>"` for every part with a newer build.
        updates: Vec<String>,
        /// `"<part> (<to>)"` for every part the install does not have.
        not_installed: Vec<String>,
    },
    Failed(String),
    /// No check ran, or the install has no update host to check against.
    Nothing,
}

impl CatalogView {
    pub fn of(state: Option<&CatalogCheckState>) -> Self {
        match state {
            Some(CatalogCheckState::Checking) => Self::Checking,
            Some(CatalogCheckState::Ready(build)) => {
                let CatalogSummary { updates, not_installed } = CatalogSummary::of(build);
                Self::Found { updates, not_installed }
            }
            Some(CatalogCheckState::Error(detail)) => Self::Failed(detail.clone()),
            Some(CatalogCheckState::NoUpdateHost) | None => Self::Nothing,
        }
    }
}

/// What a known game's version check found.
#[derive(Clone, Debug, PartialEq)]
pub enum GameVersion {
    Checking,
    Version(String),
    /// The catalog answered without a client version.
    Unreported,
    Unavailable(String),
    Nothing,
}

impl GameVersion {
    pub fn of(state: Option<&CatalogCheckState>) -> Self {
        match state {
            Some(CatalogCheckState::Checking) => Self::Checking,
            Some(CatalogCheckState::Ready(build)) => match wows_toolkit_launcher::fresh_install_version(build) {
                Some(version) => Self::Version(version),
                None => Self::Unreported,
            },
            Some(CatalogCheckState::Error(detail)) => Self::Unavailable(detail.clone()),
            Some(CatalogCheckState::NoUpdateHost) | None => Self::Nothing,
        }
    }
}

/// Which download a status line shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadTarget {
    /// The update of the install at this place in the list.
    Install(usize),
    /// The fresh install of a known game, by its place in the known games.
    Game(usize),
}

impl DownloadTarget {
    /// The download's key and state, if one was started.
    pub fn find(self, launcher: &Launcher) -> Option<(&Path, &DownloadState)> {
        match self {
            Self::Install(slot) => {
                let install = launcher.installs().get(slot)?;
                launcher.download_state(&install.path).map(|state| (install.path.as_path(), state))
            }
            Self::Game(index) => launcher.installing_download(wgc_catalog::known_games::known_games().get(index)?),
        }
    }

    /// What distinguishes the controls of this target's line from every other line's.
    pub fn slot(self) -> String {
        match self {
            Self::Install(slot) => slot.to_string(),
            Self::Game(index) => format!("game-{index}"),
        }
    }
}

/// Where a download is, without how far a running one has got.
#[derive(Clone, Debug, PartialEq)]
pub enum DownloadStage {
    Running,
    Paused,
    /// Done. Counts the delta parts downloaded but not applied.
    Installed {
        skipped_deltas: usize,
    },
    Failed(String),
    Cancelled,
}

impl DownloadStage {
    pub fn of(state: &DownloadState) -> Self {
        match state {
            DownloadState::Running { .. } => Self::Running,
            DownloadState::Paused => Self::Paused,
            DownloadState::Done { skipped_deltas } => Self::Installed { skipped_deltas: *skipped_deltas },
            DownloadState::Failed(detail) => Self::Failed(detail.clone()),
            DownloadState::Cancelled => Self::Cancelled,
        }
    }
}

/// How far a running download has got.
#[derive(Clone, Debug, PartialEq)]
pub struct RunningDownload {
    pub phase: DownloadPhase,
    /// `None` until the download knows how much it will fetch.
    pub transferred: Option<Transferred>,
    /// The file being worked on.
    pub current: String,
}

impl RunningDownload {
    pub fn of(state: &DownloadState) -> Option<Self> {
        let DownloadState::Running { phase, done_bytes, total_bytes, current } = state else { return None };

        Some(Self { phase: *phase, transferred: Transferred::new(*done_bytes, *total_bytes), current: current.clone() })
    }
}

/// Bytes fetched of a known total.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transferred {
    pub done: u64,
    pub total: NonZeroU64,
}

impl Transferred {
    /// `None` for a total of zero, which is a total not known yet.
    pub fn new(done: u64, total: u64) -> Option<Self> {
        Some(Self { done, total: NonZeroU64::new(total)? })
    }

    /// The share done, in percent.
    pub fn percent(self) -> f64 {
        (self.done as f64 / self.total.get() as f64 * 100.0).clamp(0.0, 100.0)
    }

    /// `"<done> / <total>"` in binary units.
    pub fn said(self) -> String {
        format!(
            "{} / {}",
            humansize::format_size(self.done, humansize::BINARY),
            humansize::format_size(self.total.get(), humansize::BINARY)
        )
    }
}

/// A running download as the status bar names it.
#[derive(Clone, Debug, PartialEq)]
pub struct DownloadJob {
    pub key: PathBuf,
    /// What is being installed: a known game's name, or the install's path.
    pub subject: String,
    pub progress: RunningDownload,
}

impl DownloadJob {
    /// Every running download, each once. A fresh install is named after its game.
    pub fn all(launcher: &Launcher) -> Vec<Self> {
        let games = wgc_catalog::known_games::known_games().iter().filter_map(|game: &KnownGame| {
            let (key, state) = launcher.installing_download(game)?;
            Some((game.display_name.to_owned(), key, state))
        });
        let installs = launcher.installs().iter().filter_map(|install| {
            let state = launcher.download_state(&install.path)?;
            Some((install.path.display().to_string(), install.path.as_path(), state))
        });

        let mut jobs: Vec<Self> = Vec::new();
        for (subject, key, state) in games.chain(installs) {
            let Some(progress) = RunningDownload::of(state) else { continue };
            if jobs.iter().any(|job| job.key == key) {
                continue;
            }
            jobs.push(Self { key: key.to_path_buf(), subject, progress });
        }

        jobs
    }
}

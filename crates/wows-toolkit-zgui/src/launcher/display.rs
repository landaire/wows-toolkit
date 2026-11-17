//! What the views show of the launcher's state, as values a memo can compare, so a view is rebuilt
//! only when what it shows changes.

use std::num::NonZeroU64;
use std::path::Path;
use std::path::PathBuf;

use wgc_catalog::known_games::KnownGame;
use wgc_install::GameInstall;
use wows_toolkit_launcher::AccountKey;
use wows_toolkit_launcher::AccountStatus;
use wows_toolkit_launcher::CatalogCheckState;
use wows_toolkit_launcher::CatalogSummary;
use wows_toolkit_launcher::DownloadPhase;
use wows_toolkit_launcher::DownloadState;
use wows_toolkit_launcher::GameState;
use wows_toolkit_launcher::InstallChoice;
use wows_toolkit_launcher::Launcher;
use wows_toolkit_launcher::LauncherSettings;
use wows_toolkit_launcher::PlayRoute;
use wows_toolkit_launcher::SavedAccount;
use wows_toolkit_launcher::WgcLayerState;
use wows_toolkit_launcher::WgcLoginAvailability;

use crate::launcher::model::LauncherState;

/// Which step of the sign-in form shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInStep {
    Credentials,
    /// A sign-in waits on a two-factor code.
    TwoFactor,
    /// WGNI asked for a CAPTCHA this launcher cannot show.
    Captcha,
}

impl SignInStep {
    pub fn of(launcher: &Launcher) -> Self {
        if launcher.two_factor_pending() {
            Self::TwoFactor
        } else if launcher.captcha_blocked() {
            Self::Captcha
        } else {
            Self::Credentials
        }
    }
}

/// A saved account as the Play control and the popover name it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AccountLabel {
    pub key: AccountKey,
    /// The realm's short name, as the badge shows it.
    pub realm: &'static str,
    pub nickname: String,
}

impl AccountLabel {
    pub fn of(account: &SavedAccount) -> Self {
        Self { key: account.key, realm: realm_badge(account.key.realm), nickname: account.nickname.as_str().to_owned() }
    }
}

/// The realm as a badge: short and upper case.
pub fn realm_badge(realm: wgc_auth::AccountRealm) -> &'static str {
    match realm {
        wgc_auth::AccountRealm::Eu => "EU",
        wgc_auth::AccountRealm::Na => "NA",
        wgc_auth::AccountRealm::Asia => "ASIA",
    }
}

/// What the Play control says and does.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayState {
    /// The launcher's settings are not read yet.
    Loading,
    /// No install to play, or its game files are missing.
    SetUpGame,
    /// Linux, and no runner for a non-Steam install.
    SetUpRunner,
    /// No account is saved; Play asks for one.
    AddAccount,
    /// The default account's session is being restored.
    Restoring(AccountLabel),
    /// The default account has no usable session.
    SignInAgain(AccountLabel),
    Update {
        install: PathBuf,
        size: u64,
        account: AccountLabel,
    },
    Updating {
        install: PathBuf,
        percent: Option<u8>,
    },
    Paused {
        install: PathBuf,
    },
    /// A Steam install without the Wargaming login: Play hands it to Steam, with no account.
    PlayThroughSteam {
        install: PathBuf,
    },
    Play(AccountLabel),
    Starting {
        realm: Option<wgc_auth::AccountRealm>,
    },
    Running {
        realm: Option<wgc_auth::AccountRealm>,
    },
    InBattle {
        realm: Option<wgc_auth::AccountRealm>,
    },
}

impl PlayState {
    /// `in_battle`: the live match watcher sees a battle.
    pub fn of(state: &LauncherState, wows_dir: Option<&Path>, in_battle: bool) -> Self {
        let Some(settings) = &state.settings else {
            return Self::Loading;
        };
        let launcher = &state.launcher;

        let realm =
            |game: &GameState| game.account().and_then(|key| settings.account(key)).map(|account| account.key.realm);
        match launcher.game_state() {
            GameState::Idle => {}
            game @ GameState::Starting { .. } => return Self::Starting { realm: realm(game) },
            game @ GameState::Running { .. } if in_battle => return Self::InBattle { realm: realm(game) },
            game @ GameState::Running { .. } => return Self::Running { realm: realm(game) },
        }

        let account = settings.default_account().or_else(|| settings.accounts.first());
        let install = match account {
            Some(account) => account.install.resolve(wows_dir),
            None => wows_dir.map(Path::to_path_buf),
        };
        let Some(install) = install.filter(|install| install.join("WorldOfWarships.exe").is_file()) else {
            return Self::SetUpGame;
        };
        if launcher.play_route(&install, settings) == PlayRoute::Steam {
            return Self::PlayThroughSteam { install };
        }
        if launcher.needs_runner_setup() {
            return Self::SetUpRunner;
        }

        match launcher.download_state(&install) {
            Some(DownloadState::Running { done_bytes, total_bytes, .. }) => {
                let percent =
                    Transferred::new(*done_bytes, *total_bytes).map(|transferred| transferred.percent() as u8);
                return Self::Updating { install, percent };
            }
            Some(DownloadState::Paused) => return Self::Paused { install },
            _ => {}
        }

        let Some(account) = account else {
            return Self::AddAccount;
        };
        let label = AccountLabel::of(account);
        if let Some(size) = launcher.update_size(&install) {
            return Self::Update { install, size, account: label };
        }

        match launcher.account_status(account.key) {
            // Not read yet: Play reads it, so the keyring is not asked before the user acts.
            AccountStatus::Ready(_) | AccountStatus::NotLoaded => Self::Play(label),
            AccountStatus::Restoring => Self::Restoring(label),
            AccountStatus::Expired | AccountStatus::SignedOut | AccountStatus::KeyringLocked => {
                Self::SignInAgain(label)
            }
        }
    }

    /// Whether the main part of the button is pressed in the accent color.
    pub fn is_primary(&self) -> bool {
        matches!(self, Self::Play(_) | Self::PlayThroughSteam { .. } | Self::Update { .. } | Self::AddAccount)
    }

    /// The realm whose badge the collapsed button keeps.
    pub fn realm(&self) -> Option<wgc_auth::AccountRealm> {
        match self {
            Self::Restoring(account) | Self::SignInAgain(account) | Self::Play(account) => Some(account.key.realm),
            Self::Update { account, .. } => Some(account.key.realm),
            Self::Starting { realm } | Self::Running { realm } | Self::InBattle { realm } => *realm,
            _ => None,
        }
    }

    /// The nickname the button names, which the collapsed button keeps for its tooltip.
    pub fn nickname(&self) -> Option<&str> {
        match self {
            Self::Restoring(account) | Self::SignInAgain(account) | Self::Play(account) => Some(&account.nickname),
            Self::Update { account, .. } => Some(&account.nickname),
            _ => None,
        }
    }
}

/// How an account row of the popover reads.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AccountRow {
    pub label: AccountLabel,
    pub is_default: bool,
    pub status: AccountStatus,
    /// When the signed-in session ends; `None` while not signed in.
    pub expiry: Option<wgc_auth::TokenExpiry>,
    /// The install the account plays, when it is not the app's game directory.
    pub other_install: Option<PathBuf>,
}

impl AccountRow {
    pub fn all(state: &LauncherState) -> Vec<Self> {
        let Some(settings) = &state.settings else { return Vec::new() };

        settings
            .accounts
            .iter()
            .map(|account| Self {
                label: AccountLabel::of(account),
                is_default: settings.default_account == Some(account.key),
                status: state.launcher.account_status(account.key),
                expiry: state.launcher.session(account.key).map(|session| session.expiry),
                other_install: match &account.install {
                    InstallChoice::Toolkit => None,
                    InstallChoice::Path(path) => Some(path.clone()),
                },
            })
            .collect()
    }
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

/// The Wargaming login switch of a Steam install, and where its Game Center files stand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WgcLoginView {
    pub on: bool,
    pub status: WgcLoginStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WgcLoginStatus {
    /// The files are on disk.
    Ready,
    /// WG offers the files; they download once the login is on.
    Fetchable {
        size: u64,
    },
    Downloading {
        percent: Option<u8>,
    },
    Checking,
    /// No toolkit sign-in is saved.
    NeedsSignIn,
    /// Carries the error's text.
    Unavailable(String),
}

impl WgcLoginStatus {
    /// Whether the switch can be turned on.
    pub fn allows_on(&self) -> bool {
        matches!(self, Self::Ready | Self::Fetchable { .. } | Self::Downloading { .. })
    }
}

impl WgcLoginView {
    /// The switch for the install at `path`, or `None` where it is not offered: installs Steam
    /// does not manage, and platforms without the layer.
    pub fn of(launcher: &Launcher, settings: &LauncherSettings, path: &Path) -> Option<Self> {
        if !launcher.wgc_login_supported() || launcher.steam_install(path).is_none() {
            return None;
        }

        let status = match launcher.wgc_login_availability(path, settings) {
            WgcLoginAvailability::NeedsSignIn => WgcLoginStatus::NeedsSignIn,
            WgcLoginAvailability::Checking => WgcLoginStatus::Checking,
            WgcLoginAvailability::Unavailable(err) => WgcLoginStatus::Unavailable(err.to_string()),
            WgcLoginAvailability::Available => match launcher.wgc_layer_state(path) {
                Some(WgcLayerState::Fetchable(offer)) => WgcLoginStatus::Fetchable { size: offer.size() },
                Some(WgcLayerState::Fetching { done_bytes, total_bytes }) => WgcLoginStatus::Downloading {
                    percent: Transferred::new(*done_bytes, *total_bytes).map(|done| done.percent() as u8),
                },
                _ => WgcLoginStatus::Ready,
            },
        };

        Some(Self { on: settings.uses_wgc_login(path), status })
    }
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

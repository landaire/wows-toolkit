//! Launch WoWs installs.
//!
//! On Linux a launch is a `ResolvedRunner` (Steam's Proton or umu, with a
//! toolkit-owned prefix under `RuntimeDirs`) running a `WindowsProgram`
//! inside an `InstallView`. A read-only install runs from an overlay whose
//! upper directory keeps everything the game writes. An install may also
//! have a delta layer selected, a read-only directory of replacement files
//! and whiteouts laid between it and the upper. Code that reads or edits
//! install files (preferences, replays, `res_mods`) goes through
//! `InstallView::files()`, never the install path, so it sees the game's
//! writes whether or not the game is running.

mod dirs;
mod env;
mod error;
mod external;
mod host;
mod lock;
mod overlay;
mod runner;
mod session;
mod spawn;
mod view;

pub use crate::dirs::InstallKey;
pub use crate::dirs::InstallStateDir;
pub use crate::dirs::RuntimeDirs;
pub use crate::error::KillError;
pub use crate::error::MountError;
pub use crate::error::PrefixInitError;
pub use crate::error::RelativePathError;
pub use crate::error::RunnerError;
pub use crate::error::SessionLockError;
pub use crate::error::SpawnError;
pub use crate::error::ViewError;
pub use crate::error::WaitError;
pub use crate::external::ExternalGame;
pub use crate::host::HostSearch;
pub use crate::host::ProtonInstall;
pub use crate::host::ProtonKind;
pub use crate::host::ProtonVersion;
pub use crate::host::SteamRuntime;
pub use crate::lock::SessionLock;
pub use crate::overlay::OverlayBackend;
pub use crate::overlay::OverlayLayers;
pub use crate::overlay::markers::create_whiteout;
pub use crate::runner::FirstRunWork;
pub use crate::runner::ResolvedRunner;
pub use crate::runner::Runner;
pub use crate::runner::RunnerChoice;
pub use crate::runner::UmuComponent;
pub use crate::runner::WinePrefix;
pub use crate::session::GameExit;
pub use crate::session::GameHandle;
pub use crate::session::GameProcess;
pub use crate::session::ProcessId;
pub use crate::session::SESSION_ENV;
pub use crate::session::SESSION_INSTALL_ENV;
pub use crate::session::SessionId;
pub use crate::session::SessionMember;
pub use crate::spawn::LaunchSpec;
pub use crate::spawn::WindowsProgram;
pub use crate::spawn::dos_path;
pub use crate::spawn::spawn;
pub use crate::view::InstallDirEntry;
pub use crate::view::InstallFiles;
pub use crate::view::InstallView;
pub use crate::view::OverlayView;

#[derive(Debug, Default, Clone)]
pub struct LaunchArgs {
    pub extra: Vec<String>,
    /// Ordered so it is deterministic and serde-friendly.
    pub env: Vec<(String, String)>,
}

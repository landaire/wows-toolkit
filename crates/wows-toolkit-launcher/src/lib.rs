//! Launcher state and background work shared by the front ends: install discovery, catalog
//! update checks, downloads, saved accounts and their WGNI sessions, the game runner, and the
//! game session.
//!
//! [`Launcher`]'s operations spawn work on a tokio runtime and report back as
//! [`LauncherEvent`]s; a front end drains [`LauncherEvents`] and hands each one to
//! [`Launcher::apply`]. Settings stay with the front end, which passes them in and saves them
//! when an operation reports a change.

mod accounts;
#[cfg(all(debug_assertions, target_os = "linux", target_arch = "x86_64"))]
mod diagnostics;
mod display;
mod download;
mod error;
mod files;
mod game;
mod launcher;
mod play;
mod runner;
mod settings;
#[cfg(feature = "test-support")]
pub mod test_support;

pub use accounts::AccountSession;
pub use accounts::AccountStatus;
pub use accounts::KeyringState;
pub use accounts::LoginTarget;
pub use accounts::SessionStorage;
pub use accounts::SignInFailure;
pub use accounts::is_plausible_email;
pub use display::CatalogSummary;
pub use display::REALMS;
pub use display::format_duration;
pub use display::fresh_install_version;
pub use display::realm_label;
pub use display::sign_in_failure_key;
pub use download::DownloadPhase;
pub use error::LaunchFailure;
pub use error::LauncherError;
pub use error::NativeReplayError;
pub use error::PlayError;
pub use error::ReplayCarryError;
pub use error::ReplayLaunchError;
pub use files::InstallSettingsFiles;
pub use files::ReplayCarry;
pub use files::carry_replay_settings;
pub use files::installed_builds;
pub use game::GameBuild;
pub use game::GameExit;
pub use game::GameGate;
pub use game::GameState;
pub use game::open_replay_natively;
pub use launcher::CatalogCheckState;
pub use launcher::DownloadState;
pub use launcher::Launcher;
pub use launcher::LauncherEnv;
pub use launcher::LauncherEvent;
pub use launcher::LauncherEvents;
pub use runner::RunnerState;
pub use settings::AccountKey;
pub use settings::InstallChoice;
pub use settings::LauncherConsent;
pub use settings::LauncherSettings;
pub use settings::SETTINGS_KEY;
pub use settings::SavedAccount;
pub use settings::TermsVersion;

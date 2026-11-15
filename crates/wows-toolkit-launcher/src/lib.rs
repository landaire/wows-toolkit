//! Launcher state and background work shared by the front ends: install discovery, catalog
//! update checks, downloads, saved accounts and their WGNI sessions, and the game session.
//!
//! [`Launcher`]'s operations spawn work on a tokio runtime and report back as
//! [`LauncherEvent`]s; a front end drains [`LauncherEvents`] and hands each one to
//! [`Launcher::apply`]. Settings stay with the front end, which passes them in and saves them
//! when an operation reports a change.

mod accounts;
mod display;
mod download;
mod error;
mod game;
mod launcher;
pub mod proton;
mod settings;
#[cfg(feature = "test-support")]
pub mod test_support;

pub use accounts::AccountSession;
pub use accounts::AccountStatus;
pub use accounts::KeyringState;
pub use accounts::LoginTarget;
pub use accounts::SessionStorage;
pub use accounts::is_plausible_email;
pub use display::CatalogSummary;
pub use display::REALMS;
pub use display::format_duration;
pub use display::fresh_install_version;
pub use display::realm_label;
pub use download::DownloadPhase;
pub use error::LauncherError;
pub use error::PlayError;
pub use error::ReplayLaunchError;
pub use game::GameBuild;
pub use game::GameExit;
pub use game::GameGate;
pub use game::GameState;
pub use launcher::CatalogCheckState;
pub use launcher::DownloadState;
pub use launcher::Launcher;
pub use launcher::LauncherEnv;
pub use launcher::LauncherEvent;
pub use launcher::LauncherEvents;
pub use settings::AccountKey;
pub use settings::InstallChoice;
pub use settings::LauncherConsent;
pub use settings::LauncherSettings;
pub use settings::SETTINGS_KEY;
pub use settings::SavedAccount;
pub use settings::TermsVersion;

/// Open `replay` in the game installed at `wows_dir`, natively, without the launcher's runner.
/// Replay playback is offline and never gated. `prepare` decorates the command before it spawns.
pub fn open_replay_natively(
    wows_dir: &std::path::Path,
    replay: std::path::PathBuf,
    prepare: impl FnOnce(&mut std::process::Command),
) -> Result<(), wgc_launch::LaunchError> {
    let install = wgc_install::GameInstall::manual(wows_dir.to_path_buf());
    let resolved = wgc_launch::resolve(
        &install,
        wgc_launch::LaunchMode::Replay { path: replay },
        &wgc_launch::LaunchArgs::default(),
    )?;
    let mut command = resolved.native_command();
    prepare(&mut command);
    command.spawn()?;
    Ok(())
}

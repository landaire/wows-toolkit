//! Events a front end's tests feed to [`Launcher::apply`](crate::Launcher::apply) to put the
//! launcher in a state that otherwise needs the network, the OS keyring or a scan of this machine.

use std::path::PathBuf;

use wgc_install::GameInstall;

use crate::CatalogCheckState;
use crate::DownloadState;
use crate::Event;
use crate::Launcher;
use crate::LauncherEvent;
use crate::LoginMsg;
use crate::LoginState;
use crate::PendingLogin;

/// Puts `launcher` where [`Launcher::start_login`] does, without the request it sends.
pub fn begin_sign_in(launcher: &mut Launcher, realm: wgc_auth::AccountRealm, username: String) {
    launcher.login = LoginState {
        in_progress: true,
        pending: Some(PendingLogin {
            realm,
            username,
            password: wgc_auth::SecretPassword::new(String::new()),
            twofactor_token: None,
        }),
        ..LoginState::default()
    };
}

/// A finished install discovery that found `installs`.
pub fn installs_found(installs: Vec<GameInstall>) -> LauncherEvent {
    LauncherEvent(Event::Installs(installs))
}

/// A finished catalog check of the install at `install`.
pub fn catalog_checked(install: PathBuf, state: CatalogCheckState) -> LauncherEvent {
    LauncherEvent(Event::Catalog(install, state))
}

/// A finished version check of the known game `game_id`.
pub fn game_checked(game_id: &'static str, state: CatalogCheckState) -> LauncherEvent {
    LauncherEvent(Event::AvailableGame(game_id, state))
}

/// A progress report or the outcome of the download under `key`.
pub fn download_reported(key: PathBuf, state: DownloadState) -> LauncherEvent {
    LauncherEvent(Event::Download(key, state))
}

/// A sign-in that succeeded with `session`. `persisted`: whether it reached the OS keyring.
pub fn signed_in(session: wgc_auth::StoredSession, persisted: bool) -> LauncherEvent {
    LauncherEvent(Event::Login(LoginMsg::Success { session: Box::new(session), persisted }))
}

/// A sign-in that WGNI answered with a two-factor challenge.
pub fn two_factor_required(twofactor_token: String) -> LauncherEvent {
    LauncherEvent(Event::Login(LoginMsg::TwoFactorRequired { twofactor_token }))
}

/// A sign-in that WGNI refused until a CAPTCHA is solved.
pub fn captcha_required() -> LauncherEvent {
    LauncherEvent(Event::Login(LoginMsg::CaptchaRequired))
}

/// A sign-in that failed, saying `error`.
pub fn sign_in_failed(error: String) -> LauncherEvent {
    LauncherEvent(Event::Login(LoginMsg::Failed(error)))
}

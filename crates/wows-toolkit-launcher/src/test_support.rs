//! Events and state changes a front end's tests use to put the launcher in a state that
//! otherwise needs the network, the OS keyring, a scan of this machine or a running game.

use std::path::PathBuf;
use std::sync::Arc;

use wgc_install::GameInstall;

use crate::AccountKey;
use crate::AccountSession;
use crate::CatalogCheckState;
use crate::DownloadState;
use crate::GameBuild;
use crate::GameState;
use crate::KeyringState;
use crate::Launcher;
use crate::LauncherEvent;
use crate::LoginTarget;
use crate::RunnerState;
use crate::SessionStorage;
use crate::SignInFailure;
use crate::WgcLayerState;
use crate::accounts::LoginMsg;
use crate::accounts::LoginState;
use crate::accounts::PendingLogin;
use crate::launcher::Event;

/// A session for `key` with placeholder tokens.
pub fn session(key: AccountKey, nickname: &str) -> wgc_auth::StoredSession {
    wgc_auth::StoredSession {
        realm: key.realm,
        account_id: key.account_id,
        nickname: wgc_auth::Nickname::new(nickname),
        tokens: wgc_auth::TokenPair {
            access_token: wgc_auth::AccessToken::new("token".into()),
            exchange_code: wgc_auth::ExchangeCode::from_raw("code".into()),
        },
        expiry: wgc_auth::TokenExpiry::Unknown,
    }
}

/// Puts `launcher` where [`Launcher::start_login`] does, without the request it sends.
pub fn begin_sign_in(launcher: &mut Launcher, target: LoginTarget, realm: wgc_auth::AccountRealm, username: String) {
    launcher.login = LoginState::new(target);
    launcher.login.in_progress = true;
    launcher.login.pending = Some(PendingLogin {
        realm,
        username,
        password: wgc_auth::SecretPassword::new(String::new()),
        twofactor_token: None,
    });
}

/// Sets the session state of `key` as a restore would.
pub fn set_session(launcher: &mut Launcher, key: AccountKey, state: TestSession) {
    let session = match state {
        TestSession::Restoring => AccountSession::Restoring,
        TestSession::SignedIn(nickname) => {
            AccountSession::SignedIn { session: Arc::new(session(key, &nickname)), storage: SessionStorage::Keyring }
        }
        TestSession::SignedInUntil(nickname, expires_at) => AccountSession::SignedIn {
            session: Arc::new(wgc_auth::StoredSession {
                expiry: wgc_auth::TokenExpiry::KnownAt(expires_at),
                ..session(key, &nickname)
            }),
            storage: SessionStorage::Keyring,
        },
        TestSession::SignedInUnencrypted(nickname) => AccountSession::SignedIn {
            session: Arc::new(session(key, &nickname)),
            storage: SessionStorage::PlaintextFile,
        },
        TestSession::Expired => AccountSession::Expired,
        TestSession::SignedOut => AccountSession::SignedOut,
        TestSession::KeyringLocked => AccountSession::KeyringLocked,
    };
    launcher.sessions.insert(key, session);
}

/// The session states [`set_session`] can set.
pub enum TestSession {
    Restoring,
    SignedIn(String),
    /// Signed in, with the session known to expire at the time given.
    SignedInUntil(String, std::time::SystemTime),
    /// Signed in, with the session saved unencrypted because there is no secret store.
    SignedInUnencrypted(String),
    Expired,
    SignedOut,
    KeyringLocked,
}

/// Sets the game state as a launch and the client's progress would.
pub fn set_game_state(launcher: &mut Launcher, state: GameState) {
    launcher.game = state;
}

/// Sets what the runner search found, as [`Launcher::start_runner_probe`] would.
pub fn set_runner(launcher: &mut Launcher, state: RunnerState) {
    launcher.runner = state;
}

/// A finished install discovery that found `installs`.
pub fn installs_found(installs: Vec<GameInstall>) -> LauncherEvent {
    LauncherEvent(Event::Installs(installs))
}

/// Puts `installs` in place as a discovery would, without the update checks and layer lookups it
/// starts.
pub fn set_installs(launcher: &mut Launcher, installs: Vec<GameInstall>) {
    launcher.installs = installs;
}

/// Sets where the WGC layer for the Steam install at `install`, of game build `build`, stands.
pub fn set_wgc_layer(launcher: &mut Launcher, install: PathBuf, build: GameBuild, state: WgcLayerState) {
    launcher.steam_builds.insert(install, build);
    launcher.wgc_layers.insert(build, state);
}

/// A downloaded layer for `build` at `dir`, for [`set_wgc_layer`].
pub fn ready_wgc_layer(dir: PathBuf, build: GameBuild) -> WgcLayerState {
    WgcLayerState::Ready(Arc::new(crate::wgc_layer::WgcLayer::for_test(dir, build)))
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

/// A sign-in that succeeded with `session`, kept in `storage`.
pub fn signed_in(session: wgc_auth::StoredSession, storage: SessionStorage) -> LauncherEvent {
    let keyring = match storage {
        SessionStorage::Keyring => KeyringState::Available,
        SessionStorage::PlaintextFile => KeyringState::Unavailable,
        SessionStorage::MemoryOnly => KeyringState::Unknown,
    };

    LauncherEvent(Event::Login(LoginMsg::Success { session: Box::new(session), storage, keyring }))
}

/// A sign-in that succeeded while the OS had no secret store, so the session was saved
/// unencrypted.
pub fn signed_in_unencrypted(session: wgc_auth::StoredSession) -> LauncherEvent {
    LauncherEvent(Event::Login(LoginMsg::Success {
        session: Box::new(session),
        storage: SessionStorage::PlaintextFile,
        keyring: KeyringState::Unavailable,
    }))
}

/// A probe of the OS secret store that found `state`.
pub fn keyring_probed(state: KeyringState) -> LauncherEvent {
    LauncherEvent(Event::KeyringProbed(state))
}

/// A sign-in that WGNI answered with a two-factor challenge.
pub fn two_factor_required(twofactor_token: String) -> LauncherEvent {
    LauncherEvent(Event::Login(LoginMsg::TwoFactorRequired { twofactor_token }))
}

/// A two-factor code WGNI refused while keeping the challenge open.
pub fn two_factor_rejected() -> LauncherEvent {
    LauncherEvent(Event::Login(LoginMsg::TwoFactorRetry))
}

/// A sign-in that WGNI refused until a CAPTCHA is solved.
pub fn captcha_required() -> LauncherEvent {
    LauncherEvent(Event::Login(LoginMsg::CaptchaRequired))
}

/// A sign-in that failed with `failure`.
pub fn sign_in_failed(failure: SignInFailure) -> LauncherEvent {
    LauncherEvent(Event::Login(LoginMsg::Failed(failure)))
}

/// What a scripted sign-in answers, in place of WGNI.
pub enum ScriptedSignIn {
    Succeed(wgc_auth::StoredSession),
    /// Succeed with no OS secret store, so the session is saved unencrypted.
    SucceedUnencrypted(wgc_auth::StoredSession),
    TwoFactor,
    WrongCode,
    /// Refused as WGNI refuses with `error`, which the launcher classifies and logs.
    Fail(wgc_auth::AuthError),
}

/// One sign-in a front end sent while scripted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInAttempt {
    pub realm: wgc_auth::AccountRealm,
    pub email: String,
    /// The two-factor code, for a code's submission.
    pub code: Option<String>,
}

/// The answers queued for the next sign-ins, and the sign-ins sent.
#[derive(Default)]
pub struct SignInScript {
    answers: std::collections::VecDeque<ScriptedSignIn>,
    attempts: Vec<SignInAttempt>,
}

impl SignInScript {
    /// The next answer, when one is scripted, noting the attempt either way.
    pub(crate) fn answer(
        &mut self,
        realm: wgc_auth::AccountRealm,
        email: &str,
        code: Option<String>,
    ) -> Option<LoginMsg> {
        self.attempts.push(SignInAttempt { realm, email: email.to_owned(), code });
        let answer = self.answers.pop_front()?;

        Some(match answer {
            ScriptedSignIn::Succeed(session) => LoginMsg::Success {
                session: Box::new(session),
                storage: SessionStorage::Keyring,
                keyring: KeyringState::Available,
            },
            ScriptedSignIn::SucceedUnencrypted(session) => LoginMsg::Success {
                session: Box::new(session),
                storage: SessionStorage::PlaintextFile,
                keyring: KeyringState::Unavailable,
            },
            ScriptedSignIn::TwoFactor => LoginMsg::TwoFactorRequired { twofactor_token: "challenge".into() },
            ScriptedSignIn::WrongCode => LoginMsg::TwoFactorRetry,
            ScriptedSignIn::Fail(error) => crate::accounts::refused(&rootcause::report!(error)),
        })
    }
}

/// Queues `answer` for the next sign-in or code `launcher` sends, which then reaches no server.
pub fn script_sign_in(launcher: &mut Launcher, answer: ScriptedSignIn) {
    launcher.script.answers.push_back(answer);
}

/// The sign-ins and codes `launcher` sent, oldest first.
pub fn sign_in_attempts(launcher: &Launcher) -> Vec<SignInAttempt> {
    launcher.script.attempts.clone()
}

//! Saved accounts: restoring and verifying their sessions, signing in (with two-factor), signing
//! out and forgetting.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::SystemTime;

use rootcause::hooks::builtin_hooks::report_formatter::DefaultReportFormatter;
use tracing::warn;

use crate::launcher::Event;
use crate::launcher::Launcher;
use crate::launcher::LauncherEnv;
use crate::settings::AccountKey;
use crate::settings::LauncherSettings;

/// Whether `text` could be the email address a WG account signs in with: something on each side
/// of an `@`. WGNI decides whether it is an account.
pub fn is_plausible_email(text: &str) -> bool {
    let text = text.trim();
    match text.split_once('@') {
        Some((local, domain)) => !local.is_empty() && !domain.is_empty() && !text.contains(char::is_whitespace),
        None => false,
    }
}

/// Where a signed-in session is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionStorage {
    Keyring,
    /// No OS secret store exists, so the session is saved unencrypted in the app's data folder.
    PlaintextFile,
    /// Saving failed; the session lasts this run only.
    MemoryOnly,
}

impl From<wgc_auth::StorageBackend> for SessionStorage {
    fn from(backend: wgc_auth::StorageBackend) -> SessionStorage {
        match backend {
            wgc_auth::StorageBackend::Keyring => SessionStorage::Keyring,
            wgc_auth::StorageBackend::PlaintextFile => SessionStorage::PlaintextFile,
        }
    }
}

/// Whether the OS offers a secret store to keep sessions in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyringState {
    /// Not probed yet.
    Unknown,
    Available,
    /// No Secret Service (or other OS store) answered; sessions are saved unencrypted.
    Unavailable,
    /// The store stayed locked, or its unlock prompt was dismissed.
    Locked,
}

impl KeyringState {
    /// What a store operation that failed with `error` found out, if anything.
    fn of_error(error: &wgc_auth::AuthError) -> KeyringState {
        match error {
            wgc_auth::AuthError::NoSecretStore => KeyringState::Unavailable,
            wgc_auth::AuthError::KeyringLocked => KeyringState::Locked,
            _ => KeyringState::Unknown,
        }
    }
}

impl From<wgc_auth::StorageBackend> for KeyringState {
    fn from(backend: wgc_auth::StorageBackend) -> KeyringState {
        match backend {
            wgc_auth::StorageBackend::Keyring => KeyringState::Available,
            wgc_auth::StorageBackend::PlaintextFile => KeyringState::Unavailable,
        }
    }
}

/// A saved account's session as this run knows it.
#[derive(Debug, Clone)]
pub enum AccountSession {
    /// Not read from the store yet. The store may prompt to unlock, so it is read only once a
    /// session is needed; see [`Launcher::ensure_sessions`].
    NotLoaded,
    /// Loading from the keyring and checking with WGNI.
    Restoring,
    SignedIn {
        session: Arc<wgc_auth::StoredSession>,
        storage: SessionStorage,
    },
    /// WGNI rejected the stored token. Signing in again replaces it.
    Expired,
    SignedOut,
    /// The keyring stayed locked or its unlock prompt was dismissed, so the session could not
    /// be read.
    KeyringLocked,
}

/// [`AccountSession`] without the session, for display and errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountStatus {
    NotLoaded,
    Restoring,
    Ready(SessionStorage),
    Expired,
    SignedOut,
    KeyringLocked,
}

impl AccountSession {
    pub fn status(&self) -> AccountStatus {
        match self {
            AccountSession::NotLoaded => AccountStatus::NotLoaded,
            AccountSession::Restoring => AccountStatus::Restoring,
            AccountSession::SignedIn { storage, .. } => AccountStatus::Ready(*storage),
            AccountSession::Expired => AccountStatus::Expired,
            AccountSession::SignedOut => AccountStatus::SignedOut,
            AccountSession::KeyringLocked => AccountStatus::KeyringLocked,
        }
    }
}

/// Which entry a sign-in fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginTarget {
    NewAccount,
    Reauthenticate(AccountKey),
}

/// Which keyring restore an event answers. Every sign-in, sign-out and forget of an account
/// moves its id on, so a restore that a later change has overtaken is ignored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RestoreId(u64);

/// Runs each account's session writes one at a time, in the order they were queued: a
/// sign-out's delete never lands after the save of a sign-in queued behind it. A cheap `Clone`.
#[derive(Clone, Default)]
pub(crate) struct CredentialWrites(Arc<Mutex<HashMap<AccountKey, tokio::sync::oneshot::Receiver<()>>>>);

impl CredentialWrites {
    /// Takes `key`'s next turn at once. It starts once every turn queued before it has ended.
    pub(crate) fn queue(&self, key: AccountKey) -> CredentialTurn {
        let (done, next) = tokio::sync::oneshot::channel();
        let previous = {
            let mut last = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            last.insert(key, next)
        };

        CredentialTurn { previous, _done: done }
    }
}

/// A queued session write. Dropping it ends the turn.
pub(crate) struct CredentialTurn {
    previous: Option<tokio::sync::oneshot::Receiver<()>>,
    _done: tokio::sync::oneshot::Sender<()>,
}

impl CredentialTurn {
    /// Waits until the turn before this one has ended.
    pub(crate) async fn start(&mut self) {
        let Some(previous) = self.previous.take() else {
            return;
        };

        // Nothing is ever sent: the earlier turn ends by dropping its sender.
        let _ = previous.await;
    }
}

pub(crate) enum RestoreOutcome {
    Valid {
        session: wgc_auth::StoredSession,
        backend: wgc_auth::StorageBackend,
    },
    Expired,
    Missing,
    KeyringLocked,
    /// The store answered with an error, or what it holds does not parse.
    Unreadable,
}

/// Why a sign-in did not finish, for the sign-in form to say in words. The report behind it
/// goes to the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SignInFailure {
    WrongCredentials,
    /// WGNI waits on a second factor this form cannot answer.
    TwoFactorRequired,
    TwoFactorInvalid,
    CaptchaRequired,
    RateLimited,
    AccountBanned,
    NetworkUnreachable,
    ServiceUnavailable,
    SessionExpired,
    KeyringLocked,
    /// Signed in, but the session could not be kept.
    NotSaved,
    Unexpected,
}

impl SignInFailure {
    pub const ALL: [SignInFailure; 12] = [
        SignInFailure::WrongCredentials,
        SignInFailure::TwoFactorRequired,
        SignInFailure::TwoFactorInvalid,
        SignInFailure::CaptchaRequired,
        SignInFailure::RateLimited,
        SignInFailure::AccountBanned,
        SignInFailure::NetworkUnreachable,
        SignInFailure::ServiceUnavailable,
        SignInFailure::SessionExpired,
        SignInFailure::KeyringLocked,
        SignInFailure::NotSaved,
        SignInFailure::Unexpected,
    ];

    pub fn of(error: &wgc_auth::AuthError) -> SignInFailure {
        use wgc_auth::AuthError as E;

        match error {
            E::InvalidCredentials { .. } | E::AccountNotFound => SignInFailure::WrongCredentials,
            E::TwoFactorRequired { .. } | E::VerificationPending => SignInFailure::TwoFactorRequired,
            E::TwoFactorInvalidCode => SignInFailure::TwoFactorInvalid,
            // WGNI locks two-factor sign-in after too many wrong codes, for a while.
            E::TwoFactorLoginLocked | E::RateLimited => SignInFailure::RateLimited,
            E::CaptchaRequired => SignInFailure::CaptchaRequired,
            E::AccountBanned | E::TwoFactorBanned => SignInFailure::AccountBanned,
            E::Request { .. } => SignInFailure::NetworkUnreachable,
            E::ServiceUnavailable { .. } | E::ProofOfWorkTimeout => SignInFailure::ServiceUnavailable,
            E::SessionExpired => SignInFailure::SessionExpired,
            E::KeyringLocked => SignInFailure::KeyringLocked,
            E::Rejected(_)
            | E::TaskJoin { .. }
            | E::Keyring { .. }
            | E::NoSecretStore
            | E::PlaintextStore { .. }
            | E::PlaintextStoreFormat { .. }
            | E::SerializeSession { .. }
            | E::DeserializeSession { .. }
            | E::MalformedResponse { .. }
            | E::BadResponse { .. }
            | E::GameTokenTimeout { .. }
            | E::RealmMismatch { .. } => SignInFailure::Unexpected,
        }
    }
}

pub(crate) enum LoginMsg {
    Success {
        session: Box<wgc_auth::StoredSession>,
        storage: SessionStorage,
        /// What saving the session found out about the keyring.
        keyring: KeyringState,
    },
    TwoFactorRequired {
        twofactor_token: String,
    },
    /// The code was rejected but the challenge is still open: keep it and the pending password
    /// so the user retries the code, not the whole login.
    TwoFactorRetry,
    CaptchaRequired,
    Failed(SignInFailure),
}

/// A login waiting on a two-factor code. WGNI resubmits full credentials alongside the code, so
/// the password is held, zeroizing, until the attempt ends.
pub(crate) struct PendingLogin {
    pub(crate) realm: wgc_auth::AccountRealm,
    pub(crate) username: String,
    pub(crate) password: wgc_auth::SecretPassword,
    pub(crate) twofactor_token: Option<String>,
}

pub(crate) struct LoginState {
    pub(crate) target: LoginTarget,
    pub(crate) in_progress: bool,
    pub(crate) error: Option<SignInFailure>,
    pub(crate) captcha_blocked: bool,
    pub(crate) pending: Option<PendingLogin>,
    /// The account the last attempt signed in, until the form is reset.
    pub(crate) finished: Option<(AccountKey, SessionStorage)>,
}

impl LoginState {
    pub(crate) fn new(target: LoginTarget) -> LoginState {
        LoginState { target, in_progress: false, error: None, captcha_blocked: false, pending: None, finished: None }
    }
}

impl Launcher {
    /// The session state of a saved account. An account this run has not read from the store is
    /// [`AccountSession::NotLoaded`].
    pub fn account_session(&self, key: AccountKey) -> &AccountSession {
        const NOT_LOADED: AccountSession = AccountSession::NotLoaded;

        self.sessions.get(&key).unwrap_or(&NOT_LOADED)
    }

    pub fn account_status(&self, key: AccountKey) -> AccountStatus {
        self.account_session(key).status()
    }

    /// The signed-in session of `key`, if it has one.
    pub fn session(&self, key: AccountKey) -> Option<&wgc_auth::StoredSession> {
        match self.sessions.get(&key) {
            Some(AccountSession::SignedIn { session, .. }) => Some(session),
            _ => None,
        }
    }

    /// Whether sessions can be kept across restarts, as far as this run has found out.
    pub fn keyring(&self) -> KeyringState {
        self.keyring
    }

    /// Asks the OS secret store whether it is there, off the calling thread, so the sign-in form
    /// can say before a sign-in where the session will be kept.
    pub fn start_keyring_probe(&mut self, env: &LauncherEnv) {
        if self.keyring != KeyringState::Unknown || self.keyring_probing {
            return;
        }
        self.keyring_probing = true;
        let sender = self.sender(env);
        let credentials = env.credentials.clone();
        env.runtime.spawn_blocking(move || {
            sender.send(Event::KeyringProbed(probe_keyring(&credentials)));
        });
    }

    pub(crate) fn apply_keyring_probe(&mut self, state: KeyringState) {
        self.store_answered = true;
        self.keyring_probing = false;
        // A sign-in or restore that has already found out is newer than the probe.
        if self.keyring == KeyringState::Unknown {
            self.keyring = state;
        }
    }

    /// Whether to say that the system keyring may ask to unlock: the store has not answered yet
    /// this run, and a probe is out or a saved session still has to be read.
    pub fn keyring_prompt_ahead(&self, settings: &LauncherSettings) -> bool {
        !self.store_answered
            && (self.keyring_probing
                || settings.accounts.iter().any(|account| {
                    matches!(self.account_status(account.key), AccountStatus::NotLoaded | AccountStatus::Restoring)
                }))
    }

    pub fn login_in_progress(&self) -> bool {
        self.login.in_progress
    }

    pub fn login_error(&self) -> Option<SignInFailure> {
        self.login.error
    }

    pub fn login_target(&self) -> LoginTarget {
        self.login.target
    }

    /// Whether a login is waiting on a two-factor code.
    pub fn two_factor_pending(&self) -> bool {
        self.login.pending.as_ref().is_some_and(|p| p.twofactor_token.is_some())
    }

    pub fn captcha_blocked(&self) -> bool {
        self.login.captcha_blocked
    }

    /// The account the last sign-in saved and where, until [`Launcher::reset_login`].
    pub fn login_finished(&self) -> Option<(AccountKey, SessionStorage)> {
        self.login.finished
    }

    /// Leave the CAPTCHA notice for the sign-in form, once the user has signed in elsewhere and
    /// wants to try again.
    pub fn dismiss_captcha(&mut self) {
        self.login.captcha_blocked = false;
    }

    /// Clears the form state for a new sign-in, dropping any pending two-factor challenge. A
    /// request in flight still lands, and saves its account if it succeeds.
    pub fn reset_login(&mut self, target: LoginTarget) {
        self.login = LoginState::new(target);
    }

    fn bump_restore(&mut self, key: AccountKey) -> RestoreId {
        self.next_restore += 1;
        let restore = RestoreId(self.next_restore);
        self.restores.insert(key, restore);

        restore
    }

    /// Loads every saved account's session not read yet from the store and checks it with WGNI,
    /// off the calling thread: the store may block on its service or an unlock prompt. Call it
    /// once a session is needed, not at startup. Returns whether `settings` changed (a tracking
    /// id was minted) and need saving.
    #[must_use]
    pub fn ensure_sessions(&mut self, settings: &mut LauncherSettings, env: &LauncherEnv) -> bool {
        let keys: Vec<AccountKey> = settings
            .accounts
            .iter()
            .map(|account| account.key)
            .filter(|key| self.account_status(*key) == AccountStatus::NotLoaded)
            .collect();
        if keys.is_empty() {
            return false;
        }

        let (tracking_id, changed) = settings.ensure_tracking_id();
        let restores: Vec<(AccountKey, RestoreId)> = keys
            .into_iter()
            .map(|key| {
                self.sessions.insert(key, AccountSession::Restoring);
                (key, self.bump_restore(key))
            })
            .collect();
        let sender = self.sender(env);
        let credentials = env.credentials.clone();
        // One after another, so a locked store prompts once rather than once per account.
        env.runtime.spawn(async move {
            for (key, restore) in restores {
                let outcome = restore_session(key, tracking_id.clone(), credentials.clone()).await;
                sender.send(Event::SessionRestored { key, restore, outcome });
            }
        });

        changed
    }

    pub(crate) fn apply_restore(
        &mut self,
        key: AccountKey,
        restore: RestoreId,
        outcome: RestoreOutcome,
        settings: &mut LauncherSettings,
    ) -> bool {
        self.store_answered = true;
        if self.restores.get(&key) != Some(&restore) {
            return false;
        }

        let (session, changed) = match outcome {
            RestoreOutcome::Valid { session, backend } => {
                self.keyring = KeyringState::from(backend);
                // The nickname may have changed since the row was saved.
                let changed = settings.account(key).is_some_and(|account| account.nickname != session.nickname)
                    && settings.upsert_account(key, session.nickname.clone());
                let session =
                    AccountSession::SignedIn { session: Arc::new(session), storage: SessionStorage::from(backend) };
                (session, changed)
            }
            RestoreOutcome::Expired => (AccountSession::Expired, false),
            // Shown as needing a sign-in, which saves over the unreadable session.
            RestoreOutcome::Unreadable => (AccountSession::Expired, false),
            RestoreOutcome::Missing => (AccountSession::SignedOut, false),
            RestoreOutcome::KeyringLocked => {
                self.keyring = KeyringState::Locked;
                (AccountSession::KeyringLocked, false)
            }
        };
        self.sessions.insert(key, session);

        changed
    }

    pub(crate) fn apply_login(&mut self, msg: LoginMsg, settings: &mut LauncherSettings) -> bool {
        self.login.in_progress = false;
        match msg {
            LoginMsg::Success { session, storage, keyring } => {
                self.store_answered = true;
                if keyring != KeyringState::Unknown {
                    self.keyring = keyring;
                }
                let key = AccountKey::of(&session);
                let changed = settings.upsert_account(key, session.nickname.clone());
                let _ = self.bump_restore(key);
                self.sessions.insert(key, AccountSession::SignedIn { session: Arc::from(session), storage });
                self.login = LoginState::new(self.login.target);
                self.login.finished = Some((key, storage));
                changed
            }
            LoginMsg::TwoFactorRequired { twofactor_token } => {
                if let Some(pending) = self.login.pending.as_mut() {
                    pending.twofactor_token = Some(twofactor_token);
                }
                false
            }
            LoginMsg::TwoFactorRetry => {
                self.login.error = Some(SignInFailure::TwoFactorInvalid);
                false
            }
            LoginMsg::CaptchaRequired => {
                self.login.captcha_blocked = true;
                self.login.pending = None;
                false
            }
            LoginMsg::Failed(error) => {
                self.login.error = Some(error);
                self.login.pending = None;
                false
            }
        }
    }

    /// Start a sign-in for `target`. Returns whether `settings` changed (a tracking id was
    /// minted) and need saving.
    #[must_use]
    pub fn start_login(
        &mut self,
        target: LoginTarget,
        realm: wgc_auth::AccountRealm,
        username: String,
        password: wgc_auth::SecretPassword,
        settings: &mut LauncherSettings,
        env: &LauncherEnv,
    ) -> bool {
        let (tracking_id, changed) = settings.ensure_tracking_id();
        self.login = LoginState::new(target);
        self.login.in_progress = true;
        self.login.pending = Some(PendingLogin {
            realm,
            username: username.clone(),
            password: wgc_auth::SecretPassword::new(password.expose_secret().to_string()),
            twofactor_token: None,
        });
        let sender = self.sender(env);
        #[cfg(feature = "test-support")]
        if let Some(answer) = self.script.answer(realm, &username, None) {
            sender.send(Event::Login(answer));
            return changed;
        }
        let store = env.credentials.clone();
        let writes = self.credential_writes.clone();
        env.runtime.spawn(async move {
            let client = wgc_auth::AuthClient::new(realm, tracking_id);
            let result = client.login(wgc_auth::AccountCredentials { username, password }).await;
            sender.send(Event::Login(login_result(result, store, writes).await));
        });

        changed
    }

    /// Resubmit the pending login with a two-factor code. A no-op without a pending challenge.
    /// Returns whether `settings` changed and need saving.
    #[must_use]
    pub fn submit_two_factor(
        &mut self,
        code: wgc_auth::TwoFactorCode,
        settings: &mut LauncherSettings,
        env: &LauncherEnv,
    ) -> bool {
        let Some(pending) = self.login.pending.as_ref() else {
            return false;
        };
        let Some(twofactor_token) = pending.twofactor_token.clone() else {
            return false;
        };
        let realm = pending.realm;
        let credentials = wgc_auth::AccountCredentials {
            username: pending.username.clone(),
            password: wgc_auth::SecretPassword::new(pending.password.expose_secret().to_string()),
        };
        let (tracking_id, changed) = settings.ensure_tracking_id();
        self.login.in_progress = true;
        self.login.error = None;
        let sender = self.sender(env);
        #[cfg(feature = "test-support")]
        {
            let shown = match &code {
                wgc_auth::TwoFactorCode::Otp(code) | wgc_auth::TwoFactorCode::Backup(code) => code.clone(),
            };
            if let Some(answer) = self.script.answer(realm, &credentials.username, Some(shown)) {
                sender.send(Event::Login(answer));
                return changed;
            }
        }
        let store = env.credentials.clone();
        let writes = self.credential_writes.clone();
        env.runtime.spawn(async move {
            let client = wgc_auth::AuthClient::new(realm, tracking_id);
            let result = client.login_with_two_factor(credentials, twofactor_token, code).await;
            sender.send(Event::Login(login_result(result, store, writes).await));
        });

        changed
    }

    /// Ends the account's session: locally at once, then the removal of its session and game
    /// token from whichever store holds them, then a server-side revoke (best effort). The
    /// account stays listed.
    pub fn sign_out(&mut self, key: AccountKey, settings: &mut LauncherSettings, env: &LauncherEnv) {
        let _ = self.bump_restore(key);
        let previous = self.sessions.insert(key, AccountSession::SignedOut);
        let session = match previous {
            Some(AccountSession::SignedIn { session, .. }) => Some(session),
            _ => None,
        };
        let (tracking_id, _) = settings.ensure_tracking_id();
        let credentials = env.credentials.clone();
        // Queued now, so a sign-in that saves after this returns waits for the delete.
        let mut turn = self.credential_writes.queue(key);
        env.runtime.spawn(async move {
            turn.start().await;
            let deleted = tokio::task::spawn_blocking(move || {
                let session = credentials.delete_session(key.realm, key.account_id);
                let token2 = credentials.delete_token2(key.realm, key.account_id);
                drop(turn);
                session.and(token2)
            })
            .await;
            if let Ok(Err(e)) = deleted {
                warn!("could not delete the stored session: {e}");
            }
            if let Some(session) = session {
                let client = wgc_auth::AuthClient::new(key.realm, tracking_id);
                if let Err(report) = client.logout(&session.tokens).await {
                    warn!("server-side session revoke failed (signed out locally anyway): {}", ascii(&report));
                }
            }
        });
    }

    /// Signs the account out and removes it from the list. Returns whether `settings` changed.
    #[must_use]
    pub fn forget(&mut self, key: AccountKey, settings: &mut LauncherSettings, env: &LauncherEnv) -> bool {
        self.sign_out(key, settings, env);
        self.sessions.remove(&key);
        self.restores.remove(&key);

        settings.remove_account(key)
    }

    /// Marks a signed-in account's session expired after WGNI refused it, so the front ends
    /// offer to sign in again.
    pub(crate) fn expire_session(&mut self, key: AccountKey) {
        if !matches!(self.sessions.get(&key), Some(AccountSession::SignedIn { .. })) {
            return;
        }

        let _ = self.bump_restore(key);
        self.sessions.insert(key, AccountSession::Expired);
    }
}

/// Looks up an entry no account has: a store that answers at all is there.
fn probe_keyring(credentials: &wgc_auth::CredentialStore) -> KeyringState {
    match credentials.probe() {
        Ok(backend) => KeyringState::from(backend),
        Err(error) => KeyringState::of_error(&error),
    }
}

/// Loads `key`'s session from whichever store holds it and asks WGNI whether it still holds.
async fn restore_session(
    key: AccountKey,
    tracking_id: wgc_core::TrackingId,
    credentials: wgc_auth::CredentialStore,
) -> RestoreOutcome {
    let loaded = tokio::task::spawn_blocking(move || credentials.load_session(key.realm, key.account_id)).await;
    let wgc_auth::Stored { value: session, backend } = match loaded {
        Ok(Ok(Some(stored))) => stored,
        Ok(Ok(None)) => return RestoreOutcome::Missing,
        Ok(Err(wgc_auth::AuthError::KeyringLocked)) => return RestoreOutcome::KeyringLocked,
        Ok(Err(e)) => {
            warn!("could not load a stored session: {e}");
            return RestoreOutcome::Unreadable;
        }
        Err(e) => {
            warn!("session load task did not complete: {e}");
            return RestoreOutcome::Unreadable;
        }
    };

    let client = wgc_auth::AuthClient::new(key.realm, tracking_id);
    let verified = client.verify(&session.tokens).await;
    let verified = match &verified {
        Ok((account_id, nickname)) => Ok((*account_id, nickname.clone())),
        Err(report) => {
            if !matches!(report.current_context(), wgc_auth::AuthError::SessionExpired) {
                warn!("could not verify a stored session; keeping it: {}", ascii(report));
            }
            Err(report.current_context())
        }
    };

    checked(session, backend, verified, SystemTime::now())
}

/// What WGNI's answer `verified` makes of a stored session. WGNI decides, not the stored expiry:
/// sessions saved before the expiry came from the exchanged token carry the short-lived
/// password grant's, so one already past is dropped. A session WGNI places under another account
/// than it is stored for is invalid.
fn checked(
    session: wgc_auth::StoredSession,
    backend: wgc_auth::StorageBackend,
    verified: Result<(wgc_auth::AccountId, wgc_auth::Nickname), &wgc_auth::AuthError>,
    now: SystemTime,
) -> RestoreOutcome {
    let expiry = match session.expiry {
        wgc_auth::TokenExpiry::KnownAt(expires_at) if expires_at <= now => wgc_auth::TokenExpiry::Unknown,
        expiry => expiry,
    };
    let session = wgc_auth::StoredSession { expiry, ..session };
    match verified {
        Ok((account_id, _)) if account_id != session.account_id => {
            warn!("a stored session belongs to account {account_id:?}, not {:?}; not using it", session.account_id);
            RestoreOutcome::Expired
        }
        Ok((_, nickname)) => {
            RestoreOutcome::Valid { session: wgc_auth::StoredSession { nickname, ..session }, backend }
        }
        Err(wgc_auth::AuthError::SessionExpired) => RestoreOutcome::Expired,
        // Offline or WGNI unreachable: the session may well hold, and Play finds out.
        Err(_) => RestoreOutcome::Valid { session, backend },
    }
}

/// Turns a login result into the message for it. On success the session is first saved
/// (sync, so on a blocking-pool thread).
async fn login_result(
    result: rootcause::Result<wgc_auth::StoredSession, wgc_auth::AuthError>,
    credentials: wgc_auth::CredentialStore,
    writes: CredentialWrites,
) -> LoginMsg {
    let report = match result {
        Ok(session) => {
            let mut turn = writes.queue(AccountKey::of(&session));
            turn.start().await;
            let saved = tokio::task::spawn_blocking(move || {
                let persisted = credentials.save_session(&session);
                drop(turn);
                (session, persisted)
            })
            .await;
            return match saved {
                Ok((session, Ok(backend))) => LoginMsg::Success {
                    session: Box::new(session),
                    storage: SessionStorage::from(backend),
                    keyring: KeyringState::from(backend),
                },
                Ok((session, Err(e))) => {
                    warn!("could not save the session: {e}");
                    LoginMsg::Success {
                        session: Box::new(session),
                        storage: SessionStorage::MemoryOnly,
                        keyring: KeyringState::of_error(&e),
                    }
                }
                Err(join_err) => {
                    warn!("session save task did not complete: {join_err}");
                    LoginMsg::Failed(SignInFailure::NotSaved)
                }
            };
        }
        Err(report) => report,
    };
    refused(&report)
}

/// The message for a sign-in WGNI refused. The report is logged in full whatever it says.
pub(crate) fn refused(report: &rootcause::Report<wgc_auth::AuthError>) -> LoginMsg {
    warn!("sign-in refused: {}", ascii(report));
    match report.current_context() {
        wgc_auth::AuthError::TwoFactorRequired { twofactor_token } => {
            LoginMsg::TwoFactorRequired { twofactor_token: twofactor_token.clone() }
        }
        wgc_auth::AuthError::CaptchaRequired => LoginMsg::CaptchaRequired,
        // A wrong code keeps the challenge open; a lockout or ban does not.
        wgc_auth::AuthError::TwoFactorInvalidCode => LoginMsg::TwoFactorRetry,
        error => LoginMsg::Failed(SignInFailure::of(error)),
    }
}

/// A report in the log's ASCII layout.
fn ascii(report: &rootcause::Report<wgc_auth::AuthError>) -> String {
    report.format_with(&DefaultReportFormatter::ASCII).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launcher::LauncherEvent;

    #[test]
    fn an_email_needs_text_on_both_sides_of_an_at_sign() {
        assert!(is_plausible_email("captain@example.com"));
        assert!(is_plausible_email(" captain@example.com "));
        for text in ["", "captain", "@example.com", "captain@", "cap tain@example.com"] {
            assert!(!is_plausible_email(text), "{text:?}");
        }
    }

    fn key(realm: wgc_auth::AccountRealm, id: u64) -> AccountKey {
        AccountKey { realm, account_id: wgc_auth::AccountId::new(id) }
    }

    fn session(key: AccountKey, nickname: &str) -> wgc_auth::StoredSession {
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

    /// No secret store and no data directory, so tests never touch the user's keyring.
    struct NoSecretStore;

    impl wgc_auth::SecretStore for NoSecretStore {
        fn get(&self, _: &wgc_auth::KeyringEntry) -> Result<Option<String>, wgc_auth::AuthError> {
            Err(wgc_auth::AuthError::NoSecretStore)
        }

        fn set(&self, _: &wgc_auth::KeyringEntry, _: &str) -> Result<(), wgc_auth::AuthError> {
            Err(wgc_auth::AuthError::NoSecretStore)
        }

        fn delete(&self, _: &wgc_auth::KeyringEntry) -> Result<(), wgc_auth::AuthError> {
            Err(wgc_auth::AuthError::NoSecretStore)
        }
    }

    fn env(runtime: &tokio::runtime::Runtime) -> LauncherEnv {
        LauncherEnv {
            runtime: runtime.handle().clone(),
            proxy_url: None,
            wake: Arc::new(|| {}),
            runtime_dirs: None,
            credentials: wgc_auth::CredentialStore::with_backends(Arc::new(NoSecretStore), None),
        }
    }

    fn signed_in(session: wgc_auth::StoredSession) -> LauncherEvent {
        LauncherEvent(Event::Login(LoginMsg::Success {
            session: Box::new(session),
            storage: SessionStorage::Keyring,
            keyring: KeyringState::Available,
        }))
    }

    fn nickname(launcher: &Launcher, key: AccountKey) -> Option<&str> {
        launcher.session(key).map(|session| session.nickname.as_str())
    }

    #[test]
    fn a_second_sign_in_adds_an_account_and_keeps_the_default() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let env = env(&runtime);
        let mut settings = LauncherSettings::default();
        let (mut launcher, _events) = Launcher::new();
        let eu = key(wgc_auth::AccountRealm::Eu, 697_769_785);
        let na = key(wgc_auth::AccountRealm::Na, 1_034_874_630);

        assert!(launcher.apply(signed_in(session(eu, "first")), &mut settings, &env));
        assert!(launcher.apply(signed_in(session(na, "second")), &mut settings, &env));

        assert_eq!(nickname(&launcher, eu), Some("first"));
        assert_eq!(nickname(&launcher, na), Some("second"));
        assert_eq!(settings.default_account, Some(eu));
        assert_eq!(launcher.login_finished(), Some((na, SessionStorage::Keyring)));
    }

    #[test]
    fn a_restore_overtaken_by_a_sign_in_is_ignored() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let env = env(&runtime);
        let eu = key(wgc_auth::AccountRealm::Eu, 697_769_785);
        let mut settings = LauncherSettings::default();
        let _ = settings.upsert_account(eu, wgc_auth::Nickname::new("saved"));
        let (mut launcher, _events) = Launcher::new();
        let restore = launcher.bump_restore(eu);
        launcher.sessions.insert(eu, AccountSession::Restoring);

        let _ = launcher.apply(signed_in(session(eu, "fresh")), &mut settings, &env);
        let stale = LauncherEvent(Event::SessionRestored { key: eu, restore, outcome: RestoreOutcome::Expired });
        let _ = launcher.apply(stale, &mut settings, &env);

        assert_eq!(nickname(&launcher, eu), Some("fresh"));
    }

    #[test]
    fn an_expired_session_is_replaced_by_signing_in_again() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let env = env(&runtime);
        let eu = key(wgc_auth::AccountRealm::Eu, 697_769_785);
        let mut settings = LauncherSettings::default();
        let _ = settings.upsert_account(eu, wgc_auth::Nickname::new("saved"));
        let (mut launcher, _events) = Launcher::new();
        let restore = launcher.bump_restore(eu);

        let expired = LauncherEvent(Event::SessionRestored { key: eu, restore, outcome: RestoreOutcome::Expired });
        let _ = launcher.apply(expired, &mut settings, &env);
        assert_eq!(launcher.account_status(eu), AccountStatus::Expired);

        launcher.reset_login(LoginTarget::Reauthenticate(eu));
        let _ = launcher.apply(signed_in(session(eu, "saved")), &mut settings, &env);
        assert_eq!(launcher.account_status(eu), AccountStatus::Ready(SessionStorage::Keyring));
        assert_eq!(settings.accounts.len(), 1);
    }

    #[test]
    fn a_restore_with_a_new_nickname_updates_the_row() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let env = env(&runtime);
        let eu = key(wgc_auth::AccountRealm::Eu, 697_769_785);
        let mut settings = LauncherSettings::default();
        let _ = settings.upsert_account(eu, wgc_auth::Nickname::new("old"));
        let (mut launcher, _events) = Launcher::new();
        let restore = launcher.bump_restore(eu);

        let valid =
            RestoreOutcome::Valid { session: session(eu, "renamed"), backend: wgc_auth::StorageBackend::Keyring };
        assert!(launcher.apply(
            LauncherEvent(Event::SessionRestored { key: eu, restore, outcome: valid }),
            &mut settings,
            &env
        ));
        assert_eq!(settings.account(eu).unwrap().nickname.as_str(), "renamed");
    }

    /// A secret store whose deletes wait until the test lets them through.
    struct GatedKeyring {
        entries: Mutex<HashMap<String, String>>,
        gate: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl wgc_auth::SecretStore for GatedKeyring {
        fn get(&self, entry: &wgc_auth::KeyringEntry) -> Result<Option<String>, wgc_auth::AuthError> {
            Ok(self.entries.lock().unwrap().get(&entry.user).cloned())
        }

        fn set(&self, entry: &wgc_auth::KeyringEntry, secret: &str) -> Result<(), wgc_auth::AuthError> {
            self.entries.lock().unwrap().insert(entry.user.clone(), secret.to_string());
            Ok(())
        }

        fn delete(&self, entry: &wgc_auth::KeyringEntry) -> Result<(), wgc_auth::AuthError> {
            let _ = self.gate.lock().unwrap().recv();
            self.entries.lock().unwrap().remove(&entry.user);
            Ok(())
        }
    }

    #[test]
    fn a_sign_in_after_a_sign_out_keeps_its_saved_session() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let (release, gate) = std::sync::mpsc::channel();
        let keyring = GatedKeyring { entries: Mutex::new(HashMap::new()), gate: Mutex::new(gate) };
        let credentials = wgc_auth::CredentialStore::with_backends(Arc::new(keyring), None);
        let env = LauncherEnv { credentials: credentials.clone(), ..env(&runtime) };
        let mut settings = LauncherSettings::default();
        let (mut launcher, _events) = Launcher::new();
        let eu = key(wgc_auth::AccountRealm::Eu, 697_769_785);

        launcher.sign_out(eu, &mut settings, &env);
        // The session and the token2 delete each pass the gate once, after the sign-in below
        // has had time to save.
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            for _ in 0..2 {
                release.send(()).unwrap();
            }
        });
        let msg = runtime.block_on(login_result(
            Ok(session(eu, "again")),
            credentials.clone(),
            launcher.credential_writes.clone(),
        ));
        releaser.join().unwrap();

        assert!(matches!(msg, LoginMsg::Success { storage: SessionStorage::Keyring, .. }));
        assert!(credentials.load_session(eu.realm, eu.account_id).unwrap().is_some());
    }

    #[test]
    fn forgetting_an_account_drops_its_row_and_session() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let env = env(&runtime);
        let mut settings = LauncherSettings::default();
        let (mut launcher, _events) = Launcher::new();
        let eu = key(wgc_auth::AccountRealm::Eu, 697_769_785);
        let _ = launcher.apply(signed_in(session(eu, "first")), &mut settings, &env);

        assert!(launcher.forget(eu, &mut settings, &env));

        assert!(settings.accounts.is_empty());
        assert_eq!(settings.default_account, None);
        assert!(launcher.session(eu).is_none());
    }

    #[test]
    fn sessions_are_read_once_when_first_needed() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let env = env(&runtime);
        let eu = key(wgc_auth::AccountRealm::Eu, 697_769_785);
        let mut settings = LauncherSettings::default();
        let _ = settings.upsert_account(eu, wgc_auth::Nickname::new("saved"));
        let (mut launcher, _events) = Launcher::new();
        assert_eq!(launcher.account_status(eu), AccountStatus::NotLoaded);
        assert!(launcher.keyring_prompt_ahead(&settings));

        let _ = launcher.ensure_sessions(&mut settings, &env);
        assert_eq!(launcher.account_status(eu), AccountStatus::Restoring);
        let restore = launcher.restores[&eu];
        let _ = launcher.ensure_sessions(&mut settings, &env);
        assert_eq!(launcher.restores[&eu], restore, "a session being read is not read again");

        let missing = LauncherEvent(Event::SessionRestored { key: eu, restore, outcome: RestoreOutcome::Missing });
        let _ = launcher.apply(missing, &mut settings, &env);
        assert_eq!(launcher.account_status(eu), AccountStatus::SignedOut);
        assert!(!launcher.keyring_prompt_ahead(&settings));
        let _ = launcher.ensure_sessions(&mut settings, &env);
        assert_eq!(launcher.account_status(eu), AccountStatus::SignedOut, "an answered read is kept");
    }

    #[test]
    fn a_keyring_probe_without_saved_accounts_says_the_keyring_may_ask() {
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let env = env(&runtime);
        let mut settings = LauncherSettings::default();
        let (mut launcher, _events) = Launcher::new();
        assert!(!launcher.keyring_prompt_ahead(&settings), "nothing will read the store");

        launcher.start_keyring_probe(&env);
        assert!(launcher.keyring_prompt_ahead(&settings));

        let probed = LauncherEvent(Event::KeyringProbed(KeyringState::Available));
        let _ = launcher.apply(probed, &mut settings, &env);
        assert!(!launcher.keyring_prompt_ahead(&settings));
    }

    fn expiring_at(key: AccountKey, expires_at: SystemTime) -> wgc_auth::StoredSession {
        wgc_auth::StoredSession { expiry: wgc_auth::TokenExpiry::KnownAt(expires_at), ..session(key, "saved") }
    }

    fn restored_expiry(outcome: RestoreOutcome) -> Option<wgc_auth::TokenExpiry> {
        match outcome {
            RestoreOutcome::Valid { session, .. } => Some(session.expiry),
            RestoreOutcome::Expired
            | RestoreOutcome::Missing
            | RestoreOutcome::KeyringLocked
            | RestoreOutcome::Unreadable => None,
        }
    }

    #[test]
    fn a_stored_expiry_already_past_is_dropped_when_wgni_accepts_the_session() {
        let eu = key(wgc_auth::AccountRealm::Eu, 697_769_785);
        let now = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_800_000_000);
        let past = now - std::time::Duration::from_secs(60);
        let ahead = now + std::time::Duration::from_secs(60);
        let backend = wgc_auth::StorageBackend::Keyring;
        let accepted = || Ok((eu.account_id, wgc_auth::Nickname::new("saved")));
        let unreachable = wgc_auth::AuthError::RateLimited;

        let stale = checked(expiring_at(eu, past), backend, accepted(), now);
        let offline = checked(expiring_at(eu, past), backend, Err(&unreachable), now);
        let current = checked(expiring_at(eu, ahead), backend, accepted(), now);
        let rejected = checked(expiring_at(eu, ahead), backend, Err(&wgc_auth::AuthError::SessionExpired), now);

        assert_eq!(restored_expiry(stale), Some(wgc_auth::TokenExpiry::Unknown));
        assert_eq!(restored_expiry(offline), Some(wgc_auth::TokenExpiry::Unknown));
        assert_eq!(restored_expiry(current), Some(wgc_auth::TokenExpiry::KnownAt(ahead)));
        assert!(matches!(rejected, RestoreOutcome::Expired));
    }

    #[test]
    fn a_session_wgni_places_under_another_account_is_not_used() {
        let eu = key(wgc_auth::AccountRealm::Eu, 697_769_785);
        let other = wgc_auth::AccountId::new(1_034_874_630);
        let backend = wgc_auth::StorageBackend::Keyring;

        let outcome =
            checked(session(eu, "saved"), backend, Ok((other, wgc_auth::Nickname::new("other"))), SystemTime::now());

        assert!(matches!(outcome, RestoreOutcome::Expired));
    }

    #[test]
    fn an_unreadable_session_file_needs_a_new_sign_in() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let plaintext = wgc_auth::PlaintextFile::in_dir(dir.path());
        std::fs::create_dir_all(plaintext.path().parent().unwrap()).unwrap();
        std::fs::write(plaintext.path(), b"{not json").unwrap();
        let credentials = wgc_auth::CredentialStore::with_backends(Arc::new(NoSecretStore), Some(plaintext));
        let eu = key(wgc_auth::AccountRealm::Eu, 697_769_785);
        let mut settings = LauncherSettings::default();
        let (tracking_id, _) = settings.ensure_tracking_id();

        let outcome = runtime.block_on(restore_session(eu, tracking_id, credentials));

        assert!(matches!(outcome, RestoreOutcome::Unreadable));
    }
}

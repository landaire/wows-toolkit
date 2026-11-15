//! Saved accounts: restoring and verifying their sessions, signing in (with two-factor), signing
//! out and forgetting.

use std::sync::Arc;
use std::time::SystemTime;

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
    /// The OS had no usable secret store; the session lasts this run only.
    MemoryOnly,
}

/// Whether the OS offers a secret store to keep sessions in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyringState {
    /// Not probed yet.
    Unknown,
    Available,
    /// No Secret Service (or other OS store) answered; sessions last this run only.
    Unavailable,
}

/// A saved account's session as this run knows it.
#[derive(Debug, Clone)]
pub enum AccountSession {
    /// Loading from the keyring and checking with WGNI.
    Restoring,
    SignedIn {
        session: Arc<wgc_auth::StoredSession>,
        storage: SessionStorage,
    },
    /// WGNI rejected the stored token, or its known expiry passed. Signing in again replaces it.
    Expired,
    SignedOut,
    /// `NoSecretStore`: absent or locked; the keyring does not say which.
    StoreUnavailable,
}

/// [`AccountSession`] without the session, for display and errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccountStatus {
    Restoring,
    Ready(SessionStorage),
    Expired,
    SignedOut,
    StoreUnavailable,
}

impl AccountSession {
    pub fn status(&self) -> AccountStatus {
        match self {
            AccountSession::Restoring => AccountStatus::Restoring,
            AccountSession::SignedIn { storage, .. } => AccountStatus::Ready(*storage),
            AccountSession::Expired => AccountStatus::Expired,
            AccountSession::SignedOut => AccountStatus::SignedOut,
            AccountSession::StoreUnavailable => AccountStatus::StoreUnavailable,
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

pub(crate) enum RestoreOutcome {
    Valid(wgc_auth::StoredSession),
    Expired,
    Missing,
    StoreUnavailable,
}

pub(crate) enum LoginMsg {
    Success {
        session: Box<wgc_auth::StoredSession>,
        storage: SessionStorage,
        /// The save found no OS secret store at all.
        no_store: bool,
    },
    TwoFactorRequired {
        twofactor_token: String,
    },
    /// The code was rejected but the challenge is still open: keep it and the pending password
    /// so the user retries the code, not the whole login.
    TwoFactorRetry(String),
    CaptchaRequired,
    Failed(String),
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
    pub(crate) error: Option<String>,
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
    /// The session state of a saved account. An account this run has not restored is signed out.
    pub fn account_session(&self, key: AccountKey) -> &AccountSession {
        const SIGNED_OUT: AccountSession = AccountSession::SignedOut;

        self.sessions.get(&key).unwrap_or(&SIGNED_OUT)
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
    /// can say before a sign-in that the session will not be kept.
    pub fn start_keyring_probe(&mut self, env: &LauncherEnv) {
        if self.keyring != KeyringState::Unknown {
            return;
        }
        let sender = self.sender(env);
        env.runtime.spawn_blocking(move || {
            sender.send(Event::KeyringProbed(probe_keyring()));
        });
    }

    pub(crate) fn apply_keyring_probe(&mut self, state: KeyringState) {
        // A sign-in or restore that has already found out is newer than the probe.
        if self.keyring == KeyringState::Unknown {
            self.keyring = state;
        }
    }

    pub fn login_in_progress(&self) -> bool {
        self.login.in_progress
    }

    pub fn login_error(&self) -> Option<&str> {
        self.login.error.as_deref()
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

    /// Load every saved account's session from the OS keyring and check it with WGNI, off the
    /// calling thread: the keyring may block on its service. Returns whether `settings` changed
    /// (a tracking id was minted) and need saving.
    #[must_use]
    pub fn start_session_restore(&mut self, settings: &mut LauncherSettings, env: &LauncherEnv) -> bool {
        let (tracking_id, changed) = settings.ensure_tracking_id();
        let keys: Vec<AccountKey> = settings.accounts.iter().map(|account| account.key).collect();
        for key in keys {
            let restore = self.bump_restore(key);
            self.sessions.insert(key, AccountSession::Restoring);
            let sender = self.sender(env);
            let tracking_id = tracking_id.clone();
            env.runtime.spawn(async move {
                let outcome = restore_session(key, tracking_id).await;
                sender.send(Event::SessionRestored { key, restore, outcome });
            });
        }

        changed
    }

    pub(crate) fn apply_restore(
        &mut self,
        key: AccountKey,
        restore: RestoreId,
        outcome: RestoreOutcome,
        settings: &mut LauncherSettings,
    ) -> bool {
        if self.restores.get(&key) != Some(&restore) {
            return false;
        }

        let (session, changed) = match outcome {
            RestoreOutcome::Valid(session) => {
                // The nickname may have changed since the row was saved.
                let changed = settings.account(key).is_some_and(|account| account.nickname != session.nickname)
                    && settings.upsert_account(key, session.nickname.clone());
                let session = AccountSession::SignedIn { session: Arc::new(session), storage: SessionStorage::Keyring };
                (session, changed)
            }
            RestoreOutcome::Expired => (AccountSession::Expired, false),
            RestoreOutcome::Missing => (AccountSession::SignedOut, false),
            RestoreOutcome::StoreUnavailable => {
                self.keyring = KeyringState::Unavailable;
                (AccountSession::StoreUnavailable, false)
            }
        };
        self.sessions.insert(key, session);

        changed
    }

    pub(crate) fn apply_login(&mut self, msg: LoginMsg, settings: &mut LauncherSettings) -> bool {
        self.login.in_progress = false;
        match msg {
            LoginMsg::Success { session, storage, no_store } => {
                if no_store {
                    self.keyring = KeyringState::Unavailable;
                } else if storage == SessionStorage::Keyring {
                    self.keyring = KeyringState::Available;
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
            LoginMsg::TwoFactorRetry(error) => {
                self.login.error = Some(error);
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
        env.runtime.spawn(async move {
            let client = wgc_auth::AuthClient::new(realm, tracking_id);
            let result = client.login(wgc_auth::AccountCredentials { username, password }).await;
            sender.send(Event::Login(login_result(result).await));
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
        env.runtime.spawn(async move {
            let client = wgc_auth::AuthClient::new(realm, tracking_id);
            let result = client.login_with_two_factor(credentials, twofactor_token, code).await;
            sender.send(Event::Login(login_result(result).await));
        });

        changed
    }

    /// Ends the account's session: locally at once, then a server-side revoke (best effort) and
    /// the keyring entry's removal. The account stays listed.
    pub fn sign_out(&mut self, key: AccountKey, settings: &mut LauncherSettings, env: &LauncherEnv) {
        let _ = self.bump_restore(key);
        let previous = self.sessions.insert(key, AccountSession::SignedOut);
        let session = match previous {
            Some(AccountSession::SignedIn { session, .. }) => Some(session),
            _ => None,
        };
        let (tracking_id, _) = settings.ensure_tracking_id();
        env.runtime.spawn(async move {
            if let Some(session) = session {
                let client = wgc_auth::AuthClient::new(key.realm, tracking_id);
                if let Err(e) = client.logout(&session.tokens).await {
                    warn!("server-side session revoke failed (signing out locally anyway): {e}");
                }
            }
            let deleted =
                tokio::task::spawn_blocking(move || wgc_auth::SessionStore::new().delete(key.realm, key.account_id))
                    .await;
            if let Ok(Err(e)) = deleted {
                warn!("could not delete the stored session from the keyring: {e}");
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
}

/// Looks up an entry no account has: a store that answers at all is there.
fn probe_keyring() -> KeyringState {
    let probe = wgc_auth::SessionStore::new().load(wgc_auth::AccountRealm::Eu, wgc_auth::AccountId::new(0));
    match probe {
        Err(wgc_auth::AuthError::NoSecretStore) => KeyringState::Unavailable,
        Ok(_) | Err(_) => KeyringState::Available,
    }
}

/// Loads `key`'s session from the keyring and asks WGNI whether it still holds.
async fn restore_session(key: AccountKey, tracking_id: wgc_core::TrackingId) -> RestoreOutcome {
    let loaded =
        tokio::task::spawn_blocking(move || wgc_auth::SessionStore::new().load(key.realm, key.account_id)).await;
    let session = match loaded {
        Ok(Ok(session)) => session,
        Ok(Err(wgc_auth::AuthError::NoStoredSession)) => return RestoreOutcome::Missing,
        Ok(Err(wgc_auth::AuthError::NoSecretStore)) => return RestoreOutcome::StoreUnavailable,
        Ok(Err(e)) => {
            warn!("could not load a stored session from the keyring: {e}");
            return RestoreOutcome::Missing;
        }
        Err(e) => {
            warn!("keyring load task did not complete: {e}");
            return RestoreOutcome::Missing;
        }
    };

    if let wgc_auth::TokenExpiry::KnownAt(expires_at) = session.expiry
        && expires_at <= SystemTime::now()
    {
        return RestoreOutcome::Expired;
    }

    let client = wgc_auth::AuthClient::new(key.realm, tracking_id);
    match client.verify(&session.tokens).await {
        Ok((_, nickname)) => RestoreOutcome::Valid(wgc_auth::StoredSession { nickname, ..session }),
        Err(report) => match report.current_context() {
            wgc_auth::AuthError::SessionExpired => RestoreOutcome::Expired,
            // Offline or WGNI unreachable: the session may well hold, and Play finds out.
            _ => {
                warn!("could not verify a stored session; keeping it: {report}");
                RestoreOutcome::Valid(session)
            }
        },
    }
}

/// Turns a login result into the message for it. On success the session is first saved to the
/// OS keyring (sync, so on a blocking-pool thread).
async fn login_result(result: rootcause::Result<wgc_auth::StoredSession, wgc_auth::AuthError>) -> LoginMsg {
    let report = match result {
        Ok(session) => {
            let saved = tokio::task::spawn_blocking(move || {
                let persisted = wgc_auth::SessionStore::new().save(&session);
                (session, persisted)
            })
            .await;
            return match saved {
                Ok((session, Ok(()))) => {
                    LoginMsg::Success { session: Box::new(session), storage: SessionStorage::Keyring, no_store: false }
                }
                Ok((session, Err(wgc_auth::AuthError::NoSecretStore))) => LoginMsg::Success {
                    session: Box::new(session),
                    storage: SessionStorage::MemoryOnly,
                    no_store: true,
                },
                Ok((session, Err(e))) => {
                    warn!("could not save session to keyring: {e}");
                    LoginMsg::Success {
                        session: Box::new(session),
                        storage: SessionStorage::MemoryOnly,
                        no_store: false,
                    }
                }
                Err(join_err) => {
                    warn!("keyring save task did not complete: {join_err}");
                    LoginMsg::Failed("signed in, but could not save the session; sign in again next launch".to_string())
                }
            };
        }
        Err(report) => report,
    };
    match report.current_context() {
        wgc_auth::AuthError::TwoFactorRequired { twofactor_token } => {
            LoginMsg::TwoFactorRequired { twofactor_token: twofactor_token.clone() }
        }
        wgc_auth::AuthError::CaptchaRequired => LoginMsg::CaptchaRequired,
        // A wrong code keeps the challenge open; a lockout or ban does not.
        error @ wgc_auth::AuthError::TwoFactorInvalidCode => LoginMsg::TwoFactorRetry(error.to_string()),
        // The full report, so attached diagnostic context reaches the user; none of these error
        // paths carry a password or token.
        _ => LoginMsg::Failed(report.to_string()),
    }
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

    fn env(runtime: &tokio::runtime::Runtime) -> LauncherEnv {
        LauncherEnv { runtime: runtime.handle().clone(), proxy_url: None, wake: Arc::new(|| {}) }
    }

    fn signed_in(session: wgc_auth::StoredSession) -> LauncherEvent {
        LauncherEvent(Event::Login(LoginMsg::Success {
            session: Box::new(session),
            storage: SessionStorage::Keyring,
            no_store: false,
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

        let valid = RestoreOutcome::Valid(session(eu, "renamed"));
        assert!(launcher.apply(
            LauncherEvent(Event::SessionRestored { key: eu, restore, outcome: valid }),
            &mut settings,
            &env
        ));
        assert_eq!(settings.account(eu).unwrap().nickname.as_str(), "renamed");
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
        assert_eq!(launcher.account_status(eu), AccountStatus::SignedOut);
    }
}

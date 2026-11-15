use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::info;
use tracing::warn;
use wgc_auth::AccountId;
use wgc_auth::AccountRealm;
use wgc_auth::AuthClient;
use wgc_auth::AuthError;
use wgc_auth::GameTitle;
use wgc_auth::GameToken2;
use wgc_auth::GameTokenStore;
use wgc_auth::StoredSession;

use crate::responder::BoxFuture;
use crate::responder::GameLanguage;
use crate::responder::IpcResponder;
use crate::responder::LoginInfo;
use crate::responder::Token1Failure;
use crate::responder::Token1Grant;
use crate::responder::Token2Grant;

/// What the game did with the account, for the launcher's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountEvent {
    /// The game handed over a new token2 for the session's account.
    Token2Saved,
    /// WGNI rejected the session token while minting token1; the account
    /// must sign in again.
    SessionRejected,
}

pub type AccountObserver = Arc<dyn Fn(AccountEvent) + Send + Sync>;

/// Answers for one signed-in account: its realm and identity from the stored
/// session, token1 minted through `client`, and token2 kept in the OS keyring.
pub struct AccountResponder {
    session: Arc<StoredSession>,
    client: AuthClient,
    title: GameTitle,
    language: GameLanguage,
    observer: AccountObserver,

    /// Held across keyring calls so rotations persist in the order the game
    /// sent them.
    token2: Mutex<Token2Cache>,
}

/// The account's token2 as this launch knows it. It also serves
/// LoadSpaToken2 when the keyring cannot store it.
enum Token2Cache {
    /// The keyring has not been read yet.
    Unloaded,
    /// None stored, or the game discarded it.
    Absent,
    Present(GameToken2),
}

impl AccountResponder {
    /// `client` must target the session's realm.
    pub fn new(
        session: Arc<StoredSession>,
        client: AuthClient,
        language: GameLanguage,
        observer: AccountObserver,
    ) -> Self {
        let title = GameTitle::wows(session.realm);

        Self { session, client, title, language, observer, token2: Mutex::new(Token2Cache::Unloaded) }
    }

    fn realm(&self) -> AccountRealm {
        self.session.realm
    }

    fn account_id(&self) -> AccountId {
        self.session.account_id
    }
}

impl IpcResponder for AccountResponder {
    fn client_realm(&self) -> AccountRealm {
        self.realm()
    }

    fn login_info(&self) -> Option<LoginInfo> {
        Some(LoginInfo { realm: self.realm(), nickname: self.session.nickname.clone(), spa_id: self.account_id() })
    }

    fn game_language(&self) -> GameLanguage {
        self.language.clone()
    }

    fn query_token1(&self) -> BoxFuture<'_, Result<Token1Grant, Token1Failure>> {
        Box::pin(async move {
            match self.client.create_game_token1(&self.session, &self.title).await {
                Ok(token1) => {
                    info!(account = self.account_id().get(), "minted token1 for the game");
                    Ok(Token1Grant { spa_id: self.account_id(), token1 })
                }
                Err(report) => {
                    warn!(error = %report, "token1 mint failed");
                    match report.current_context() {
                        AuthError::SessionExpired => {
                            (self.observer)(AccountEvent::SessionRejected);
                            Err(Token1Failure::NoUser)
                        }
                        _ => Err(Token1Failure::Network),
                    }
                }
            }
        })
    }

    fn load_token2(&self) -> BoxFuture<'_, Option<Token2Grant>> {
        let (realm, account_id) = (self.realm(), self.account_id());
        Box::pin(async move {
            let mut cache = self.token2.lock().await;
            match &*cache {
                Token2Cache::Present(token2) => {
                    return Some(Token2Grant { spa_id: account_id, token2: token2.clone() });
                }
                Token2Cache::Absent => return None,
                Token2Cache::Unloaded => {}
            }

            // The keyring client blocks on D-Bus.
            let loaded = tokio::task::spawn_blocking(move || GameTokenStore::new().load(realm, account_id)).await;
            match loaded {
                Ok(Ok(Some(token2))) => {
                    *cache = Token2Cache::Present(token2.clone());
                    Some(Token2Grant { spa_id: account_id, token2 })
                }
                Ok(Ok(None)) => {
                    *cache = Token2Cache::Absent;
                    None
                }
                Ok(Err(err)) => {
                    warn!(%err, "cannot read the stored token2");
                    None
                }
                Err(err) => {
                    warn!(%err, "token2 load task failed");
                    None
                }
            }
        })
    }

    fn save_token2(&self, spa_id: AccountId, token2: GameToken2) -> BoxFuture<'_, ()> {
        let realm = self.realm();
        let expected = self.account_id();
        Box::pin(async move {
            if spa_id != expected && !(spa_id.get() == 0 && token2.expose_secret().is_empty()) {
                warn!(spa_id = spa_id.get(), expected = expected.get(), "game saved a token2 for another account");
                return;
            }

            let mut cache = self.token2.lock().await;

            // After a login without a usable token the client saves an empty
            // token2 for account 0, discarding the one it had.
            if token2.expose_secret().is_empty() {
                *cache = Token2Cache::Absent;
                let deleted = tokio::task::spawn_blocking(move || GameTokenStore::new().delete(realm, expected)).await;
                match deleted {
                    Ok(Ok(())) => info!(account = expected.get(), "discarded the stored token2"),
                    Ok(Err(err)) => warn!(%err, "cannot discard the stored token2"),
                    Err(err) => warn!(%err, "token2 delete task failed"),
                }
                return;
            }

            *cache = Token2Cache::Present(token2.clone());
            (self.observer)(AccountEvent::Token2Saved);
            let saved = tokio::task::spawn_blocking(move || GameTokenStore::new().save(realm, spa_id, &token2)).await;
            match saved {
                Ok(Ok(())) => info!(account = spa_id.get(), "stored the game's token2"),
                Ok(Err(err)) => warn!(%err, "cannot store the game's token2, keeping it for this launch only"),
                Err(err) => warn!(%err, "token2 save task failed"),
            }
        })
    }
}

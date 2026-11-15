use std::future::Future;
use std::pin::Pin;

use wgc_auth::AccountId;
use wgc_auth::AccountRealm;
use wgc_auth::GameToken1;
use wgc_auth::GameToken2;
use wgc_auth::Nickname;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The account-specific answers. The server answers the capability queries
/// (API version, overlay, install state) itself.
pub trait IpcResponder: Send + Sync + 'static {
    /// Selects the login servers through `res/scripts_config.xml`.
    fn client_realm(&self) -> AccountRealm;

    /// `None` when no account is signed in.
    fn login_info(&self) -> Option<LoginInfo>;

    fn game_language(&self) -> GameLanguage;

    /// Mints a one-time token1 for the selected account. Called only when the
    /// client has no usable token2, because token1 is short-lived.
    fn query_token1(&self) -> BoxFuture<'_, Result<Token1Grant, Token1Failure>>;

    /// The stored token2 of the selected account. The client sends no usable
    /// account id with this request.
    fn load_token2(&self) -> BoxFuture<'_, Option<Token2Grant>>;

    /// Stores the token2 the login server issued. The client rotates it on
    /// every login. WGC reports success regardless, so failures are the
    /// responder's to log.
    fn save_token2(&self, spa_id: AccountId, token2: GameToken2) -> BoxFuture<'_, ()>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginInfo {
    pub realm: AccountRealm,
    pub nickname: Nickname,
    pub spa_id: AccountId,
}

/// A client language code such as `en`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameLanguage(String);

impl GameLanguage {
    pub fn new(code: impl Into<String>) -> GameLanguage {
        GameLanguage(code.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
pub struct Token1Grant {
    pub spa_id: AccountId,
    pub token1: GameToken1,
}

#[derive(Debug)]
pub struct Token2Grant {
    pub spa_id: AccountId,
    pub token2: GameToken2,
}

/// Why no token1 was issued, in the terms the client distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token1Failure {
    Cancelled,
    Network,
    NoUser,
}

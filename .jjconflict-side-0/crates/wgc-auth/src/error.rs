//! Errors from WGNI authentication.
//!
//! `client.rs`'s fallible functions return `rootcause::Result<T, AuthError>`:
//! the `AuthError` here is the classified error kind (a report's
//! `current_context()`), and diagnostic detail -- what the client was doing,
//! or the raw [`TokenRejection`] a classification was made from -- is
//! attached to the `rootcause::Report` rather than folded into a string
//! field. `store.rs`'s keyring-facing functions are synchronous and return
//! plain `Result<T, AuthError>`.
//!
//! `Rejected` is the catch-all for a WGNI token-endpoint error this crate
//! does not recognize; it always carries the full [`TokenRejection`] so no
//! upstream detail is lost, unlike a stringly-matched description.

use thiserror::Error;

/// A WGNI OAuth token-endpoint error body, verbatim.
///
/// `error` is WG's OAuth machine code (RFC 6749 `error` field); `error_description`
/// is WG's human text. Both are captured unconditionally so a caller never
/// needs to re-parse a `Display` string to recover them.
#[derive(Debug, Clone)]
pub struct TokenRejection {
    pub status: u16,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

impl std::fmt::Display for TokenRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}", self.status)?;
        if let Some(error) = &self.error {
            write!(f, "; error={error}")?;
        }
        if let Some(description) = &self.error_description {
            write!(f, "; error_description={description:?}")?;
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("network request to {url} failed: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("WGNI rejected the request: {0}")]
    Rejected(TokenRejection),

    #[error("invalid username or password")]
    InvalidCredentials,

    #[error("account not found")]
    AccountNotFound,

    #[error("account is banned")]
    AccountBanned,

    #[error("two-factor authentication required")]
    TwoFactorRequired { twofactor_token: String },

    #[error("two-factor code was invalid")]
    TwoFactorInvalidCode,

    #[error("two-factor login is locked")]
    TwoFactorLoginLocked,

    #[error("account was banned during two-factor login")]
    TwoFactorBanned,

    #[error("this login requires solving a CAPTCHA, which wgc-auth does not support")]
    CaptchaRequired,

    #[error(
        "login is awaiting a second factor; approve it in Wargaming Auth or via the emailed code, then sign in again"
    )]
    VerificationPending,

    #[error("proof-of-work challenge could not be solved before timeout")]
    ProofOfWorkTimeout,

    #[error("proof-of-work solver task did not complete: {source}")]
    TaskJoin {
        #[source]
        source: tokio::task::JoinError,
    },

    #[error("no session found for this account/realm")]
    NoStoredSession,

    #[error("stored session token was rejected by the server; re-authentication required")]
    SessionExpired,

    #[error("keyring access failed: {source}")]
    Keyring {
        #[source]
        source: keyring::Error,
    },

    #[error("no OS secret store is available; session will not persist across restarts")]
    NoSecretStore,

    #[error("could not serialize session for storage: {source}")]
    SerializeSession {
        #[source]
        source: serde_json::Error,
    },

    #[error("could not parse stored session: {source}")]
    DeserializeSession {
        #[source]
        source: serde_json::Error,
    },

    #[error("could not parse response from {url}: {source}")]
    MalformedResponse {
        url: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("unexpected response from WGNI: {detail}")]
    BadResponse { detail: String },
}

//! Errors from WGNI authentication (WGNI-protocol.md section 8,
//! design-draft.md section 4).
//!
//! `Rejected` is the only variant that carries a raw string: it is the
//! catch-all for an upstream `error_description` this crate does not
//! recognize. Every *known* WGNI error taxon gets its own variant so callers
//! never need to match on `Display`/a string to make a decision.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("network request to {url} failed: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("WGNI rejected the request: {description}")]
    Rejected { description: String },

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

    #[error("proof-of-work challenge could not be solved before timeout")]
    ProofOfWorkTimeout,

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

    #[error("unexpected response from WGNI: {detail}")]
    BadResponse { detail: String },
}

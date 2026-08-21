//! Secret-bearing token/credential newtypes.
//!
//! `SecretPassword` and `AccessToken` never derive `Debug`, `Serialize`, or
//! `Clone`: a hand-written `Debug` redacts the value, there is no persist
//! path in this phase, and copies of secret material should be explicit,
//! not implicit. `Drop` zeroizes the backing buffer.

use std::fmt;
use std::time::SystemTime;

use zeroize::Zeroize;
use zeroize::ZeroizeOnDrop;

/// A plaintext account password, held only long enough to send over HTTPS.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SecretPassword(String);

impl SecretPassword {
    pub fn new(password: String) -> SecretPassword {
        SecretPassword(password)
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SecretPassword").field(&"<redacted>").finish()
    }
}

/// The WGNI account-session bearer token.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct AccessToken(String);

impl AccessToken {
    pub fn new(token: String) -> AccessToken {
        AccessToken(token)
    }

    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("AccessToken").field(&"<redacted>").finish()
    }
}

/// Client-generated nonce paired with an `AccessToken` for the
/// `Bearer <access_token>:<exchange_code>` header. Not secret-strength (it never authenticates on its own),
/// so a plain `Debug` is fine.
///
/// Generation (32 chars from `[0-9A-F]`) belongs to the client phase, which
/// owns the rng dependency; this phase only offers a pure constructor over
/// an already-generated string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeCode(String);

impl ExchangeCode {
    pub fn from_raw(code: String) -> ExchangeCode {
        ExchangeCode(code)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The `AccessToken`/`ExchangeCode` pair the `Bearer` header requires
/// together -- makes "have a token but forgot the exchange code"
/// unrepresentable.
#[derive(Debug)]
pub struct TokenPair {
    pub access_token: AccessToken,
    pub exchange_code: ExchangeCode,
}

/// Whether a stored `AccessToken` is known to still be valid.
///
/// WGNI has no refresh-token grant: expiry is
/// otherwise only detectable via a failing authenticated call. `Unknown`
/// models "no `expires_in` observed yet / not verified this run" -- never a
/// sentinel timestamp like zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenExpiry {
    Unknown,
    KnownAt(SystemTime),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_password_debug_is_redacted() {
        let secret = SecretPassword::new("hunter2".to_string());
        let debug = format!("{secret:?}");
        assert!(!debug.contains("hunter2"), "debug output leaked the password: {debug}");
        assert!(debug.contains("redacted"), "debug output missing redaction marker: {debug}");
    }

    #[test]
    fn access_token_debug_is_redacted() {
        let token = AccessToken::new("super-secret-token".to_string());
        let debug = format!("{token:?}");
        assert!(!debug.contains("super-secret-token"), "debug output leaked the token: {debug}");
        assert!(debug.contains("redacted"), "debug output missing redaction marker: {debug}");
    }

    #[test]
    fn token_pair_debug_is_redacted() {
        let pair = TokenPair {
            access_token: AccessToken::new("super-secret-token".to_string()),
            exchange_code: ExchangeCode::from_raw("DEADBEEF".to_string()),
        };
        let debug = format!("{pair:?}");
        assert!(!debug.contains("super-secret-token"), "debug output leaked the token: {debug}");
        assert!(debug.contains("redacted"), "debug output missing redaction marker: {debug}");
    }

    #[test]
    fn secret_password_expose_secret_roundtrips() {
        let secret = SecretPassword::new("hunter2".to_string());
        assert_eq!(secret.expose_secret(), "hunter2");
    }

    #[test]
    fn access_token_expose_secret_roundtrips() {
        let token = AccessToken::new("super-secret-token".to_string());
        assert_eq!(token.expose_secret(), "super-secret-token");
    }

    // Compile-time assertion that Drop zeroizes: ZeroizeOnDrop's derive only
    // compiles if the type also implements Zeroize (derived above). If
    // either derive were missing or misapplied this module would not build.
    fn _assert_zeroize_on_drop<T: ZeroizeOnDrop>() {}
    const _: fn() = || {
        _assert_zeroize_on_drop::<SecretPassword>();
        _assert_zeroize_on_drop::<AccessToken>();
    };
}

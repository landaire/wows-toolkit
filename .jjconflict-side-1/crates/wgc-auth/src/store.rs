//! Keyring-backed persistence for `StoredSession`.
//!
//! `SessionStore` is the only place in this crate that touches the
//! `keyring` crate; `AuthClient` never persists anything itself.

use std::time::Duration;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use serde::Serialize;
use zeroize::Zeroize;
use zeroize::ZeroizeOnDrop;

use crate::error::AuthError;
use crate::identity::AccountId;
use crate::identity::Nickname;
use crate::realm::AccountRealm;
use crate::session::StoredSession;
use crate::token::AccessToken;
use crate::token::ExchangeCode;
use crate::token::TokenExpiry;
use crate::token::TokenPair;

const KEYRING_SERVICE: &str = "wows-toolkit-wgc";

pub struct SessionStore;

impl SessionStore {
    pub fn new() -> SessionStore {
        SessionStore
    }

    /// Errs with `NoSecretStore` on platforms with no OS secret store
    /// (Linux without a Secret Service); the caller falls back to
    /// session-only, in-memory storage (locked decision: no plaintext
    /// file fallback).
    pub fn save(&self, session: &StoredSession) -> Result<(), AuthError> {
        let entry = self.entry(session.realm, session.account_id)?;
        // `blob` is ZeroizeOnDrop, so its transient plaintext token copies are
        // wiped when it drops -- including on the serialize `?` early return.
        let blob = StoredSessionBlob::from_session(session);
        let mut json = serde_json::to_string(&blob).map_err(|source| AuthError::SerializeSession { source })?;
        let result = entry.set_password(&json).map_err(map_keyring_error);
        json.zeroize();
        result
    }

    /// `NoStoredSession` if no entry exists for this realm/account.
    pub fn load(&self, realm: AccountRealm, account_id: AccountId) -> Result<StoredSession, AuthError> {
        let entry = self.entry(realm, account_id)?;
        let mut json = match entry.get_password() {
            Ok(json) => json,
            Err(keyring::Error::NoEntry) => return Err(AuthError::NoStoredSession),
            Err(source) => return Err(map_keyring_error(source)),
        };
        let blob: Result<StoredSessionBlob, _> = serde_json::from_str(&json);
        json.zeroize();
        let blob = blob.map_err(|source| AuthError::DeserializeSession { source })?;
        Ok(blob.into_session(realm, account_id))
    }

    pub fn delete(&self, realm: AccountRealm, account_id: AccountId) -> Result<(), AuthError> {
        let entry = self.entry(realm, account_id)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(source) => Err(map_keyring_error(source)),
        }
    }

    fn entry(&self, realm: AccountRealm, account_id: AccountId) -> Result<keyring::Entry, AuthError> {
        let username = format!("{}:{}", realm_key(realm), account_id.get());
        keyring::Entry::new(KEYRING_SERVICE, &username).map_err(map_keyring_error)
    }
}

impl Default for SessionStore {
    fn default() -> SessionStore {
        SessionStore::new()
    }
}

fn realm_key(realm: AccountRealm) -> &'static str {
    match realm {
        AccountRealm::Eu => "eu",
        AccountRealm::Na => "na",
        AccountRealm::Asia => "asia",
        AccountRealm::Ru => "ru",
    }
}

/// Platforms/sessions with no OS secret store (Linux without a Secret
/// Service) surface as `NoStorageAccess`/`PlatformFailure`; per the locked
/// "session-only, no plaintext fallback" decision these map to
/// `NoSecretStore` so the caller can fall back to an in-memory session.
fn map_keyring_error(source: keyring::Error) -> AuthError {
    match source {
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => AuthError::NoSecretStore,
        other => AuthError::Keyring { source: other },
    }
}

/// Separate serializable copy of `StoredSession`'s secret fields: A1's
/// `AccessToken` deliberately has no `Serialize` (it zeroizes on drop). This
/// blob is the JSON value stored as the keyring secret.
#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
struct StoredSessionBlob {
    access_token: String,
    exchange_code: String,
    #[zeroize(skip)]
    nickname: String,
    #[zeroize(skip)]
    expiry: BlobExpiry,
}

#[derive(Serialize, Deserialize)]
enum BlobExpiry {
    Unknown,
    KnownAt { unix_secs: u64 },
}

impl StoredSessionBlob {
    fn from_session(session: &StoredSession) -> StoredSessionBlob {
        StoredSessionBlob {
            access_token: session.tokens.access_token.expose_secret().to_string(),
            exchange_code: session.tokens.exchange_code.as_str().to_string(),
            nickname: session.nickname.as_str().to_string(),
            expiry: match session.expiry {
                TokenExpiry::Unknown => BlobExpiry::Unknown,
                TokenExpiry::KnownAt(t) => BlobExpiry::KnownAt { unix_secs: crate::session::unix_secs(t) },
            },
        }
    }

    // `mut self` + take/replace rather than field moves: StoredSessionBlob is
    // ZeroizeOnDrop (has a Drop impl), so its fields cannot be moved out. The
    // token strings move into the zeroizing AccessToken/ExchangeCode; the now
    // empty blob drops harmlessly.
    fn into_session(mut self, realm: AccountRealm, account_id: AccountId) -> StoredSession {
        let expiry = match std::mem::replace(&mut self.expiry, BlobExpiry::Unknown) {
            BlobExpiry::Unknown => TokenExpiry::Unknown,
            BlobExpiry::KnownAt { unix_secs } => TokenExpiry::KnownAt(UNIX_EPOCH + Duration::from_secs(unix_secs)),
        };
        StoredSession {
            realm,
            account_id,
            nickname: Nickname::new(std::mem::take(&mut self.nickname)),
            tokens: TokenPair {
                access_token: AccessToken::new(std::mem::take(&mut self.access_token)),
                exchange_code: ExchangeCode::from_raw(std::mem::take(&mut self.exchange_code)),
            },
            expiry,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;

    #[test]
    fn blob_roundtrips_unknown_expiry() {
        let session = StoredSession {
            realm: AccountRealm::Eu,
            account_id: AccountId::new(697_769_785),
            nickname: Nickname::new("TestPlayer"),
            tokens: TokenPair {
                access_token: AccessToken::new("secret-token".to_string()),
                exchange_code: ExchangeCode::from_raw("DEADBEEF00112233445566778899AABB".to_string()),
            },
            expiry: TokenExpiry::Unknown,
        };
        let blob = StoredSessionBlob::from_session(&session);
        let json = serde_json::to_string(&blob).expect("serializes");
        let restored: StoredSessionBlob = serde_json::from_str(&json).expect("parses");
        let restored_session = restored.into_session(session.realm, session.account_id);
        assert_eq!(restored_session.tokens.access_token.expose_secret(), "secret-token");
        assert_eq!(restored_session.tokens.exchange_code.as_str(), session.tokens.exchange_code.as_str());
        assert_eq!(restored_session.nickname.as_str(), "TestPlayer");
        assert!(matches!(restored_session.expiry, TokenExpiry::Unknown));
    }

    #[test]
    fn blob_roundtrips_known_expiry_to_the_same_unix_second() {
        let expires_at = SystemTime::now() + Duration::from_secs(3600);
        let session = StoredSession {
            realm: AccountRealm::Na,
            account_id: AccountId::new(1_000_000_001),
            nickname: Nickname::new("Other"),
            tokens: TokenPair {
                access_token: AccessToken::new("tok2".to_string()),
                exchange_code: ExchangeCode::from_raw("AABBCCDD".to_string()),
            },
            expiry: TokenExpiry::KnownAt(expires_at),
        };
        let blob = StoredSessionBlob::from_session(&session);
        let json = serde_json::to_string(&blob).expect("serializes");
        let restored: StoredSessionBlob = serde_json::from_str(&json).expect("parses");
        let restored_session = restored.into_session(session.realm, session.account_id);
        match restored_session.expiry {
            TokenExpiry::KnownAt(t) => {
                assert_eq!(
                    t.duration_since(UNIX_EPOCH).unwrap().as_secs(),
                    expires_at.duration_since(UNIX_EPOCH).unwrap().as_secs()
                );
            }
            TokenExpiry::Unknown => panic!("expected KnownAt"),
        }
    }

    // Keyring-touching: CI/headless environments have no Secret Service.
    // Run manually on a real desktop session.
    #[test]
    #[ignore = "touches the OS keyring / Secret Service"]
    fn save_load_delete_roundtrip_via_os_keyring() {
        let store = SessionStore::new();
        let session = StoredSession {
            realm: AccountRealm::Eu,
            account_id: AccountId::new(999_999_001),
            nickname: Nickname::new("KeyringTest"),
            tokens: TokenPair {
                access_token: AccessToken::new("keyring-test-token".to_string()),
                exchange_code: ExchangeCode::from_raw("00112233445566778899AABBCCDDEEFF".to_string()),
            },
            expiry: TokenExpiry::Unknown,
        };
        store.save(&session).expect("save");
        let loaded = store.load(session.realm, session.account_id).expect("load");
        assert_eq!(loaded.nickname.as_str(), "KeyringTest");
        store.delete(session.realm, session.account_id).expect("delete");
        assert!(matches!(store.load(session.realm, session.account_id), Err(AuthError::NoStoredSession)));
    }
}

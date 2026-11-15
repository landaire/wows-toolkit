//! Persistence for `StoredSession` and game token2: the OS secret store first, and an
//! unencrypted file in the app's data directory only when there is no secret store at all.
//!
//! This module is the only place in this crate that persists anything; `AuthClient` never
//! does.

mod os_keyring;
mod plaintext;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use std::time::UNIX_EPOCH;

use serde::Deserialize;
use serde::Serialize;
use zeroize::Zeroize;
use zeroize::ZeroizeOnDrop;

use crate::error::AuthError;
use crate::game_token::GameToken2;
use crate::identity::AccountId;
use crate::identity::Nickname;
use crate::realm::AccountRealm;
use crate::session::StoredSession;
use crate::token::AccessToken;
use crate::token::ExchangeCode;
use crate::token::TokenExpiry;
use crate::token::TokenPair;

pub use os_keyring::OsKeyring;
pub use plaintext::PlaintextFile;

/// Where a stored entry lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StorageBackend {
    /// The OS secret store: a Secret Service provider, the macOS Keychain or the Windows
    /// Credential Manager.
    Keyring,
    /// [`PlaintextFile`], used only while no secret store exists.
    PlaintextFile,
}

/// A loaded value and the backend that held it.
#[derive(Debug)]
pub struct Stored<T> {
    pub value: T,
    pub backend: StorageBackend,
}

/// The OS secret store as [`CredentialStore`] uses it. A trait so tests can stand in for it.
///
/// `NoSecretStore` means there is no store at all; `KeyringLocked` means there is one but it
/// stayed locked, for example because its unlock prompt was dismissed.
pub trait SecretStore: Send + Sync {
    /// `None` when no entry exists.
    fn get(&self, entry: &KeyringEntry) -> Result<Option<String>, AuthError>;

    /// Creates or replaces the entry, with the entry's label and attributes.
    fn set(&self, entry: &KeyringEntry, secret: &str) -> Result<(), AuthError>;

    /// Succeeds when no entry exists.
    fn delete(&self, entry: &KeyringEntry) -> Result<(), AuthError>;
}

/// What a keyring entry is looked up by and what a keyring browser shows for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyringEntry {
    /// The lookup key under the toolkit's service name. Unchanged from earlier releases, so
    /// their entries are still found.
    pub user: String,
    pub label: String,
}

/// Sessions and game token2s, kept in the OS secret store, or in [`PlaintextFile`] when there
/// is none. A cheap `Clone`.
#[derive(Clone)]
pub struct CredentialStore {
    keyring: Arc<dyn SecretStore>,
    /// `None` without an app data directory: then nothing persists without a secret store.
    plaintext: Option<PlaintextFile>,
}

impl CredentialStore {
    /// The OS secret store, with the plaintext fallback under `data_dir`.
    pub fn new(data_dir: Option<&Path>) -> CredentialStore {
        CredentialStore::with_backends(Arc::new(OsKeyring), data_dir.map(PlaintextFile::in_dir))
    }

    pub fn with_backends(keyring: Arc<dyn SecretStore>, plaintext: Option<PlaintextFile>) -> CredentialStore {
        CredentialStore { keyring, plaintext }
    }

    /// The backend a save would use now. Asks the secret store for an entry no account has,
    /// which needs no unlock.
    pub fn probe(&self) -> Result<StorageBackend, AuthError> {
        let probe = KeyringEntry { user: "probe:0".to_string(), label: "WoWs Toolkit".to_string() };
        match self.keyring.get(&probe) {
            Ok(_) => Ok(StorageBackend::Keyring),
            Err(AuthError::NoSecretStore) if self.plaintext.is_some() => Ok(StorageBackend::PlaintextFile),
            Err(err) => Err(err),
        }
    }

    pub fn save_session(&self, session: &StoredSession) -> Result<StorageBackend, AuthError> {
        let account = AccountRef { realm: session.realm, account_id: session.account_id };
        // `blob` is ZeroizeOnDrop, so its transient plaintext token copies are wiped when it
        // drops, including on the serialize `?` early return.
        let blob = StoredSessionBlob::from_session(session);
        let mut json = serde_json::to_string(&blob).map_err(|source| AuthError::SerializeSession { source })?;
        let saved = self.save(Kind::Session, account, Some(&session.nickname), &json);
        json.zeroize();

        saved
    }

    pub fn load_session(
        &self,
        realm: AccountRealm,
        account_id: AccountId,
    ) -> Result<Option<Stored<StoredSession>>, AuthError> {
        let Some(Stored { value: mut json, backend }) = self.load(Kind::Session, AccountRef { realm, account_id })?
        else {
            return Ok(None);
        };
        let blob: Result<StoredSessionBlob, _> = serde_json::from_str(&json);
        json.zeroize();
        let session = blob.map_err(|source| AuthError::DeserializeSession { source })?.into_session(realm, account_id);

        Ok(Some(Stored { value: session, backend }))
    }

    /// Removes the session from whichever backend holds it.
    pub fn delete_session(&self, realm: AccountRealm, account_id: AccountId) -> Result<(), AuthError> {
        self.delete(Kind::Session, AccountRef { realm, account_id })
    }

    pub fn save_token2(
        &self,
        realm: AccountRealm,
        account_id: AccountId,
        nickname: &Nickname,
        token: &GameToken2,
    ) -> Result<StorageBackend, AuthError> {
        self.save(Kind::Token2, AccountRef { realm, account_id }, Some(nickname), token.expose_secret())
    }

    pub fn load_token2(
        &self,
        realm: AccountRealm,
        account_id: AccountId,
    ) -> Result<Option<Stored<GameToken2>>, AuthError> {
        let loaded = self.load(Kind::Token2, AccountRef { realm, account_id })?;

        Ok(loaded.map(|Stored { value, backend }| Stored { value: GameToken2::new(value), backend }))
    }

    /// Removes the token2 from whichever backend holds it.
    pub fn delete_token2(&self, realm: AccountRealm, account_id: AccountId) -> Result<(), AuthError> {
        self.delete(Kind::Token2, AccountRef { realm, account_id })
    }

    fn save(
        &self,
        kind: Kind,
        account: AccountRef,
        nickname: Option<&Nickname>,
        secret: &str,
    ) -> Result<StorageBackend, AuthError> {
        match self.keyring.set(&kind.entry(account, nickname), secret) {
            Ok(()) => {
                // The file's entry for this secret is older than the one just saved.
                if let Some(plaintext) = &self.plaintext {
                    plaintext.remove(kind, &account.key())?;
                }
                self.migrate_plaintext()?;
                Ok(StorageBackend::Keyring)
            }
            Err(AuthError::NoSecretStore) => {
                let Some(plaintext) = &self.plaintext else {
                    return Err(AuthError::NoSecretStore);
                };
                plaintext.insert(kind, &account.key(), secret)?;
                Ok(StorageBackend::PlaintextFile)
            }
            Err(err) => Err(err),
        }
    }

    fn load(&self, kind: Kind, account: AccountRef) -> Result<Option<Stored<String>>, AuthError> {
        match self.keyring.get(&kind.entry(account, None)) {
            Ok(found) => {
                // Moves an entry saved while there was no secret store over the keyring's.
                if self.migrate_plaintext()? {
                    let moved = self.keyring.get(&kind.entry(account, None))?;
                    return Ok(moved.map(|value| Stored { value, backend: StorageBackend::Keyring }));
                }
                Ok(found.map(|value| Stored { value, backend: StorageBackend::Keyring }))
            }
            Err(AuthError::NoSecretStore) => {
                let Some(plaintext) = &self.plaintext else {
                    return Ok(None);
                };
                let found = plaintext.get(kind, &account.key())?;
                Ok(found.map(|value| Stored { value, backend: StorageBackend::PlaintextFile }))
            }
            Err(err) => Err(err),
        }
    }

    fn delete(&self, kind: Kind, account: AccountRef) -> Result<(), AuthError> {
        let from_keyring = match self.keyring.delete(&kind.entry(account, None)) {
            Ok(()) | Err(AuthError::NoSecretStore) => Ok(()),
            Err(err) => Err(err),
        };
        // The file entry goes even when the keyring refused, so a sign-out never leaves
        // plaintext behind.
        let from_file = match &self.plaintext {
            Some(plaintext) => plaintext.remove(kind, &account.key()),
            None => Ok(()),
        };

        from_keyring.and(from_file)
    }

    /// Moves every plaintext entry into the keyring, which just answered, and deletes the file.
    /// Returns whether there was anything to move. An entry the keyring refuses stays in the
    /// file, so nothing is lost.
    fn migrate_plaintext(&self) -> Result<bool, AuthError> {
        let Some(plaintext) = &self.plaintext else {
            return Ok(false);
        };
        let Some(contents) = plaintext.read()? else {
            return Ok(false);
        };

        for (key, json) in contents.sessions() {
            let Some(account) = AccountRef::parse(key) else {
                continue;
            };
            let nickname = nickname_of(json);
            self.keyring.set(&Kind::Session.entry(account, nickname.as_ref()), json)?;
            plaintext.remove(Kind::Session, key)?;
        }
        for (key, token) in contents.token2s() {
            let Some(account) = AccountRef::parse(key) else {
                continue;
            };
            let nickname = contents.session(key).and_then(nickname_of);
            self.keyring.set(&Kind::Token2.entry(account, nickname.as_ref()), token)?;
            plaintext.remove(Kind::Token2, key)?;
        }

        Ok(true)
    }
}

/// Which secret an entry holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Session,
    Token2,
}

impl Kind {
    fn entry(self, account: AccountRef, nickname: Option<&Nickname>) -> KeyringEntry {
        let realm = realm_label(account.realm);
        let holder = match nickname {
            Some(nickname) => nickname.as_str().to_string(),
            None => format!("account {}", account.account_id.get()),
        };
        let (user, label) = match self {
            Kind::Session => (account.key(), format!("WoWs Toolkit: Wargaming sign-in for {holder} ({realm})")),
            // Prefixed so a token2 entry never collides with the session entry of the same
            // account under the shared service name.
            Kind::Token2 => {
                (format!("token2:{}", account.key()), format!("WoWs Toolkit: game token for {holder} ({realm})"))
            }
        };

        KeyringEntry { user, label }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AccountRef {
    realm: AccountRealm,
    account_id: AccountId,
}

impl AccountRef {
    /// `realm:account_id`, the key in both backends.
    fn key(self) -> String {
        format!("{}:{}", realm_key(self.realm), self.account_id.get())
    }

    fn parse(key: &str) -> Option<AccountRef> {
        let (realm, account_id) = key.split_once(':')?;
        let realm = match realm {
            "eu" => AccountRealm::Eu,
            "na" => AccountRealm::Na,
            "asia" => AccountRealm::Asia,
            _ => return None,
        };

        Some(AccountRef { realm, account_id: AccountId::new(account_id.parse().ok()?) })
    }
}

fn realm_key(realm: AccountRealm) -> &'static str {
    match realm {
        AccountRealm::Eu => "eu",
        AccountRealm::Na => "na",
        AccountRealm::Asia => "asia",
    }
}

fn realm_label(realm: AccountRealm) -> &'static str {
    match realm {
        AccountRealm::Eu => "EU",
        AccountRealm::Na => "NA",
        AccountRealm::Asia => "Asia",
    }
}

/// The nickname in a stored session's JSON, for the label of the entry it moves to.
fn nickname_of(json: &str) -> Option<Nickname> {
    let blob: StoredSessionBlob = serde_json::from_str(json).ok()?;

    Some(Nickname::new(blob.nickname.clone()))
}

/// Separate serializable copy of `StoredSession`'s secret fields: `AccessToken` deliberately
/// has no `Serialize` (it zeroizes on drop). This blob is the JSON value either backend
/// stores.
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
mod tests;

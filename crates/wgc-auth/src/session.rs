//! `StoredSession`, the unit kept in the keyring.

use std::time::SystemTime;

use crate::identity::AccountId;
use crate::identity::Nickname;
use crate::realm::AccountRealm;
use crate::token::TokenExpiry;
use crate::token::TokenPair;

/// The unit that goes into/out of the keyring, keyed by (realm, account_id).
#[derive(Debug)]
pub struct StoredSession {
    pub realm: AccountRealm,
    pub account_id: AccountId,
    pub nickname: Nickname,
    pub tokens: TokenPair,
    pub expiry: TokenExpiry,
}

/// A `SystemTime` before the epoch cannot be represented as a unix-seconds
/// `u64` (only happens with a badly-wrong system clock). Fails safe: reports
/// "already expired" (0) rather than silently treating an unrepresentable
/// time as valid. Used by `store.rs`'s keyring blob expiry encoding.
pub(crate) fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

//! `StoredSession` (design-draft.md section 3) and the adapter that maps it
//! to `wgc_launch::TokenStatus` (design-draft.md section 6).

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
/// time as valid. Shared with `store.rs`'s keyring blob expiry encoding.
pub(crate) fn unix_secs(t: SystemTime) -> u64 {
    t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Maps a loaded session (if any) to `wgc-launch`'s `TokenStatus` gate.
///
/// Lives here (depending on `wgc-launch`), not in `wgc-launch` itself, so
/// `wgc-launch` never depends back on `wgc-auth`.
pub fn launch_token_status(session: Option<&StoredSession>, now: SystemTime) -> wgc_launch::TokenStatus {
    let Some(session) = session else {
        return wgc_launch::TokenStatus::NotConfigured;
    };
    match session.expiry {
        TokenExpiry::KnownAt(expires_at) => {
            if expires_at > now {
                wgc_launch::TokenStatus::Valid { expires_at: unix_secs(expires_at) }
            } else {
                wgc_launch::TokenStatus::Expired
            }
        }
        // DECISION (a) optimistic-valid: WGNI has no refresh_token, so real
        // expiry is only detectable via a failing authenticated call
        // (WGNI-protocol.md section 3). A freshly-loaded, not-yet-verified
        // session is therefore treated as launchable; a dead token surfaces
        // as a LaunchError from the actual launch attempt instead. The
        // `expires_at` field carries `now`, meaning "assumed valid as of
        // load" -- the UI must read `StoredSession.expiry` for honest
        // display, NOT this value.
        TokenExpiry::Unknown => wgc_launch::TokenStatus::Valid { expires_at: unix_secs(now) },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::token::AccessToken;
    use crate::token::ExchangeCode;

    fn sample_session(expiry: TokenExpiry) -> StoredSession {
        StoredSession {
            realm: AccountRealm::Eu,
            account_id: AccountId::new(697_769_785),
            nickname: Nickname::new("test"),
            tokens: TokenPair {
                access_token: AccessToken::new("token".to_string()),
                exchange_code: ExchangeCode::from_raw("DEADBEEF".to_string()),
            },
            expiry,
        }
    }

    #[test]
    fn none_session_is_not_configured() {
        assert_eq!(launch_token_status(None, SystemTime::now()), wgc_launch::TokenStatus::NotConfigured);
    }

    #[test]
    fn future_known_expiry_is_valid() {
        let now = SystemTime::now();
        let session = sample_session(TokenExpiry::KnownAt(now + Duration::from_secs(3600)));
        let status = launch_token_status(Some(&session), now);
        assert!(matches!(status, wgc_launch::TokenStatus::Valid { .. }));
    }

    #[test]
    fn past_known_expiry_is_expired() {
        let now = SystemTime::now();
        let session = sample_session(TokenExpiry::KnownAt(now - Duration::from_secs(3600)));
        assert_eq!(launch_token_status(Some(&session), now), wgc_launch::TokenStatus::Expired);
    }

    #[test]
    fn unknown_expiry_is_optimistically_valid() {
        let now = SystemTime::now();
        let session = sample_session(TokenExpiry::Unknown);
        let status = launch_token_status(Some(&session), now);
        assert!(matches!(status, wgc_launch::TokenStatus::Valid { .. }));
    }
}

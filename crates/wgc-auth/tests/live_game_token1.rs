//! Live WoWs game token1 mint against the session stored in the OS keyring.
//!
//! Ignored by default: it reaches the real network with a real account and
//! mints one single-use token. Sign the account in through the toolkit
//! first, then run:
//!
//!   WGC_TEST_REALM=na WGC_TEST_ACCOUNT_ID=1034874630 \
//!     buck2 test //crates/wgc-auth:live_game_token1_test -- --ignored --nocapture
//!
//! `WGC_TEST_REALM` is one of: eu, na, asia. The test skips cleanly when
//! either variable is unset. It prints the token's length and alphabet, never
//! the token.

use wgc_auth::AccountId;
use wgc_auth::AccountRealm;
use wgc_auth::AuthClient;
use wgc_auth::GameTitle;
use wgc_auth::SessionStore;
use wgc_core::TrackingId;

#[tokio::test]
#[ignore = "hits the live WGNI network with a real account; see module docs"]
async fn live_game_token1_mints_for_the_stored_session() {
    let (Ok(realm), Ok(account_id)) = (std::env::var("WGC_TEST_REALM"), std::env::var("WGC_TEST_ACCOUNT_ID")) else {
        eprintln!("skipping: set WGC_TEST_REALM and WGC_TEST_ACCOUNT_ID to run");
        return;
    };
    let realm = match realm.as_str() {
        "eu" => AccountRealm::Eu,
        "na" => AccountRealm::Na,
        "asia" => AccountRealm::Asia,
        other => panic!("unknown WGC_TEST_REALM {other:?}, expected eu/na/asia"),
    };
    let account_id = AccountId::new(account_id.parse().expect("WGC_TEST_ACCOUNT_ID is a decimal SPA id"));

    let session = SessionStore::new().load(realm, account_id).expect("a stored session for this account");
    // WGNI requires a numeric tid; WGC uses a nanosecond unix timestamp.
    let tid = std::env::var("WGC_TEST_TID").unwrap_or_else(|_| "1700000000000000000".to_string());
    let tracking_id = TrackingId::parse(&tid).expect("tracking id is non-empty");
    let client = AuthClient::new(realm, tracking_id);
    let title = GameTitle::wows(realm);

    let token = client.create_game_token1(&session, &title).await.expect("token1 minted");

    let secret = token.expose_secret();
    let all_digits = secret.bytes().all(|b| b.is_ascii_digit());
    eprintln!("token1: {} chars, all digits: {all_digits}", secret.len());
    assert!(!secret.is_empty(), "token1 is empty");
}

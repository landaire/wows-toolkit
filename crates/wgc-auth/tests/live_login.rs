//! Live WGNI login integration test.
//!
//! Ignored by default: it hits the real network and a real WGNI account.
//! Run manually against a disposable test account:
//!
//!   WGC_TEST_USERNAME=... WGC_TEST_PASSWORD=... WGC_TEST_REALM=eu \
//!     cargo test -p wgc-auth --test live_login -- --ignored --nocapture
//!
//! `WGC_TEST_REALM` is one of: eu, na, asia, ru. The test skips cleanly
//! (prints a message and returns without failing) if any of the three env
//! vars is unset.

use wgc_auth::AccountCredentials;
use wgc_auth::AccountRealm;
use wgc_auth::AuthClient;
use wgc_auth::SecretPassword;
use wgc_core::TrackingId;

#[tokio::test]
#[ignore = "hits the live WGNI network with a real account; see module docs"]
async fn live_login_returns_nickname_and_account_id() {
    let (username, password, realm) =
        match (std::env::var("WGC_TEST_USERNAME"), std::env::var("WGC_TEST_PASSWORD"), std::env::var("WGC_TEST_REALM"))
        {
            (Ok(u), Ok(p), Ok(r)) => (u, p, r),
            _ => {
                eprintln!(
                    "skipping live_login_returns_nickname_and_account_id: set \
                 WGC_TEST_USERNAME/WGC_TEST_PASSWORD/WGC_TEST_REALM to run"
                );
                return;
            }
        };
    let realm = match realm.as_str() {
        "eu" => AccountRealm::Eu,
        "na" => AccountRealm::Na,
        "asia" => AccountRealm::Asia,
        "ru" => AccountRealm::Ru,
        other => panic!("unknown WGC_TEST_REALM {other:?}, expected eu/na/asia/ru"),
    };
    // WGNI requires a numeric tid (WGC uses a nanosecond unix timestamp);
    // override with WGC_TEST_TID to reuse a specific device id.
    let tid = std::env::var("WGC_TEST_TID").unwrap_or_else(|_| "1700000000000000000".to_string());
    let tracking_id = TrackingId::parse(&tid).expect("tracking id is non-empty");
    let client = AuthClient::new(realm, tracking_id);
    let credentials = AccountCredentials { username, password: SecretPassword::new(password) };

    let session = client.login(credentials).await.expect("live login should succeed");

    assert!(!session.nickname.as_str().is_empty(), "nickname should not be empty");
    assert!(session.account_id.get() > 0, "account id should be a positive SPA id");
}

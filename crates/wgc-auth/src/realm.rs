//! WGNI account realm -> host/client_id map (WGNI-protocol.md section 5).
//!
//! This is a distinct concept from `wgc_core::Realm` (the *product* realm
//! literal embedded in a `ProductId`, e.g. `WOWS.WW.PRODUCTION`). An
//! account's WGNI realm and the product realm of a locally installed game
//! are independent axes (protocol report section 4's mixed-realm example).

use serde::Deserialize;
use serde::Serialize;

use crate::identity::AccountId;

/// Which WGNI account realm to authenticate against.
///
/// Selecting a realm selects the whole `RealmConfig` tuple together;
/// `client_id` is realm-bound, not global.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountRealm {
    Eu,
    Na,
    Asia,
    /// UNCONFIRMED post-2022 Lesta split; may be dead - see WGNI-protocol.md
    /// open question 5.
    Ru,
}

/// A realm's WGNI auth host and OAuth client id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealmConfig {
    pub domain_wgnet: url::Host,
    pub client_id: ClientId,
}

/// The realm-specific WGC OAuth `client_id` (WGNI-protocol.md section 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientId(String);

impl ClientId {
    pub fn new(id: impl Into<String>) -> ClientId {
        ClientId(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AccountRealm {
    /// Returns this realm's WGNI auth host and OAuth client id.
    ///
    /// Table source: WGNI-protocol.md section 5, `Mixaill/galaxy-integration-wargaming`
    /// `wgc_constants.py` (`WGCRealms`/`WGCIds`), INFERRED from a third-party
    /// OSS constant table, not independently re-derived from a live capture.
    pub fn config(self) -> RealmConfig {
        let (host, client_id) = match self {
            AccountRealm::Eu => ("eu.wargaming.net", "JJ5yuABVKqZekaktUR8cejMzxbbHAtUVmY2eamsS"),
            AccountRealm::Na => ("na.wargaming.net", "AJ5PLrEuz5C2d0hHmmjQJtjaMpueSahYY8CiswHE"),
            AccountRealm::Asia => ("asia.wargaming.net", "Xe2oDM8Z6A4N70VZIV8RyVLHpvdtVPYNRIIYBklJ"),
            AccountRealm::Ru => ("ru.wargaming.net", "77cxLwtEJ9uvlcm2sYe4O8viIIWn1FEWlooMTTqF"),
        };
        // Table hosts are hand-verified bare domains; a parse failure here
        // means the table itself is wrong, which is a programming error.
        let domain_wgnet = url::Host::parse(host).expect("realm table host is a valid domain");
        RealmConfig { domain_wgnet, client_id: ClientId::new(client_id) }
    }

    /// Sanity-check heuristic mapping a numeric SPA account id to a probable
    /// realm via `SPAIDRealms` (WGNI-protocol.md section 5). This is NOT the
    /// primary realm selector -- the authoritative realm is whichever
    /// `domain_wgnet` a login succeeded against (protocol report section 4).
    pub fn from_account_id(id: AccountId) -> Option<AccountRealm> {
        let id = id.get();
        match id {
            1..=499_999_999 => Some(AccountRealm::Ru),
            500_000_000..=999_999_999 => Some(AccountRealm::Eu),
            1_000_000_000..=1_499_999_999 => Some(AccountRealm::Na),
            2_000_000_000..=3_499_999_999 => Some(AccountRealm::Asia),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_returns_correct_client_id_per_realm() {
        assert_eq!(AccountRealm::Eu.config().client_id.as_str(), "JJ5yuABVKqZekaktUR8cejMzxbbHAtUVmY2eamsS");
        assert_eq!(AccountRealm::Na.config().client_id.as_str(), "AJ5PLrEuz5C2d0hHmmjQJtjaMpueSahYY8CiswHE");
        assert_eq!(AccountRealm::Asia.config().client_id.as_str(), "Xe2oDM8Z6A4N70VZIV8RyVLHpvdtVPYNRIIYBklJ");
        assert_eq!(AccountRealm::Ru.config().client_id.as_str(), "77cxLwtEJ9uvlcm2sYe4O8viIIWn1FEWlooMTTqF");
    }

    #[test]
    fn config_returns_correct_host_per_realm() {
        assert_eq!(AccountRealm::Eu.config().domain_wgnet, url::Host::parse("eu.wargaming.net").unwrap());
        assert_eq!(AccountRealm::Na.config().domain_wgnet, url::Host::parse("na.wargaming.net").unwrap());
        assert_eq!(AccountRealm::Asia.config().domain_wgnet, url::Host::parse("asia.wargaming.net").unwrap());
        assert_eq!(AccountRealm::Ru.config().domain_wgnet, url::Host::parse("ru.wargaming.net").unwrap());
    }

    #[test]
    fn from_account_id_maps_boundary_values() {
        assert_eq!(AccountRealm::from_account_id(AccountId::new(499_999_999)), Some(AccountRealm::Ru));
        assert_eq!(AccountRealm::from_account_id(AccountId::new(500_000_000)), Some(AccountRealm::Eu));
        // Live example from WGNI-protocol.md section 4.
        assert_eq!(AccountRealm::from_account_id(AccountId::new(697_769_785)), Some(AccountRealm::Eu));
        assert_eq!(AccountRealm::from_account_id(AccountId::new(1_000_000_000)), Some(AccountRealm::Na));
        assert_eq!(AccountRealm::from_account_id(AccountId::new(2_000_000_000)), Some(AccountRealm::Asia));
    }

    #[test]
    fn from_account_id_rejects_out_of_range_values() {
        // Below the lowest range (Ru starts at 1): explicitly None, not a
        // fallback guess.
        assert_eq!(AccountRealm::from_account_id(AccountId::new(0)), None);
        // Above the highest range (Asia ends at 3_499_999_999).
        assert_eq!(AccountRealm::from_account_id(AccountId::new(3_500_000_000)), None);
    }
}

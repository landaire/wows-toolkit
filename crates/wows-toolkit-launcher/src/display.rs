//! Text both front ends show for launcher state.

use wgc_catalog::models::PartName;
use wgc_catalog::update::LatestBuild;
use wgc_catalog::update::PartStatus;

/// Every realm the sign-in form offers, in display order.
pub const REALMS: [wgc_auth::AccountRealm; 3] =
    [wgc_auth::AccountRealm::Eu, wgc_auth::AccountRealm::Na, wgc_auth::AccountRealm::Asia];

/// Display label for a WGNI account realm.
pub fn realm_label(realm: wgc_auth::AccountRealm) -> &'static str {
    match realm {
        wgc_auth::AccountRealm::Eu => "EU",
        wgc_auth::AccountRealm::Na => "NA",
        wgc_auth::AccountRealm::Asia => "Asia",
    }
}

/// The translation key of the sentence the sign-in form shows for `failure`.
pub fn sign_in_failure_key(failure: crate::SignInFailure) -> &'static str {
    use crate::SignInFailure as F;

    match failure {
        F::WrongCredentials => "ui-launcher-sign_in_error-wrong_credentials",
        F::TwoFactorRequired => "ui-launcher-sign_in_error-two_factor_required",
        F::TwoFactorInvalid => "ui-launcher-sign_in_error-two_factor_invalid",
        F::CaptchaRequired => "ui-launcher-sign_in_error-captcha_required",
        F::RateLimited => "ui-launcher-sign_in_error-rate_limited",
        F::AccountBanned => "ui-launcher-sign_in_error-account_banned",
        F::NetworkUnreachable => "ui-launcher-sign_in_error-network_unreachable",
        F::ServiceUnavailable => "ui-launcher-sign_in_error-service_unavailable",
        F::SessionExpired => "ui-launcher-sign_in_error-session_expired",
        F::KeyringLocked => "ui-launcher-sign_in_error-keyring_locked",
        F::NotSaved => "ui-launcher-sign_in_error-not_saved",
        F::Unexpected => "ui-launcher-sign_in_error-unexpected",
    }
}

/// Coarse human-readable duration ("2h 15m", "3d 4h") for the token-expiry
/// display.
pub fn format_duration(d: std::time::Duration) -> String {
    let total_secs = d.as_secs();
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        "less than a minute".to_string()
    }
}

/// What a catalog check found for an existing install, one entry per part.
pub struct CatalogSummary {
    /// `"<part> <from> -> <to>"` for every part with a newer build.
    pub updates: Vec<String>,
    /// `"<part> (<to>)"` for every part the install does not have.
    pub not_installed: Vec<String>,
}

impl CatalogSummary {
    pub fn of(build: &LatestBuild) -> CatalogSummary {
        let mut updates = Vec::new();
        let mut not_installed = Vec::new();
        for (part, status) in &build.parts {
            match status {
                PartStatus::UpdateAvailable { from, to } => updates.push(format!("{part} {from} -> {to}")),
                PartStatus::NotInstalled { to } => not_installed.push(format!("{part} ({to})")),
                PartStatus::UpToDate => {}
            }
        }
        CatalogSummary { updates, not_installed }
    }
}

/// The client version a fresh install of a game gets. A fresh check reports
/// every part `NotInstalled`, and WGUS serves one current build per product,
/// so the client part's target is "the" version.
pub fn fresh_install_version(build: &LatestBuild) -> Option<String> {
    build.parts.iter().find_map(|(part, status)| match (part, status) {
        (PartName::Client, PartStatus::NotInstalled { to }) => Some(to.to_string()),
        _ => None,
    })
}

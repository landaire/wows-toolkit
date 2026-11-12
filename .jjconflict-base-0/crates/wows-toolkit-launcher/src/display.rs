//! Text both front ends show for launcher state.

use wgc_catalog::models::PartName;
use wgc_catalog::update::LatestBuild;
use wgc_catalog::update::PartStatus;

/// Every realm the sign-in form offers, in display order.
pub const REALMS: [wgc_auth::AccountRealm; 4] =
    [wgc_auth::AccountRealm::Eu, wgc_auth::AccountRealm::Na, wgc_auth::AccountRealm::Asia, wgc_auth::AccountRealm::Ru];

/// Display label for a WGNI account realm. Ru is flagged experimental
/// everywhere it appears; see `wgc_auth::AccountRealm::Ru`.
pub fn realm_label(realm: wgc_auth::AccountRealm) -> &'static str {
    match realm {
        wgc_auth::AccountRealm::Eu => "EU",
        wgc_auth::AccountRealm::Na => "NA",
        wgc_auth::AccountRealm::Asia => "Asia",
        wgc_auth::AccountRealm::Ru => "RU (experimental)",
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

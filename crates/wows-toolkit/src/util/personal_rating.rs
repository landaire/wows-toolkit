use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use tracing::instrument;

pub use wows_replay_insights::personal_rating::{
    ExpectedValuesData, PersonalRatingCategory, PersonalRatingData, PersonalRatingResult, ShipBattleStats,
};

/// URL to fetch expected values from wows-numbers.com
const EXPECTED_VALUES_URL: &str = "https://api.wows-numbers.com/personal/rating/expected/json/";

/// How often to check for updates (7 days)
const UPDATE_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// File name for cached expected values
const EXPECTED_VALUES_FILENAME: &str = "pr_expected_values.json";

/// egui color per rating category. An extension trait because the enum now
/// lives in wows-replay-insights and cannot carry an inherent egui method.
pub trait PersonalRatingCategoryColor {
    fn color(&self) -> egui::Color32;
}

impl PersonalRatingCategoryColor for PersonalRatingCategory {
    fn color(&self) -> egui::Color32 {
        match self {
            Self::Bad => egui::Color32::from_rgb(0xFF, 0x00, 0x00),
            Self::BelowAverage => egui::Color32::from_rgb(0xFE, 0x79, 0x03),
            Self::Average => egui::Color32::from_rgb(0xFF, 0xC7, 0x1F),
            Self::Good => egui::Color32::from_rgb(0x44, 0xB3, 0x00),
            Self::VeryGood => egui::Color32::from_rgb(0x31, 0x80, 0x00),
            Self::Great => egui::Color32::from_rgb(0x02, 0xC9, 0xB3),
            Self::Unicum => egui::Color32::from_rgb(0xD0, 0x42, 0xF3),
            Self::SuperUnicum => egui::Color32::from_rgb(0xA0, 0x0D, 0xC5),
        }
    }
}

/// Get the path for storing expected values
pub fn get_expected_values_path() -> PathBuf {
    let mut path = PathBuf::from(EXPECTED_VALUES_FILENAME);
    if let Some(storage_dir) = crate::storage_dir() {
        path = storage_dir.join(path);
    }
    path
}

/// Check if expected values need to be updated
pub fn needs_update() -> bool {
    let path = get_expected_values_path();

    if !path.exists() {
        return true;
    }

    // Check file modification time
    if let Ok(metadata) = fs::metadata(&path)
        && let Ok(modified) = metadata.modified()
        && let Ok(elapsed) = SystemTime::now().duration_since(modified)
    {
        return elapsed > UPDATE_INTERVAL;
    }

    // If we can't determine the age, assume it needs updating
    true
}

/// Failure fetching or validating the wows-numbers expected-values data.
#[derive(Debug, thiserror::Error)]
pub enum FetchExpectedValuesError {
    #[error("request failed")]
    Http(#[from] reqwest::Error),
    #[error("response was not valid expected-values JSON")]
    InvalidJson(#[from] serde_json::Error),
    #[error("expected-values response contained no ship data")]
    Empty,
}

/// Validate a downloaded expected-values payload before it is cached. The body
/// must parse into `ExpectedValuesData` with a non-empty ship map; wows-numbers
/// downtime has served HTML error pages with a 200 status that must not
/// overwrite good cached data.
fn validate_expected_values(bytes: &[u8]) -> Result<(), FetchExpectedValuesError> {
    let parsed: ExpectedValuesData = serde_json::from_slice(bytes)?;
    if parsed.data.is_empty() {
        return Err(FetchExpectedValuesError::Empty);
    }
    Ok(())
}

/// Fetch expected values from the API, returning the raw bytes only if the
/// response is a valid, non-empty expected-values document.
#[instrument]
pub async fn fetch_expected_values() -> Result<Vec<u8>, FetchExpectedValuesError> {
    let client = crate::util::http::async_client()?;
    let response = crate::util::http::get_with_retry(&client, EXPECTED_VALUES_URL).await?;
    let bytes = response.bytes().await?.to_vec();
    validate_expected_values(&bytes)?;
    Ok(bytes)
}

/// Save expected values to disk
#[instrument(skip(data), fields(data_len = data.len()))]
pub fn save_expected_values(data: &[u8]) -> std::io::Result<()> {
    let path = get_expected_values_path();
    fs::write(path, data)
}

/// Load expected values from disk
#[allow(dead_code)]
pub fn load_expected_values_from_disk() -> std::io::Result<Vec<u8>> {
    let path = get_expected_values_path();
    fs::read(path)
}

#[cfg(test)]
mod tests {
    use wows_replays::types::GameParamId;

    use super::*;

    /// Path to the checked-in expected values fixture.
    fn fixture_bytes() -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("tests")
            .join("fixtures")
            .join("pr_expected_values.json");
        fs::read(&path).unwrap_or_else(|e| panic!("missing fixture {}: {e}", path.display()))
    }

    fn loaded_pr_data() -> PersonalRatingData {
        let mut pr = PersonalRatingData::new();
        pr.load_from_bytes(&fixture_bytes()).expect("should parse expected values JSON");
        pr
    }

    // -- Loading --

    #[test]
    fn load_from_bytes_parses_fixture() {
        let pr = loaded_pr_data();
        assert!(pr.is_loaded());
    }

    #[test]
    fn fixture_contains_ships() {
        let pr = loaded_pr_data();
        // The first ship ID in the fixture is 3374266064
        let ev = pr.get_ship_expected(GameParamId::from(3374266064u64));
        assert!(ev.is_some(), "fixture should contain ship 3374266064");
        let ev = ev.unwrap();
        assert!(ev.average_damage_dealt > 0.0);
        assert!(ev.average_frags > 0.0);
        assert!(ev.win_rate > 0.0);
    }

    #[test]
    fn empty_array_entries_return_none() {
        let pr = loaded_pr_data();
        // Ship 3330258928 is [] in the fixture
        let ev = pr.get_ship_expected(GameParamId::from(3330258928u64));
        assert!(ev.is_none(), "empty-array entries should return None");
    }

    #[test]
    fn missing_ship_returns_none() {
        let pr = loaded_pr_data();
        let ev = pr.get_ship_expected(GameParamId::from(9999999999u64));
        assert!(ev.is_none());
    }

    // -- Download validation --

    #[test]
    fn validate_accepts_real_fixture() {
        validate_expected_values(&fixture_bytes()).expect("fixture should pass validation");
    }

    #[test]
    fn validate_rejects_html_error_page() {
        let html = b"<!DOCTYPE html><html><body>503 Service Unavailable</body></html>";
        assert!(matches!(validate_expected_values(html), Err(FetchExpectedValuesError::InvalidJson(_))));
    }

    #[test]
    fn validate_rejects_truncated_json() {
        let truncated = br#"{"time":123,"data":{"3374266064":{"average_damage_dealt":"#;
        assert!(matches!(validate_expected_values(truncated), Err(FetchExpectedValuesError::InvalidJson(_))));
    }

    #[test]
    fn validate_rejects_empty_data() {
        let empty = br#"{"time":123,"data":{}}"#;
        assert!(matches!(validate_expected_values(empty), Err(FetchExpectedValuesError::Empty)));
    }
}

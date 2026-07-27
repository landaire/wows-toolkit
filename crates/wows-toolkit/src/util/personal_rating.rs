use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

use tracing::instrument;

pub use wows_replay_insights::personal_rating::ExpectedValuesData;
pub use wows_replay_insights::personal_rating::PersonalRatingCategory;
pub use wows_replay_insights::personal_rating::PersonalRatingData;
pub use wows_replay_insights::personal_rating::PersonalRatingResult;
pub use wows_replay_insights::personal_rating::ShipBattleStats;

/// URL to fetch expected values from wows-numbers.com
const EXPECTED_VALUES_URL: &str = "https://api.wows-numbers.com/personal/rating/expected/json/";

/// How often to check for updates (7 days)
const UPDATE_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// File name for cached expected values
const EXPECTED_VALUES_FILENAME: &str = "pr_expected_values.json";

/// Alpha for the chip background: the canonical hue laid faintly over whatever
/// row it lands on, so the chip reads the same on card, striped and selected
/// rows without needing a value per row state.
const CHIP_TINT_ALPHA: u8 = 46;

/// A rating chip. The canonical hue is preserved exactly: `tint` is that hue at
/// low alpha, `text` a theme-adjusted version of it that clears the contrast
/// floor over the tint on every row state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RatingSwatch {
    pub tint: egui::Color32,
    pub text: egui::Color32,
}

/// Chip colours per rating category. An extension trait because the enum
/// lives in wows-replay-insights and cannot carry an inherent egui method.
pub trait PersonalRatingCategorySwatch {
    fn swatch(&self, visuals: &egui::Visuals) -> RatingSwatch;
}

impl PersonalRatingCategorySwatch for PersonalRatingCategory {
    fn swatch(&self, visuals: &egui::Visuals) -> RatingSwatch {
        let hue = match self {
            Self::Bad => egui::Color32::from_rgb(0xFF, 0x00, 0x00),
            Self::BelowAverage => egui::Color32::from_rgb(0xFE, 0x79, 0x03),
            Self::Average => egui::Color32::from_rgb(0xFF, 0xC7, 0x1F),
            Self::Good => egui::Color32::from_rgb(0x44, 0xB3, 0x00),
            Self::VeryGood => egui::Color32::from_rgb(0x31, 0x80, 0x00),
            Self::Great => egui::Color32::from_rgb(0x02, 0xC9, 0xB3),
            Self::Unicum => egui::Color32::from_rgb(0xD0, 0x42, 0xF3),
            Self::SuperUnicum => egui::Color32::from_rgb(0xA0, 0x0D, 0xC5),
        };
        let tint = egui::Color32::from_rgba_unmultiplied(hue.r(), hue.g(), hue.b(), CHIP_TINT_ALPHA);
        // Solved against the composited chip over card, striped and selected rows.
        // Several sit just above the floor to keep the chips quiet; retuning any of
        // those row colours requires re-solving this table, which the contrast test
        // will catch.
        let text = if visuals.dark_mode {
            match self {
                Self::Bad => egui::Color32::from_rgb(0xF5, 0x68, 0x64),
                Self::BelowAverage => egui::Color32::from_rgb(0xFB, 0x86, 0x1D),
                Self::Average => egui::Color32::from_rgb(0xFF, 0xC7, 0x1F),
                Self::Good => egui::Color32::from_rgb(0x5A, 0xBA, 0x1E),
                Self::VeryGood => egui::Color32::from_rgb(0x7A, 0xA8, 0x58),
                Self::Great => egui::Color32::from_rgb(0x02, 0xC9, 0xB3),
                Self::Unicum => egui::Color32::from_rgb(0xD8, 0x78, 0xEC),
                Self::SuperUnicum => egui::Color32::from_rgb(0xC3, 0x76, 0xD0),
            }
        } else {
            match self {
                Self::Bad => egui::Color32::from_rgb(0x9D, 0x04, 0x03),
                Self::BelowAverage => egui::Color32::from_rgb(0x88, 0x43, 0x05),
                Self::Average => egui::Color32::from_rgb(0x71, 0x59, 0x12),
                Self::Good => egui::Color32::from_rgb(0x28, 0x62, 0x04),
                Self::VeryGood => egui::Color32::from_rgb(0x25, 0x5D, 0x02),
                Self::Great => egui::Color32::from_rgb(0x06, 0x63, 0x58),
                Self::Unicum => egui::Color32::from_rgb(0x80, 0x2B, 0x94),
                Self::SuperUnicum => egui::Color32::from_rgb(0x7F, 0x0C, 0x9B),
            }
        };
        RatingSwatch { tint, text }
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

    const ALL_CATEGORIES: [PersonalRatingCategory; 8] = [
        PersonalRatingCategory::Bad,
        PersonalRatingCategory::BelowAverage,
        PersonalRatingCategory::Average,
        PersonalRatingCategory::Good,
        PersonalRatingCategory::VeryGood,
        PersonalRatingCategory::Great,
        PersonalRatingCategory::Unicum,
        PersonalRatingCategory::SuperUnicum,
    ];

    #[test]
    fn swatch_keeps_the_canonical_hue() {
        // Compare against the same encoding (hue at CHIP_TINT_ALPHA) rather than
        // decoding tint back to opaque: Color32's premultiplied storage round-trips
        // through linear light and can round differently by 1 bit at low alpha.
        let average = PersonalRatingCategory::Average.swatch(&egui::Visuals::dark());
        assert_eq!(average.tint, egui::Color32::from_rgba_unmultiplied(0xFF, 0xC7, 0x1F, CHIP_TINT_ALPHA));
        let super_unicum = PersonalRatingCategory::SuperUnicum.swatch(&egui::Visuals::dark());
        assert_eq!(super_unicum.tint, egui::Color32::from_rgba_unmultiplied(0xA0, 0x0D, 0xC5, CHIP_TINT_ALPHA));
    }

    #[test]
    fn every_tier_chip_is_legible_on_every_row_state() {
        use crate::ui::theme::contrast::CONTRAST_FLOOR;
        use crate::ui::theme::contrast::contrast_ratio;
        use crate::ui::theme::palette;

        fn over(fg: egui::Color32, bg: egui::Color32) -> egui::Color32 {
            // Color32 is premultiplied, so the source is already scaled by alpha.
            let inv = 1.0 - (f32::from(fg.a()) / 255.0);
            let mix = |f: u8, b: u8| (f32::from(f) + f32::from(b) * inv).round() as u8;
            egui::Color32::from_rgb(mix(fg.r(), bg.r()), mix(fg.g(), bg.g()), mix(fg.b(), bg.b()))
        }

        for (visuals, grounds) in [
            (egui::Visuals::dark(), [palette::dark::CARD, palette::dark::FAINT, palette::dark::SELECTION]),
            (egui::Visuals::light(), [palette::light::CARD, palette::light::FAINT, palette::light::SELECTION]),
        ] {
            for category in ALL_CATEGORIES {
                let swatch = category.swatch(&visuals);
                for ground in grounds {
                    let chip = over(swatch.tint, ground);
                    let r = contrast_ratio(swatch.text, chip);
                    assert!(r >= CONTRAST_FLOOR, "{category:?} on {ground:?} is {r}");
                }
            }
        }
    }
}

use std::fs;
use std::path::PathBuf;

use tracing::instrument;

pub use wows_replay_insights::personal_rating::ExpectedValuesData;
pub use wows_replay_insights::personal_rating::PersonalRatingCategory;
pub use wows_replay_insights::personal_rating::PersonalRatingData;
pub use wows_replay_insights::personal_rating::PersonalRatingResult;
pub use wows_replay_insights::personal_rating::ShipBattleStats;

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
        let hue = rgb(wows_toolkit_viewmodel::personal_rating::chip_hue(*self));
        let tint = egui::Color32::from_rgba_unmultiplied(
            hue.r(),
            hue.g(),
            hue.b(),
            wows_toolkit_viewmodel::personal_rating::CHIP_TINT_ALPHA,
        );
        let text = rgb(wows_toolkit_viewmodel::personal_rating::chip_text(*self, visuals.dark_mode));
        RatingSwatch { tint, text }
    }
}

/// Unpacks a `0xRRGGBB` colour from the shared palette.
fn rgb(packed: u32) -> egui::Color32 {
    egui::Color32::from_rgb((packed >> 16) as u8, (packed >> 8) as u8, packed as u8)
}

/// Get the path for storing expected values
pub fn get_expected_values_path() -> PathBuf {
    wows_toolkit_viewmodel::personal_rating::expected_values_path()
}

/// Check if expected values need to be updated
pub fn needs_update() -> bool {
    wows_toolkit_viewmodel::personal_rating::needs_update()
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

/// Validate a downloaded expected-values payload before it is cached.
fn validate_expected_values(bytes: &[u8]) -> Result<(), FetchExpectedValuesError> {
    use wows_toolkit_viewmodel::personal_rating::InvalidExpectedValues;

    wows_toolkit_viewmodel::personal_rating::validate_expected_values(bytes).map_err(|err| match err {
        InvalidExpectedValues::Parse(err) => FetchExpectedValuesError::InvalidJson(err),
        InvalidExpectedValues::NoShips => FetchExpectedValuesError::Empty,
    })
}

/// Fetch expected values from the API, returning the raw bytes only if the
/// response is a valid, non-empty expected-values document.
#[instrument(skip(proxy))]
pub async fn fetch_expected_values(
    proxy: Option<&crate::util::proxy::ProxyConfig>,
) -> Result<Vec<u8>, FetchExpectedValuesError> {
    let client = crate::util::http::async_client(proxy, reqwest::redirect::Policy::default())?;
    let response =
        crate::util::http::get_with_retry(&client, wows_toolkit_viewmodel::personal_rating::EXPECTED_VALUES_URL)
            .await?;
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

    /// The checked-in expected values, shipped by the viewmodel crate.
    fn fixture_bytes() -> &'static [u8] {
        wows_toolkit_viewmodel::personal_rating::EXPECTED_VALUES_FIXTURE
    }

    fn loaded_pr_data() -> PersonalRatingData {
        let mut pr = PersonalRatingData::new();
        pr.load_from_bytes(fixture_bytes()).expect("should parse expected values JSON");
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
        validate_expected_values(fixture_bytes()).expect("fixture should pass validation");
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
        assert_eq!(
            average.tint,
            egui::Color32::from_rgba_unmultiplied(
                0xFF,
                0xC7,
                0x1F,
                wows_toolkit_viewmodel::personal_rating::CHIP_TINT_ALPHA
            )
        );
        let super_unicum = PersonalRatingCategory::SuperUnicum.swatch(&egui::Visuals::dark());
        assert_eq!(
            super_unicum.tint,
            egui::Color32::from_rgba_unmultiplied(
                0xA0,
                0x0D,
                0xC5,
                wows_toolkit_viewmodel::personal_rating::CHIP_TINT_ALPHA
            )
        );
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

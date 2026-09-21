//! Personal rating: the expected-values table it is computed against, and
//! where that table is cached.
//!
//! The rating itself is `wows_replay_insights::personal_rating`. This names
//! the cache both front ends share, so the port reads the table the egui app
//! already downloaded rather than fetching a second copy.

use std::path::PathBuf;
use std::time::Duration;
use std::time::SystemTime;

pub use wows_replay_insights::personal_rating::PersonalRatingCategory;
pub use wows_replay_insights::personal_rating::PersonalRatingData;
pub use wows_replay_insights::personal_rating::PersonalRatingResult;
pub use wows_replay_insights::personal_rating::ShipBattleStats;

/// Where the expected values are published.
pub const EXPECTED_VALUES_URL: &str = "https://api.wows-numbers.com/personal/rating/expected/json/";

/// How stale a cached table may get before it is refetched.
pub const UPDATE_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Whether the cache is missing or older than [`UPDATE_INTERVAL`].
///
/// A cache whose age cannot be read counts as stale: refetching costs one
/// request, while trusting an unreadable timestamp could pin the rating to a
/// table that never refreshes again.
pub fn needs_update() -> bool {
    let path = expected_values_path();
    if !path.exists() {
        return true;
    }

    std::fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_none_or(|age| age > UPDATE_INTERVAL)
}

/// Rejects a payload that is not a usable expected-values document.
///
/// wows-numbers downtime has served HTML error pages under a 200 status; one
/// of those must not overwrite a good cache.
pub fn validate_expected_values(bytes: &[u8]) -> Result<(), InvalidExpectedValues> {
    let mut data = PersonalRatingData::new();
    data.load_from_bytes(bytes).map_err(InvalidExpectedValues::Parse)?;
    if data.ship_count() == 0 {
        return Err(InvalidExpectedValues::NoShips);
    }
    Ok(())
}

/// Caches a validated payload for both front ends to read.
pub fn save_cached(bytes: &[u8]) -> Result<(), ExpectedValuesError> {
    validate_expected_values(bytes)?;
    std::fs::write(expected_values_path(), bytes).map_err(ExpectedValuesError::Write)
}

/// Alpha for a rating chip's background: the canonical hue laid faintly over
/// whatever row it lands on, so the chip reads the same on card, striped and
/// selected rows without a value per row state.
pub const CHIP_TINT_ALPHA: u8 = 46;

/// A rating band's canonical hue, packed `0xRRGGBB`. The chip's background is
/// this at [`CHIP_TINT_ALPHA`].
pub fn chip_hue(category: PersonalRatingCategory) -> u32 {
    match category {
        PersonalRatingCategory::Bad => 0xff0000,
        PersonalRatingCategory::BelowAverage => 0xfe7903,
        PersonalRatingCategory::Average => 0xffc71f,
        PersonalRatingCategory::Good => 0x44b300,
        PersonalRatingCategory::VeryGood => 0x318000,
        PersonalRatingCategory::Great => 0x02c9b3,
        PersonalRatingCategory::Unicum => 0xd042f3,
        PersonalRatingCategory::SuperUnicum => 0xa00dc5,
    }
}

/// A rating band's text color over its own tint, packed `0xRRGGBB`.
///
/// Solved against the composited chip over card, striped and selected rows.
/// Several sit just above the contrast floor to keep the chips quiet;
/// retuning any of those row colors requires re-solving this table, which the
/// egui app's contrast test catches.
pub fn chip_text(category: PersonalRatingCategory, dark_mode: bool) -> u32 {
    if dark_mode {
        match category {
            PersonalRatingCategory::Bad => 0xf56864,
            PersonalRatingCategory::BelowAverage => 0xfb861d,
            PersonalRatingCategory::Average => 0xffc71f,
            PersonalRatingCategory::Good => 0x5aba1e,
            PersonalRatingCategory::VeryGood => 0x7aa858,
            PersonalRatingCategory::Great => 0x02c9b3,
            PersonalRatingCategory::Unicum => 0xd878ec,
            PersonalRatingCategory::SuperUnicum => 0xc376d0,
        }
    } else {
        match category {
            PersonalRatingCategory::Bad => 0x9d0403,
            PersonalRatingCategory::BelowAverage => 0x884305,
            PersonalRatingCategory::Average => 0x715912,
            PersonalRatingCategory::Good => 0x286204,
            PersonalRatingCategory::VeryGood => 0x255d02,
            PersonalRatingCategory::Great => 0x066358,
            PersonalRatingCategory::Unicum => 0x802b94,
            PersonalRatingCategory::SuperUnicum => 0x7f0c9b,
        }
    }
}

/// The cache file, under the toolkit's storage directory.
pub const EXPECTED_VALUES_FILENAME: &str = "pr_expected_values.json";

/// The shipped expected-values table, for tests that want real figures rather
/// than invented ones. Behind `test-support` so only a test build carries it.
#[cfg(any(test, feature = "test-support"))]
pub const EXPECTED_VALUES_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/pr_expected_values.json");

/// Where the expected values are cached.
///
/// Falls back to a bare relative name when there is no storage directory,
/// which is what the egui app does; a caller with no storage directory has
/// bigger problems than the rating table.
pub fn expected_values_path() -> PathBuf {
    match wows_toolkit_config::storage_dir() {
        Some(dir) => dir.join(EXPECTED_VALUES_FILENAME),
        None => PathBuf::from(EXPECTED_VALUES_FILENAME),
    }
}

/// Why a payload is not a usable expected-values table.
#[derive(Debug, thiserror::Error)]
pub enum InvalidExpectedValues {
    #[error("the expected values are not valid JSON")]
    Parse(#[source] serde_json::Error),
    #[error("the expected values name no ships, so nothing can be rated")]
    NoShips,
}

#[derive(Debug, thiserror::Error)]
pub enum ExpectedValuesError {
    #[error("the expected values have not been downloaded yet")]
    NotCached,
    #[error("the cached expected values could not be read")]
    Read(#[source] std::io::Error),
    #[error(transparent)]
    Invalid(#[from] InvalidExpectedValues),
    #[error("the expected values could not be cached")]
    Write(#[source] std::io::Error),
}

/// Loads the cached table.
///
/// An absent cache is reported separately from a broken one: the first is the
/// ordinary state before a download, the second means the file needs
/// replacing. A table that parses but names no ships is refused too, since it
/// would otherwise pass every "is it loaded" check and then rate nothing.
pub fn load_cached() -> Result<PersonalRatingData, ExpectedValuesError> {
    let path = expected_values_path();
    if !path.exists() {
        return Err(ExpectedValuesError::NotCached);
    }

    let bytes = std::fs::read(&path).map_err(ExpectedValuesError::Read)?;
    validate_expected_values(&bytes)?;

    let mut data = PersonalRatingData::new();
    data.load_from_bytes(&bytes).map_err(InvalidExpectedValues::Parse)?;
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_sits_under_the_storage_directory_when_there_is_one() {
        let path = expected_values_path();
        assert!(path.ends_with(EXPECTED_VALUES_FILENAME));
        if let Some(dir) = wows_toolkit_config::storage_dir() {
            assert!(path.starts_with(dir), "the cache belongs beside the other toolkit data");
        }
    }

    #[test]
    fn an_empty_table_reports_itself_as_unloaded() {
        assert!(!PersonalRatingData::new().is_loaded());
    }

    /// A payload with no ships passes `is_loaded` but rates nothing, which is
    /// why `load_cached` refuses it rather than handing it on.
    #[test]
    fn a_loaded_table_with_no_ships_counts_none() {
        let mut data = PersonalRatingData::new();
        data.load_from_bytes(br#"{"time":0,"data":{}}"#).expect("the payload is valid JSON");
        assert!(data.is_loaded());
        assert_eq!(data.ship_count(), 0);
    }

    #[test]
    fn entries_with_no_values_do_not_count_as_ships() {
        let mut data = PersonalRatingData::new();
        data.load_from_bytes(br#"{"time":0,"data":{"1":[]}}"#).expect("the payload is valid JSON");
        assert_eq!(data.ship_count(), 0, "an empty-array entry carries no expected values");
    }
}

//! Personal rating: the expected-values table it is computed against, and
//! where that table is cached.
//!
//! The rating itself is `wows_replay_insights::personal_rating`. This names
//! the cache both front ends share, so the port reads the table the egui app
//! already downloaded rather than fetching a second copy.

use std::path::PathBuf;

pub use wows_replay_insights::personal_rating::PersonalRatingData;
pub use wows_replay_insights::personal_rating::PersonalRatingResult;
pub use wows_replay_insights::personal_rating::ShipBattleStats;

/// The cache file, under the toolkit's storage directory.
pub const EXPECTED_VALUES_FILENAME: &str = "pr_expected_values.json";

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

#[derive(Debug, thiserror::Error)]
pub enum ExpectedValuesError {
    #[error("the expected values have not been downloaded yet")]
    NotCached,
    #[error("the cached expected values could not be read")]
    Read(#[source] std::io::Error),
    #[error("the cached expected values could not be parsed")]
    Parse(#[source] serde_json::Error),
}

/// Loads the cached table.
///
/// An absent cache is reported separately from a broken one: the first is the
/// ordinary state before a download, the second means the file needs
/// replacing.
pub fn load_cached() -> Result<PersonalRatingData, ExpectedValuesError> {
    let path = expected_values_path();
    if !path.exists() {
        return Err(ExpectedValuesError::NotCached);
    }

    let bytes = std::fs::read(&path).map_err(ExpectedValuesError::Read)?;
    let mut data = PersonalRatingData::new();
    data.load_from_bytes(&bytes).map_err(ExpectedValuesError::Parse)?;
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
}

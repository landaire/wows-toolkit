//! Documents for tests that have no game install: the fabricated sample
//! report read in place of any replay file.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use wows_replay_insights::personal_rating::ExpectedValuesData;
use wows_replay_insights::personal_rating::ShipExpectedValues;
use wows_replay_insights::personal_rating::ShipExpectedValuesEntry;
use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;

use crate::replay_inspector::document;
use crate::replay_inspector::document::ReplayDocument;
use crate::replay_inspector::document::TitleIcons;
use crate::replay_inspector::error::InspectorError;

/// The sample report as if it had been read from `path`.
pub fn sample_document(path: &Path, alts: &[PathBuf]) -> ReplayDocument {
    let normalized = wows_toolkit_replay_data::test_support::fixture_empty_battle_report();

    ReplayDocument {
        path: path.to_path_buf(),
        alts: alts.to_vec(),
        model: wows_toolkit_replay_data::sample::sample_model(),
        export: ExportedMatch::new(&normalized, &[], &[], true),
        export_stem: "sample-export".to_owned(),
        raw_metadata_json: "{\n  \"clientVersionFromXml\": \"0.0.0\"\n}".to_owned(),
        raw_results_json: Some("{\n  \"arenaUniqueID\": 1\n}".to_owned()),
        mapped_results_json: Some("{\n  \"arena_unique_id\": 1\n}".to_owned()),
        vfs: None,
        session_stat: None,
        constants_mismatch: None,
        title_icons: TitleIcons::default(),
    }
}

/// Makes every replay the inspector opens read as the sample report, except
/// files whose name does not end in `.wowsreplay`, which fail as a broken
/// replay does.
pub fn read_replays_as_sample() {
    document::set_source(sample_source);
}

/// The ship the unrated sample's recording player sails.
const UNRATED_SAMPLE_SHIP: u64 = 4_181_604_048;

/// Makes every replay read as the sample report with no row rated, its
/// recording player in a ship [`sample_personal_rating`] rates.
pub fn read_replays_as_unrated_sample() {
    document::set_source(unrated_sample_source);
}

/// A personal-rating table that rates the unrated sample's recording player.
pub fn sample_personal_rating() -> PersonalRatingData {
    let mut data = HashMap::new();
    data.insert(
        UNRATED_SAMPLE_SHIP.to_string(),
        ShipExpectedValuesEntry::Values(ShipExpectedValues { average_damage_dealt: 50_000.0, average_frags: 1.0, win_rate: 50.0 }),
    );
    let mut table = PersonalRatingData::new();
    table.load(ExpectedValuesData { time: 0, data });

    table
}

/// The sample report with no row rated, its recording player in a ship
/// [`sample_personal_rating`] rates.
pub fn unrated_sample_document(path: &Path, alts: &[PathBuf]) -> ReplayDocument {
    let mut document = sample_document(path, alts);
    for row in &mut document.model.rows {
        row.personal_rating = None;
        if row.is_self {
            row.ship_id = Some(GameParamId::from(UNRATED_SAMPLE_SHIP));
        }
    }

    document
}

fn unrated_sample_source(path: &Path, alts: &[PathBuf]) -> Result<ReplayDocument, InspectorError> {
    sample_source(path, alts)?;

    Ok(unrated_sample_document(path, alts))
}

fn sample_source(path: &Path, alts: &[PathBuf]) -> Result<ReplayDocument, InspectorError> {
    if path.extension().is_none_or(|extension| extension != "wowsreplay") {
        return Err(InspectorError::Load(wows_toolkit_replay_data::test_support::not_a_replay_error(path)));
    }

    Ok(sample_document(path, alts))
}

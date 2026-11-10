//! Documents for tests that have no game install: the fabricated sample
//! report read in place of any replay file.

use std::path::Path;
use std::path::PathBuf;

use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;

use crate::replay_inspector::document;
use crate::replay_inspector::document::ReplayDocument;
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
    }
}

/// Makes every replay the inspector opens read as the sample report, except
/// files whose name does not end in `.wowsreplay`, which fail as a broken
/// replay does.
pub fn read_replays_as_sample() {
    document::set_source(sample_source);
}

fn sample_source(path: &Path, alts: &[PathBuf]) -> Result<ReplayDocument, InspectorError> {
    if path.extension().is_none_or(|extension| extension != "wowsreplay") {
        return Err(InspectorError::Load(wows_toolkit_replay_data::test_support::not_a_replay_error(path)));
    }

    Ok(sample_document(path, alts))
}

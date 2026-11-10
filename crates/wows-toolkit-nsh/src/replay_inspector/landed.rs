//! What happens to a battle the watcher saw land, whether or not it is
//! opened: it is written to the auto-export directory, as the egui app's
//! background parser writes each replay it reads, and the index pass is
//! asked to pick it up. The index pass contributes what it indexes.

use std::path::Path;
use std::path::PathBuf;

use nsh_core::prelude::JsObject;
use nsh_core::prelude::blocking;
use wows_toolkit_replay_data::export::AutoExport;
use wows_toolkit_replay_data::replay_index::IndexMode;

use crate::replay_inspector::auto_export;
use crate::replay_inspector::error::InspectorError;
use crate::replay_inspector::settings;

/// What became of a landed battle's automatic export.
#[derive(Debug, Clone, PartialEq, Eq, JsObject)]
#[non_exhaustive]
pub struct LandedReport {
    /// Where the battle was written, when it was.
    pub exported: Option<String>,
    /// The notice for an export that failed, ready to show.
    pub export_error: Option<String>,
}

/// Exports the battle at `path` and queues an index pass that reads what is
/// not indexed yet. `debug` is the session's debug mode, which keeps the
/// debug fields in the export.
pub async fn process(path: PathBuf, debug: bool) -> Result<LandedReport, InspectorError> {
    blocking(move || {
        let report = export(&path, debug)?;
        // `false` when no WoWs directory is set, and then there is no index to fill.
        crate::game_data::start_indexing(IndexMode::FillGaps);

        Ok(report)
    })
    .await
    .map_err(|source| InspectorError::Task { source })?
}

/// Writes the battle to the auto-export directory when the setting asks. A
/// battle that does not parse is logged and not written.
fn export(path: &Path, debug: bool) -> Result<LandedReport, InspectorError> {
    let inspector = settings::load()?;
    let mut report = LandedReport { exported: None, export_error: None };
    if AutoExport::from_settings(&inspector.replay) == AutoExport::Off {
        return Ok(report);
    }

    let parsed = match crate::game_data::parse_replay(path) {
        Ok(parsed) => parsed,
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "landed replay: the battle did not parse");
            return Ok(report);
        }
    };

    // A battle the client left before the server reported it would be a hollow
    // document, which the egui app does not write either.
    if parsed.raw_results_json.is_none() {
        return Ok(report);
    }

    match auto_export::write(&parsed.export, &parsed.export_stem, &inspector.replay, debug) {
        Ok(written) => report.exported = written.map(|written| written.display().to_string()),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "auto-export failed");
            report.export_error = Some(rust_i18n::t!("ui.replay.auto_export_failed", error = error).into_owned());
        }
    }

    Ok(report)
}

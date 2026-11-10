//! What happens to a battle the watcher saw land, whether or not it is
//! opened: it is written to the auto-export directory, as the egui app's
//! background parser writes each replay it reads, and indexed. A battle in
//! the game's directory is left to an index pass, which contributes what it
//! indexes; one in an opened directory is indexed and contributed on its own.

use std::path::Path;
use std::path::PathBuf;

use nsh_core::prelude::JsObject;
use nsh_core::prelude::blocking;
use wows_toolkit_replay_data::export::AutoExport;
use wows_toolkit_replay_data::replay_index::IndexMode;

use crate::game_data::LandedReplay;
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

/// The listing that saw a battle land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LandedIn {
    /// The game's own replay directory.
    GameDirectory,
    /// A directory the reader opened.
    OpenedDirectory(PathBuf),
}

/// Exports the battle at `path` and indexes it: on its own when it landed in
/// an opened directory, else through a pass over the game's directory. The
/// index does not wait on the export, which may fail.
pub async fn process(path: PathBuf, landed_in: LandedIn) -> Result<LandedReport, InspectorError> {
    blocking(move || {
        let report = export(&path);
        match landed_in {
            LandedIn::OpenedDirectory(root) => crate::game_data::take_landed(LandedReplay { root, path }),
            // `false` when no WoWs directory is set, and then there is no index to fill.
            LandedIn::GameDirectory => {
                crate::game_data::start_indexing(IndexMode::FillGaps);
            }
        }

        report
    })
    .await
    .map_err(|source| InspectorError::Task { source })?
}

/// Writes the battle to the auto-export directory when the setting asks. A
/// battle that does not parse is logged and not written.
fn export(path: &Path) -> Result<LandedReport, InspectorError> {
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

    match auto_export::write(&parsed.export, &parsed.export_stem, &inspector.replay, inspector.debug_mode) {
        Ok(written) => report.exported = written.map(|written| written.display().to_string()),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "auto-export failed");
            report.export_error = Some(rust_i18n::t!("ui.replay.auto_export_failed", error = error).into_owned());
        }
    }

    Ok(report)
}

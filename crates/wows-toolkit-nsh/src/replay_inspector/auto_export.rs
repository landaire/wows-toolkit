//! Writing a battle out where the auto-export setting asks, as a replay is
//! opened and as one lands.

use std::path::PathBuf;

use wows_toolkit_config::ReplaySettings;
use wows_toolkit_replay_data::export::AutoExport;
use wows_toolkit_replay_data::export::ExportError;
use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;

/// Writes `export`, named `stem`, to the auto-export directory. `Ok(None)`
/// when the setting is off or names no directory. The debug form is written
/// only in debug mode; everyone else gets the stripped document.
pub fn write(
    export: &ExportedMatch,
    stem: &str,
    settings: &ReplaySettings,
    debug: bool,
) -> Result<Option<PathBuf>, ExportError> {
    let AutoExport::To { directory, format } = AutoExport::from_settings(settings) else {
        return Ok(None);
    };

    let path = directory.join(wows_toolkit_replay_data::export::file_name(stem, format));
    if debug {
        wows_toolkit_replay_data::export::write_export(export, &path, format)?;
    } else {
        wows_toolkit_replay_data::export::write_export(&export.clone().stripped(), &path, format)?;
    }

    Ok(Some(path))
}

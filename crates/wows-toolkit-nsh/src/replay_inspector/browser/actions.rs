//! What the listing's menus do outside the listing: the clipboard text, the
//! file manager, the session statistics and the replay controls reference.
//! The blocking ones run in `blocking`.

use std::path::Path;
use std::path::PathBuf;

use wows_toolkit_config::queries::SessionStatRow;
use wows_toolkit_config::queries::add_session_stat;
use wows_toolkit_config::queries::clear_session_stats;
use wows_toolkit_viewmodel::controls::CommandGroup;
use wows_toolkit_viewmodel::controls::read_scheme;

use crate::replay_inspector::document;
use crate::replay_inspector::error::InspectorError;
use crate::replay_inspector::settings::with_pool;

/// What the replay controls reference can show.
pub(super) enum Controls {
    /// No build is loaded to read the scheme from.
    NoBuild,
    /// The loaded build carries no scheme this parse makes anything of.
    Unavailable,
    Groups(Vec<CommandGroup>),
}

/// `paths` one per line, as "Copy Path" puts them on the clipboard.
pub(super) fn path_list(paths: &[PathBuf]) -> String {
    paths.iter().map(|path| path.to_string_lossy()).collect::<Vec<_>>().join("\n")
}

/// Shows `path` in the system file manager. A file manager that is not
/// there, or refuses, leaves a log line.
pub(super) fn reveal(path: &Path) {
    if let Err(err) = wows_toolkit_viewmodel::file_manager::reveal(path, |_command| {}) {
        tracing::warn!("replay listing: {} could not be shown in the file manager: {err}", path.display());
    }
}

/// Reads the game's keyboard reference from the newest loaded build.
pub(super) fn read_controls() -> Controls {
    let Some(build) = crate::game_data::newest_loaded() else {
        return Controls::NoBuild;
    };

    match read_scheme(build.vfs()) {
        Some(groups) => Controls::Groups(groups),
        None => Controls::Unavailable,
    }
}

/// Records each replay in the session statistics, forgetting what is
/// recorded first when `replace` is set ("Set as" rather than "Add to"). A
/// replay that will not parse, or names no recording player, is logged and
/// skipped rather than taking the rest of the set down with it.
pub(super) fn record_session_stats(paths: &[PathBuf], replace: bool) -> Result<(), InspectorError> {
    let rows: Vec<SessionStatRow> = paths
        .iter()
        .filter_map(|path| {
            let document = match document::read(path, &[]) {
                Ok(document) => document,
                Err(err) => {
                    tracing::warn!(path = %path.display(), "session stats: the replay would not parse: {err}");
                    return None;
                }
            };
            let Some(stat) = document.session_stat else {
                tracing::warn!(path = %path.display(), "session stats: the replay names no recording player");
                return None;
            };

            Some(stat.to_row())
        })
        .collect();

    with_pool(|pool| async move {
        if replace {
            clear_session_stats(&pool).await.map_err(|source| InspectorError::SessionStats { source })?;
        }
        for row in &rows {
            add_session_stat(&pool, row).await.map_err(|source| InspectorError::SessionStats { source })?;
        }

        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn copied_paths_are_lines() {
        let paths = vec![PathBuf::from("/replays/a b.wowsreplay"), PathBuf::from("/replays/c.wowsreplay")];

        assert_eq!(path_list(&paths), "/replays/a b.wowsreplay\n/replays/c.wowsreplay");
    }
}

//! The watch on the listed directory, so a match the game has just written
//! joins the listing without a rescan.

use std::path::Path;
use std::path::PathBuf;

use notify::EventKind;
use notify::RecommendedWatcher;
use notify::RecursiveMode;
use notify::Watcher as _;
use notify::event::ModifyKind;
use notify::event::RenameMode;
use rust_i18n::t;
use tokio::sync::mpsc::UnboundedReceiver;
use wows_toolkit_replay_data::replay_scan::is_finished_replay;

/// What the watcher saw happen to one replay. One channel of them keeps the
/// reads in the order the directory reported: a replay created and then
/// written to must not be read in the other order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DirChange {
    Appeared(PathBuf),
    Changed(PathBuf),
    Removed(PathBuf),
}

/// Why the directory is not watched. No watcher at all is a platform or
/// resource failure; an unwatchable directory is usually a network share or
/// one that has since gone.
#[derive(Debug, thiserror::Error)]
pub(super) enum WatchFailure {
    #[error("{}", t!("error.file_watcher_creation", error = source))]
    NoWatcher {
        #[source]
        source: notify::Error,
    },
    #[error("{}", t!("error.replay_dir_watch", error = source))]
    NotWatchable {
        #[source]
        source: notify::Error,
    },
}

/// Watches `dir`, not its subdirectories. The watch lasts as long as the
/// watcher is held; the changes stop when it is dropped.
pub(super) fn watch(dir: &Path) -> Result<(RecommendedWatcher, UnboundedReceiver<DirChange>), WatchFailure> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        let Ok(event) = event else { return };
        for change in changes(event) {
            // The receiver is gone once the directory changes or the
            // listing closes; this runs on notify's own thread, so the
            // change is dropped.
            let _ = sender.send(change);
        }
    })
    .map_err(|source| WatchFailure::NoWatcher { source })?;

    watcher.watch(dir, RecursiveMode::NonRecursive).map_err(|source| WatchFailure::NotWatchable { source })?;

    Ok((watcher, receiver))
}

/// The changes to finished replays one notify event reports.
fn changes(event: notify::Event) -> Vec<DirChange> {
    let make: fn(PathBuf) -> DirChange = match event.kind {
        EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To)) => DirChange::Appeared,
        // A re-save, or a copy that landed before its writer was done.
        EventKind::Modify(ModifyKind::Data(_)) => DirChange::Changed,
        EventKind::Remove(_) => DirChange::Removed,
        _ => return Vec::new(),
    };

    event.paths.into_iter().filter(|path| is_finished_replay(path)).map(make).collect()
}

#[cfg(test)]
mod tests {
    use notify::event::AccessKind;
    use notify::event::CreateKind;
    use notify::event::DataChange;
    use notify::event::RemoveKind;

    use super::*;

    fn event(kind: EventKind, path: &str) -> notify::Event {
        notify::Event::new(kind).add_path(PathBuf::from(path))
    }

    #[test]
    fn a_finished_replay_appears_changes_and_goes() {
        let replay = "replays/20260818_132652_PWSD610-Smaland.wowsreplay";
        let path = PathBuf::from(replay);

        assert_eq!(changes(event(EventKind::Create(CreateKind::File), replay)), vec![DirChange::Appeared(path.clone())]);
        assert_eq!(
            changes(event(EventKind::Modify(ModifyKind::Name(RenameMode::To)), replay)),
            vec![DirChange::Appeared(path.clone())],
            "the game renames the finished match into place"
        );
        assert_eq!(
            changes(event(EventKind::Modify(ModifyKind::Data(DataChange::Content)), replay)),
            vec![DirChange::Changed(path.clone())]
        );
        assert_eq!(changes(event(EventKind::Remove(RemoveKind::File), replay)), vec![DirChange::Removed(path)]);
        assert!(changes(event(EventKind::Access(AccessKind::Read), replay)).is_empty());
    }

    #[test]
    fn the_watcher_ignores_everything_but_a_finished_replay() {
        assert!(changes(event(EventKind::Create(CreateKind::File), "replays/temp.wowsreplay")).is_empty());
        assert!(changes(event(EventKind::Create(CreateKind::File), "replays/tempArenaInfo.json")).is_empty());
    }
}

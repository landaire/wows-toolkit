//! Building the replay index: walking a directory of replays, parsing each
//! one, and storing what it says.
//!
//! The mapping is `wows_toolkit_viewmodel::index_rows` and the writes are
//! `wows_toolkit_config::index::query`, both shared with the egui app, so a
//! replay indexed here and the same one indexed there produce identical rows.
//! What is here is the pass itself: which files to walk, and reporting how
//! far it has got.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use jiff::Timestamp;
use sqlx::SqlitePool;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::IndexWriteMode;
use wows_toolkit_config::index::rows::ResultsWrite;
use wows_toolkit_config::index::rows::SourceId;
use wows_toolkit_config::index::unindexable::Unindexable;
use wows_toolkit_viewmodel::index_rows::IndexContext;
use wows_toolkit_viewmodel::index_rows::map_rows;

use crate::replay_inspector::load::GameDataCache;
use crate::replay_inspector::load::parse_replay;

/// The extension the game records replays under.
const REPLAY_EXTENSION: &str = "wowsreplay";

/// How far a build has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IndexProgress {
    /// Replays looked at, whether or not each one indexed.
    pub done: u64,
    pub total: u64,
    /// Replays whose rows were written.
    pub indexed: u64,
    /// Replays that could not be read or parsed. Expected on a directory
    /// holding a partial recording or a build whose data is absent.
    pub failed: u64,
    /// Replays this pass did not read: already indexed, or known unreadable.
    pub skipped: u64,
}

/// Every replay under `root`, newest first.
///
/// Newest first so a build interrupted part-way has covered the replays most
/// likely to be looked for. Not recursive: the game records into one
/// directory, and walking deeper would index whatever else is under it.
pub fn replays_under(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some(REPLAY_EXTENSION) {
                return None;
            }
            // The live recording, which grows for as long as the battle runs: it
            // has no results yet and is rewritten as the next battle's. Counting
            // it as a failure is how the port used to report an open battle.
            if path.file_name().is_some_and(|name| name == LIVE_REPLAY) {
                return None;
            }
            // A file whose time cannot be read still indexes; it simply sorts
            // as the oldest.
            let modified = entry.metadata().and_then(|meta| meta.modified()).unwrap_or(std::time::UNIX_EPOCH);
            Some((modified, path))
        })
        .collect();
    // Reversed, so the newest is first.
    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    found.into_iter().map(|(_, path)| path).collect()
}

/// The file the game writes the battle in progress into.
const LIVE_REPLAY: &str = "temp.wowsreplay";

/// How much of the directory a pass re-reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    /// Read what is not indexed yet and leave the rest alone. What the Build
    /// Index button asks for, and what makes a second run cheap.
    FillGaps,
    /// Read everything again, whatever the index already holds. For rows an
    /// earlier pass decoded through constants that did not fit: the egui app's
    /// own `ReindexMode::RefreshAll` (`task/replays.rs:1370`).
    RefreshAll,
}

/// Why a replay contributed nothing.
#[derive(Debug, thiserror::Error)]
pub enum IndexOneError {
    #[error("the replay could not be read")]
    Parse(String),
    #[error("the rows could not be stored")]
    Store(#[from] wows_toolkit_config::index::rows::IndexError),
}

/// Parses one replay and stores what it says.
///
/// Blocking: the parse walks the packet stream and the writes are awaited on
/// the runtime the caller provides. Run it off whatever thread is drawing.
pub fn index_one(
    runtime: &tokio::runtime::Runtime,
    pool: &SqlitePool,
    path: &Path,
    game_data: &GameDataCache,
    source_id: SourceId,
    indexed_at: Timestamp,
) -> Result<(), IndexOneError> {
    let parsed = parse_replay(path, game_data, None).map_err(|err| IndexOneError::Parse(err.to_string()))?;
    let battle = &parsed.indexable;

    let context = IndexContext {
        arena_id: battle.arena_id,
        game_mode_id: battle.game_mode_id,
        version_build: battle.version_build,
        source_id,
        replay_path: path.to_path_buf(),
        file_mtime: std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|since| since.as_secs() as i64),
        self_ship_id: battle.self_ship_id,
        results_pending: battle.results_pending,
        indexed_at,
        // Judged as the replay was read: a build whose published mapping is not
        // here yet is read through the nearest one there is, and results read
        // that way are not worth storing.
        fit: battle.constants_fit,
    };

    let rows = map_rows(&battle.normalized, &context);
    runtime.block_on(async {
        // Incremental so a rating an earlier pass computed is not thrown
        // away by one that had no rating table to hand.
        query::upsert_match_with_mode(pool, &rows.objective, IndexWriteMode::Incremental).await?;
        query::upsert_vehicles_with_mode(pool, &rows.vehicles, IndexWriteMode::Incremental, ResultsWrite::Store)
            .await?;
        query::upsert_record_with_mode(pool, &rows.record, IndexWriteMode::Incremental, ResultsWrite::Store).await?;
        Ok::<(), wows_toolkit_config::index::rows::IndexError>(())
    })?;
    Ok(())
}

/// Walks `root` and indexes every replay under it, reporting as it goes.
///
/// One replay's failure does not stop the pass: a directory of a few hundred
/// replays reliably holds one the parser cannot read, and stopping there
/// would leave the rest unindexed.
///
/// `cancel` is checked between replays, so a build stops within one parse of
/// being asked to.
pub fn build_index(
    runtime: &tokio::runtime::Runtime,
    pool: &SqlitePool,
    root: &Path,
    game_data: &GameDataCache,
    cancel: &Arc<AtomicBool>,
    mode: IndexMode,
    report: impl Fn(IndexProgress),
) -> Result<IndexProgress, wows_toolkit_config::index::rows::IndexError> {
    let now = Timestamp::now();
    let source_id = runtime.block_on(query::ensure_default_source(pool, root, now))?;

    // What is already in, so a second run costs a directory listing rather than
    // a re-parse of every replay in it. The egui app skips on the same ledger
    // (`task/replays.rs:1434`).
    let indexed: std::collections::HashSet<String> = match mode {
        IndexMode::FillGaps => runtime.block_on(query::record_paths_in_source(pool, source_id)).unwrap_or_default(),
        IndexMode::RefreshAll => std::collections::HashSet::new(),
    };
    // And what could not be read before, so an unreadable file is not re-read on
    // every pass. Keyed by path and modification time, so a replaced file is
    // tried again.
    let mut unindexable = runtime.block_on(Unindexable::load(pool));
    let mut ledger_grew = false;

    let replays = replays_under(root);
    let mut progress = IndexProgress { total: replays.len() as u64, ..IndexProgress::default() };
    report(progress);

    for path in replays {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let known = indexed.contains(path.to_string_lossy().as_ref());
        if known || unindexable.contains(&path) {
            progress.skipped += 1;
            progress.done += 1;
            report(progress);
            continue;
        }

        match index_one(runtime, pool, &path, game_data, source_id, Timestamp::now()) {
            Ok(()) => progress.indexed += 1,
            Err(err) => {
                tracing::warn!("replay index: {} contributed nothing: {err}", path.display());
                progress.failed += 1;
                ledger_grew |= unindexable.insert(&path);
            }
        }
        progress.done += 1;
        report(progress);
    }

    if ledger_grew && let Err(err) = runtime.block_on(unindexable.save(pool)) {
        tracing::warn!("replay index: the unreadable-file ledger was not written: {err}");
    }

    Ok(progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"not a real replay").expect("the file can be written");
        path
    }

    #[test]
    fn only_replays_are_walked() {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        touch(dir.path(), "a.wowsreplay");
        touch(dir.path(), "notes.txt");
        touch(dir.path(), "temp.wowsreplay.part");
        // The battle in progress: it has no results yet and is rewritten as the
        // next battle's, so counting it as a failure is all it could contribute.
        touch(dir.path(), "temp.wowsreplay");
        std::fs::create_dir(dir.path().join("sub")).expect("the directory can be made");
        touch(&dir.path().join("sub"), "b.wowsreplay");

        let found = replays_under(dir.path());

        // The game records into one directory; what is under it belongs to
        // whoever put it there.
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("a.wowsreplay"));
    }

    #[test]
    fn a_directory_that_is_not_there_yields_nothing() {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");

        assert!(replays_under(&dir.path().join("was-never-created")).is_empty());
    }

    #[test]
    fn replays_are_walked_newest_first() {
        let dir = tempfile::tempdir().expect("a temporary directory can be made");
        let older = touch(dir.path(), "older.wowsreplay");
        let newer = touch(dir.path(), "newer.wowsreplay");
        // Set explicitly: two files written in the same moment carry the same
        // time, and the order would then be the directory's own.
        let base = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        filetime::set_file_mtime(&older, filetime::FileTime::from_system_time(base)).expect("mtime is settable");
        filetime::set_file_mtime(
            &newer,
            filetime::FileTime::from_system_time(base + std::time::Duration::from_secs(3600)),
        )
        .expect("mtime is settable");

        let found = replays_under(dir.path());

        assert_eq!(found.len(), 2);
        assert!(found[0].ends_with("newer.wowsreplay"), "a build stopped part-way covers the newest first");
        assert!(found[1].ends_with("older.wowsreplay"));
    }
}

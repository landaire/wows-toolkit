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
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use jiff::Timestamp;
use sqlx::SqlitePool;
use wows_core::game_types::MatchContributionVersion;
use wows_toolkit_config::index::query;
use wows_toolkit_config::index::rows::IndexError;
use wows_toolkit_config::index::rows::IndexWriteMode;
use wows_toolkit_config::index::rows::ResultsWrite;
use wows_toolkit_config::index::rows::SourceId;
use wows_toolkit_config::index::rows::SourceKind;
use wows_toolkit_config::index::unindexable::Unindexable;
use wows_toolkit_viewmodel::index_rows::ConstantsFit;
use wows_toolkit_viewmodel::index_rows::IndexContext;
use wows_toolkit_viewmodel::index_rows::map_rows;
use wows_toolkit_viewmodel::index_rows::pr_repairs;
use wows_toolkit_viewmodel::index_rows::rateable_match_contribution;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;

use crate::load::GameDataCache;
use crate::load::IndexableBattle;
use crate::load::parse_replay;
use crate::replay_folder::ReplayFolder;

/// The extension the game records replays under.
const REPLAY_EXTENSION: &str = "wowsreplay";

/// The index source a listed replay is written under.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SourceScope {
    /// The install's replays directory: the single Live source, which the
    /// Player Tracker's clan list reads alone.
    Live,
    /// A directory the reader opened, under a source of its own, as the egui
    /// app indexes each directory workspace.
    Folder(PathBuf),
}

impl SourceScope {
    /// The scope's source, created when absent. `live_root` roots the Live
    /// source when this is the first write to it.
    ///
    /// A folder whose directory some other source already owns resolves to
    /// that source, as a directory has one source whatever kind registered
    /// it, unless that source is the Live one: a chosen folder never writes
    /// into it.
    pub async fn ensure(
        &self,
        pool: &SqlitePool,
        live_root: &Path,
        now: Timestamp,
    ) -> Result<SourceId, ScopeSourceError> {
        let Self::Folder(root) = self else {
            return Ok(query::ensure_default_source(pool, live_root, now).await?);
        };

        let name = crate::paths::shortened_path(root);
        let source = query::ensure_source(pool, &name, SourceKind::ImportedDir, root, now).await?;
        if query::live_source_id(pool).await? == Some(source) {
            return Err(ScopeSourceError::LiveRoot { root: root.clone() });
        }

        Ok(source)
    }

    /// The scope's source, or `None` while nothing has created it. Readers
    /// call this rather than [`Self::ensure`]. A folder never reads the Live
    /// source, for the reason [`Self::ensure`] never writes it.
    pub async fn existing(&self, pool: &SqlitePool) -> Result<Option<SourceId>, IndexError> {
        let Self::Folder(root) = self else {
            return query::live_source_id(pool).await;
        };

        let sources = query::list_sources(pool).await?;
        Ok(sources
            .into_iter()
            .find(|source| source.root_path.as_deref() == Some(root))
            .filter(|source| source.kind != SourceKind::Live)
            .map(|source| source.id))
    }
}

/// Why a scope has no source to write under.
#[derive(Debug, thiserror::Error)]
pub enum ScopeSourceError {
    #[error(transparent)]
    Index(#[from] IndexError),
    /// The directory is where the Live source is rooted, which a chosen
    /// folder must not write into.
    #[error("{} is indexed as the game's own replays directory", root.display())]
    LiveRoot { root: PathBuf },
}

/// One replay and the source it is written under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedReplay {
    pub path: PathBuf,
    pub scope: SourceScope,
}

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

impl IndexMode {
    fn write_mode(self) -> IndexWriteMode {
        match self {
            Self::FillGaps => IndexWriteMode::Incremental,
            Self::RefreshAll => IndexWriteMode::Replace,
        }
    }
}

/// Why a replay contributed nothing.
#[derive(Debug, thiserror::Error)]
pub enum IndexOneError {
    #[error("the replay could not be read")]
    Parse(#[source] crate::error::ReplayLoadError),
    #[error("the rows could not be stored")]
    Store(#[from] wows_toolkit_config::index::rows::IndexError),
    #[error("the replay has no source to be stored under")]
    Source(#[from] ScopeSourceError),
}

impl IndexOneError {
    /// Whether the file itself is what failed, and so is worth recording as
    /// unreadable until it changes.
    ///
    /// Missing or broken game data and a database that would not take the
    /// rows are this machine's state, not the file's: recording them would
    /// have later passes skip a good replay. A reader fault is this build's,
    /// and a later build may read the file.
    pub fn is_unreadable_file(&self) -> bool {
        use crate::error::ReplayLoadError;

        match self {
            Self::Parse(ReplayLoadError::Io(_) | ReplayLoadError::VersionParse | ReplayLoadError::Parse(_)) => true,
            Self::Parse(
                ReplayLoadError::UnsupportedVersion { .. }
                | ReplayLoadError::GameData(_)
                | ReplayLoadError::Panicked { .. }
                | ReplayLoadError::LoaderStopped { .. }
                | ReplayLoadError::LoaderGone { .. }
                | ReplayLoadError::AltRefused(_),
            )
            | Self::Store(_)
            | Self::Source(_) => false,
        }
    }
}

/// Parses one replay and stores what it says.
///
/// Blocking: the parse walks the packet stream and the writes are awaited on
/// the runtime the caller provides. Run it off whatever thread is drawing.
#[allow(clippy::too_many_arguments)]
pub fn index_one(
    runtime: &tokio::runtime::Runtime,
    pool: &SqlitePool,
    path: &Path,
    game_data: &GameDataCache,
    personal_rating: Option<&PersonalRatingData>,
    source_id: SourceId,
    indexed_at: Timestamp,
    mode: IndexWriteMode,
) -> Result<(), IndexOneError> {
    // Rated here rather than by the parse: the parse would also rate every row
    // of the player table, which the index never reads.
    let parsed = parse_replay(path, game_data, None).map_err(IndexOneError::Parse)?;
    let file_mtime = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_secs() as i64);
    let stored = StoredReplay { path, file_mtime, source_id, indexed_at, mode };

    runtime.block_on(store_battle(pool, parsed.indexable, personal_rating, &stored))
}

/// Where and how one parsed battle is written.
struct StoredReplay<'a> {
    path: &'a Path,
    file_mtime: Option<i64>,
    source_id: SourceId,
    indexed_at: Timestamp,
    mode: IndexWriteMode,
}

/// Rates `battle` against `personal_rating`, when there is one, and writes
/// its rows.
///
/// A battle written with no table is stored unrated, and
/// [`repair_missing_pr`] rates it once a table arrives.
async fn store_battle(
    pool: &SqlitePool,
    mut battle: IndexableBattle,
    personal_rating: Option<&PersonalRatingData>,
    stored: &StoredReplay<'_>,
) -> Result<(), IndexOneError> {
    if let Some(table) = personal_rating {
        battle.normalized.populate_personal_ratings(table);
        // An unloaded table rates nobody, and its revision would mark the rows rated.
        if table.is_loaded() {
            battle.normalized.populate_match_contribution(table);
        }
    }

    let context = IndexContext {
        arena_id: battle.arena_id,
        game_mode_id: battle.game_mode_id,
        version_build: battle.version_build,
        source_id: stored.source_id,
        replay_path: stored.path.to_path_buf(),
        file_mtime: stored.file_mtime,
        self_ship_id: battle.self_ship_id,
        results_pending: battle.results_pending,
        indexed_at: stored.indexed_at,
        // Judged as the replay was read: a build whose published mapping is not
        // here yet is read through the nearest one there is, and results read
        // that way are not worth storing.
        fit: battle.constants_fit,
    };

    let rows = map_rows(&battle.normalized, &context);
    let results = match battle.constants_fit {
        ConstantsFit::Matched => ResultsWrite::Store,
        ConstantsFit::Mismatched => ResultsWrite::Keep,
    };
    query::write_indexed_replay(
        pool,
        &rows.objective,
        &rows.vehicles,
        &rows.record,
        stored.mode,
        results,
        rows.rating_scope,
    )
    .await?;

    Ok(())
}

/// Rates every stored row that has no rating against `personal_rating`,
/// returning how many rows were filled.
///
/// A stored rating is never replaced: expected values drift, and a battle's
/// number changing month to month would change what a saved `pr < 800`
/// search matches. Scans `replay_record` and `indexed_vehicle`, so run it off
/// the UI thread.
pub async fn repair_missing_pr(pool: &SqlitePool, personal_rating: &PersonalRatingData) -> Result<u64, IndexError> {
    if !personal_rating.is_loaded() {
        return Ok(0);
    }

    let gaps = query::pr_gaps(pool).await?;
    let repairs = pr_repairs(&gaps, personal_rating);

    query::apply_pr_repairs(pool, &repairs).await
}

/// Walks `root` and indexes every replay under it into `source_id`. See
/// [`build_folder_index`].
#[allow(clippy::too_many_arguments)]
pub fn build_index(
    runtime: &tokio::runtime::Runtime,
    pool: &SqlitePool,
    source_id: SourceId,
    root: &Path,
    game_data: &GameDataCache,
    personal_rating: Option<&PersonalRatingData>,
    cancel: &AtomicBool,
    mode: IndexMode,
    report: impl Fn(IndexProgress),
) -> Result<IndexProgress, IndexError> {
    let folder = ReplayFolder::from_dir(root.to_path_buf());

    build_folder_index(runtime, pool, source_id, &folder, game_data, personal_rating, cancel, mode, report)
}

/// Walks `folder` and indexes every replay in it into `source_id`,
/// reporting as it goes. A replay is recorded under the real path it is read
/// from. A record of it under another spelling of that path is renamed rather
/// than read again, and when an overlaid install's game rewrites one, the
/// record of the copy it replaced is dropped, so the battle is held once.
///
/// One replay's failure does not stop the pass: a directory of a few hundred
/// replays reliably holds one the parser cannot read, and stopping there
/// would leave the rest unindexed.
///
/// `cancel` is checked between replays, so a build stops within one parse of
/// being asked to.
#[allow(clippy::too_many_arguments)]
pub fn build_folder_index(
    runtime: &tokio::runtime::Runtime,
    pool: &SqlitePool,
    source_id: SourceId,
    folder: &ReplayFolder,
    game_data: &GameDataCache,
    personal_rating: Option<&PersonalRatingData>,
    cancel: &AtomicBool,
    mode: IndexMode,
    report: impl Fn(IndexProgress),
) -> Result<IndexProgress, IndexError> {
    // What is already in, so a second run costs a directory listing rather than
    // a re-parse of every replay in it. The egui app skips on the same ledger
    // (`task/replays.rs:1434`).
    let rateable_mc = rateable_match_contribution(personal_rating);
    let indexed: std::collections::HashSet<String> = match mode {
        IndexMode::FillGaps => {
            runtime.block_on(query::record_paths_in_source(pool, source_id, rateable_mc)).unwrap_or_default()
        }
        IndexMode::RefreshAll => std::collections::HashSet::new(),
    };
    // And what could not be read before, so an unreadable file is not re-read on
    // every pass. Keyed by path and modification time, so a replaced file is
    // tried again.
    let mut unindexable = runtime.block_on(Unindexable::load(pool));
    let mut ledger_grew = false;
    // Records of a replay that is now indexed under its other spellings, and of the copies it
    // replaced in another layer.
    let mut superseded: Vec<PathBuf> = Vec::new();

    let replays = folder_replays(folder);
    let mut progress = IndexProgress { total: replays.len() as u64, ..IndexProgress::default() };
    report(progress);

    for path in replays {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let spellings = folder.spellings(&path);
        let indexed_here = indexed.contains(path.to_string_lossy().as_ref());
        let indexed_as = if indexed_here {
            None
        } else {
            spellings.iter().find(|other| indexed.contains(other.to_string_lossy().as_ref()))
        };
        // A record the path already has, though stale, is the one to refresh: the read below
        // updates it, and the other spelling's record is forgotten.
        let renamed = match indexed_as {
            Some(stored) => runtime.block_on(query::rename_record(pool, source_id, stored, &path))?,
            None => false,
        };

        let known = indexed_here || renamed;
        if known || unindexable.contains(&path) {
            if known {
                superseded.extend(spellings);
                superseded.extend(folder.layer_copies(&path));
            }
            progress.skipped += 1;
            progress.done += 1;
            report(progress);
            continue;
        }

        let indexed_at = Timestamp::now();
        match index_one(runtime, pool, &path, game_data, personal_rating, source_id, indexed_at, mode.write_mode()) {
            Ok(()) => {
                progress.indexed += 1;
                superseded.extend(spellings);
                superseded.extend(folder.layer_copies(&path));
            }
            Err(err) => {
                tracing::warn!("replay index: {} contributed nothing: {err}", path.display());
                progress.failed += 1;
                if err.is_unreadable_file() {
                    ledger_grew |= unindexable.insert(&path);
                }
            }
        }
        progress.done += 1;
        report(progress);
    }

    if ledger_grew && let Err(err) = runtime.block_on(unindexable.save(pool)) {
        tracing::warn!("replay index: the unreadable-file ledger was not written: {err}");
    }
    if !superseded.is_empty() {
        runtime.block_on(query::forget_records(pool, source_id, &superseded))?;
    }
    if !cancel.load(Ordering::Relaxed) {
        for dir in folder.content_dirs() {
            runtime.block_on(query::mark_replay_records_current(pool, source_id, &dir, rateable_mc))?;
        }
    }

    Ok(progress)
}

/// Whether a replay directly in any of `dirs`, the directories a live folder spans, needs the
/// current parser to refresh it.
pub async fn has_stale_records_in(
    pool: &SqlitePool,
    dirs: &[PathBuf],
    rateable_mc: Option<MatchContributionVersion>,
) -> Result<bool, IndexError> {
    for dir in dirs {
        if query::has_stale_replay_records(pool, dir, rateable_mc).await? {
            return Ok(true);
        }
    }

    Ok(false)
}

/// The replays of `folder`, newest first. A plain directory is walked as [`replays_under`] walks
/// it.
fn folder_replays(folder: &ReplayFolder) -> Vec<PathBuf> {
    if !folder.is_overlaid() {
        return replays_under(folder.path());
    }

    let mut found: Vec<(std::time::SystemTime, PathBuf)> = folder
        .replay_files()
        .into_iter()
        .map(|path| {
            // As in `replays_under`: a time that cannot be read sorts as the oldest.
            let modified = std::fs::metadata(&path).and_then(|meta| meta.modified()).unwrap_or(std::time::UNIX_EPOCH);
            (modified, path)
        })
        .collect();
    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));

    found.into_iter().map(|(_, path)| path).collect()
}

/// Reads each of `replays` and writes everything the index holds for it
/// under its own scope's source, whatever an earlier pass recorded as
/// unreadable.
///
/// `live_root` roots the Live source if this pass is the first to create it;
/// the replays need not be under it. Every scope's source is resolved before
/// any replay is read. `cancel` is checked between replays, as in
/// [`build_index`].
#[allow(clippy::too_many_arguments)]
pub fn reindex_replays(
    runtime: &tokio::runtime::Runtime,
    pool: &SqlitePool,
    live_root: &Path,
    replays: &[ScopedReplay],
    write_mode: IndexWriteMode,
    game_data: &GameDataCache,
    personal_rating: Option<&PersonalRatingData>,
    cancel: &AtomicBool,
    report: impl Fn(IndexProgress),
) -> Result<IndexProgress, ScopeSourceError> {
    let mut progress = IndexProgress { total: replays.len() as u64, ..IndexProgress::default() };
    if replays.is_empty() {
        return Ok(progress);
    }

    let mut sources: std::collections::HashMap<&SourceScope, SourceId> = std::collections::HashMap::new();
    for replay in replays {
        if !sources.contains_key(&replay.scope) {
            let source = runtime.block_on(replay.scope.ensure(pool, live_root, Timestamp::now()))?;
            sources.insert(&replay.scope, source);
        }
    }
    let mut unindexable = runtime.block_on(Unindexable::load(pool));
    let mut ledger_changed = false;
    report(progress);

    for replay in replays {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let path = &replay.path;
        let source_id = sources[&replay.scope];
        match index_one(runtime, pool, path, game_data, personal_rating, source_id, Timestamp::now(), write_mode) {
            // A file that reads is no longer one for later passes to skip.
            Ok(()) => {
                progress.indexed += 1;
                ledger_changed |= unindexable.remove(path);
            }
            Err(err) => {
                tracing::warn!("replay index: {} contributed nothing when re-indexed: {err}", path.display());
                progress.failed += 1;
                if err.is_unreadable_file() {
                    ledger_changed |= unindexable.insert(path);
                }
            }
        }
        progress.done += 1;
        report(progress);
    }

    if ledger_changed && let Err(err) = runtime.block_on(unindexable.save(pool)) {
        tracing::warn!("replay index: the unreadable-file ledger was not written: {err}");
    }

    Ok(progress)
}

#[cfg(test)]
mod tests {
    use wows_replays::analyzer::battle_controller::BattleResult;
    use wows_replays::types::ArenaId;
    use wows_replays::types::GameParamId;
    use wows_toolkit_viewmodel::rating_breakdown::MatchContributionWithheld;

    use super::*;
    use crate::test_support::FIXTURE_PR_SHIP_ID;
    use crate::test_support::fixture_normalized_battle_report;

    fn touch(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, b"not a real replay").expect("the file can be written");
        path
    }

    /// Expected values for the one ship [`rated_battle`] is played in.
    fn expected_values(win_rate: f64) -> PersonalRatingData {
        let json = format!(
            r#"{{"time":0,"data":{{"{FIXTURE_PR_SHIP_ID}":{{"average_damage_dealt":50000.0,"average_frags":1.0,"win_rate":{win_rate}}}}}}}"#
        );
        let mut table = PersonalRatingData::new();
        table.load_from_bytes(json.as_bytes()).expect("the table is valid JSON");
        table
    }

    /// A won battle with 50,000 damage and one kill. Against
    /// `expected_values(100.0)` that is play exactly at expectation, which
    /// rates 1150.
    fn rated_battle() -> IndexableBattle {
        let ship = GameParamId::from(FIXTURE_PR_SHIP_ID);
        let mut normalized = fixture_normalized_battle_report();
        normalized.metadata.battle_result = Some(BattleResult::Win(0));
        for player in &mut normalized.players {
            player.ship_id = ship;
        }

        IndexableBattle {
            normalized,
            arena_id: ArenaId::new(1),
            game_mode_id: None,
            version_build: Some(100),
            constants_fit: ConstantsFit::Matched,
            self_ship_id: Some(ship),
            results_pending: false,
        }
    }

    /// Writes [`rated_battle`] as an index pass given `personal_rating` would,
    /// returning the source it went under.
    async fn store_rated_battle(
        pool: &SqlitePool,
        path: &Path,
        personal_rating: Option<&PersonalRatingData>,
    ) -> SourceId {
        let now = Timestamp::now();
        let source = query::ensure_default_source(pool, Path::new("G:/wows/replays"), now)
            .await
            .expect("the live source is made");
        let stored = StoredReplay {
            path,
            file_mtime: None,
            source_id: source,
            indexed_at: now,
            mode: IndexWriteMode::Incremental,
        };
        store_battle(pool, rated_battle(), personal_rating, &stored).await.expect("the battle is written");

        source
    }

    /// The owner's rating as the listing reads it.
    async fn listed_rating(pool: &SqlitePool, source: SourceId, path: &Path) -> Option<f64> {
        let summaries = query::row_summaries_for_source(pool, source).await.expect("the summaries read");
        summaries.get(path).expect("the battle is indexed").self_pr
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("a tokio runtime")
    }

    /// A pass given the expected values stores the owner's rating with the
    /// battle, which is what the listing's chip shows.
    #[test]
    fn a_battle_indexed_with_the_expected_values_stores_the_owners_rating() {
        runtime().block_on(async {
            let pool = wows_toolkit_config::test_pool().await;
            let path = Path::new("G:/wows/replays/rated.wowsreplay");
            let table = expected_values(100.0);

            let source = store_rated_battle(&pool, path, Some(&table)).await;

            let rating = listed_rating(&pool, source, path).await.expect("the battle was rated");
            assert!((rating - 1150.0).abs() < 1e-6, "expected 1150, stored {rating}");
        });
    }

    /// A battle indexed before the expected values loaded is stored unrated,
    /// and the repair rates it as indexing with the table would have. A rating
    /// once stored is kept when another table arrives.
    #[test]
    fn a_battle_indexed_before_the_expected_values_is_rated_once_they_arrive() {
        runtime().block_on(async {
            let pool = wows_toolkit_config::test_pool().await;
            let path = Path::new("G:/wows/replays/early.wowsreplay");

            let source = store_rated_battle(&pool, path, None).await;
            assert_eq!(listed_rating(&pool, source, path).await, None, "nothing rated it yet");

            let unloaded = repair_missing_pr(&pool, &PersonalRatingData::new()).await.expect("the repair runs");
            assert_eq!(unloaded, 0, "a table that has not loaded rates nothing");

            let filled = repair_missing_pr(&pool, &expected_values(100.0)).await.expect("the repair runs");
            assert!(filled > 0, "the unrated rows were filled");
            let rating = listed_rating(&pool, source, path).await.expect("the battle was rated");
            assert!((rating - 1150.0).abs() < 1e-6, "expected 1150, stored {rating}");

            let refilled = repair_missing_pr(&pool, &expected_values(50.0)).await.expect("the repair runs");
            assert_eq!(refilled, 0, "a stored rating is not replaced");
            assert_eq!(listed_rating(&pool, source, path).await, Some(rating));
        });
    }

    /// [`rated_battle`] as a battle of `game_type` in `match_group`.
    fn battle_of_type(game_type: &str, match_group: &str) -> IndexableBattle {
        let mut battle = rated_battle();
        battle.normalized.metadata.game_type = game_type.to_owned();
        battle.normalized.metadata.match_group = match_group.to_owned();
        battle
    }

    /// MC is rated for Random and Ranked battles only, read from `gameType`,
    /// or from `matchGroup` in a replay too old to carry one.
    #[test]
    fn match_contribution_is_rated_only_for_random_and_ranked_battles() {
        let cases = [
            ("RandomBattle", "pvp", true),
            ("RankedBattle", "ranked", true),
            ("", "ranked", true),
            ("CooperativeBattle", "cooperative", false),
            ("PVEBattle", "pve", false),
            ("ClanBattle", "clan", false),
            ("EventBattle", "event", false),
            ("BrawlBattle", "brawl", false),
            ("TrainingBattle", "training", false),
            ("", "cooperative", false),
        ];
        let table = expected_values(100.0);

        for (game_type, match_group, rated) in cases {
            let mut normalized = battle_of_type(game_type, match_group).normalized;
            normalized.populate_match_contribution(&table);

            let own = normalized.players.iter().find(|player| player.is_self).expect("the report has a self player");
            assert_eq!(own.match_contribution.is_some(), rated, "{game_type:?} in {match_group:?}");
            let withheld = MatchContributionWithheld::of_report(&normalized);
            let expected = (!rated).then_some(MatchContributionWithheld::UnratedBattle);
            assert_eq!(withheld, expected, "{game_type:?} in {match_group:?}");
        }
    }

    /// The owner's MC as the listing reads it.
    async fn listed_match_contribution(pool: &SqlitePool, source: SourceId, path: &Path) -> Option<f64> {
        let summaries = query::row_summaries_for_source(pool, source).await.expect("the summaries read");
        summaries.get(path).expect("the battle is indexed").self_mc
    }

    /// An MC stored for an arena is cleared once the arena is indexed as a
    /// battle type MC does not apply to, even by a pass whose constants do
    /// not fit and that keeps the stored results.
    #[test]
    fn indexing_an_unrated_battle_clears_a_stored_match_contribution() {
        runtime().block_on(async {
            let pool = wows_toolkit_config::test_pool().await;
            let path = Path::new("G:/wows/replays/coop.wowsreplay");
            let table = expected_values(100.0);
            let source = store_rated_battle(&pool, path, Some(&table)).await;
            let rated = listed_match_contribution(&pool, source, path).await;
            assert!(rated.is_some(), "the Random battle was rated");

            let mut unrated = battle_of_type("CooperativeBattle", "cooperative");
            unrated.constants_fit = ConstantsFit::Mismatched;
            let stored = StoredReplay {
                path,
                file_mtime: None,
                source_id: source,
                indexed_at: Timestamp::now(),
                mode: IndexWriteMode::Incremental,
            };
            store_battle(&pool, unrated, Some(&table), &stored).await.expect("the battle is written");

            assert_eq!(listed_match_contribution(&pool, source, path).await, None);
        });
    }

    /// Only a file that will not read is recorded as unreadable. Game data
    /// this machine lacks, or a store that refused the rows, is put right
    /// without touching the file, so later passes must try it again.
    #[test]
    fn only_a_file_that_will_not_read_is_recorded_as_unreadable() {
        use crate::error::ReplayLoadError;

        assert!(IndexOneError::Parse(ReplayLoadError::Parse("garbage".to_owned())).is_unreadable_file());
        assert!(IndexOneError::Parse(ReplayLoadError::Io("truncated".to_owned())).is_unreadable_file());
        assert!(!IndexOneError::Parse(ReplayLoadError::UnsupportedVersion { build: 300 }).is_unreadable_file());
        assert!(!IndexOneError::Parse(ReplayLoadError::GameData("broken dump".to_owned())).is_unreadable_file());
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

    /// A replay the game rewrote under an overlay is held once: the record of the copy it replaced
    /// in the install is dropped once the rewritten one is indexed.
    #[cfg(unix)]
    #[test]
    fn a_copy_up_replaces_the_record_of_the_install_copy() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let base = std::fs::canonicalize(tmp.path()).expect("the directory resolves");
        let install = base.join("install");
        let dirs = wgc_launch::RuntimeDirs::new(&base.join("app"));
        let state = dirs.install_state(&install);
        std::fs::create_dir_all(state.upper().join("replays")).expect("the upper directory is made");
        std::fs::write(state.backend_record(), "fuse-overlayfs").expect("the backend is recorded");
        std::fs::create_dir_all(install.join("replays")).expect("the replays directory is made");
        let lower = touch(&install.join("replays"), "battle.wowsreplay");
        let upper = touch(&state.upper().join("replays"), "battle.wowsreplay");
        let folder = ReplayFolder::from_install(&install, Some(&dirs));

        let rt = runtime();
        let pool = rt.block_on(wows_toolkit_config::test_pool());
        rt.block_on(store_rated_battle(&pool, &lower, None));
        let source = rt.block_on(store_rated_battle(&pool, &upper, None));

        let cancel = AtomicBool::new(false);
        let built = build_folder_index(
            &rt,
            &pool,
            source,
            &folder,
            &GameDataCache::without_install(),
            None,
            &cancel,
            IndexMode::FillGaps,
            |_| {},
        )
        .expect("the pass runs");

        assert_eq!(built.skipped, 1, "the rewritten copy is already indexed");
        let summaries = rt.block_on(query::row_summaries_for_source(&pool, source)).expect("the summaries read");
        let held: Vec<&PathBuf> = summaries.keys().collect();
        assert_eq!(held, vec![&upper], "only the copy the game sees is held");
    }

    /// An install named through a link stays indexed once the launcher runs it from an overlay:
    /// its replays are listed under the name it was given, a record stored under its resolved
    /// name is moved rather than read again, and every replay, the one only the overlay holds
    /// included, is held once however many passes run.
    #[cfg(unix)]
    #[test]
    fn an_install_named_through_a_link_stays_indexed_once_overlaid() {
        let tmp = tempfile::tempdir().expect("a temporary directory");
        let base = std::fs::canonicalize(tmp.path()).expect("the directory resolves");
        let real = base.join("real");
        std::fs::create_dir_all(real.join("install/replays")).expect("the install is made");
        let link = base.join("link");
        std::os::unix::fs::symlink(&real, &link).expect("the link is made");
        let install = link.join("install");
        let dirs = wgc_launch::RuntimeDirs::new(&link.join("app"));
        let older = touch(&install.join("replays"), "older.wowsreplay");
        let newer = touch(&install.join("replays"), "newer.wowsreplay");

        let rt = runtime();
        let pool = rt.block_on(wows_toolkit_config::test_pool());
        let cancel = AtomicBool::new(false);
        let pass = |folder: &ReplayFolder, source: SourceId| {
            let game_data = GameDataCache::without_install();
            build_folder_index(&rt, &pool, source, folder, &game_data, None, &cancel, IndexMode::FillGaps, |_| {})
                .expect("the pass runs")
        };
        let held = |source: SourceId| {
            let summaries = rt.block_on(query::row_summaries_for_source(&pool, source)).expect("the summaries read");
            let mut held: Vec<PathBuf> = summaries.into_keys().collect();
            held.sort();
            held
        };

        // The files are not real replays, so storing stands in for the read of a pass.
        rt.block_on(store_rated_battle(&pool, &older, None));
        let source = rt.block_on(store_rated_battle(&pool, &newer, None));
        let plain = ReplayFolder::from_install(&install, Some(&dirs));
        let built = pass(&plain, source);
        assert_eq!((built.skipped, built.indexed, built.failed), (2, 0, 0), "indexed before any overlay");

        // The launcher's first run from an overlay. A battle indexed while an overlay named the
        // install by its resolved name, and one the game wrote into the overlay.
        let state = dirs.install_state(&std::fs::canonicalize(&install).expect("the install resolves"));
        std::fs::create_dir_all(state.upper().join("replays")).expect("the upper directory is made");
        std::fs::write(state.backend_record(), "kernel-bwrap").expect("the backend is recorded");
        let respelled = touch(&install.join("replays"), "respelled.wowsreplay");
        rt.block_on(store_rated_battle(&pool, &real.join("install/replays/respelled.wowsreplay"), None));
        let upper = touch(&state.upper().join("replays"), "upper.wowsreplay");
        rt.block_on(store_rated_battle(&pool, &upper, None));

        let folder = ReplayFolder::from_install(&install, Some(&dirs));
        assert!(folder.is_overlaid());
        let mut listed = folder.replay_files();
        listed.sort();
        let mut expected = vec![older, newer, respelled, upper];
        expected.sort();
        assert_eq!(listed, expected, "the install's replays keep the name it was given");

        for _ in 0..2 {
            let built = pass(&folder, source);
            assert_eq!((built.skipped, built.indexed, built.failed), (4, 0, 0), "nothing is read again");
            assert_eq!(held(source), expected, "every listed replay is held once");
        }
    }
}

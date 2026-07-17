//! Background replay parse into a [`ReplayReportModel`].
//!
//! Reuses the shared pipeline end to end: `ReplayFile::from_file` -> a
//! packet walk through `BattleWorld` -> `NormalizedBattleReport::from_battle_report`
//! -> `ReplayReportModel::from_normalized`. None of those steps are
//! reimplemented here; this module only wires them together and caches the
//! expensive game-data load.
//!
//! **Scope.** Every replay is parsed against the exact build it was recorded
//! on, resolved from its own `clientVersionFromExe` -- never a "latest
//! installed build" heuristic, which would misparse (or outright fail to
//! parse) a replay recorded on an older build still present under `bin/` next
//! to a newer one. A replay whose build has no matching `bin/<build>`
//! directory in the install returns [`ReplayLoadError::UnsupportedVersion`]
//! rather than attempting to resolve it from a `wows-data-mgr` dump -- that
//! fallback path is deferred to a later milestone.
//!
//! **Caching.** Building the game VFS and [`GameMetadataProvider`] for one
//! build loads that whole build's data and is expensive; [`GameDataCache`]
//! loads each build once, lazily, on the first replay that needs it, and
//! reuses it for every later replay recorded on the same build. Clone the
//! cache (cheap: an `Arc` around a lock) to share it across views.
//!
//! **Versioned constants.** Per-build `CONSUMABLE_IDS`/`BATTLE_STAGES`
//! overrides (fetched from the wows-constants repo by the egui app and cached
//! on disk) are read fresh on every parse from the shared disk cache at
//! `wows_toolkit_config::storage_dir()/constants_{build}.json`. A missing or
//! unreadable cache file is not an error: it falls back to
//! `serde_json::Value::Null`, which the downstream resolvers already treat as
//! "no overrides"; only a warning is logged.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use gpui::App;
use gpui::AppContext;
use gpui::Task;
use serde_json::Value;
use wows_battle_world::BattleWorld;
use wows_battle_world::ids::ShotTracking;
use wows_replay_insights::battle_report::NormalizedBattleReport;
use wows_replays::ParseError;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::game_constants::GameConstants;
use wows_replays::packet2::Parser;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;

use super::model::ReplayReportModel;

/// Reasons a replay could not become a [`ReplayReportModel`].
#[derive(Debug, thiserror::Error)]
pub enum ReplayLoadError {
    /// Reading the replay file off disk failed (not found, permissions, truncated).
    #[error("failed to read replay file: {0}")]
    Io(String),
    /// The replay's `clientVersionFromExe` did not parse into a build number.
    #[error("could not parse the replay's client version")]
    VersionParse,
    /// The replay's build has no matching `bin/<build>` directory in the
    /// install. Loading an uninstalled build's data from a dump is not
    /// implemented yet.
    #[error("replay build {build} is not installed (no bin/{build} directory found)")]
    UnsupportedVersion { build: u32 },
    /// The game VFS, `GameParams`, or entity scripts could not be loaded.
    #[error("failed to load game data: {0}")]
    GameData(String),
    /// The replay's header/metadata block itself was corrupt or malformed.
    #[error("failed to parse replay: {0}")]
    Parse(String),
}

/// One installed build's `GameMetadataProvider` and base `GameConstants`
/// (before a replay's own versioned-constants overrides are merged in).
/// Building this loads that whole build's game data; see [`GameDataCache`].
struct LoadedGameData {
    provider: Arc<GameMetadataProvider>,
    base_constants: GameConstants,
}

impl LoadedGameData {
    /// Loads `build`'s game data from `wows_dir`. Callers are expected to
    /// have already checked `build` is present under `bin/` (see
    /// [`GameDataCache::get_or_load_build`]); this only reports the errors
    /// that can still occur while actually reading that build's files.
    fn load_build(wows_dir: &Path, build: u32) -> Result<Self, ReplayLoadError> {
        let vfs = wowsunpack::game_data::build_game_vfs_for_build(wows_dir, build)
            .map_err(|e| ReplayLoadError::GameData(e.to_string()))?;
        let provider =
            Arc::new(GameMetadataProvider::from_vfs(&vfs).map_err(|e| ReplayLoadError::GameData(e.to_string()))?);
        let base_constants = GameConstants::from_vfs(&vfs);

        Ok(Self { provider, base_constants })
    }
}

/// Lazily loads and caches each installed build's game data, keyed by build
/// number, so repeated [`spawn_parse`] calls for replays on the same build
/// never reload that build's whole game VFS. Cheap to clone (an `Arc` around
/// a lock); share one instance across every replay-inspector view that opens
/// replays.
#[derive(Clone)]
pub struct GameDataCache {
    wows_dir: PathBuf,
    loaded: Arc<Mutex<HashMap<u32, Arc<LoadedGameData>>>>,
}

impl GameDataCache {
    pub fn new(wows_dir: PathBuf) -> Self {
        Self { wows_dir, loaded: Arc::new(Mutex::new(HashMap::new())) }
    }

    /// Returns `build`'s cached game data, loading and caching it first if
    /// this is the first replay on that build. Checks `build` is actually
    /// installed before attempting the (expensive) VFS build, so a replay
    /// from an uninstalled build reports [`ReplayLoadError::UnsupportedVersion`]
    /// rather than a generic [`ReplayLoadError::GameData`].
    fn get_or_load_build(&self, build: u32) -> Result<Arc<LoadedGameData>, ReplayLoadError> {
        let mut guard = self.loaded.lock().expect("game data cache mutex poisoned");
        if let Some(data) = guard.get(&build) {
            return Ok(Arc::clone(data));
        }

        let available = wowsunpack::game_data::list_available_builds(&self.wows_dir)
            .map_err(|e| ReplayLoadError::GameData(e.to_string()))?;
        if !available.contains(&build) {
            return Err(ReplayLoadError::UnsupportedVersion { build });
        }

        let data = Arc::new(LoadedGameData::load_build(&self.wows_dir, build)?);
        guard.insert(build, Arc::clone(&data));
        Ok(data)
    }
}

/// Reads the disk-cached versioned constants for `build`
/// (`constants_{build}.json` under the shared storage directory). Falls back
/// to `Value::Null` (no overrides) whenever the storage directory is
/// unavailable, the file is missing, or it fails to parse, logging a warning
/// in each case; this is a degraded-but-valid parse, not an error.
fn load_versioned_constants(build: u32) -> Value {
    let Some(storage_dir) = wows_toolkit_config::storage_dir() else {
        tracing::warn!(build, "no storage directory available; parsing without versioned constants");
        return Value::Null;
    };

    let path = storage_dir.join(format!("constants_{build}.json"));
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::warn!(build, path = %path.display(), error = %e, "no cached versioned constants; parsing without overrides");
            return Value::Null;
        }
    };

    match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(e) => {
            tracing::warn!(build, path = %path.display(), error = %e, "cached versioned constants did not parse; parsing without overrides");
            Value::Null
        }
    }
}

/// Reads and parses one replay file into a presentation-ready
/// [`ReplayReportModel`], resolving `game_data` to the build the replay was
/// actually recorded on (never a "latest installed" guess; see the module
/// doc). Synchronous and CPU-bound; callers run it off the UI thread (see
/// [`spawn_parse`]).
fn parse_replay(path: &Path, game_data: &GameDataCache) -> Result<ReplayReportModel, ReplayLoadError> {
    let replay_file = ReplayFile::from_file(path).map_err(|report| {
        let is_io = matches!(report.current_context(), ParseError::Io(_));
        let message = format!("{report:?}");
        if is_io { ReplayLoadError::Io(message) } else { ReplayLoadError::Parse(message) }
    })?;
    let meta = &replay_file.meta;

    let version = Version::try_from_client_exe(&meta.clientVersionFromExe).ok_or(ReplayLoadError::VersionParse)?;
    let build = version.build_number().ok_or(ReplayLoadError::VersionParse)?;
    let loaded = game_data.get_or_load_build(build)?;

    let constants_json = load_versioned_constants(build);

    let mut constants = loaded.base_constants.clone();
    constants.merge_replay_constants(&constants_json, version);
    wowsunpack::game_constants::apply_version_consumables(constants.common_mut(), version);

    let mut world = BattleWorld::new(meta, loaded.provider.as_ref(), Some(&constants));
    world.set_shot_tracking(ShotTracking::Untracked);

    let mut parser = Parser::with_version(loaded.provider.entity_specs(), version);
    let mut remaining = replay_file.packet_data.as_slice();
    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(packet) => world.process(&packet),
            Err(_) => break,
        }
    }
    world.finish();

    let report = world.into_report();
    let normalized =
        NormalizedBattleReport::from_battle_report(&report, meta, loaded.provider.as_ref(), &constants_json);
    let model = ReplayReportModel::from_normalized(&normalized, meta, loaded.provider.as_ref(), &constants_json);

    Ok(model)
}

/// Parses `path` into a [`ReplayReportModel`] on the background executor
/// (`cx.background_spawn`, not the tokio bridge -- this work is CPU-bound, not
/// async I/O). `game_data` lazily loads and caches each replay's own build's
/// data on first use.
pub fn spawn_parse(
    path: PathBuf,
    game_data: GameDataCache,
    cx: &App,
) -> Task<Result<ReplayReportModel, ReplayLoadError>> {
    cx.background_spawn(async move { parse_replay(&path, &game_data) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses a real replay against a real game install. Needs local game
    /// data + a replay recorded on an installed build, which cannot be
    /// cheaply fabricated (see `model.rs`'s equivalent ignored test). Exercises
    /// the exact `GameDataCache` path `spawn_parse` uses in production: the
    /// build is resolved from the replay's own `clientVersionFromExe`, not
    /// supplied by the test, so this also proves the per-build resolution
    /// works against a real install. Run with:
    ///
    /// ```text
    /// WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR="E:\WoWs\World_of_Warships" \
    /// WOWS_REPLAY_INSPECTOR_LOAD_TEST_REPLAY="E:\WoWs\World_of_Warships\replays\some.wowsreplay" \
    /// cargo test -p wows-toolkit-gpui -- --ignored parse_replay_against_a_real_current_version_install_produces_a_sane_model
    /// ```
    #[test]
    #[ignore = "needs a local game install + a replay recorded on an installed build; see the doc comment for the run command"]
    fn parse_replay_against_a_real_current_version_install_produces_a_sane_model() {
        let wows_dir = std::env::var("WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR")
            .expect("set WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR to a WoWs install directory");
        let replay_path = std::env::var("WOWS_REPLAY_INSPECTOR_LOAD_TEST_REPLAY")
            .expect("set WOWS_REPLAY_INSPECTOR_LOAD_TEST_REPLAY to a .wowsreplay path recorded on an installed build");

        let game_data = GameDataCache::new(PathBuf::from(&wows_dir));

        let model = parse_replay(Path::new(&replay_path), &game_data).expect("failed to parse the replay");

        assert!(!model.rows.is_empty(), "expected at least one player row");
        let self_row = model.rows.iter().find(|r| r.is_self).expect("expected a self player row");
        println!(
            "parsed {} rows; self player {:?} observed_damage={}",
            model.rows.len(),
            self_row.display_name,
            self_row.observed_damage
        );
        let any_nonzero_damage =
            model.rows.iter().any(|r| r.observed_damage > 0 || r.actual_damage.is_some_and(|d| d > 0));
        assert!(any_nonzero_damage, "expected at least one row with nonzero damage");
    }
}

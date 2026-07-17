//! Background replay parse into a [`ReplayReportModel`].
//!
//! Reuses the shared pipeline end to end: `ReplayFile::from_file` -> a
//! packet walk through `BattleWorld` -> `NormalizedBattleReport::from_battle_report`
//! -> `ReplayReportModel::from_normalized`. None of those steps are
//! reimplemented here; this module only wires them together and caches the
//! expensive game-data load.
//!
//! **Scope.** Only the currently installed game build is supported. A replay
//! recorded by a different build returns [`ReplayLoadError::UnsupportedVersion`]
//! rather than attempting to resolve older data from a `wows-data-mgr` dump --
//! that fallback path is deferred to a later milestone.
//!
//! **Caching.** Building the game VFS and [`GameMetadataProvider`] loads the
//! whole game install and is expensive; [`GameDataCache`] loads it once,
//! lazily, on the first parse and reuses it for every later replay. Clone the
//! cache (cheap: an `Arc` around a lock) to share it across views.
//!
//! **Versioned constants.** Per-build `CONSUMABLE_IDS`/`BATTLE_STAGES`
//! overrides (fetched from the wows-constants repo by the egui app and cached
//! on disk) are read fresh on every parse from the shared disk cache at
//! `wows_toolkit_config::storage_dir()/constants_{build}.json`. A missing or
//! unreadable cache file is not an error: it falls back to
//! `serde_json::Value::Null`, which the downstream resolvers already treat as
//! "no overrides"; only a warning is logged.

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
    /// The replay's build does not match the installed game build. Loading an
    /// older build's data from a dump is not implemented yet.
    #[error("replay build {build} is not the installed game version; only the currently installed build is supported")]
    UnsupportedVersion { build: u32 },
    /// The game VFS, `GameParams`, or entity scripts could not be loaded.
    #[error("failed to load game data: {0}")]
    GameData(String),
    /// The replay's header/metadata block itself was corrupt or malformed.
    #[error("failed to parse replay: {0}")]
    Parse(String),
}

/// The installed game build's `GameMetadataProvider` and base `GameConstants`
/// (before a replay's own versioned-constants overrides are merged in).
/// Building this loads the whole game install; see [`GameDataCache`].
struct LoadedGameData {
    provider: Arc<GameMetadataProvider>,
    base_constants: GameConstants,
    build: u32,
}

impl LoadedGameData {
    /// Loads the game install's latest available build, mirroring
    /// `wowsunpack::game_data::build_game_vfs`'s "highest `bin/<n>` directory"
    /// selection, but keeping the resolved build number around so callers can
    /// compare it against a replay's own build.
    fn load(wows_dir: &Path) -> Result<Self, ReplayLoadError> {
        let builds = wowsunpack::game_data::list_available_builds(wows_dir)
            .map_err(|e| ReplayLoadError::GameData(e.to_string()))?;
        let build = *builds.last().ok_or_else(|| {
            ReplayLoadError::GameData(format!("no installed game builds found under {}", wows_dir.display()))
        })?;

        Self::load_build(wows_dir, build)
    }

    /// Loads a specific build number's game data, independent of which build
    /// happens to be "latest" in `bin/`. Used directly by [`Self::load`] and
    /// by tests that need to target a build with a known-good replay on disk.
    fn load_build(wows_dir: &Path, build: u32) -> Result<Self, ReplayLoadError> {
        let vfs = wowsunpack::game_data::build_game_vfs_for_build(wows_dir, build)
            .map_err(|e| ReplayLoadError::GameData(e.to_string()))?;
        let provider =
            Arc::new(GameMetadataProvider::from_vfs(&vfs).map_err(|e| ReplayLoadError::GameData(e.to_string()))?);
        let base_constants = GameConstants::from_vfs(&vfs);

        Ok(Self { provider, base_constants, build })
    }
}

/// Lazily loads and caches the installed game build's data so repeated
/// [`spawn_parse`] calls never reload the whole game VFS. Cheap to clone
/// (an `Arc` around a lock); share one instance across every replay-inspector
/// view that opens replays.
#[derive(Clone)]
pub struct GameDataCache {
    wows_dir: PathBuf,
    loaded: Arc<Mutex<Option<Arc<LoadedGameData>>>>,
}

impl GameDataCache {
    pub fn new(wows_dir: PathBuf) -> Self {
        Self { wows_dir, loaded: Arc::new(Mutex::new(None)) }
    }

    fn get_or_load(&self) -> Result<Arc<LoadedGameData>, ReplayLoadError> {
        let mut guard = self.loaded.lock().expect("game data cache mutex poisoned");
        if let Some(data) = guard.as_ref() {
            return Ok(Arc::clone(data));
        }
        let data = Arc::new(LoadedGameData::load(&self.wows_dir)?);
        *guard = Some(Arc::clone(&data));
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
/// [`ReplayReportModel`], using `game_data`'s already-loaded provider and base
/// constants. Synchronous and CPU-bound; callers run it off the UI thread
/// (see [`spawn_parse`]).
fn parse_replay(path: &Path, game_data: &LoadedGameData) -> Result<ReplayReportModel, ReplayLoadError> {
    let replay_file = ReplayFile::from_file(path).map_err(|report| {
        let is_io = matches!(report.current_context(), ParseError::Io(_));
        let message = format!("{report:?}");
        if is_io { ReplayLoadError::Io(message) } else { ReplayLoadError::Parse(message) }
    })?;
    let meta = &replay_file.meta;

    let version = Version::try_from_client_exe(&meta.clientVersionFromExe).ok_or(ReplayLoadError::VersionParse)?;
    let build = version.build_number().ok_or(ReplayLoadError::VersionParse)?;
    if build != game_data.build {
        return Err(ReplayLoadError::UnsupportedVersion { build });
    }

    let constants_json = load_versioned_constants(build);

    let mut constants = game_data.base_constants.clone();
    constants.merge_replay_constants(&constants_json, version);
    wowsunpack::game_constants::apply_version_consumables(constants.common_mut(), version);

    let mut world = BattleWorld::new(meta, game_data.provider.as_ref(), Some(&constants));
    world.set_shot_tracking(ShotTracking::Untracked);

    let mut parser = Parser::with_version(game_data.provider.entity_specs(), version);
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
        NormalizedBattleReport::from_battle_report(&report, meta, game_data.provider.as_ref(), &constants_json);
    let model = ReplayReportModel::from_normalized(&normalized, meta, game_data.provider.as_ref(), &constants_json);

    Ok(model)
}

/// Parses `path` into a [`ReplayReportModel`] on the background executor
/// (`cx.background_spawn`, not the tokio bridge -- this work is CPU-bound, not
/// async I/O). `game_data` lazily loads and caches the installed game build's
/// data on first use.
pub fn spawn_parse(
    path: PathBuf,
    game_data: GameDataCache,
    cx: &App,
) -> Task<Result<ReplayReportModel, ReplayLoadError>> {
    cx.background_spawn(async move {
        let loaded = game_data.get_or_load()?;
        parse_replay(&path, &loaded)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses a real replay against a real game install. Needs local game
    /// data + a replay recorded on a known build, which cannot be cheaply
    /// fabricated (see `model.rs`'s equivalent ignored test), so this targets
    /// a specific build directly via `LoadedGameData::load_build` rather than
    /// `GameDataCache`'s "latest installed build" heuristic: an install can
    /// have a newer build already downloaded (e.g. after an auto-update)
    /// before any replay has actually been recorded on it, in which case
    /// "latest" would not match any replay on disk yet even though the
    /// install is otherwise perfectly current. Run with:
    ///
    /// ```text
    /// WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR="E:\WoWs\World_of_Warships" \
    /// WOWS_REPLAY_INSPECTOR_LOAD_TEST_BUILD=12668706 \
    /// WOWS_REPLAY_INSPECTOR_LOAD_TEST_REPLAY="E:\WoWs\World_of_Warships\replays\some.wowsreplay" \
    /// cargo test -p wows-toolkit-gpui -- --ignored parse_replay_against_a_real_current_version_install_produces_a_sane_model
    /// ```
    #[test]
    #[ignore = "needs a local game install + a replay recorded on that build; see the doc comment for the run command"]
    fn parse_replay_against_a_real_current_version_install_produces_a_sane_model() {
        let wows_dir = std::env::var("WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR")
            .expect("set WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR to a WoWs install directory");
        let build: u32 = std::env::var("WOWS_REPLAY_INSPECTOR_LOAD_TEST_BUILD")
            .expect("set WOWS_REPLAY_INSPECTOR_LOAD_TEST_BUILD to the build number that produced the replay")
            .parse()
            .expect("WOWS_REPLAY_INSPECTOR_LOAD_TEST_BUILD must be a u32");
        let replay_path = std::env::var("WOWS_REPLAY_INSPECTOR_LOAD_TEST_REPLAY")
            .expect("set WOWS_REPLAY_INSPECTOR_LOAD_TEST_REPLAY to a .wowsreplay path recorded on that build");

        let game_data = LoadedGameData::load_build(Path::new(&wows_dir), build)
            .expect("failed to load game data for the given build");

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

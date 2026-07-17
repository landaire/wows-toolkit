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
//! loads each build once -- either from [`spawn_startup_preload`] at app
//! startup for the current install's build, or lazily on the first replay
//! that needs some other build -- and reuses it for every later replay
//! recorded on the same build. The load itself runs outside any cache-wide
//! lock, so opening replays from two different builds concurrently does not
//! serialize one behind the other; only same-build opens dedupe onto one
//! load. Clone the cache (cheap: an `Arc` around a lock) to share it across
//! views.
//!
//! **GameParams disk cache.** Parsing `GameParams.data` out of the game
//! files is most of a build's load cost. [`GameMetadataProvider::from_vfs`]
//! is only called once per build ever, the first time any install parses it;
//! the parsed params are then written to `game_params_{build}.bin` under
//! [`wows_toolkit_config::storage_dir`] (see [`game_params_bin_path`]), and
//! every later load -- this session or a future one, from either this app or
//! the egui app, since both write the identical cache file -- deserializes
//! that file instead of re-walking the VFS.
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
use std::sync::OnceLock;

use gettext::Catalog;
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
use wowsunpack::game_params::cache as game_params_cache;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::vfs::VfsPath;

use super::model::ReplayReportModel;

/// Reasons a replay could not become a [`ReplayReportModel`].
#[derive(Debug, Clone, thiserror::Error)]
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

/// Path to the versioned GameParams cache for `build`, matching the egui
/// app's `game_params_bin_path` (`util/game_params.rs`) exactly -- same file
/// name, same directory -- so both apps share one on-disk cache.
fn game_params_bin_path(build: u32) -> PathBuf {
    let filename = format!("game_params_{build}.bin");
    if let Some(storage_dir) = wows_toolkit_config::storage_dir() {
        storage_dir.join(filename)
    } else {
        PathBuf::from(filename)
    }
}

/// Loads the English gettext translation catalog for `build` from the live
/// install (`bin/{build}/res/texts/en/LC_MESSAGES/global.mo`), matching the
/// egui app's `WowsData::reload_translations` (`data/wows_data.rs`) minus its
/// locale-preference and dump-directory fallbacks -- this port has neither a
/// locale setting nor dump-directory support yet, so English from the live
/// install is the only path. A missing or unparsable catalog is not fatal:
/// ship/map names simply keep showing their untranslated raw form (see
/// `browser_view.rs`'s translation fallback), and this is only logged.
fn load_translations_catalog(wows_dir: &Path, build: u32) -> Option<Catalog> {
    let mo_path = wows_dir.join(format!("bin/{build}/res/texts/en/LC_MESSAGES/global.mo"));
    let file = match std::fs::File::open(&mo_path) {
        Ok(file) => file,
        Err(e) => {
            tracing::warn!(build, path = %mo_path.display(), error = %e, "no English translation catalog for this build");
            return None;
        }
    };
    match Catalog::parse(file) {
        Ok(catalog) => Some(catalog),
        Err(e) => {
            tracing::warn!(build, path = %mo_path.display(), error = %e, "failed to parse translation catalog");
            None
        }
    }
}

/// One installed build's `GameMetadataProvider` and base `GameConstants`
/// (before a replay's own versioned-constants overrides are merged in).
/// Building this loads that whole build's game data; see [`GameDataCache`].
pub struct LoadedGameData {
    provider: Arc<GameMetadataProvider>,
    base_constants: GameConstants,
    vfs: VfsPath,
}

impl LoadedGameData {
    /// The loaded build's metadata provider (ship/module/consumable
    /// GameParams, entity specs, asset lookups) -- the handle later
    /// milestones use to translate listing labels and resolve icons.
    pub fn provider(&self) -> &Arc<GameMetadataProvider> {
        &self.provider
    }

    /// This build's base `GameConstants`, before a specific replay's own
    /// versioned-constants overrides are merged in (see `parse_replay`).
    pub fn base_constants(&self) -> &GameConstants {
        &self.base_constants
    }

    /// This build's VFS -- the handle `icons::IconCache::populate_from_rows`/
    /// `populate_nation_flags` read GUI asset bytes (ship-class, captain-skill,
    /// achievement, ribbon, consumable, nation flag) from, without a second
    /// game-data load.
    pub fn vfs(&self) -> &VfsPath {
        &self.vfs
    }

    /// Loads `build`'s game data from `wows_dir`. Callers are expected to
    /// have already checked `build` is present under `bin/` (see
    /// [`GameDataCache::get_or_load_build`]); this only reports the errors
    /// that can still occur while actually reading that build's files.
    fn load_build(wows_dir: &Path, build: u32) -> Result<Self, ReplayLoadError> {
        let vfs = wowsunpack::game_data::build_game_vfs_for_build(wows_dir, build)
            .map_err(|e| ReplayLoadError::GameData(e.to_string()))?;
        let provider = Self::load_provider(&vfs, build)?;
        if let Some(catalog) = load_translations_catalog(wows_dir, build) {
            provider.set_translations(catalog);
        }
        let provider = Arc::new(provider);
        let base_constants = GameConstants::from_vfs(&vfs);

        Ok(Self { provider, base_constants, vfs })
    }

    /// Loads `build`'s `GameMetadataProvider`, preferring the on-disk
    /// GameParams cache (see the module doc) over a full VFS parse. A cache
    /// miss (first load ever for this build, on either app) parses from the
    /// VFS and writes the cache for next time; a write failure is logged but
    /// not fatal, since the freshly parsed provider is still usable this
    /// session.
    fn load_provider(vfs: &VfsPath, build: u32) -> Result<GameMetadataProvider, ReplayLoadError> {
        let cache_path = game_params_bin_path(build);

        if let Some(params) = game_params_cache::load(&cache_path) {
            tracing::debug!(build, path = %cache_path.display(), "loaded GameParams from disk cache");
            return GameMetadataProvider::from_params_with_vfs(params, vfs)
                .map_err(|e| ReplayLoadError::GameData(e.to_string()));
        }

        tracing::info!(build, "no GameParams disk cache; parsing from game files");
        let provider = GameMetadataProvider::from_vfs(vfs).map_err(|e| ReplayLoadError::GameData(e.to_string()))?;
        let params: Vec<_> = provider.params().iter().map(|param| Arc::unwrap_or_clone(Arc::clone(param))).collect();
        if let Err(e) = game_params_cache::save(&cache_path, &params) {
            tracing::warn!(build, path = %cache_path.display(), error = %e, "failed to write GameParams disk cache");
        }

        Ok(provider)
    }
}

/// One build's load outcome, filled in exactly once. `Arc`-shared so every
/// caller waiting on the same build's slot (see [`GameDataCache::get_or_load_build`])
/// observes the same result without holding any lock while the load itself
/// runs.
type BuildSlot = OnceLock<Result<Arc<LoadedGameData>, ReplayLoadError>>;

/// Lazily loads and caches each installed build's game data, keyed by build
/// number, so repeated [`spawn_parse`] calls for replays on the same build
/// never reload that build's whole game VFS. Cheap to clone (an `Arc` around
/// a lock); share one instance across every replay-inspector view that opens
/// replays.
///
/// The map lock only ever guards inserting/looking up a build's [`BuildSlot`];
/// it is released before the (multi-second) VFS + `GameParams` load runs, so
/// opening two different builds concurrently does not serialize one behind
/// the other. Same-build concurrent opens still dedupe onto that build's
/// single `OnceLock`, which loads once and hands every waiter the same
/// result. A poisoned map lock (some other panic while holding it very
/// briefly) is recovered via the poisoned guard's inner value rather than
/// propagated as a hard panic, so a single bad load can never take down every
/// future open for the session.
#[derive(Clone)]
pub struct GameDataCache {
    wows_dir: PathBuf,
    loaded: Arc<Mutex<HashMap<u32, Arc<BuildSlot>>>>,
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
    ///
    /// A failed load is not cached permanently: its slot is dropped from the
    /// map afterward so a later open (e.g. once a transient I/O error clears,
    /// or the build gets installed mid-session) gets to retry instead of
    /// replaying the same cached failure forever.
    fn get_or_load_build(&self, build: u32) -> Result<Arc<LoadedGameData>, ReplayLoadError> {
        let slot = {
            let mut guard = self.loaded.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            Arc::clone(guard.entry(build).or_insert_with(|| Arc::new(OnceLock::new())))
        };

        let result = slot.get_or_init(|| Self::load_build_checked(&self.wows_dir, build)).clone();

        if result.is_err() {
            let mut guard = self.loaded.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.remove(&build);
        }

        result
    }

    /// Runs the actual (expensive) load for `build`, outside the map lock.
    /// Wrapped in `catch_unwind` so a panic deep in VFS/`GameParams` parsing
    /// (a malformed idx or params blob) surfaces as
    /// [`ReplayLoadError::GameData`] instead of unwinding out of the
    /// `OnceLock` initializer and the background task that runs this.
    fn load_build_checked(wows_dir: &Path, build: u32) -> Result<Arc<LoadedGameData>, ReplayLoadError> {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let available = wowsunpack::game_data::list_available_builds(wows_dir)
                .map_err(|e| ReplayLoadError::GameData(e.to_string()))?;
            if !available.contains(&build) {
                return Err(ReplayLoadError::UnsupportedVersion { build });
            }
            LoadedGameData::load_build(wows_dir, build)
        }));

        match outcome {
            Ok(result) => result.map(Arc::new),
            Err(panic) => Err(ReplayLoadError::GameData(format!("game data load panicked: {}", panic_message(&panic)))),
        }
    }
}

/// Outcome of [`spawn_startup_preload`]: the current installed build's game
/// data, loaded once at app startup rather than on the first replay click.
/// The browser and every [`ReplayPanel`](super::panel::ReplayPanel) consume
/// this once it settles into `Ready`/`Failed`; `spawn_parse` itself does not
/// need it directly, since preloading through the shared [`GameDataCache`]
/// already warms the same per-build slot it reads from.
#[derive(Clone)]
pub enum GameDataStatus {
    Loading,
    Ready(Arc<LoadedGameData>),
    Failed(String),
}

/// Determines the current installed build (the highest build number under
/// `wows_dir/bin`, matching [`wowsunpack::game_data::build_game_vfs`]'s own
/// "latest build" choice) and loads it through `game_data`. Loading through
/// the shared cache -- rather than a separate one-off load -- is what lets
/// `spawn_parse` skip straight to an already-warm slot for replays recorded
/// on this build.
fn preload_current_build(wows_dir: &Path, game_data: &GameDataCache) -> Result<Arc<LoadedGameData>, String> {
    let available = wowsunpack::game_data::list_available_builds(wows_dir).map_err(|e| e.to_string())?;
    let build =
        *available.last().ok_or_else(|| format!("no installed builds found under {}/bin", wows_dir.display()))?;
    game_data.get_or_load_build(build).map_err(|e| e.to_string())
}

/// Kicks off the startup game-data preload on the background executor.
/// Callers await the returned task (typically via `cx.spawn`, to fold the
/// result back into an entity's state) rather than blocking on it.
pub fn spawn_startup_preload(wows_dir: PathBuf, game_data: GameDataCache, cx: &App) -> Task<GameDataStatus> {
    cx.background_spawn(async move {
        match preload_current_build(&wows_dir, &game_data) {
            Ok(loaded) => GameDataStatus::Ready(loaded),
            Err(reason) => {
                tracing::warn!(wows_dir = %wows_dir.display(), %reason, "startup game-data preload failed");
                GameDataStatus::Failed(reason)
            }
        }
    })
}

/// Extracts a human-readable message out of a caught panic payload, falling
/// back to a generic message for payloads that are neither `&str` nor
/// `String` (the two types `panic!`/`.expect()` actually produce).
fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic".to_string()
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

/// One parsed replay: the presentation model, plus the [`LoadedGameData`] it
/// was parsed against. Callers that need to resolve icon bytes for the
/// model's rows (ship-class, achievement, ribbon, captain-skill, consumable,
/// nation flag) read them from `game_data.vfs()` -- the exact VFS the parse
/// itself used, so resolving icons never triggers a second game-data load.
pub struct ParsedReplay {
    pub model: ReplayReportModel,
    pub game_data: Arc<LoadedGameData>,
    /// Pretty-printed replay header/metadata JSON (`ReplayFile::raw_meta`),
    /// for the debug-mode raw-metadata viewer (mirrors the egui app's
    /// `ui.replay.debug.raw_metadata` button, `ui/replay_parser/mod.rs`
    /// ~3018-3034). Falls back to the unparsed raw string when it does not
    /// parse as JSON, rather than panicking like the egui app's `.expect()`.
    pub raw_metadata_json: String,
    /// Pretty-printed battle-results JSON (`BattleReport::battle_results`),
    /// for the debug-mode raw-results viewer (mirrors the egui app's
    /// `ui.replay.debug.raw_json`, `mod.rs` ~3039-3060). `None` when the
    /// replay carries no battle-results packet (e.g. the player left before
    /// the server sent one).
    pub raw_results_json: Option<String>,
}

/// Pretty-prints `raw` as JSON when it parses, falling back to the original
/// string unchanged otherwise. Used for the debug-mode raw viewers; unlike
/// the egui app's equivalent `serde_json::from_str(...).expect(...)` calls,
/// this never panics on a malformed payload.
fn pretty_json_or_raw(raw: &str) -> String {
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| raw.to_string())
}

/// Reads and parses one replay file into a presentation-ready
/// [`ReplayReportModel`], resolving `game_data` to the build the replay was
/// actually recorded on (never a "latest installed" guess; see the module
/// doc). Synchronous and CPU-bound; callers run it off the UI thread (see
/// [`spawn_parse`]).
fn parse_replay(path: &Path, game_data: &GameDataCache) -> Result<ParsedReplay, ReplayLoadError> {
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
    let raw_results_json = report.battle_results().map(pretty_json_or_raw);
    let normalized =
        NormalizedBattleReport::from_battle_report(&report, meta, loaded.provider.as_ref(), &constants_json);
    let model = ReplayReportModel::from_normalized(
        &normalized,
        meta,
        loaded.provider.as_ref(),
        &constants_json,
        report.game_chat(),
    );
    let raw_metadata_json = pretty_json_or_raw(&replay_file.raw_meta);

    Ok(ParsedReplay { model, game_data: loaded, raw_metadata_json, raw_results_json })
}

/// Parses `path` into a [`ParsedReplay`] on the background executor
/// (`cx.background_spawn`, not the tokio bridge -- this work is CPU-bound, not
/// async I/O). `game_data` lazily loads and caches each replay's own build's
/// data on first use.
pub fn spawn_parse(path: PathBuf, game_data: GameDataCache, cx: &App) -> Task<Result<ParsedReplay, ReplayLoadError>> {
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

        let parsed = parse_replay(Path::new(&replay_path), &game_data).expect("failed to parse the replay");
        let model = &parsed.model;

        assert!(!model.rows.is_empty(), "expected at least one player row");
        let self_row = model.rows.iter().find(|r| r.is_self).expect("expected a self player row");
        println!(
            "parsed {} rows; self player {:?} observed_damage={}; {} chat messages",
            model.rows.len(),
            self_row.display_name,
            self_row.observed_damage,
            model.chat.len()
        );
        let any_nonzero_damage =
            model.rows.iter().any(|r| r.observed_damage > 0 || r.actual_damage.is_some_and(|d| d > 0));
        assert!(any_nonzero_damage, "expected at least one row with nonzero damage");
    }

    /// A directory under the OS temp dir with an empty `bin/` subfolder, so
    /// `list_available_builds` succeeds (no builds installed) instead of
    /// failing outright on a missing `bin/` dir. Removed on drop.
    struct EmptyGameDir(PathBuf);

    impl EmptyGameDir {
        fn new(unique: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("wt-gpui-load-test-{unique}"));
            std::fs::create_dir_all(dir.join("bin")).expect("failed to create empty test bin/ dir");
            Self(dir)
        }
    }

    impl Drop for EmptyGameDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Fix under test: a panic that occurs while some other caller briefly
    /// holds `GameDataCache`'s map-level lock must not turn every later
    /// `get_or_load_build` call into a hard panic. Before the per-build
    /// `OnceLock` restructuring, this cache used a single `Mutex` held across
    /// the whole (expensive) load, and looked it up with
    /// `.expect("game data cache mutex poisoned")`; poisoning it here would
    /// have made this test's second call panic too.
    #[test]
    fn get_or_load_build_survives_a_poisoned_map_lock() {
        let game_dir = EmptyGameDir::new("poison");
        let cache = GameDataCache::new(game_dir.0.clone());

        let loaded = Arc::clone(&cache.loaded);
        let poisoned = std::thread::spawn(move || {
            let _guard = loaded.lock().unwrap();
            panic!("deliberately poisoning the cache lock for the test");
        })
        .join();
        assert!(poisoned.is_err(), "the spawned thread was expected to panic");

        match cache.get_or_load_build(1) {
            Err(ReplayLoadError::UnsupportedVersion { build: 1 }) => {}
            Err(other) => panic!("expected UnsupportedVersion despite the poisoned lock, got: {other}"),
            Ok(_) => panic!("expected UnsupportedVersion despite the poisoned lock, got Ok"),
        }
    }

    /// Fix under test: a failed build load is not cached forever -- the next
    /// call for the same (still-uninstalled) build retries rather than
    /// short-circuiting on a stale cached error.
    #[test]
    fn get_or_load_build_retries_after_a_failed_load() {
        let game_dir = EmptyGameDir::new("retry");
        let cache = GameDataCache::new(game_dir.0.clone());

        assert!(matches!(cache.get_or_load_build(7), Err(ReplayLoadError::UnsupportedVersion { build: 7 })));
        assert!(matches!(cache.get_or_load_build(7), Err(ReplayLoadError::UnsupportedVersion { build: 7 })));
        assert!(cache.loaded.lock().unwrap().is_empty(), "a failed build's slot should not linger in the cache");
    }

    #[test]
    fn panic_message_reads_str_and_string_payloads_and_falls_back_otherwise() {
        let str_payload: Box<dyn std::any::Any + Send> = Box::new("boom");
        assert_eq!(panic_message(&str_payload), "boom");

        let string_payload: Box<dyn std::any::Any + Send> = Box::new(String::from("also boom"));
        assert_eq!(panic_message(&string_payload), "also boom");

        let other_payload: Box<dyn std::any::Any + Send> = Box::new(42_i32);
        assert_eq!(panic_message(&other_payload), "unknown panic");
    }
}

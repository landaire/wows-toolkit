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
use gpui_kit::App;
use gpui_kit::AppContext;
use gpui_kit::Task;
use serde_json::Value;
use wows_battle_world::BattleWorld;
use wows_battle_world::ids::ShotTracking;
use wows_data_mgr::cas_vfs::BuildCas;
use wows_replay_insights::battle_report::NormalizedBattleReport;
use wows_replay_insights::fire_chance::analysis::EffectiveFireChance;
use wows_replays::ParseError;
use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::game_constants::GameConstants;
use wows_replays::packet2::Parser;
use wows_replays::types::ArenaId;
use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::replay_export::Match as ExportedMatch;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::cache as game_params_cache;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
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

/// The English catalogue out of a dumped build, which carries its texts under
/// `translations/` rather than `res/texts/`.
fn load_dump_translations(cas: &BuildCas) -> Option<Catalog> {
    let mo_path = cas.derived_path("translations/en/LC_MESSAGES/global.mo")?;
    let file = std::fs::File::open(&mo_path)
        .inspect_err(|err| tracing::warn!(path = %mo_path.display(), error = %err, "no catalog in this dump"))
        .ok()?;
    Catalog::parse(file)
        .inspect_err(|err| tracing::warn!(path = %mo_path.display(), error = ?err, "a dump's catalog would not parse"))
        .ok()
}

/// One installed build's `GameMetadataProvider` and base `GameConstants`
/// (before a replay's own versioned-constants overrides are merged in).
/// Building this loads that whole build's game data; see [`GameDataCache`].
pub struct LoadedGameData {
    provider: Arc<GameMetadataProvider>,
    base_constants: GameConstants,
    vfs: VfsPath,
    /// The build this data is for. A dump of a different build of the same
    /// version reports its own, which is what a per-build cache is keyed by.
    build: u32,
}

impl LoadedGameData {
    /// The loaded build's metadata provider (ship/module/consumable
    /// GameParams, entity specs, asset lookups) -- the handle later
    /// milestones use to translate listing labels and resolve icons.
    pub fn provider(&self) -> &Arc<GameMetadataProvider> {
        &self.provider
    }

    /// The build this data is for.
    pub fn build(&self) -> u32 {
        self.build
    }

    /// This build's base `GameConstants`, before a specific replay's own
    /// versioned-constants overrides are merged in (see `parse_replay`).
    pub fn base_constants(&self) -> &GameConstants {
        &self.base_constants
    }

    /// This build's VFS -- the handle `icons::IconCache::populate_from_rows`
    /// reads GUI asset bytes (ship-class, captain-skill, achievement, ribbon,
    /// consumable) from, without a second game-data load.
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

        Ok(Self { provider, base_constants, vfs, build })
    }

    /// Loads a build out of the game-data cache, for a replay recorded on one
    /// the install no longer has.
    ///
    /// The cache is what the Settings tab downloads, validates and repairs, and
    /// what the egui app writes as it loads builds. Without this the port can
    /// open only replays from the installed build, which is most of a week's
    /// worth and none of last year's.
    ///
    /// `build` is what the replay asked for; the dump that answers may be a
    /// different one of the same version, because build numbers are per server
    /// (the China client ships its own for the same major.minor.patch). The
    /// params are cached under the dump's own build number for that reason: one
    /// server's parameters must not answer for another's.
    fn load_dump(dump_dir: &Path, build: u32) -> Result<Self, ReplayLoadError> {
        let cas = BuildCas::open(dump_dir).ok_or_else(|| {
            ReplayLoadError::GameData(format!("no metadata.toml in the dump at {}", dump_dir.display()))
        })?;
        let dump_build = cas.metadata().build;
        if dump_build != build {
            tracing::info!(
                requested_build = build,
                dump_build,
                version = %cas.metadata().version,
                "serving a build from a different dump of the same version"
            );
        }

        let vfs = cas.vfs();
        // The dump's own rkyv cache is free where a parse is seconds and
        // hundreds of megabytes, so it is preferred; `load_game_params` then
        // falls back to this app's cache and finally to parsing the dump's VFS.
        let params = match cas.derived_path("game_params.rkyv").as_deref().and_then(game_params_cache::load) {
            Some(params) => params,
            None => load_game_params(&vfs, dump_build)?,
        };
        let provider = GameMetadataProvider::from_params_with_vfs(params, &vfs)
            .map_err(|e| ReplayLoadError::GameData(e.to_string()))?;
        if let Some(catalog) = load_dump_translations(&cas) {
            provider.set_translations(catalog);
        }

        let base_constants = GameConstants::from_vfs(&vfs);

        Ok(Self { provider: Arc::new(provider), base_constants, vfs, build: dump_build })
    }

    /// Loads `build`'s `GameMetadataProvider`, preferring the on-disk
    /// GameParams cache (see the module doc) over a full VFS parse. A cache
    /// miss (first load ever for this build, on either app) parses from the
    /// VFS and writes the cache for next time; a write failure is logged but
    /// not fatal, since the freshly parsed provider is still usable this
    /// session.
    fn load_provider(vfs: &VfsPath, build: u32) -> Result<GameMetadataProvider, ReplayLoadError> {
        GameMetadataProvider::from_params_with_vfs(load_game_params(vfs, build)?, vfs)
            .map_err(|e| ReplayLoadError::GameData(e.to_string()))
    }
}

/// `build`'s decoded game parameters, preferring the on-disk cache (see the
/// module doc) over a full VFS parse. A cache miss (first load ever for this
/// build, on either app) parses from the VFS and writes the cache for next
/// time; a write failure is logged but not fatal, since the freshly parsed
/// parameters are still usable this session.
pub fn load_game_params(vfs: &VfsPath, build: u32) -> Result<Vec<Param>, ReplayLoadError> {
    let cache_path = game_params_bin_path(build);

    if let Some(params) = game_params_cache::load(&cache_path) {
        tracing::debug!(build, path = %cache_path.display(), "loaded GameParams from disk cache");
        return Ok(params);
    }

    tracing::info!(build, "no GameParams disk cache; parsing from game files");
    let provider = GameMetadataProvider::from_vfs(vfs).map_err(|e| ReplayLoadError::GameData(e.to_string()))?;
    let params: Vec<_> = provider.params().iter().map(|param| Arc::unwrap_or_clone(Arc::clone(param))).collect();
    if let Err(e) = game_params_cache::save(&cache_path, &params) {
        tracing::warn!(build, path = %cache_path.display(), error = %e, "failed to write GameParams disk cache");
    }

    Ok(params)
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
    /// Where the dumped builds are, for a replay the install cannot answer.
    ///
    /// `None` only when there is no storage directory at all, which is the one
    /// case nothing can be cached anywhere.
    dump_base: Option<PathBuf>,
    /// Whether a build loaded out of the install is written to that cache, which
    /// is the `auto_dump_game_data` setting.
    auto_dump: bool,
    loaded: Arc<Mutex<HashMap<u32, Arc<BuildSlot>>>>,
}

impl GameDataCache {
    pub fn new(wows_dir: PathBuf) -> Self {
        Self {
            wows_dir,
            dump_base: wows_toolkit_config::game_data_dump_base(),
            auto_dump: false,
            loaded: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Whether loading a build out of the install also writes it to the cache,
    /// so the replays recorded on it still open after the game updates.
    ///
    /// This is what the `auto_dump_game_data` checkbox governs. Off by default,
    /// which is the stored default too: a dump is gigabytes.
    pub fn with_auto_dump(mut self, auto_dump: bool) -> Self {
        self.auto_dump = auto_dump;
        self
    }

    /// Points the cache at the directory the reader chose for it, which is the
    /// `game_data_cache_dir` row both apps share. An empty string is the
    /// default location.
    pub fn with_cache_dir(mut self, custom: &str) -> Self {
        self.dump_base = wows_toolkit_config::game_data_dump_base_with_override(custom);
        self
    }

    /// Whether this build could be read at all: installed, already loaded, or in
    /// the game-data cache.
    ///
    /// Asked before anything is loaded, so a listing can say which of its
    /// replays nothing can open. `version` is the replay's own, for the
    /// cross-server fallback the cache index applies.
    pub fn can_read_build(&self, build: u32, version: Option<&str>) -> bool {
        if self.loaded_build(build).is_some() {
            return true;
        }
        if wowsunpack::game_data::list_available_builds(&self.wows_dir)
            .is_ok_and(|available| available.contains(&build))
        {
            return true;
        }
        self.dump_base.as_deref().and_then(|base| dump_for_build(base, build, version)).is_some()
    }

    /// `build`'s game data if it is already loaded, without loading it.
    ///
    /// For a caller that would like a name but will not pay a build load for
    /// it: the Search tab's results can span years of replays, and loading
    /// every build they were recorded on to label a column would cost far
    /// more than the labels are worth.
    pub fn loaded_build(&self, build: u32) -> Option<Arc<LoadedGameData>> {
        let slot = {
            let guard = self.loaded.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            Arc::clone(guard.get(&build)?)
        };
        slot.get()?.as_ref().ok().cloned()
    }

    /// The newest build already open, whatever the caller was asking about.
    ///
    /// For drawing a map before a replay has been read: map art is the live
    /// install's for all but a removed map, and a hover has to put something
    /// on screen long before the build a replay was recorded on could be
    /// loaded. `None` before the startup preload has finished, which is the
    /// case a caller shows nothing for rather than waiting.
    pub fn newest_loaded(&self) -> Option<Arc<LoadedGameData>> {
        let builds: Vec<u32> = {
            let guard = self.loaded.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.keys().copied().collect()
        };
        builds.into_iter().max().and_then(|build| self.loaded_build(build))
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
    pub fn get_or_load_build(&self, build: u32) -> Result<Arc<LoadedGameData>, ReplayLoadError> {
        self.get_or_load_build_for(build, None)
    }

    /// As [`Self::get_or_load_build`], with the version of the replay that is
    /// asking.
    ///
    /// The version is what identifies the data a replay needs when the exact
    /// build is not cached: build numbers are per server, so the China client
    /// ships a different one for the same `major.minor.patch` and its dump serves
    /// just as well. A caller with no version gets the exact build or nothing.
    pub fn get_or_load_build_for(
        &self,
        build: u32,
        version: Option<&Version>,
    ) -> Result<Arc<LoadedGameData>, ReplayLoadError> {
        let hint = version.map(|version| format!("{}.{}.{}", version.major, version.minor, version.patch));
        let slot = {
            let mut guard = self.loaded.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            Arc::clone(guard.entry(build).or_insert_with(|| Arc::new(OnceLock::new())))
        };

        let result = slot
            .get_or_init(|| Self::load_build_checked(&self.wows_dir, self.dump_base.as_deref(), build, hint.as_deref()))
            .clone();

        // Written after the load rather than during it: the dump reads the same
        // files, and a reader waiting for a replay should not wait for gigabytes
        // of copying first.
        if result.is_ok() && self.auto_dump {
            self.dump_installed_build(build);
        }

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
    /// Writes `build` into the game-data cache, if it is the installed build and
    /// the cache has no dump of it yet.
    ///
    /// Only the installed build: a build that came out of the cache is already
    /// there, and there is nothing else to dump from. The version comes from the
    /// install's own `preferences.xml`, which is what names the data being
    /// copied, and a dump already present is left alone.
    fn dump_installed_build(&self, build: u32) {
        let Some(dump_base) = self.dump_base.clone() else { return };
        let Some(version) = installed_version(&self.wows_dir) else {
            tracing::warn!("auto-dump: the install names no version, so nothing is dumped");
            return;
        };
        if version.build_number() != Some(build) {
            return;
        }

        let version_str = format!("{}.{}.{}", version.major, version.minor, version.patch);
        if wows_data_mgr::dump::dump_exists(&dump_base, &version_str, build) {
            return;
        }

        let wows_dir = self.wows_dir.clone();
        // On a thread of its own: this walks the whole install and writes
        // gigabytes, and nothing waits for the result.
        std::thread::Builder::new()
            .name("auto-dump-game-data".to_owned())
            .spawn(move || {
                match wows_data_mgr::dump::dump_renderer_data(&wows_dir, build, &version_str, &dump_base, None, true) {
                    Ok(()) => {
                        tracing::info!(build, version = %version_str, "auto-dump: the build is now cached");
                        copy_constants_into_dump(&dump_base, &version_str, build);
                    }
                    Err(err) => tracing::warn!(build, version = %version_str, error = ?err, "auto-dump failed"),
                }
            })
            .map(|_| ())
            .unwrap_or_else(|err| tracing::warn!(error = %err, "auto-dump: no thread to dump on"));
    }

    fn load_build_checked(
        wows_dir: &Path,
        dump_base: Option<&Path>,
        build: u32,
        version: Option<&str>,
    ) -> Result<Arc<LoadedGameData>, ReplayLoadError> {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let available = wowsunpack::game_data::list_available_builds(wows_dir)
                .map_err(|e| ReplayLoadError::GameData(e.to_string()))?;
            if available.contains(&build) {
                return LoadedGameData::load_build(wows_dir, build);
            }

            // Not installed: the game-data cache is asked next, which is what it
            // is kept for.
            match dump_base.and_then(|base| dump_for_build(base, build, version)) {
                Some(dump_dir) => LoadedGameData::load_dump(&dump_dir, build),
                None => Err(ReplayLoadError::UnsupportedVersion { build }),
            }
        }));

        match outcome {
            Ok(result) => result.map(Arc::new),
            Err(panic) => Err(ReplayLoadError::GameData(format!("game data load panicked: {}", panic_message(&panic)))),
        }
    }
}

/// The version the install says it is, from its own `preferences.xml`.
///
/// This is what names a dump of it. `None` when the file is absent or names
/// something that will not parse, in which case nothing can be dumped under a
/// name that would be found again.
pub(crate) fn installed_version(wows_dir: &Path) -> Option<Version> {
    let preferences = std::fs::read_to_string(wows_dir.join("preferences.xml")).ok()?;
    let version = super::browser_view::last_server_version(&preferences)?;
    Version::try_from_client_exe(&version)
}

/// Puts this app's versioned constants in the dump beside the build, so a later
/// read of that dump decodes with the same mappings this one did.
fn copy_constants_into_dump(dump_base: &Path, version: &str, build: u32) {
    let constants = load_versioned_constants(build);
    if constants.is_null() {
        return;
    }
    let dump_dir = wows_data_mgr::dump::dump_dir(dump_base, version, build);
    match serde_json::to_vec_pretty(&constants) {
        Ok(bytes) => {
            if let Err(err) = std::fs::write(dump_dir.join("constants.json"), bytes) {
                tracing::warn!(build, error = %err, "auto-dump: the constants were not written beside the dump");
            }
        }
        Err(err) => tracing::warn!(build, error = %err, "auto-dump: the constants would not serialize"),
    }
}

/// The dumped build that answers for `build`, if the cache holds one.
///
/// Exact by build number where the cache has it. Failing that, the index's own
/// version fallback picks the nearest build of the same version, which is how a
/// replay from another server's client is served (`BuildsIndex::resolve_build`).
/// A cache written before `builds.toml` existed is read by its directory name,
/// which ends in the build number.
fn dump_for_build(dump_base: &Path, build: u32, version: Option<&str>) -> Option<PathBuf> {
    let index = wows_data_mgr::builds::BuildsIndex::load(&dump_base.join("builds.toml"));
    if let Some((entry, exact)) = index.resolve_build(build, version) {
        if !exact {
            tracing::warn!(build, served_by = entry.build, version = %entry.version, "no exact dump for this build");
        }
        return Some(dump_base.join(&entry.dir));
    }

    let suffix = format!("_{build}");
    std::fs::read_dir(dump_base).ok()?.flatten().map(|entry| entry.path()).find(|path| {
        path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.ends_with(&suffix))
            && path.join("metadata.toml").exists()
    })
}

/// Drops the per-build caches for builds nothing can open any more.
///
/// A `game_params_<build>.bin` is hundreds of megabytes and one is written per
/// build ever opened, so a year of game updates leaves gigabytes behind
/// (`util/game_params.rs`'s `cleanup_stale_caches` is the egui app's equivalent).
///
/// A build is kept when the install has it or the game-data cache has a dump for
/// it: this port opens both, and a dumped build's cache is as expensive to rebuild
/// as an installed one's. The egui app keeps only installed builds, which is the
/// one deliberate difference here.
pub fn prune_stale_caches(wows_dir: &Path, dump_base: Option<&Path>) {
    let Some(storage_dir) = wows_toolkit_config::storage_dir() else { return };

    let mut keep: std::collections::HashSet<u32> =
        wowsunpack::game_data::list_available_builds(wows_dir).unwrap_or_default().into_iter().collect();
    if let Some(base) = dump_base {
        let index = wows_data_mgr::builds::BuildsIndex::load(&base.join("builds.toml"));
        keep.extend(index.builds.iter().map(|entry| entry.build));
        // A dump written before the index existed is named for its build.
        if let Ok(entries) = std::fs::read_dir(base) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(build) = name.to_string_lossy().rsplit('_').next().and_then(|tail| tail.parse::<u32>().ok())
                else {
                    continue;
                };
                if entry.path().join("metadata.toml").exists() {
                    keep.insert(build);
                }
            }
        }
    }

    // Nothing to compare against is not licence to delete everything: an install
    // that could not be listed and no cache means this pass knows nothing.
    if keep.is_empty() {
        return;
    }

    // The cache from before these were keyed by build, which no build can use.
    let _ = std::fs::remove_file(storage_dir.join("game_params.bin"));

    let Ok(entries) = std::fs::read_dir(&storage_dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let stale = stale_cache_build(&name).is_some_and(|build| !keep.contains(&build));
        if stale && let Err(err) = std::fs::remove_file(entry.path()) {
            tracing::warn!(file = %name, error = %err, "a stale per-build cache could not be dropped");
        }
    }
}

/// The build a per-build cache file belongs to, if it is one.
fn stale_cache_build(name: &str) -> Option<u32> {
    let rest = name.strip_prefix("game_params_").and_then(|rest| rest.strip_suffix(".bin"));
    let rest = rest.or_else(|| name.strip_prefix("constants_").and_then(|rest| rest.strip_suffix(".json")));
    rest?.parse().ok()
}

/// Outcome of [`spawn_startup_preload`]: the current installed build's game
/// data, loaded once at app startup rather than on the first replay click.
/// The browser and every [`ReplayPanel`](super::panel::ReplayPanel) consume
/// this once it settles into `Ready`/`Failed`; `spawn_parse` itself does not
/// need it directly, since preloading through the shared [`GameDataCache`]
/// already warms the same per-build slot it reads from.
#[derive(Clone)]
pub enum GameDataStatus {
    /// `version` is what the install says it is, so the listing can name what it
    /// is waiting for. `None` when `preferences.xml` could not be read, which is
    /// the one case nothing can be named.
    Loading {
        version: Option<String>,
    },
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

/// Reads every recording of one battle into a single report.
///
/// The merge walks all of the streams together so the world sees what any of
/// them saw. A stream that will not parse fails the whole read rather than being
/// dropped: a report that silently left one perspective out would look exactly
/// like one that merged it.
fn merged_report(
    primary: &ReplayFile,
    alts: &[PathBuf],
    loaded: &LoadedGameData,
    constants: &GameConstants,
    version: Version,
) -> Result<wows_battle_world::report::BattleReport, ReplayLoadError> {
    let mut read = Vec::with_capacity(alts.len());
    for path in alts {
        let alt = ReplayFile::from_file(path).map_err(|report| ReplayLoadError::Parse(format!("{report:?}")))?;
        read.push(alt);
    }

    let mut session = wows_battle_world::merged::MergedReplays::new(
        loaded.provider.entity_specs(),
        loaded.provider.as_ref(),
        constants,
        version,
        primary,
        &read,
    )
    .map_err(|err| ReplayLoadError::Parse(err.to_string()))?;

    while session.step().map_err(|err| ReplayLoadError::Parse(err.to_string()))?.is_some() {}
    session.finish();
    Ok(session.into_world().into_report())
}

/// Whether a recording belongs to the battle a report was read from.
///
/// Two answers that both mean no: a different game version, whose packets the
/// same parser would read as something else, and a different battle. `None` for
/// an arena id that could not be read at all, which is not evidence either way
/// and is refused for that reason.
pub(crate) fn alt_belongs(
    primary_version: &str,
    alt_version: &str,
    primary_arena: Option<wows_replays::types::ArenaId>,
    alt_arena: Option<wows_replays::types::ArenaId>,
) -> Result<(), AltRefusal> {
    if primary_version != alt_version {
        return Err(AltRefusal::Version { primary: primary_version.to_owned(), alt: alt_version.to_owned() });
    }
    match (primary_arena, alt_arena) {
        (_, None) => Err(AltRefusal::NoArenaId),
        (Some(primary), Some(alt)) if primary != alt => Err(AltRefusal::OtherBattle { primary, alt }),
        _ => Ok(()),
    }
}

/// Why another recording was refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AltRefusal {
    #[error("it was recorded on {alt} and this battle on {primary}")]
    Version { primary: String, alt: String },
    #[error("the battle it belongs to could not be read from it")]
    NoArenaId,
    #[error("it is a recording of battle {alt}, not {primary}")]
    OtherBattle { primary: wows_replays::types::ArenaId, alt: wows_replays::types::ArenaId },
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
    /// The battle as the replay index reads it, kept from this parse so
    /// indexing does not walk the packets a second time.
    pub indexable: IndexableBattle,
    /// The match as the Export menu writes it, kept in its debug form: the
    /// debug toggle is a runtime one, so an ordinary export strips a copy
    /// rather than reparsing.
    pub export: ExportedMatch,
    /// What the data-sharing setting needs to decide what this battle
    /// contributes, taken while the report was still open.
    pub shareable: crate::upload::Shareable,
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
    /// The same results with their positional arrays resolved to named
    /// fields, for the debug-mode mapped viewer. `None` for the same reason
    /// the raw payload is.
    pub mapped_results_json: Option<String>,
    /// The recording player's effective fire chance, when the facts behind it
    /// resolve. See
    /// [`wows_replay_insights::fire_chance::sections::compute_fire_chance`].
    pub fire_chance: Option<EffectiveFireChance>,
    /// This battle as the Stats tab records it. `None` when the replay names
    /// no recording player.
    pub session_stat: Option<PerGameStat>,
}

/// What the replay index needs from one parsed battle.
///
/// The figures themselves are all on [`NormalizedBattleReport`]; what is
/// beside it here is what the battle report carries and the normalized form
/// does not.
pub struct IndexableBattle {
    pub normalized: NormalizedBattleReport,
    pub arena_id: ArenaId,
    /// The mode's numeric id, when the table recognises it. `None` for one it
    /// does not, which the re-index hint counts.
    pub game_mode_id: Option<i32>,
    pub version_build: Option<u32>,
    /// Whether the mapping the results were read through belongs to this build.
    /// Results read through one that does not are never stored: the keys they are
    /// read by move between builds, so the figures would be wrong rather than
    /// missing.
    pub constants_fit: wows_toolkit_viewmodel::index_rows::ConstantsFit,
    /// The ship the recording player was in.
    pub self_ship_id: Option<GameParamId>,
    /// Whether the server results are still to come, which is ordinary for a
    /// replay of a battle that has only just ended.
    pub results_pending: bool,
}

/// Where the fire-section cache lives for `build`.
///
/// The same directory the egui app writes, so a build resolved by one app is
/// not re-parsed by the other. `None` when there is no storage directory, in
/// which case resolution still runs and simply is not persisted.
fn fire_section_cache_dir(build: u32) -> Option<std::path::PathBuf> {
    wows_toolkit_config::storage_dir().map(|dir| dir.join("game_data").join("fire_sections").join(build.to_string()))
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
///
/// `personal_rating` is the expected-values table the per-row PR column is
/// computed against; absent until it has been downloaded, in which case the
/// column stays empty until [`ReplayReportModel::populate_personal_ratings`]
/// is applied later (see `panel.rs::ReplayPanel::set_personal_rating`).
pub(crate) fn parse_replay(
    path: &Path,
    game_data: &GameDataCache,
    personal_rating: Option<&PersonalRatingData>,
) -> Result<ParsedReplay, ReplayLoadError> {
    parse_replay_with_alts(path, &[], game_data, personal_rating)
}

/// The same, reading other recordings of the same battle alongside it.
///
/// One replay records one player's view: what their team saw, when they saw it.
/// Another player's recording of the same battle saw different things, and a
/// battle read through both has no fog between them -- enemy positions, builds,
/// torpedoes and consumables the primary never saw. The egui app calls this the
/// other-team perspective (`ui/replay_parser/mod.rs`'s `load_alt_perspective`);
/// what merges them is `wows_battle_world::merged::MergedReplays`, shared.
///
/// `alts` that do not belong to this battle are refused before anything is read
/// (see [`alt_belongs`]).
pub(crate) fn parse_replay_with_alts(
    path: &Path,
    alts: &[PathBuf],
    game_data: &GameDataCache,
    personal_rating: Option<&PersonalRatingData>,
) -> Result<ParsedReplay, ReplayLoadError> {
    let replay_file = ReplayFile::from_file(path).map_err(|report| {
        let is_io = matches!(report.current_context(), ParseError::Io(_));
        let message = format!("{report:?}");
        if is_io { ReplayLoadError::Io(message) } else { ReplayLoadError::Parse(message) }
    })?;
    let meta = &replay_file.meta;

    let version = Version::try_from_client_exe(&meta.clientVersionFromExe).ok_or(ReplayLoadError::VersionParse)?;
    let build = version.build_number().ok_or(ReplayLoadError::VersionParse)?;
    let loaded = game_data.get_or_load_build_for(build, Some(&version))?;

    let constants_json = load_versioned_constants(build);

    let mut constants = loaded.base_constants.clone();
    constants.merge_replay_constants(&constants_json, version);
    wowsunpack::game_constants::apply_version_consumables(constants.common_mut(), version);

    // Whether the stream was read to its end, which is what tells a battle with
    // no results from one whose tail was never reached. A merge either consumes
    // every stream or fails, so it is always whole.
    let mut read_whole = true;
    let report = if alts.is_empty() {
        let mut world = BattleWorld::new(meta, loaded.provider.as_ref(), Some(&constants));
        world.set_shot_tracking(ShotTracking::Untracked);

        let mut parser = Parser::with_version(loaded.provider.entity_specs(), version);
        let mut remaining = replay_file.packet_data();
        while !remaining.is_empty() {
            match parser.parse_packet(&mut remaining) {
                Ok(packet) => world.process(&packet),
                Err(_) => break,
            }
        }
        read_whole = remaining.is_empty();
        world.finish();
        world.into_report()
    } else {
        merged_report(&replay_file, alts, &loaded, &constants, version)?
    };
    let raw_results_json = report.battle_results().map(pretty_json_or_raw);
    // What the egui debug menu calls "Battle Results: Mapped JSON": the same
    // resolution the normalized report reads its per-player figures through.
    let mapped_results_json = report
        .battle_results()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .map(|raw| wows_replay_insights::battle_report::resolve_battle_results(raw, &constants_json))
        .and_then(|resolved| serde_json::to_string_pretty(&resolved).ok());
    let mut normalized =
        NormalizedBattleReport::from_battle_report(&report, meta, loaded.provider.as_ref(), &constants_json);
    // Rated before anything reads it, so the table, the badge and the export
    // all carry the same numbers.
    if let Some(table) = personal_rating {
        normalized.populate_personal_ratings(table);
    }
    let model = ReplayReportModel::from_normalized(
        &normalized,
        meta,
        loaded.provider.as_ref(),
        &constants_json,
        report.game_chat(),
        report.players(),
    );
    // Reading the build's assets.bin is the expensive half, which is why the
    // result is cached per build on disk. A build that ships none simply has
    // no fire chance rather than failing the parse.
    let cache_dir = fire_section_cache_dir(build);
    let fire_chance = wows_replay_insights::fire_chance::sections::compute_fire_chance(
        &report,
        loaded.provider.as_ref(),
        loaded.vfs(),
        build,
        cache_dir.as_deref(),
    );

    // What sharing needs, taken here because this is the one place the whole
    // report exists: the payloads are per player, and the report is dropped
    // below.
    let shareable = crate::upload::Shareable {
        game_type: meta.gameType.clone().unwrap_or_default(),
        version,
        self_confirmed_non_test: crate::upload::self_is_not_a_test_ship(&report),
        // The end-of-battle results being in the stream is what says the file is
        // the whole battle. A parse that stopped early is not evidence of
        // absence, and the loop above stops on the first packet it cannot read.
        results: if report.battle_results().is_some() {
            wows_toolkit_viewmodel::upload::ResultsScan::Present
        } else if read_whole {
            wows_toolkit_viewmodel::upload::ResultsScan::Absent
        } else {
            wows_toolkit_viewmodel::upload::ResultsScan::Truncated
        },
        builds: report
            .players()
            .iter()
            .filter(|player| !player.is_bot())
            .filter_map(|player| {
                let realm = player.initial_state().realm().filter(|realm| !realm.trim().is_empty())?;
                wows_toolkit_viewmodel::upload::build_tracker::BuildTrackerPayload::build_from(
                    player,
                    realm.to_owned(),
                    report.version(),
                    meta.gameType.clone().unwrap_or_default(),
                    loaded.provider.as_ref(),
                )
            })
            .collect(),
    };

    let export = ExportedMatch::new(&normalized, report.players(), report.game_chat(), true);
    let raw_metadata_json = pretty_json_or_raw(&replay_file.raw_meta);
    // Built here because this is where the build that named the achievements
    // is open; the row itself is only written when the reader asks for it.
    let session_stat = PerGameStat::from_report(&normalized, &meta.dateTime, &|name| {
        <GameMetadataProvider as GameParamProvider>::game_param_by_name(loaded.provider.as_ref(), name)
            .map(|param| param.id())
    });

    let indexable = IndexableBattle {
        arena_id: report.arena_id(),
        game_mode_id: report.game_mode_id().known().map(|mode| mode.id()),
        version_build: version.build_number(),
        constants_fit: wows_toolkit_viewmodel::index_rows::constants_fit(&constants_json, build, Some(version)),
        self_ship_id: normalized.players.iter().find(|player| player.is_self).map(|player| player.ship_id),
        // The results are pending when the packet stream carries none, which
        // is what a replay of a battle that just ended looks like.
        results_pending: report.battle_results().is_none(),
        normalized,
    };

    Ok(ParsedReplay {
        model,
        indexable,
        session_stat,
        shareable,
        export,
        game_data: loaded,
        raw_metadata_json,
        raw_results_json,
        mapped_results_json,
        fire_chance,
    })
}

/// Parses `path` into a [`ParsedReplay`] on the background executor
/// (`cx.background_spawn`, not the tokio bridge -- this work is CPU-bound, not
/// async I/O). `game_data` lazily loads and caches each replay's own build's
/// data on first use.
pub fn spawn_parse(
    path: PathBuf,
    game_data: GameDataCache,
    personal_rating: Option<Arc<PersonalRatingData>>,
    cx: &App,
) -> Task<Result<ParsedReplay, ReplayLoadError>> {
    spawn_parse_with_alts(path, Vec::new(), game_data, personal_rating, cx)
}

/// The same, reading other recordings of the same battle alongside it.
pub fn spawn_parse_with_alts(
    path: PathBuf,
    alts: Vec<PathBuf>,
    game_data: GameDataCache,
    personal_rating: Option<Arc<PersonalRatingData>>,
    cx: &App,
) -> Task<Result<ParsedReplay, ReplayLoadError>> {
    cx.background_spawn(async move { parse_replay_with_alts(&path, &alts, &game_data, personal_rating.as_deref()) })
}

/// Reads a recording and says whether it belongs to the battle `primary` was read
/// from, without parsing either in full.
///
/// The header carries the version, and one scan of the stream carries the battle
/// it belongs to; a recording that fails either is refused before it can reach a
/// merge, where it would cost a whole re-read to find out.
pub fn check_alt(
    primary: PathBuf,
    alt: PathBuf,
    game_data: GameDataCache,
    cx: &App,
) -> Task<Result<(), ReplayLoadError>> {
    cx.background_spawn(async move {
        let read =
            |path: &Path| ReplayFile::from_file(path).map_err(|report| ReplayLoadError::Parse(format!("{report:?}")));
        let primary_file = read(&primary)?;
        let alt_file = read(&alt)?;

        let version = Version::try_from_client_exe(&primary_file.meta.clientVersionFromExe)
            .ok_or(ReplayLoadError::VersionParse)?;
        let build = version.build_number().ok_or(ReplayLoadError::VersionParse)?;
        let loaded = game_data.get_or_load_build_for(build, Some(&version))?;
        let specs = loaded.provider.entity_specs();

        let arena_of = |file: &ReplayFile| wows_replays::analyzer::arena_scan::scan_arena_id(specs, version, file);
        alt_belongs(
            &primary_file.meta.clientVersionFromExe,
            &alt_file.meta.clientVersionFromExe,
            arena_of(&primary_file),
            arena_of(&alt_file),
        )
        .map_err(|refused| ReplayLoadError::Parse(refused.to_string()))
    })
}

#[cfg(test)]
mod alt_perspective_tests {
    use wows_replays::types::ArenaId;

    use super::AltRefusal;
    use super::alt_belongs;

    /// Another recording of the same battle on the same version is taken; the
    /// three ways it can fail to be that are each refused with their own reason.
    #[test]
    fn only_another_recording_of_the_same_battle_is_taken() {
        let battle = ArenaId::new(4_242);
        assert_eq!(alt_belongs("12,3,0,0", "12,3,0,0", Some(battle), Some(battle)), Ok(()));

        assert_eq!(
            alt_belongs("12,3,0,0", "12,4,0,0", Some(battle), Some(battle)),
            Err(AltRefusal::Version { primary: "12,3,0,0".to_owned(), alt: "12,4,0,0".to_owned() }),
            "a recording from another version reads as different packets entirely"
        );

        let other = ArenaId::new(7);
        assert_eq!(
            alt_belongs("12,3,0,0", "12,3,0,0", Some(battle), Some(other)),
            Err(AltRefusal::OtherBattle { primary: battle, alt: other })
        );

        assert_eq!(
            alt_belongs("12,3,0,0", "12,3,0,0", Some(battle), None),
            Err(AltRefusal::NoArenaId),
            "a recording whose battle cannot be read is not evidence either way"
        );
    }

    /// A primary whose own battle could not be read still takes a recording that
    /// names one: the merge itself is what would fail, and it says so.
    #[test]
    fn a_primary_with_no_battle_of_its_own_is_not_the_refusal() {
        assert_eq!(alt_belongs("12,3,0,0", "12,3,0,0", None, Some(ArenaId::new(1))), Ok(()));
    }
}

#[cfg(test)]
mod session_stat_tests {
    use super::super::test_support::fixture_normalized_battle_report;
    use wows_replay_insights::battle_report::AchievementResult;
    use wows_replays::analyzer::battle_controller::BattleResult;
    use wows_replays::types::GameParamId;
    use wows_toolkit_viewmodel::stats::PerGameStat;

    /// The game writes `DD.MM.YYYY HH:MM:SS`, which the stored row carries
    /// verbatim so both apps dedupe on the same string.
    const GAME_TIME: &str = "28.12.2023 00:52:26";

    fn named(_name: &str) -> Option<GameParamId> {
        Some(GameParamId::from(4242u32))
    }

    fn unnamed(_name: &str) -> Option<GameParamId> {
        None
    }

    /// The row is read off the recording player, not off whoever is first in
    /// the roster.
    #[test]
    fn the_row_carries_the_recording_players_own_figures() {
        let report = fixture_normalized_battle_report();
        let stat = PerGameStat::from_report(&report, GAME_TIME, &named).expect("the report names a self player");

        assert_eq!(stat.damage, 50_000);
        assert_eq!(stat.spotting_damage, 8_000);
        assert_eq!(stat.frags, 1);
        assert_eq!(stat.base_xp, 1_500);
        assert_eq!(stat.raw_xp, 1_200);
    }

    /// The date is rewritten so a string sort is a chronological one; the
    /// stored `game_time` is left as the game wrote it.
    #[test]
    fn the_sort_key_reorders_the_date_and_the_game_time_does_not() {
        let report = fixture_normalized_battle_report();
        let stat = PerGameStat::from_report(&report, GAME_TIME, &named).expect("the report names a self player");

        assert_eq!(stat.game_time, GAME_TIME);
        assert_eq!(stat.sort_key, "2023-12-28 00:52:26");
    }

    /// A battle whose result never resolved is none of won, lost or drawn,
    /// rather than counting as a loss.
    #[test]
    fn an_unresolved_result_is_not_recorded_as_any_outcome() {
        let report = fixture_normalized_battle_report();
        let stat = PerGameStat::from_report(&report, GAME_TIME, &named).expect("the report names a self player");

        assert!(!stat.is_win && !stat.is_loss && !stat.is_draw);
    }

    #[test]
    fn a_win_is_recorded_as_one() {
        let mut report = fixture_normalized_battle_report();
        report.metadata.battle_result = Some(BattleResult::Win(0));
        let stat = PerGameStat::from_report(&report, GAME_TIME, &named).expect("the report names a self player");

        assert!(stat.is_win);
        assert!(!stat.is_loss);
    }

    /// An achievement the build cannot name is left out: stored under a
    /// stand-in id, every unnamed achievement would aggregate as one.
    #[test]
    fn an_achievement_the_build_cannot_name_is_left_out() {
        let mut report = fixture_normalized_battle_report();
        let earned = AchievementResult {
            name: "PCH001_Achievement".to_string(),
            display_name: "First Blood".to_string(),
            description: String::new(),
            icon_key: "first_blood".to_string(),
            count: 1,
        };
        report.players[0].achievements = vec![earned];

        let named_stat = PerGameStat::from_report(&report, GAME_TIME, &named).expect("a self player");
        assert_eq!(named_stat.achievements.len(), 1);

        let unnamed_stat = PerGameStat::from_report(&report, GAME_TIME, &unnamed).expect("a self player");
        assert!(unnamed_stat.achievements.is_empty());
    }

    /// A replay nobody was recording has no game to record.
    #[test]
    fn a_report_with_no_recording_player_records_nothing() {
        let mut report = fixture_normalized_battle_report();
        report.players.clear();
        assert!(PerGameStat::from_report(&report, GAME_TIME, &named).is_none());
    }
}

#[cfg(test)]
mod tests {
    /// A per-build cache file is recognised by its name, and nothing else in the
    /// storage directory is: the settings database lives there too.
    #[test]
    fn only_a_per_build_cache_is_named_as_one() {
        assert_eq!(super::stale_cache_build("game_params_12345.bin"), Some(12345));
        assert_eq!(super::stale_cache_build("constants_12345.json"), Some(12345));
        assert_eq!(super::stale_cache_build("wows_toolkit.db"), None);
        assert_eq!(super::stale_cache_build("game_params.bin"), None, "the unkeyed one is handled on its own");
        assert_eq!(super::stale_cache_build("game_params_notabuild.bin"), None);
    }

    /// A build the install no longer has is answered from the game-data cache:
    /// by its own number where the cache holds it, by another dump of the same
    /// version otherwise (build numbers are per server), and by a pre-index
    /// dump's directory name failing both.
    #[test]
    fn a_dump_answers_for_a_build_that_is_not_installed() {
        let base = tempfile::tempdir().expect("a temp directory");
        let base = base.path();

        for dir in ["0.10.5_100", "0.10.5_101", "0.9.0_99"] {
            std::fs::create_dir_all(base.join(dir)).expect("the dump directory is created");
            std::fs::write(base.join(dir).join("metadata.toml"), "").expect("the dump is marked");
        }
        std::fs::write(
            base.join("builds.toml"),
            r#"
[[builds]]
version = "0.10.5"
build = 100
dir = "0.10.5_100"
dumped_at = "2026-01-01T00:00:00Z"

[[builds]]
version = "0.10.5"
build = 101
dir = "0.10.5_101"
dumped_at = "2026-01-01T00:00:00Z"
"#,
        )
        .expect("the index is written");

        assert_eq!(super::dump_for_build(base, 100, None), Some(base.join("0.10.5_100")), "exact by build number");
        assert_eq!(
            super::dump_for_build(base, 102, Some("0.10.5")),
            Some(base.join("0.10.5_101")),
            "the nearest build of the version the replay names"
        );
        assert_eq!(super::dump_for_build(base, 102, None), None, "and nothing without a version to fall back on");
        assert_eq!(
            super::dump_for_build(base, 99, None),
            Some(base.join("0.9.0_99")),
            "a dump the index does not list is found by its directory name"
        );
    }

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

        // The real cached expected-values table when there is one, so this
        // also exercises the rating path end to end against real data.
        let personal_rating = wows_toolkit_viewmodel::personal_rating::load_cached().ok();
        let parsed = parse_replay(Path::new(&replay_path), &game_data, personal_rating.as_ref())
            .expect("failed to parse the replay");
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

        // A replay with no battle-results packet carries no actual damage,
        // which is what a rating needs; the egui app leaves those unrated
        // too, so only a replay with results is expected to rate.
        if personal_rating.is_some() && parsed.raw_results_json.is_some() {
            let rated = model.rows.iter().filter(|row| row.personal_rating.is_some()).count();
            println!("{rated} of {} rows carry a personal rating", model.rows.len());
            assert!(rated > 0, "a replay with results should rate at least one row");
        }
    }

    /// Loads a real install's decoded parameters and writes them out in the
    /// minimal format, which is what the Unpacker's dump menu does. Needs a
    /// local game install. Run with:
    ///
    /// ```text
    /// WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR="E:\WoWs\World_of_Warships" \
    /// cargo test -p wows-toolkit-gpui -- --ignored --nocapture the_minimal_parameter_dump_writes_a_real_installs_parameters
    /// ```
    #[test]
    #[ignore = "needs a local game install; see the doc comment for the run command"]
    fn the_minimal_parameter_dump_writes_a_real_installs_parameters() {
        use wows_toolkit_viewmodel::unpacker::game_params;

        let wows_dir = std::env::var("WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR")
            .expect("set WOWS_REPLAY_INSPECTOR_LOAD_TEST_DIR to a WoWs install directory");
        let dir = PathBuf::from(&wows_dir);
        let build = wowsunpack::game_data::list_available_builds(&dir)
            .expect("the install lists its builds")
            .into_iter()
            .max()
            .expect("the install has at least one build");
        let vfs = wowsunpack::game_data::build_game_vfs_for_build(&dir, build).expect("the build's VFS opens");

        let params = load_game_params(&vfs, build).expect("the build's parameters load");
        assert!(!params.is_empty(), "a real install decodes to at least one parameter");

        let out = std::env::temp_dir().join(format!("wt-gpui-minparams-{}.json", std::process::id()));
        game_params::write_value(&params, &out, game_params::GameParamsFormat::MinimalJson)
            .expect("the minimal dump writes");

        let written = std::fs::metadata(&out).expect("the dump exists").len();
        println!("wrote {} parameters as {written} bytes of minimal JSON", params.len());
        assert!(written > 0, "the dump is not empty");
        let _ = std::fs::remove_file(&out);
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

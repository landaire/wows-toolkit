use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;

use parking_lot::Mutex;
use parking_lot::RwLock;
use tracing::debug;
use tracing::error;
use tracing::instrument;
use tracing::warn;
use wows_replays::ReplayFile;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::CrewSkillName;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

use crate::task::BackgroundTask;
use crate::task::BackgroundTaskCompletion;
use crate::task::BackgroundTaskKind;
use crate::task::NetworkJob;
use crate::task::ReplaySource;
use crate::task::load_wows_data_for_build;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::SortOrder;
use crate::util::error::ToolkitError;

pub struct GameAsset {
    pub path: String,
    pub data: Vec<u8>,
}

impl std::fmt::Debug for GameAsset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GameAsset").field("path", &self.path).field("data", &"...").finish()
    }
}

pub type SharedWoWsData = Arc<RwLock<Box<WorldOfWarshipsData>>>;

/// How many builds other than the live install's stay loaded. A replay
/// directory can span dozens of game versions and each build carries its own
/// game params and VFS index, so old builds are held only long enough to serve
/// a run of replays from the same era.
const NON_MAIN_BUILD_CAPACITY: usize = 2;

/// The loaded builds of a [`WoWsDataMap`].
///
/// The build of the live WoWs install is pinned, since nearly every replay
/// wants it. The rest are bounded: the least recently used one is dropped once
/// there are more than `capacity` of them.
///
/// Dropping is only ever the removal of this cache's `Arc`. [`SharedWoWsData`]
/// is reference counted, so a caller that is already holding one keeps reading
/// through it, and the memory comes back when the last holder lets go. The
/// build itself is not gone: resolving it again loads it again.
struct LoadedBuilds {
    /// The live install's build, if it has been loaded.
    main: Option<(u32, SharedWoWsData)>,
    /// Every other loaded build, least recently used first.
    others: VecDeque<(u32, SharedWoWsData)>,
    capacity: usize,
}

impl LoadedBuilds {
    fn new(capacity: usize) -> Self {
        Self { main: None, others: VecDeque::new(), capacity }
    }

    /// Look up `build`, counting the lookup as a use.
    fn get(&mut self, build: u32) -> Option<SharedWoWsData> {
        if let Some((main_build, data)) = &self.main
            && *main_build == build
        {
            return Some(Arc::clone(data));
        }

        let position = self.others.iter().position(|(other, _)| *other == build)?;
        let entry = self.others.remove(position)?;
        let data = Arc::clone(&entry.1);
        self.others.push_back(entry);
        Some(data)
    }

    /// Data for every loaded build, most recently used last, without counting
    /// as a use of any of them.
    fn all(&self) -> Vec<SharedWoWsData> {
        self.main.iter().chain(self.others.iter()).map(|(_, data)| Arc::clone(data)).collect()
    }

    /// Whether `build` is resident, without counting as a use of it. An
    /// availability check must not reorder the cache it is asking about.
    fn contains(&self, build: u32) -> bool {
        self.main.as_ref().is_some_and(|(resident, _)| *resident == build)
            || self.others.iter().any(|(resident, _)| *resident == build)
    }

    fn insert(&mut self, build: u32, data: SharedWoWsData) {
        if let Some((main_build, main_data)) = &mut self.main
            && *main_build == build
        {
            *main_data = data;
            return;
        }

        self.remove_other(build);
        self.others.push_back((build, data));
        while self.others.len() > self.capacity {
            self.others.pop_front();
        }
    }

    fn insert_main(&mut self, build: u32, data: SharedWoWsData) {
        self.remove_other(build);
        self.main = Some((build, data));
    }

    fn remove_other(&mut self, build: u32) {
        if let Some(position) = self.others.iter().position(|(other, _)| *other == build) {
            self.others.remove(position);
        }
    }
}

/// Manages all loaded game data versions, keyed by build number.
/// Provides version resolution for replay parsing and lazy-loading of build data.
#[derive(Clone)]
pub struct WoWsDataMap {
    builds: Arc<RwLock<LoadedBuilds>>,
    /// Builds that were looked for and are not on this machine. Kept apart from
    /// `builds` so a failure is never confusable with a loaded build.
    unresolvable_builds: Arc<RwLock<HashSet<u32>>>,
    /// How many times the loading path in [`Self::resolve_build_with_version`]
    /// has run for each build. Reads as the number of times the full cost of a
    /// lookup was paid.
    resolution_attempts: Arc<RwLock<HashMap<u32, u32>>>,
    wows_dir: PathBuf,
    locale: String,
    network_job_tx: Option<mpsc::Sender<NetworkJob>>,
    /// Custom game data cache directory. Empty means use the default.
    game_data_cache_dir: String,
}

impl WoWsDataMap {
    /// `game_data_cache_dir` is where dumped game data lives; empty means the
    /// default location. It is fixed for the life of the map: editing the
    /// setting takes effect on the next start.
    pub fn new(wows_dir: PathBuf, locale: String, game_data_cache_dir: String) -> Self {
        Self {
            builds: Arc::new(RwLock::new(LoadedBuilds::new(NON_MAIN_BUILD_CAPACITY))),
            unresolvable_builds: Arc::new(RwLock::new(HashSet::new())),
            resolution_attempts: Arc::new(RwLock::new(HashMap::new())),
            wows_dir,
            locale,
            network_job_tx: None,
            game_data_cache_dir,
        }
    }

    /// Allow `build` to be looked for again after its data was downloaded.
    pub fn forget_unresolvable_build(&self, build: u32) {
        self.unresolvable_builds.write().remove(&build);
    }

    /// Allow every previously unresolvable build to be looked for again.
    pub fn forget_unresolvable_builds(&self) {
        self.unresolvable_builds.write().clear();
    }

    /// Whether `build` is recorded as not available on this machine, which is
    /// what stops the loading path being paid for it again.
    pub fn is_unresolvable_build(&self, build: u32) -> bool {
        self.unresolvable_builds.read().contains(&build)
    }

    /// How many times the loading path ran for `build`.
    pub fn resolution_attempts(&self, build: u32) -> u32 {
        self.resolution_attempts.read().get(&build).copied().unwrap_or(0)
    }

    /// Whether data for `request` is on this machine, without loading it.
    ///
    /// Resolution proper loads the build, which is exactly the cost a scan must
    /// not pay to discover a build is absent: this checks what is already
    /// resident, then the live install's `bin/<build>`, then the dump index, and
    /// nothing else.
    pub fn has_data_for(&self, request: &crate::task::BuildRequest) -> bool {
        let build = request.build_u32();
        if self.builds.read().contains(build) {
            return true;
        }
        if self.wows_dir.join("bin").join(build.to_string()).exists() {
            return true;
        }
        let Some(dump_base) = crate::task::replays::game_data_dump_base_with_override(&self.game_data_cache_dir) else {
            return false;
        };
        let index = wows_data_mgr::builds::BuildsIndex::load(&dump_base.join("builds.toml"));
        if index.resolve_build(build, Some(&request.friendly_version())).is_some() {
            return true;
        }
        if index.builds.is_empty()
            && let Ok(entries) = std::fs::read_dir(&dump_base)
        {
            for entry in entries.flatten() {
                let name_str = entry.file_name().to_string_lossy().to_string();
                if name_str.ends_with(&format!("_{build}")) && entry.path().join("metadata.toml").exists() {
                    return true;
                }
            }
        }
        false
    }

    /// Custom game data cache directory as configured in settings. Empty means
    /// the default app data location; resolve it with
    /// [`crate::task::replays::game_data_dump_base_with_override`].
    pub fn game_data_cache_dir(&self) -> &str {
        &self.game_data_cache_dir
    }

    pub fn set_network_job_tx(&mut self, tx: mpsc::Sender<NetworkJob>) {
        self.network_job_tx = Some(tx);
    }

    /// Insert data for a specific build number. The build is subject to
    /// eviction once [`NON_MAIN_BUILD_CAPACITY`] others are in front of it;
    /// use [`Self::insert_main`] for the live install's build.
    pub fn insert(&self, build: u32, data: SharedWoWsData) {
        self.builds.write().insert(build, data);
    }

    /// Insert the build of the live WoWs install, which stays loaded.
    pub fn insert_main(&self, build: u32, data: SharedWoWsData) {
        self.builds.write().insert_main(build, data);
    }

    /// Look up already-loaded data by build number, counting as a use of that
    /// build. Does NOT lazy-load.
    pub fn get(&self, build: u32) -> Option<SharedWoWsData> {
        self.builds.write().get(build)
    }

    /// Data for every currently loaded build. Taking a snapshot keeps the
    /// cache's lock held for the clone alone, and leaves eviction order alone.
    pub fn loaded_builds(&self) -> Vec<SharedWoWsData> {
        self.builds.read().all()
    }

    /// Rebuild all loaded builds' data after constants have changed.
    /// Returns `true` if all builds rebuilt successfully, `false` if any failed.
    #[instrument(skip(self))]
    pub fn rebuild_all_with_new_constants(&self) -> bool {
        let mut all_ok = true;
        for data in self.loaded_builds() {
            let mut data = data.write();
            debug!("Rebuilding data for build {}", data.build_number);
            if !data.rebuild_with_new_constants() {
                all_ok = false;
            }
        }
        all_ok
    }

    /// Swap the gettext translation catalog for all loaded builds.
    ///
    /// Loads the `.mo` file for the given locale from each build's `res/texts/`
    /// directory and calls `set_translations()` on the provider. Falls back through
    /// the language tag's primary language, then "en".
    #[instrument(skip(self))]
    pub fn reload_translations(&self, locale: &str) {
        // WoWs locale codes use underscores (e.g. "zh_tw") but BCP 47 uses hyphens.
        let bcp47 = locale.replace('_', "-");
        let primary_lang = bcp47
            .parse::<language_tags::LanguageTag>()
            .map(|tag| tag.primary_language().to_string())
            .unwrap_or_else(|_| locale.to_string());
        let attempted_dirs = [locale, &primary_lang, "en"];

        for data in self.loaded_builds() {
            let data = data.read();
            let build = data.build_number;
            let provider = match data.game_metadata.as_ref() {
                Some(p) => p,
                None => continue,
            };

            let mut found = false;
            for dir in &attempted_dirs {
                // Try live install first, then dump directory
                let live_path = self.wows_dir.join(format!("bin/{build}/res/texts/{dir}/LC_MESSAGES/global.mo"));
                let dump_path =
                    data.dump_dir.as_ref().map(|d| d.join(format!("translations/{dir}/LC_MESSAGES/global.mo")));
                let mo_path = if live_path.exists() {
                    live_path
                } else if let Some(ref dp) = dump_path
                    && dp.exists()
                {
                    dp.clone()
                } else {
                    continue;
                };
                match std::fs::File::open(&mo_path).and_then(|f| {
                    gettext::Catalog::parse(f).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
                }) {
                    Ok(catalog) => {
                        debug!(build, locale = dir, "Reloaded translations");
                        provider.set_translations(catalog);
                    }
                    Err(e) => {
                        warn!(build, path = ?mo_path, error = %e, "Failed to reload translations");
                    }
                }
                found = true;
                break;
            }
            if !found {
                debug!(build, "No translations found for any attempted locale");
            }
        }
    }

    /// Resolve the correct game data for a replay's version.
    /// Checks the map first, then tries to lazy-load from disk.
    /// Returns None if the version's build data is unavailable.
    #[instrument(skip(self))]
    pub fn resolve(&self, version: &Version) -> Option<SharedWoWsData> {
        self.resolve_build_with_version(version.build_number()?, Some(*version))
    }

    /// Resolve game data for a specific build number.
    /// Checks the map first, then tries to lazy-load from disk.
    /// Returns None if the build data is unavailable.
    #[instrument(skip(self))]
    pub fn resolve_build(&self, build: u32) -> Option<SharedWoWsData> {
        self.resolve_build_with_version(build, None)
    }

    /// Open the newest dumped build's VFS by build number, recovering its
    /// semantic version from the index. Used to borrow GUI assets (ship/ribbon
    /// icons) for an older replay whose own build predates per-file icons, when
    /// no build new enough to carry them is currently loaded. Opening the VFS is
    /// cheap (a `PhysicalFS` over the dump dir); only the handful of icon files
    /// are read. Returns `None` when there is no dump index or extracted VFS.
    fn newest_dump_gui_vfs(&self) -> Option<(VfsPath, Option<Version>)> {
        let base = crate::task::replays::game_data_dump_base_with_override(&self.game_data_cache_dir)?;
        let index = wows_data_mgr::builds::BuildsIndex::load(&base.join("builds.toml"));
        let entry = index.builds.iter().max_by_key(|e| e.build)?;
        let build_dir = base.join(&entry.dir);
        let vfs = wows_data_mgr::cas_vfs::BuildCas::open(&build_dir)?.vfs();
        let mut parts = entry.version.split('.').filter_map(|p| p.trim().parse::<u32>().ok());
        let version = parts.next().map(|major| Version {
            major,
            minor: parts.next().unwrap_or(0),
            patch: parts.next().unwrap_or(0),
            build: std::num::NonZeroU32::new(entry.build),
        });
        Some((vfs, version))
    }

    /// Ship class icons from the newest build available. Used as a fallback when an
    /// older replay's own build predates these assets -- pre-12.0 clients shipped no
    /// `gui/fla/minimap/ship_icons`, so those dumps have an empty icon set. The class
    /// icons (DD/CA/BB/CV) are generic, so borrowing the current build's is correct.
    /// Prefers the newest loaded build; if that build also lacks them (e.g. only an
    /// old replay is loaded), reads them straight from the newest dump on disk.
    pub fn newest_ship_icons(&self) -> HashMap<Species, Arc<GameAsset>> {
        let loaded = self
            .loaded_builds()
            .into_iter()
            .max_by_key(|data| data.read().build_number)
            .map(|data| data.read().ship_icons.clone())
            .unwrap_or_default();
        if !loaded.is_empty() {
            return loaded;
        }
        self.newest_dump_gui_vfs()
            .map(|(vfs, version)| crate::task::load_ship_icons(&vfs, version.as_ref()))
            .unwrap_or_default()
    }

    /// Ribbon icons from the newest build available. Used as a fallback when an older
    /// replay's own build has no per-file ribbon PNGs -- Flash-era clients (~0.9.5
    /// through 0.10.4) and older embed ribbons as vector symbols inside
    /// `achievements.swf`, so those dumps have an empty ribbon icon set. The icons
    /// are keyed by stable ribbon names (`ribbon_main_caliber`, ...), so borrowing
    /// the current build's is correct for the ribbon types an old replay can earn.
    /// Prefers the newest loaded build, then falls back to the newest dump on disk.
    pub fn newest_ribbon_icons(&self) -> HashMap<String, Arc<GameAsset>> {
        let loaded = self
            .loaded_builds()
            .into_iter()
            .max_by_key(|data| data.read().build_number)
            .map(|data| data.read().ribbon_icons.clone())
            .unwrap_or_default();
        if !loaded.is_empty() {
            return loaded;
        }
        self.newest_dump_gui_vfs()
            .map(|(vfs, version)| {
                crate::task::load_ribbon_icons(&vfs, wowsunpack::game_assets::GuiAssetDir::Ribbons, version.as_ref())
            })
            .unwrap_or_default()
    }

    /// Subribbon icons from the newest build available. Companion to
    /// [`Self::newest_ribbon_icons`] for the same Flash-era fallback.
    pub fn newest_subribbon_icons(&self) -> HashMap<String, Arc<GameAsset>> {
        let loaded = self
            .loaded_builds()
            .into_iter()
            .max_by_key(|data| data.read().build_number)
            .map(|data| data.read().subribbon_icons.clone())
            .unwrap_or_default();
        if !loaded.is_empty() {
            return loaded;
        }
        self.newest_dump_gui_vfs()
            .map(|(vfs, version)| {
                crate::task::load_ribbon_icons(&vfs, wowsunpack::game_assets::GuiAssetDir::SubRibbons, version.as_ref())
            })
            .unwrap_or_default()
    }

    /// Like [`Self::resolve_build`], but threads the replay's friendly version through
    /// so version-aware constants (consumable id layouts) resolve against the client
    /// that produced the replay rather than the latest layout.
    #[instrument(skip(self))]
    pub fn resolve_build_with_version(&self, build: u32, version: Option<Version>) -> Option<SharedWoWsData> {
        // Check if already loaded
        if let Some(data) = self.get(build) {
            return Some(data);
        }

        // Every path below is expensive and every one of them stays expensive
        // when it fails, so a build already looked for and not found is
        // answered from the record until new data arrives for it.
        if self.unresolvable_builds.read().contains(&build) {
            debug!("Build {} was already looked for and is not available", build);
            return None;
        }

        *self.resolution_attempts.write().entry(build).or_insert(0) += 1;

        // Constants (CONSUMABLE_IDS / BATTLE_STAGES) are version-specific. Only
        // bridge them FORWARD: an already-loaded build's constants may stand in
        // for a build we're loading that is NEWER than it (a fresh game version
        // we haven't dumped yet). Never apply newer constants to an OLDER replay
        // -- that corrupts the interpretation (consumables, battle stages,
        // connection/observed state all read wrong). For older builds, fall back
        // to the build's own VFS constants (Null = no override).
        let fallback_constants = {
            let mut best: Option<(u32, serde_json::Value)> = None;
            for data in self.loaded_builds() {
                let guard = data.read();
                if guard.build_number < build && best.as_ref().is_none_or(|(b, _)| guard.build_number > *b) {
                    best = Some((guard.build_number, guard.replay_constants.read().clone()));
                }
            }
            best.map(|(_, constants)| constants).unwrap_or_default()
        };

        // Try to load from the live game install first
        let build_dir = self.wows_dir.join("bin").join(build.to_string());
        if build_dir.exists() {
            debug!("Lazily loading game data for build {}", build);
            match load_wows_data_for_build(&self.wows_dir, build, &self.locale, &fallback_constants, version) {
                Ok(wows_data) => {
                    if !wows_data.replay_constants_exact_match
                        && let Some(tx) = &self.network_job_tx
                    {
                        let version = version.map(|v| format!("{}.{}.{}", v.major, v.minor, v.patch));
                        let _ = tx.send(NetworkJob::FetchVersionedConstants { build, version });
                    }
                    let shared: SharedWoWsData = Arc::new(RwLock::new(Box::new(wows_data)));
                    self.insert(build, Arc::clone(&shared));
                    return Some(shared);
                }
                Err(e) => {
                    warn!("Could not load data for build {} from live install: {}", build, e);
                }
            }
        }

        // Fall back to auto-dumped game data via BuildsIndex
        if let Some(dump_base) = crate::task::replays::game_data_dump_base_with_override(&self.game_data_cache_dir) {
            let index = wows_data_mgr::builds::BuildsIndex::load(&dump_base.join("builds.toml"));

            // Friendly version for the cross-region fallback. Build numbers are
            // per-server (the China client ships a different one for the same
            // major.minor.patch), so the version is what identifies the data a
            // replay needs. Prefer the caller's, which is the replay's own; a
            // loaded build's version is only a guess about an unrelated replay,
            // and stands in solely when the caller had none.
            let version_hint = version.map(|v| format!("{}.{}.{}", v.major, v.minor, v.patch)).or_else(|| {
                self.loaded_builds().first().and_then(|d| {
                    d.read().full_version.as_ref().map(|v| format!("{}.{}.{}", v.major, v.minor, v.patch))
                })
            });

            if let Some((entry, exact)) = index.resolve_build(build, version_hint.as_deref()) {
                if !exact {
                    warn!("No exact data for build {}; using {} (build {})", build, entry.version, entry.build);
                }
                let dump_dir = dump_base.join(&entry.dir);
                debug!("Loading game data for build {} from dump: {}", build, dump_dir.display());
                match crate::task::replays::load_wows_data_from_dump(
                    &dump_dir,
                    build,
                    &self.locale,
                    &fallback_constants,
                    version,
                ) {
                    Ok(wows_data) => {
                        if !wows_data.replay_constants_exact_match
                            && let Some(tx) = &self.network_job_tx
                        {
                            let version = version.map(|v| format!("{}.{}.{}", v.major, v.minor, v.patch));
                            let _ = tx.send(NetworkJob::FetchVersionedConstants { build, version });
                        }
                        let shared: SharedWoWsData = Arc::new(RwLock::new(Box::new(wows_data)));
                        self.insert(build, Arc::clone(&shared));
                        return Some(shared);
                    }
                    Err(e) => {
                        warn!("Could not load data for build {} from dump: {}", build, e);
                    }
                }
            } else if index.builds.is_empty() {
                // Legacy fallback: scan directories for old-format dumps without builds.toml
                if let Ok(entries) = std::fs::read_dir(&dump_base) {
                    for entry in entries.flatten() {
                        let name_str = entry.file_name().to_string_lossy().to_string();
                        if name_str.ends_with(&format!("_{build}")) && entry.path().join("metadata.toml").exists() {
                            match crate::task::replays::load_wows_data_from_dump(
                                &entry.path(),
                                build,
                                &self.locale,
                                &fallback_constants,
                                version,
                            ) {
                                Ok(wows_data) => {
                                    let shared: SharedWoWsData = Arc::new(RwLock::new(Box::new(wows_data)));
                                    self.insert(build, Arc::clone(&shared));
                                    return Some(shared);
                                }
                                Err(e) => {
                                    warn!("Could not load build {} from legacy dump: {}", build, e);
                                }
                            }
                        }
                    }
                }
            }
        }

        self.unresolvable_builds.write().insert(build);
        None
    }
}

#[cfg(test)]
mod build_resolution_tests {
    use super::*;

    /// A map pointed at directories that hold no game data at all, so every
    /// lookup runs the whole loading path and comes back empty: no live
    /// install, no builds index, no legacy dump.
    fn test_map_with_no_data() -> WoWsDataMap {
        let root = std::env::temp_dir().join(format!("wt-no-game-data-{}", std::process::id()));
        WoWsDataMap::new(root.join("wows"), "en".to_string(), root.join("cache").to_string_lossy().into_owned())
    }

    fn request(major: u32, minor: u32, patch: u32, build: u32) -> crate::task::BuildRequest {
        crate::task::BuildRequest::new(Version { major, minor, patch, build: std::num::NonZeroU32::new(build) })
            .expect("build is present")
    }

    /// Game data with nothing in it, standing in for a loaded build. The cache
    /// only ever moves the `Arc` around, so what it points at does not matter
    /// beyond being able to tell one build's data from another's.
    fn empty_build_data(build: u32) -> SharedWoWsData {
        Arc::new(RwLock::new(Box::new(WorldOfWarshipsData {
            vfs: VfsPath::new(wowsunpack::vfs::MemoryFS::new()),
            game_metadata: None,
            ship_icons: HashMap::new(),
            ribbon_icons: HashMap::new(),
            subribbon_icons: HashMap::new(),
            achievement_icons: HashMap::new(),
            consumable_icons: HashMap::new(),
            crew_skill_icons: HashMap::new(),
            modernization_icons: HashMap::new(),
            signal_flag_icons: HashMap::new(),
            game_constants: Arc::new(GameConstants::defaults()),
            replay_constants: Arc::new(RwLock::new(serde_json::Value::Null)),
            replay_constants_exact_match: false,
            full_version: None,
            patch_version: 0,
            build_number: build,
            replays_dir: PathBuf::new(),
            build_dir: PathBuf::new(),
            dump_dir: None,
        })))
    }

    /// The loading path reads and parses the whole builds index and clones every
    /// loaded build's constants. A directory of replays for one build the user
    /// does not have pays that once per replay unless the failure is remembered.
    #[test]
    fn an_unresolvable_build_is_only_attempted_once() {
        let map = test_map_with_no_data();
        assert!(map.resolve_build(9_999_999).is_none());
        assert!(map.resolve_build(9_999_999).is_none());
        assert!(map.resolve_build(9_999_999).is_none());
        assert_eq!(map.resolution_attempts(9_999_999), 1);
    }

    /// Remembering one build's failure must not stand in for every build's:
    /// a second build is a separate question and is still asked.
    #[test]
    fn a_different_build_is_still_attempted() {
        let map = test_map_with_no_data();
        assert!(map.resolve_build(9_999_999).is_none());
        assert!(map.resolve_build(8_888_888).is_none());
        assert_eq!(map.resolution_attempts(9_999_999), 1);
        assert_eq!(map.resolution_attempts(8_888_888), 1);
    }

    /// The app tells the user which build to download and then downloads it.
    /// A record that outlives that download leaves the app refusing to read
    /// replays whose data is now sitting on disk.
    #[test]
    fn downloading_a_build_lets_it_be_attempted_again() {
        let map = test_map_with_no_data();
        assert!(map.resolve_build(9_999_999).is_none());
        assert_eq!(map.resolution_attempts(9_999_999), 1);

        map.forget_unresolvable_build(9_999_999);

        assert!(map.resolve_build(9_999_999).is_none());
        assert_eq!(map.resolution_attempts(9_999_999), 2, "the download must buy the build a second attempt");
    }

    /// The live install's build is the one nearly every replay wants. Loading
    /// it costs seconds, so it is pinned: cycling through old builds must not
    /// push it out.
    #[test]
    fn the_main_build_is_never_evicted() {
        let map = test_map_with_no_data();
        map.insert_main(7_000_000, empty_build_data(7_000_000));
        for build in 1..=NON_MAIN_BUILD_CAPACITY as u32 {
            map.insert(build, empty_build_data(build));
        }

        map.insert(9_000_000, empty_build_data(9_000_000));

        assert!(map.get(7_000_000).is_some(), "the live install's build stays loaded");
        assert!(map.get(1).is_none(), "the least recently used other build is the one that goes");
        assert!(map.get(9_000_000).is_some());
    }

    /// Eviction is a cache decision, never a failure. The evicted build has to
    /// be looked for again on the next resolve; an implementation that treats
    /// it as unresolvable never re-runs the loading path.
    ///
    /// Nothing this test can point the map at holds real game data, so the
    /// reachable observation is that the loading path runs again rather than
    /// that it succeeds.
    #[test]
    fn resolving_an_evicted_build_reloads_it_rather_than_failing() {
        let map = test_map_with_no_data();
        map.insert(1, empty_build_data(1));
        for build in 2..=NON_MAIN_BUILD_CAPACITY as u32 + 1 {
            map.insert(build, empty_build_data(build));
        }
        assert!(map.get(1).is_none(), "build 1 was evicted");

        assert_eq!(map.resolution_attempts(1), 0, "build 1 was inserted, never resolved");
        map.resolve_build(1);
        assert_eq!(map.resolution_attempts(1), 1, "an evicted build is looked for again");
    }

    /// What separates an LRU from an insertion-ordered queue: build 1 is the
    /// oldest insertion but the newest use, so build 2 is what goes.
    #[test]
    fn a_recently_used_build_outlives_an_older_one() {
        let map = test_map_with_no_data();
        assert_eq!(NON_MAIN_BUILD_CAPACITY, 2, "this test reasons about exactly two resident builds");
        map.insert(1, empty_build_data(1));
        map.insert(2, empty_build_data(2));
        assert!(map.get(1).is_some(), "using build 1 makes it the most recently used");

        map.insert(3, empty_build_data(3));

        assert!(map.get(1).is_some(), "the recently used build stays");
        assert!(map.get(2).is_none(), "the least recently used build goes");
        assert!(map.get(3).is_some());
    }

    /// Eviction drops the map's `Arc` and nothing else. A caller mid-parse
    /// holds its own and must keep reading through it.
    #[test]
    fn eviction_does_not_invalidate_a_handle_a_caller_already_holds() {
        let map = test_map_with_no_data();
        map.insert(1, empty_build_data(1));
        let held = map.get(1).expect("the build was just inserted");

        for build in 2..=NON_MAIN_BUILD_CAPACITY as u32 + 1 {
            map.insert(build, empty_build_data(build));
        }

        assert!(map.get(1).is_none(), "the map dropped its handle on build 1");
        assert_eq!(held.read().build_number, 1, "the caller's handle still reads");
    }

    /// The negative cache and the LRU are individually correct and would
    /// destroy each other if joined: a directory spanning many builds evicts
    /// constantly, and an eviction that counted as a failure would walk the
    /// whole directory into "unavailable".
    #[test]
    fn evicting_a_build_does_not_mark_it_unresolvable() {
        let map = test_map_with_no_data();
        map.insert(1, empty_build_data(1));
        for build in 2..=NON_MAIN_BUILD_CAPACITY as u32 + 1 {
            map.insert(build, empty_build_data(build));
        }

        assert!(map.get(1).is_none(), "build 1 was evicted");
        assert!(!map.is_unresolvable_build(1), "an evicted build was loadable and still is");
    }

    /// The scan asks about every build in a directory. Answering by loading would
    /// cost seconds per build for the ones that are absent, which is the whole
    /// reason this check exists apart from resolution.
    #[test]
    fn checking_availability_does_not_run_the_loading_path() {
        let map = test_map_with_no_data();
        assert!(!map.has_data_for(&request(15, 0, 0, 9_999_999)));
        assert_eq!(map.resolution_attempts(9_999_999), 0, "no load may be paid to answer this");
    }

    /// A build already resident is available without consulting the disk at all.
    #[test]
    fn a_loaded_build_is_available() {
        let map = test_map_with_no_data();
        map.insert(7_000_000, empty_build_data(7_000_000));
        assert!(map.has_data_for(&request(15, 0, 0, 7_000_000)));
    }
}

pub struct WorldOfWarshipsData {
    pub vfs: VfsPath,

    /// We may fail to load game params
    pub game_metadata: Option<Arc<GameMetadataProvider>>,

    pub ship_icons: HashMap<Species, Arc<GameAsset>>,

    /// Ribbon icons keyed by ribbon name (e.g., "ribbon_main_caliber")
    pub ribbon_icons: HashMap<String, Arc<GameAsset>>,

    /// Subribbon icons keyed by ribbon name (e.g., "ribbon_main_caliber")
    pub subribbon_icons: HashMap<String, Arc<GameAsset>>,

    /// Achievement icons, lazy-loaded and cached. Keyed by achievement name (lowercase).
    pub achievement_icons: HashMap<String, Arc<GameAsset>>,

    /// Consumable icons, lazy-loaded and cached. Keyed by PCY name
    /// (e.g. `"PCY009_CrashCrewPremium"`).
    pub consumable_icons: HashMap<String, Arc<GameAsset>>,

    /// Captain-skill icons, lazy-loaded and cached. Keyed by skill name.
    pub crew_skill_icons: HashMap<CrewSkillName, Arc<GameAsset>>,

    /// Modernization (upgrade) icons, lazy-loaded and cached. Keyed by PCM name.
    pub modernization_icons: HashMap<String, Arc<GameAsset>>,

    /// Signal-flag icons, lazy-loaded and cached. Keyed by PCEF name.
    pub signal_flag_icons: HashMap<String, Arc<GameAsset>>,

    /// Cached game constants loaded from game files.
    pub game_constants: Arc<GameConstants>,

    /// Version-matched replay constants (from wows-constants repo).
    pub replay_constants: Arc<RwLock<serde_json::Value>>,

    /// Whether the replay constants are an exact match for this build,
    /// or a fallback from a previous build.
    pub replay_constants_exact_match: bool,

    pub full_version: Option<Version>,
    pub patch_version: usize,

    /// The build number this data was loaded for.
    pub build_number: u32,

    pub replays_dir: PathBuf,

    #[allow(dead_code)]
    pub build_dir: PathBuf,

    /// If this data was loaded from a dump directory (not the live install),
    /// this holds the dump path for translation reloading.
    pub dump_dir: Option<PathBuf>,
}

impl WorldOfWarshipsData {
    /// The full `major.minor.patch` game version for this data, if known. The
    /// resolver branches on this (never the build number, which differs across
    /// servers); `None` means use the newest layout with its fallbacks.
    pub fn version(&self) -> Option<&Version> {
        self.full_version.as_ref()
    }

    /// Load a GUI asset by what it is, letting the resolver pick the right path
    /// for this build's version. Returns `None` when the asset isn't present.
    fn load_gui_asset(&self, asset: wowsunpack::game_assets::GuiAsset<'_>) -> Option<Arc<GameAsset>> {
        let resolved = asset.resolve(&self.vfs, self.version())?;
        let path = resolved.as_str().trim_start_matches('/').to_owned();
        let mut data = Vec::new();
        resolved.open_file().ok()?.read_to_end(&mut data).ok()?;
        Some(Arc::new(GameAsset { path, data }))
    }

    /// Look up a cached achievement icon (read-only, no loading).
    pub fn cached_achievement_icon(&self, icon_key: &str) -> Option<Arc<GameAsset>> {
        self.achievement_icons.get(icon_key).cloned()
    }

    /// Load and cache an achievement icon from the game files.
    /// Only call this on a cache miss (when `cached_achievement_icon` returns None).
    pub fn load_achievement_icon(&mut self, icon_key: &str) -> Option<Arc<GameAsset>> {
        // Double-check in case another call populated it
        if let Some(icon) = self.achievement_icons.get(icon_key) {
            return Some(icon.clone());
        }

        let asset = self.load_gui_asset(wowsunpack::game_assets::GuiAsset::Achievement(icon_key))?;
        self.achievement_icons.insert(icon_key.to_string(), asset.clone());
        Some(asset)
    }

    /// Look up a cached consumable icon (read-only, no loading).
    pub fn cached_consumable_icon(&self, icon_key: &str) -> Option<Arc<GameAsset>> {
        self.consumable_icons.get(icon_key).cloned()
    }

    /// Load and cache a consumable icon by PCY identifier.
    /// Only call this on a cache miss (when `cached_consumable_icon` returns None).
    pub fn load_consumable_icon(&mut self, icon_key: &str) -> Option<Arc<GameAsset>> {
        if let Some(icon) = self.consumable_icons.get(icon_key) {
            return Some(icon.clone());
        }

        let asset = self.load_gui_asset(wowsunpack::game_assets::GuiAsset::Consumable(icon_key))?;
        self.consumable_icons.insert(icon_key.to_string(), asset.clone());
        Some(asset)
    }

    /// Look up a cached crew-skill icon (read-only, no loading).
    pub fn cached_crew_skill_icon(&self, name: &CrewSkillName) -> Option<Arc<GameAsset>> {
        self.crew_skill_icons.get(name).cloned()
    }

    /// Load and cache a crew-skill icon by skill name.
    /// Only call this on a cache miss (when `cached_crew_skill_icon` returns None).
    pub fn load_crew_skill_icon(&mut self, name: &CrewSkillName) -> Option<Arc<GameAsset>> {
        if let Some(icon) = self.crew_skill_icons.get(name) {
            return Some(icon.clone());
        }

        let asset = self.load_gui_asset(wowsunpack::game_assets::GuiAsset::CrewSkill { name })?;
        self.crew_skill_icons.insert(name.clone(), asset.clone());
        Some(asset)
    }

    /// Look up a cached modernization icon (read-only, no loading).
    pub fn cached_modernization_icon(&self, name: &str) -> Option<Arc<GameAsset>> {
        self.modernization_icons.get(name).cloned()
    }

    /// Load and cache a modernization icon by PCM name.
    /// Only call this on a cache miss (when `cached_modernization_icon` returns None).
    pub fn load_modernization_icon(&mut self, name: &str) -> Option<Arc<GameAsset>> {
        if let Some(icon) = self.modernization_icons.get(name) {
            return Some(icon.clone());
        }

        let asset = self.load_gui_asset(wowsunpack::game_assets::GuiAsset::Modernization(name))?;
        self.modernization_icons.insert(name.to_string(), asset.clone());
        Some(asset)
    }

    /// Look up a cached signal-flag icon (read-only, no loading).
    pub fn cached_signal_flag_icon(&self, name: &str) -> Option<Arc<GameAsset>> {
        self.signal_flag_icons.get(name).cloned()
    }

    /// Load and cache a signal-flag icon by PCEF name.
    /// Only call this on a cache miss (when `cached_signal_flag_icon` returns None).
    pub fn load_signal_flag_icon(&mut self, name: &str) -> Option<Arc<GameAsset>> {
        if let Some(icon) = self.signal_flag_icons.get(name) {
            return Some(icon.clone());
        }

        let asset = self.load_gui_asset(wowsunpack::game_assets::GuiAsset::SignalFlag(name))?;
        self.signal_flag_icons.insert(name.to_string(), asset.clone());
        Some(asset)
    }

    /// Rebuild this data from scratch after constants have changed.
    /// Retains: build_dir, replays_dir, game_metadata, pkg_loader, file_tree,
    /// full_version, patch_version, build_number.
    /// Regenerates everything else (icons, game_constants, replay_constants, etc.).
    /// Returns `false` if versioned constants could not be found on disk.
    #[instrument(skip(self), fields(build = self.build_number))]
    pub fn rebuild_with_new_constants(&mut self) -> bool {
        use crate::task::build_game_constants;
        use crate::task::load_versioned_constants_from_disk_with_fallback;

        debug!("Rebuilding WorldOfWarshipsData for build {}", self.build_number);

        // Reload version-matched replay constants from disk only (no network I/O).
        // If not on disk, use our current constants as fallback (better than failing).
        let (new_replay_constants, exact_match) =
            match load_versioned_constants_from_disk_with_fallback(self.build_number) {
                Some((data, exact)) => (data, exact),
                None => {
                    debug!(
                        "No cached versioned constants for build {} during rebuild, using current constants",
                        self.build_number
                    );
                    (self.replay_constants.read().clone(), false)
                }
            };

        // Rebuild game constants from VFS + new replay constants
        let new_game_constants = build_game_constants(&self.vfs, &new_replay_constants, self.full_version);

        // Reload all icons from game files
        let version = self.full_version.as_ref();
        let new_ship_icons = crate::task::load_ship_icons(&self.vfs, version);
        let new_ribbon_icons =
            crate::task::load_ribbon_icons(&self.vfs, wowsunpack::game_assets::GuiAssetDir::Ribbons, version);
        let new_subribbon_icons =
            crate::task::load_ribbon_icons(&self.vfs, wowsunpack::game_assets::GuiAssetDir::SubRibbons, version);

        // Apply all regenerated fields
        self.ship_icons = new_ship_icons;
        self.ribbon_icons = new_ribbon_icons;
        self.subribbon_icons = new_subribbon_icons;
        self.achievement_icons = HashMap::new();
        self.consumable_icons = HashMap::new();
        self.crew_skill_icons = HashMap::new();
        self.modernization_icons = HashMap::new();
        self.signal_flag_icons = HashMap::new();
        self.game_constants = Arc::new(new_game_constants);
        *self.replay_constants.write() = new_replay_constants;
        self.replay_constants_exact_match = exact_match;

        debug!("Rebuild complete for build {}", self.build_number);
        true
    }
}

/// Shared dependencies needed for loading and parsing replays.
/// This bundles together all the Arc-wrapped state that replay loading requires.
#[derive(Clone)]
pub struct ReplayDependencies {
    pub wows_data_map: WoWsDataMap,
    pub twitch_state: Arc<RwLock<crate::twitch::TwitchState>>,
    pub replay_sort: Arc<Mutex<SortOrder>>,
    pub background_task_sender: mpsc::Sender<BackgroundTask>,
    pub is_debug_mode: bool,
    pub personal_rating_data: Arc<RwLock<crate::util::personal_rating::PersonalRatingData>>,
}

impl ReplayDependencies {
    /// Resolve version-matched deps for a specific build. Returns None if
    /// the build data can't be loaded.
    pub fn resolve_versioned_deps(&self, version: &Version) -> Option<SharedWoWsData> {
        self.wows_data_map.resolve(version)
    }

    /// Read a replay file from disk and start loading it in the background.
    ///
    /// The file read and the (potentially expensive) game data resolution both
    /// happen on the background thread so the UI never blocks.
    pub fn parse_replay_from_path<P: AsRef<Path>>(
        &self,
        replay_path: P,
        source: ReplaySource,
    ) -> Option<BackgroundTask> {
        ReplayLoader::from_path(self.clone(), replay_path.as_ref().to_path_buf()).source(source).load()
    }

    /// Load an already-parsed replay in the background.
    pub fn load_replay(&self, replay: Arc<RwLock<Replay>>, source: ReplaySource) -> Option<BackgroundTask> {
        ReplayLoader::from_replay(self.clone(), replay).source(source).load()
    }
}

/// What a [`ReplayLoader`] starts from: either a replay already constructed by
/// the caller, or a path to read and construct on the background thread.
enum ReplayInput {
    Built(Arc<RwLock<Replay>>),
    Path(PathBuf),
}

/// Builder for loading replays in the background with configurable options
pub struct ReplayLoader {
    deps: ReplayDependencies,
    input: ReplayInput,
    replay_source: ReplaySource,
}

impl ReplayLoader {
    pub fn from_replay(deps: ReplayDependencies, replay: Arc<RwLock<Replay>>) -> Self {
        Self { deps, input: ReplayInput::Built(replay), replay_source: ReplaySource::FileListing }
    }

    pub fn from_path(deps: ReplayDependencies, path: PathBuf) -> Self {
        Self { deps, input: ReplayInput::Path(path), replay_source: ReplaySource::FileListing }
    }

    /// Set the source of this replay load request.
    pub fn source(mut self, source: ReplaySource) -> Self {
        self.replay_source = source;
        self
    }

    /// Start loading the replay in the background
    pub fn load(self) -> Option<BackgroundTask> {
        let source = self.replay_source;

        let (tx, rx) = mpsc::channel();

        let deps = self.deps;
        let input = self.input;

        let _join_handle = crate::util::thread::spawn_logged("load-replay", move || {
            // For a path input, read the file and construct the replay here so
            // the retry loop that waits out a game still flushing it, and the
            // build resolution that may lazily load a whole build, stay off the
            // UI thread.
            let replay = match input {
                ReplayInput::Built(replay) => replay,
                ReplayInput::Path(path) => match Self::build_replay_from_path(&deps, path) {
                    Ok((replay, _)) => replay,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                },
            };

            // Determine the replay's build and get version-matched data
            let raw_version = replay.read().replay_file.meta.clientVersionFromExe.clone();
            let Some(replay_version) = Version::try_from_client_exe(&raw_version) else {
                let _ =
                    tx.send(Err(rootcause::report!("replay reports an unreadable client version {:?}", raw_version)));
                return;
            };

            let Some(wows_data_for_build) = deps.wows_data_map.resolve(&replay_version) else {
                error!("Failed to load game data for version {}", replay_version.to_path());
                let replay_path = replay.read().source_path.clone();
                let _ = tx.send(Err(missing_build_report(&replay_version, replay_path)));
                return;
            };

            let game_version = {
                let data = wows_data_for_build.read();
                // Update the replay's resource loader and game constants to match
                // the version-matched data, in case it was originally constructed
                // with a different version's metadata (e.g. at startup).
                if let Some(game_metadata) = &data.game_metadata {
                    let mut replay_guard = replay.write();
                    replay_guard.resource_loader = Arc::clone(game_metadata);
                    replay_guard.game_constants = Some(Arc::clone(&data.game_constants));
                }
                data.patch_version
            };

            let res = { replay.read().parse(game_version.to_string().as_str()) };
            let res = res.map(|report| {
                {
                    #[cfg(feature = "shipbuilds_debugging")]
                    {
                        let wows_data_inner = wows_data_for_build.read();
                        let metadata_provider = wows_data_inner.game_metadata.as_ref().unwrap();
                        // Send the replay builds to the remote server
                        for player in report.players() {
                            let client = reqwest::blocking::Client::new();
                            client
                                .post("http://shipbuilds.com/api/ship_builds")
                                .json(&crate::util::build_tracker::BuildTrackerPayload::build_from(
                                    player,
                                    player.initial_state().realm().unwrap_or_default().to_owned(),
                                    report.version(),
                                    report.game_type().to_string(),
                                    metadata_provider,
                                ))
                                .send()
                                .expect("failed to POST build data");
                        }
                        drop(wows_data_inner);
                    }

                    let mut replay_guard = replay.write();
                    replay_guard.battle_report = Some(report);
                    replay_guard.build_ui_report(&deps);
                }
                BackgroundTaskCompletion::ReplayLoaded { replay, source }
            });

            let _ = tx.send(res);
        });

        Some(BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingReplay })
    }

    /// Read a replay file and construct a [`Replay`] wired to the version-matched
    /// game data. Runs on the background thread; resolving the data may lazily
    /// load a build, which is exactly the work we keep off the UI thread.
    ///
    /// The resolved data is returned with the replay so a caller that needs the
    /// build behind it does not resolve the same version again.
    pub(crate) fn build_replay_from_path(
        deps: &ReplayDependencies,
        path: PathBuf,
    ) -> Result<(Arc<RwLock<Replay>>, SharedWoWsData), rootcause::Report> {
        let replay_file = read_replay_file_with_retry(&path)?;
        Self::build_replay_from_file(deps, replay_file, path)
    }

    /// Read a replay the caller already knows exists and construct a [`Replay`]
    /// for it. Unlike [`Self::build_replay_from_path`] this does not wait out a
    /// game still flushing the file: a listing row names a file that was
    /// complete when it was listed, and a retry loop here would run on the UI
    /// thread.
    pub(crate) fn build_replay_from_existing_file(
        deps: &ReplayDependencies,
        path: PathBuf,
    ) -> Result<(Arc<RwLock<Replay>>, SharedWoWsData), rootcause::Report> {
        let replay_file =
            ReplayFile::from_file(&path).map_err(|e| e.into_dynamic().attach(format!("path: {}", path.display())))?;
        Self::build_replay_from_file(deps, replay_file, path)
    }

    fn build_replay_from_file(
        deps: &ReplayDependencies,
        replay_file: ReplayFile,
        path: PathBuf,
    ) -> Result<(Arc<RwLock<Replay>>, SharedWoWsData), rootcause::Report> {
        let raw_version = &replay_file.meta.clientVersionFromExe;
        let Some(replay_version) = Version::try_from_client_exe(raw_version) else {
            return Err(rootcause::report!("replay reports an unreadable client version {raw_version:?}")
                .attach(format!("path: {}", path.display())));
        };

        let Some(wows_data_for_build) = deps.wows_data_map.resolve(&replay_version) else {
            return Err(missing_build_report(&replay_version, Some(path)));
        };

        let (game_metadata, game_constants) = {
            let data = wows_data_for_build.read();
            let Some(metadata) = data.game_metadata.clone() else {
                return Err(rootcause::report!("game metadata unavailable for version {}", replay_version.to_path()));
            };
            (metadata, Arc::clone(&data.game_constants))
        };

        let mut replay = Replay::new(replay_file, game_metadata);
        replay.game_constants = Some(game_constants);
        replay.source_path = Some(path);
        Ok((Arc::new(RwLock::new(replay)), wows_data_for_build))
    }
}

/// The failure for a replay whose build's game data is not on this machine.
///
/// `clientVersionFromExe` carries a `0` build field for the pre-0.10 era, and a
/// version with no build resolves to no data at all. There is nothing to offer
/// for download in that case, so it gets a plain report rather than a
/// [`ToolkitError::ReplayBuildUnavailable`] naming a build that does not exist.
pub(crate) fn missing_build_report(version: &Version, replay_path: Option<PathBuf>) -> rootcause::Report {
    let Some(build) = version.build_number() else {
        return rootcause::report!(
            "replay reports version {} with no build number, so its game data cannot be resolved",
            version.to_path()
        );
    };
    let report: rootcause::Report =
        ToolkitError::ReplayBuildUnavailable { build, version: version.to_path(), replay_path }.into();
    report.attach("try installing the matching game client version")
}

/// Read and parse a replay, retrying while the game finishes flushing it.
///
/// The filesystem watcher fires as soon as the game creates or renames the
/// file, but the metadata JSON and packet stream may still be mid-write, and
/// Windows virus scanners can briefly hold the new file open. Retrying over a
/// few seconds off the UI thread avoids dropping a replay right after a match.
fn read_replay_file_with_retry(path: &Path) -> Result<ReplayFile, rootcause::Report> {
    const MAX_ATTEMPTS: u32 = 8;
    const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(750);

    let mut attempt = 1;
    loop {
        match ReplayFile::from_file(path) {
            Ok(replay_file) => return Ok(replay_file),
            Err(e) => {
                let e = e.into_dynamic().attach(format!("path: {}", path.display()));
                if attempt >= MAX_ATTEMPTS {
                    error!("failed to read replay after {MAX_ATTEMPTS} attempts: {e:?}");
                    return Err(e.attach(format!("gave up after {MAX_ATTEMPTS} attempts")));
                }
                warn!("replay not ready (attempt {attempt}/{MAX_ATTEMPTS}), retrying: {e:?}");
                attempt += 1;
                std::thread::sleep(RETRY_DELAY);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `clientVersionFromExe` whose build field is `0` is what the pre-0.10
    /// era records, and [`WoWsDataMap::resolve`] returns `None` for any version
    /// carrying no build. Every one of those replays therefore reaches this
    /// failure path, which has to describe the failure rather than assume a
    /// build it can name.
    #[test]
    fn a_version_with_no_build_reports_the_version_instead_of_a_build() {
        let version = Version::from_client_exe("0,9,4,0");
        assert_eq!(version.build_number(), None, "the fixture must actually carry no build");

        let report = missing_build_report(&version, Some(PathBuf::from("old.wowsreplay")));

        assert!(
            report.downcast_current_context::<ToolkitError>().is_none(),
            "there is no build to offer for download, so this must not be a ReplayBuildUnavailable"
        );
        assert!(format!("{report:?}").contains("0.9.4"), "the report must name the version it could not resolve");
    }

    /// The download prompt and the ingest failure tally both downcast to
    /// `ReplayBuildUnavailable` to learn which build to offer, so a version that
    /// does carry one must still produce that error.
    #[test]
    fn a_version_with_a_build_still_offers_that_build_for_download() {
        let version = Version::from_client_exe("15,4,0,11965230");

        let report = missing_build_report(&version, None);

        match report.downcast_current_context::<ToolkitError>() {
            Some(ToolkitError::ReplayBuildUnavailable { build, version, .. }) => {
                assert_eq!(*build, 11_965_230);
                assert_eq!(version, "15.4.0");
            }
            _ => panic!("a resolvable build must produce ReplayBuildUnavailable: {report:?}"),
        }
    }
}

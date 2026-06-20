use std::collections::HashMap;
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

/// Manages all loaded game data versions, keyed by build number.
/// Provides version resolution for replay parsing and lazy-loading of build data.
#[derive(Clone)]
pub struct WoWsDataMap {
    builds: Arc<RwLock<HashMap<u32, SharedWoWsData>>>,
    wows_dir: PathBuf,
    locale: String,
    network_job_tx: Option<mpsc::Sender<NetworkJob>>,
    /// Custom game data cache directory. Empty means use the default.
    game_data_cache_dir: String,
}

impl WoWsDataMap {
    pub fn new(wows_dir: PathBuf, locale: String) -> Self {
        Self {
            builds: Arc::new(RwLock::new(HashMap::new())),
            wows_dir,
            locale,
            network_job_tx: None,
            game_data_cache_dir: String::new(),
        }
    }

    pub fn set_game_data_cache_dir(&mut self, dir: String) {
        self.game_data_cache_dir = dir;
    }

    pub fn set_network_job_tx(&mut self, tx: mpsc::Sender<NetworkJob>) {
        self.network_job_tx = Some(tx);
    }

    /// Insert data for a specific build number.
    pub fn insert(&self, build: u32, data: SharedWoWsData) {
        self.builds.write().insert(build, data);
    }

    /// Look up already-loaded data by build number. Does NOT lazy-load.
    pub fn get(&self, build: u32) -> Option<SharedWoWsData> {
        self.builds.read().get(&build).cloned()
    }

    /// Iterate over loaded builds with a closure (avoids exposing the inner lock).
    pub fn with_builds<R>(&self, f: impl FnOnce(&HashMap<u32, SharedWoWsData>) -> R) -> R {
        f(&self.builds.read())
    }

    /// Rebuild all loaded builds' data after constants have changed.
    /// Returns `true` if all builds rebuilt successfully, `false` if any failed.
    #[instrument(skip(self))]
    pub fn rebuild_all_with_new_constants(&self) -> bool {
        let builds = self.builds.read();
        let mut all_ok = true;
        for (build, data) in builds.iter() {
            debug!("Rebuilding data for build {}", build);
            if !data.write().rebuild_with_new_constants() {
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

        let builds = self.builds.read();
        for (build, data) in builds.iter() {
            let data = data.read();
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
            .builds
            .read()
            .values()
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
            .builds
            .read()
            .values()
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
            .builds
            .read()
            .values()
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

        // Constants (CONSUMABLE_IDS / BATTLE_STAGES) are version-specific. Only
        // bridge them FORWARD: an already-loaded build's constants may stand in
        // for a build we're loading that is NEWER than it (a fresh game version
        // we haven't dumped yet). Never apply newer constants to an OLDER replay
        // -- that corrupts the interpretation (consumables, battle stages,
        // connection/observed state all read wrong). For older builds, fall back
        // to the build's own VFS constants (Null = no override).
        let fallback_constants = {
            let builds = self.builds.read();
            let mut best: Option<(u32, serde_json::Value)> = None;
            for data in builds.values() {
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

            // Construct version string for cross-region fallback
            let version_hint = {
                let builds = self.builds.read();
                builds.values().next().and_then(|d| {
                    d.read().full_version.as_ref().map(|v| format!("{}.{}.{}", v.major, v.minor, v.patch))
                })
            };

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

        None
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
            // For a path input, read the file and construct the replay here on
            // the background thread so the UI never blocks on file I/O.
            let replay = match input {
                ReplayInput::Built(replay) => replay,
                ReplayInput::Path(path) => match Self::build_replay_from_path(&deps, path) {
                    Ok(replay) => replay,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                },
            };

            // Determine the replay's build and get version-matched data
            let replay_version = {
                let r = replay.read();
                Version::from_client_exe(&r.replay_file.meta.clientVersionFromExe)
            };
            let build = replay_version.build_number().expect("replay version carries a build");

            let Some(wows_data_for_build) = deps.wows_data_map.resolve(&replay_version) else {
                error!("Failed to load game data for build {}", build);
                let replay_path = replay.read().source_path.clone();
                let report: rootcause::Report =
                    ToolkitError::ReplayBuildUnavailable { build, version: replay_version.to_path(), replay_path }
                        .into();
                let _ = tx.send(Err(report.attach("try installing the matching game client version")));
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
    fn build_replay_from_path(
        deps: &ReplayDependencies,
        path: PathBuf,
    ) -> Result<Arc<RwLock<Replay>>, rootcause::Report> {
        let replay_file = ReplayFile::from_file(&path).map_err(|e| e.into_dynamic())?;
        let replay_version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);

        let Some(wows_data_for_build) = deps.wows_data_map.resolve(&replay_version) else {
            let report: rootcause::Report = ToolkitError::ReplayBuildUnavailable {
                build: replay_version.build_number().expect("replay version carries a build"),
                version: replay_version.to_path(),
                replay_path: Some(path),
            }
            .into();
            return Err(report.attach("try installing the matching game client version"));
        };

        let (game_metadata, game_constants) = {
            let data = wows_data_for_build.read();
            let Some(metadata) = data.game_metadata.clone() else {
                return Err(rootcause::report!(
                    "game metadata unavailable for build {}",
                    replay_version.build_number().expect("replay version carries a build")
                ));
            };
            (metadata, Arc::clone(&data.game_constants))
        };

        let mut replay = Replay::new(replay_file, game_metadata);
        replay.game_constants = Some(game_constants);
        replay.source_path = Some(path);
        Ok(Arc::new(RwLock::new(replay)))
    }
}

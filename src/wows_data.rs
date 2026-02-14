use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;

use parking_lot::Mutex;
use parking_lot::RwLock;
use tracing::debug;
use tracing::error;
use tracing::warn;
use wows_replays::ReplayFile;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::Version;
use wowsunpack::data::idx::FileNode;
use wowsunpack::data::pkg::PkgFileLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::Species;

use crate::error::ToolkitError;
use crate::task::BackgroundTask;
use crate::task::BackgroundTaskCompletion;
use crate::task::BackgroundTaskKind;
use crate::task::load_wows_data_for_build;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::SortOrder;

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
}

impl WoWsDataMap {
    pub fn new(wows_dir: PathBuf, locale: String) -> Self {
        Self { builds: Arc::new(RwLock::new(HashMap::new())), wows_dir, locale }
    }

    /// Insert data for a specific build number.
    pub fn insert(&self, build: u32, data: SharedWoWsData) {
        self.builds.write().insert(build, data);
    }

    /// Look up already-loaded data by build number. Does NOT lazy-load.
    pub fn get(&self, build: u32) -> Option<SharedWoWsData> {
        self.builds.read().get(&build).cloned()
    }

    /// Returns all loaded build numbers.
    pub fn loaded_builds(&self) -> Vec<u32> {
        self.builds.read().keys().copied().collect()
    }

    /// Iterate over loaded builds with a closure (avoids exposing the inner lock).
    pub fn with_builds<R>(&self, f: impl FnOnce(&HashMap<u32, SharedWoWsData>) -> R) -> R {
        f(&self.builds.read())
    }

    /// Rebuild all loaded builds' data after constants have changed.
    /// Returns `true` if all builds rebuilt successfully, `false` if any failed.
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

    /// Resolve the correct game data for a replay's version.
    /// Checks the map first, then tries to lazy-load from disk.
    /// Returns None if the version's build data is unavailable.
    pub fn resolve(&self, version: &Version) -> Option<SharedWoWsData> {
        let build = version.build;

        // Check if already loaded
        if let Some(data) = self.get(build) {
            return Some(data);
        }

        // Try to load from disk
        let build_dir = self.wows_dir.join("bin").join(build.to_string());
        if !build_dir.exists() {
            return None;
        }

        debug!("Lazily loading game data for build {}", build);
        let fallback_constants = {
            // Use any already-loaded build's constants as fallback
            let builds = self.builds.read();
            builds.values().next().map(|d| d.read().replay_constants.read().clone())
        };
        let fallback_constants = fallback_constants.unwrap_or_default();

        match load_wows_data_for_build(&self.wows_dir, build, &self.locale, &fallback_constants) {
            Ok(wows_data) => {
                let shared: SharedWoWsData = Arc::new(RwLock::new(Box::new(wows_data)));
                self.insert(build, Arc::clone(&shared));
                Some(shared)
            }
            Err(e) => {
                warn!("Could not load data for build {}: {}", build, e);
                None
            }
        }
    }
}

pub struct WorldOfWarshipsData {
    pub file_tree: FileNode,

    pub filtered_files: Vec<(wowsunpack::Rc<PathBuf>, FileNode)>,

    pub pkg_loader: Arc<PkgFileLoader>,

    /// We may fail to load game params
    pub game_metadata: Option<Arc<GameMetadataProvider>>,

    pub ship_icons: HashMap<Species, Arc<GameAsset>>,

    /// Ribbon icons keyed by ribbon name (e.g., "ribbon_main_caliber")
    pub ribbon_icons: HashMap<String, Arc<GameAsset>>,

    /// Subribbon icons keyed by ribbon name (e.g., "ribbon_main_caliber")
    pub subribbon_icons: HashMap<String, Arc<GameAsset>>,

    /// Achievement icons, lazy-loaded and cached. Keyed by achievement name (lowercase).
    pub achievement_icons: HashMap<String, Arc<GameAsset>>,

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

    pub build_dir: PathBuf,
}

impl WorldOfWarshipsData {
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

        let path = wowsunpack::game_params::translations::achievement_icon_path(icon_key);
        let icon_node = self.file_tree.find(&path).ok()?;
        let file_info = icon_node.file_info()?;

        let mut icon_data = Vec::with_capacity(file_info.unpacked_size as usize);
        icon_node.read_file(&self.pkg_loader, &mut icon_data).ok()?;

        let asset = Arc::new(GameAsset { path, data: icon_data });
        self.achievement_icons.insert(icon_key.to_string(), asset.clone());
        Some(asset)
    }

    /// Returns a display-friendly version string (e.g., "15.0.0" or "build 11791718").
    pub fn version_label(&self) -> String {
        if let Some(v) = &self.full_version { v.to_path() } else { format!("build {}", self.build_number) }
    }

    /// Rebuild this data from scratch after constants have changed.
    /// Retains: build_dir, replays_dir, game_metadata, pkg_loader, filtered_files, file_tree,
    /// full_version, patch_version, build_number.
    /// Regenerates everything else (icons, game_constants, replay_constants, etc.).
    /// Returns `false` if versioned constants could not be fetched (network/disk failure).
    pub fn rebuild_with_new_constants(&mut self) -> bool {
        use crate::task::build_game_constants;
        use crate::task::fetch_versioned_constants_with_fallback;

        debug!("Rebuilding WorldOfWarshipsData for build {}", self.build_number);

        // Reload version-matched replay constants
        let (new_replay_constants, exact_match) = match fetch_versioned_constants_with_fallback(self.build_number) {
            Some((data, exact)) => (data, exact),
            None => {
                warn!("Failed to fetch versioned constants for build {} during rebuild", self.build_number);
                return false;
            }
        };

        // Rebuild game constants from pkg files + new replay constants
        let new_game_constants =
            build_game_constants(&self.file_tree, &self.pkg_loader, &new_replay_constants, self.build_number);

        // Reload all icons from game files
        let new_ship_icons = crate::task::load_ship_icons(self.file_tree.clone(), &self.pkg_loader);
        let new_ribbon_icons = crate::task::load_ribbon_icons(
            &self.file_tree,
            &self.pkg_loader,
            wowsunpack::game_params::translations::RIBBON_ICONS_DIR,
        );
        let new_subribbon_icons = crate::task::load_ribbon_icons(
            &self.file_tree,
            &self.pkg_loader,
            wowsunpack::game_params::translations::RIBBON_SUBICONS_DIR,
        );

        // Apply all regenerated fields
        self.ship_icons = new_ship_icons;
        self.ribbon_icons = new_ribbon_icons;
        self.subribbon_icons = new_subribbon_icons;
        self.achievement_icons = HashMap::new();
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

    /// Parse a replay file from disk and start loading it in the background.
    pub fn parse_replay_from_path<P: AsRef<Path>>(
        &self,
        replay_path: P,
        update_ui: bool,
        track_session_stats: bool,
    ) -> Option<BackgroundTask> {
        let path = replay_path.as_ref();

        let replay_file: ReplayFile = ReplayFile::from_file(path).unwrap();
        let replay_version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);

        let wows_data_for_build = self.wows_data_map.resolve(&replay_version)?;

        let (game_metadata, game_constants) = {
            let data = wows_data_for_build.read();
            (data.game_metadata.clone()?, Arc::clone(&data.game_constants))
        };
        let mut replay = Replay::new(replay_file, game_metadata);
        replay.game_constants = Some(game_constants);
        replay.source_path = Some(path.to_path_buf());

        let mut loader = ReplayLoader::new(self.clone(), Arc::new(RwLock::new(replay)));
        if !update_ui {
            loader = loader.skip_ui_update();
        }
        if !track_session_stats {
            loader = loader.skip_session_stats();
        }
        loader.load()
    }

    /// Load an already-parsed replay in the background.
    pub fn load_replay(&self, replay: Arc<RwLock<Replay>>, update_ui: bool) -> Option<BackgroundTask> {
        let loader = ReplayLoader::new(self.clone(), replay);

        if update_ui { loader.load() } else { loader.skip_ui_update().load() }
    }
}

/// Builder for loading replays in the background with configurable options
pub struct ReplayLoader {
    deps: ReplayDependencies,
    replay: Arc<RwLock<Replay>>,
    skip_ui_update: bool,
    track_session_stats: bool,
}

impl ReplayLoader {
    pub fn new(deps: ReplayDependencies, replay: Arc<RwLock<Replay>>) -> Self {
        Self { deps, replay, skip_ui_update: false, track_session_stats: true }
    }

    /// Skip updating the UI when the replay finishes loading.
    /// Useful for batch loading like session stats.
    pub fn skip_ui_update(mut self) -> Self {
        self.skip_ui_update = true;
        self
    }

    /// Don't track this replay in session stats.
    /// Used for manually opened or drag-and-dropped replays.
    pub fn skip_session_stats(mut self) -> Self {
        self.track_session_stats = false;
        self
    }

    /// Start loading the replay in the background
    pub fn load(self) -> Option<BackgroundTask> {
        let skip_ui_update = self.skip_ui_update;
        let track_session_stats = self.track_session_stats;

        let (tx, rx) = mpsc::channel();

        let deps = self.deps;
        let replay = self.replay;

        let _join_handle = std::thread::spawn(move || {
            // Determine the replay's build and get version-matched data
            let replay_version = {
                let r = replay.read();
                Version::from_client_exe(&r.replay_file.meta.clientVersionFromExe)
            };
            let build = replay_version.build;

            let Some(wows_data_for_build) = deps.wows_data_map.resolve(&replay_version) else {
                error!("Failed to load game data for build {}", build);
                let _ = tx.send(Err(
                    ToolkitError::ReplayBuildUnavailable { build, version: replay_version.to_path() }.into()
                ));
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
                                .json(&crate::build_tracker::BuildTrackerPayload::build_from(
                                    player,
                                    player.initial_state().realm().to_owned(),
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
                BackgroundTaskCompletion::ReplayLoaded { replay, skip_ui_update, track_session_stats }
            });

            let _ = tx.send(res);
        });

        Some(BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingReplay })
    }
}

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

/// Maps build numbers to their loaded game data.
pub type WoWsDataMap = Arc<RwLock<HashMap<u32, SharedWoWsData>>>;

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
    /// Get an achievement icon by name, lazy-loading and caching it from the game files.
    /// The icon_key should be the lowercase achievement name (e.g., "pve_honorsstar").
    pub fn achievement_icon(&mut self, icon_key: &str) -> Option<Arc<GameAsset>> {
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
}

/// Shared dependencies needed for loading and parsing replays.
/// This bundles together all the Arc-wrapped state that replay loading requires.
#[derive(Clone)]
pub struct ReplayDependencies {
    pub game_constants: Arc<RwLock<serde_json::Value>>,
    pub wows_data: SharedWoWsData,
    pub wows_data_map: WoWsDataMap,
    pub wows_dir: PathBuf,
    pub locale: String,
    pub twitch_state: Arc<RwLock<crate::twitch::TwitchState>>,
    pub replay_sort: Arc<Mutex<SortOrder>>,
    pub background_task_sender: mpsc::Sender<BackgroundTask>,
    pub is_debug_mode: bool,
}

impl ReplayDependencies {
    /// Create a copy of these dependencies pointing at a specific build's data.
    /// This ensures `wows_data` and `game_constants` are version-matched.
    fn with_versioned_data(&self, versioned_wows_data: &SharedWoWsData) -> ReplayDependencies {
        let mut deps = self.clone();
        deps.wows_data = Arc::clone(versioned_wows_data);
        deps.game_constants = versioned_wows_data.read().replay_constants.clone();
        deps
    }

    /// Resolve version-matched deps for a specific build. Returns None if
    /// the build data can't be loaded (caller should fall back to latest).
    pub fn resolve_versioned_deps(&self, build: u32, version: &Version) -> Option<ReplayDependencies> {
        match self.get_or_load_build(build, version) {
            Ok(versioned_data) => Some(self.with_versioned_data(&versioned_data)),
            Err(e) => {
                warn!("Could not resolve versioned deps for build {}: {}", build, e);
                None
            }
        }
    }

    /// Get or load the WorldOfWarshipsData for a specific build number.
    /// Returns the SharedWoWsData for the build, or an error if the build is unavailable.
    fn get_or_load_build(&self, build: u32, version: &Version) -> Result<SharedWoWsData, ToolkitError> {
        // Check if already loaded
        {
            let map = self.wows_data_map.read();
            if let Some(data) = map.get(&build) {
                return Ok(Arc::clone(data));
            }
        }

        // Check if the build directory exists
        let build_dir = self.wows_dir.join("bin").join(build.to_string());
        if !build_dir.exists() {
            return Err(ToolkitError::ReplayBuildUnavailable { build, version: version.to_path() });
        }

        // Load data for this build
        debug!("Lazily loading game data for build {}", build);
        let wows_data = load_wows_data_for_build(&self.wows_dir, build, &self.locale, &self.game_constants.read())
            .map_err(|_| ToolkitError::ReplayBuildUnavailable { build, version: version.to_path() })?;
        let shared: SharedWoWsData = Arc::new(RwLock::new(Box::new(wows_data)));

        // Insert into map
        {
            let mut map = self.wows_data_map.write();
            map.insert(build, Arc::clone(&shared));
        }

        Ok(shared)
    }

    /// Parse a replay file from disk and start loading it in the background.
    pub fn parse_replay_from_path<P: AsRef<Path>>(&self, replay_path: P, update_ui: bool) -> Option<BackgroundTask> {
        let path = replay_path.as_ref();

        let replay_file: ReplayFile = ReplayFile::from_file(path).unwrap();
        let replay_version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
        let build = replay_version.build;

        let wows_data_for_build = match self.get_or_load_build(build, &replay_version) {
            Ok(data) => data,
            Err(e) => {
                warn!("Failed to load game data for replay: {}", e);
                // Fall back to latest version data
                Arc::clone(&self.wows_data)
            }
        };

        let (game_metadata, game_constants) = {
            let data = wows_data_for_build.read();
            (data.game_metadata.clone()?, Arc::clone(&data.game_constants))
        };
        let mut replay = Replay::new(replay_file, game_metadata);
        replay.game_constants = Some(game_constants);

        self.load_replay(Arc::new(RwLock::new(replay)), update_ui)
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
}

impl ReplayLoader {
    pub fn new(deps: ReplayDependencies, replay: Arc<RwLock<Replay>>) -> Self {
        Self { deps, replay, skip_ui_update: false }
    }

    /// Skip updating the UI when the replay finishes loading.
    /// Useful for batch loading like session stats.
    pub fn skip_ui_update(mut self) -> Self {
        self.skip_ui_update = true;
        self
    }

    /// Start loading the replay in the background
    pub fn load(self) -> Option<BackgroundTask> {
        let skip_ui_update = self.skip_ui_update;

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

            let wows_data_for_build = match deps.get_or_load_build(build, &replay_version) {
                Ok(data) => data,
                Err(e) => {
                    error!("Failed to load game data for build {}: {}", build, e);
                    let _ = tx.send(Err(e.into()));
                    return;
                }
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
                                    report.game_type().to_owned(),
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
                BackgroundTaskCompletion::ReplayLoaded { replay, skip_ui_update }
            });

            let _ = tx.send(res);
        });

        Some(BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingReplay })
    }
}

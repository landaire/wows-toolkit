use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;

use parking_lot::Mutex;
use parking_lot::RwLock;
use wows_replays::ReplayFile;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::idx::FileNode;
use wowsunpack::data::pkg::PkgFileLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::Species;

use crate::task::BackgroundTask;
use crate::task::BackgroundTaskCompletion;
use crate::task::BackgroundTaskKind;
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

    #[allow(dead_code)]
    pub full_version: Option<wowsunpack::data::Version>,
    pub patch_version: usize,

    pub replays_dir: PathBuf,

    #[allow(dead_code)]
    pub build_dir: PathBuf,
}

impl WorldOfWarshipsData {
    /// Get an achievement icon by name, lazy-loading and caching it from the game files.
    /// The icon_key should be the lowercase achievement name (e.g., "pve_honorsstar").
    pub fn achievement_icon(&mut self, icon_key: &str) -> Option<Arc<GameAsset>> {
        if let Some(icon) = self.achievement_icons.get(icon_key) {
            return Some(icon.clone());
        }

        let path = format!("gui/achievements/icon_achievement_{icon_key}.png");
        let icon_node = self.file_tree.find(&path).ok()?;
        let file_info = icon_node.file_info()?;

        let mut icon_data = Vec::with_capacity(file_info.unpacked_size as usize);
        icon_node.read_file(&self.pkg_loader, &mut icon_data).ok()?;

        let asset = Arc::new(GameAsset { path, data: icon_data });
        self.achievement_icons.insert(icon_key.to_string(), asset.clone());
        Some(asset)
    }
}

/// Shared dependencies needed for loading and parsing replays.
/// This bundles together all the Arc-wrapped state that replay loading requires.
#[derive(Clone)]
pub struct ReplayDependencies {
    pub game_constants: Arc<RwLock<serde_json::Value>>,
    pub wows_data: SharedWoWsData,
    pub twitch_state: Arc<RwLock<crate::twitch::TwitchState>>,
    pub replay_sort: Arc<Mutex<SortOrder>>,
    pub background_task_sender: mpsc::Sender<BackgroundTask>,
    pub is_debug_mode: bool,
}

impl ReplayDependencies {
    /// Parse a replay file from disk and start loading it in the background.
    pub fn parse_replay_from_path<P: AsRef<Path>>(&self, replay_path: P, update_ui: bool) -> Option<BackgroundTask> {
        let path = replay_path.as_ref();

        let replay_file: ReplayFile = ReplayFile::from_file(path).unwrap();
        let (game_metadata, game_constants) = {
            let data = self.wows_data.read();
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
        let game_version = { self.deps.wows_data.read().patch_version };
        let skip_ui_update = self.skip_ui_update;

        let (tx, rx) = mpsc::channel();

        let deps = self.deps;
        let replay = self.replay;

        let _join_handle = std::thread::spawn(move || {
            let res = { replay.read().parse(game_version.to_string().as_str()) };
            let res = res.map(|report| {
                {
                    #[cfg(feature = "shipbuilds_debugging")]
                    {
                        let wows_data_inner = deps.wows_data.read();
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

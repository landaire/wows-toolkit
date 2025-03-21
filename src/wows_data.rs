use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;

use parking_lot::Mutex;
use parking_lot::RwLock;
use wows_replays::ReplayFile;
use wowsunpack::data::idx::FileNode;
use wowsunpack::data::pkg::PkgFileLoader;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::Species;

use crate::task::BackgroundTask;
use crate::task::BackgroundTaskCompletion;
use crate::task::BackgroundTaskKind;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::SortOrder;

pub struct ShipIcon {
    pub path: String,
    pub data: Vec<u8>,
}

pub struct WorldOfWarshipsData {
    pub file_tree: FileNode,

    pub filtered_files: Vec<(wowsunpack::Rc<PathBuf>, FileNode)>,

    pub pkg_loader: Arc<PkgFileLoader>,

    /// We may fail to load game params
    pub game_metadata: Option<Arc<GameMetadataProvider>>,

    pub ship_icons: HashMap<Species, Arc<ShipIcon>>,

    pub game_version: usize,

    pub replays_dir: PathBuf,

    #[allow(dead_code)]
    pub build_dir: PathBuf,
}

pub fn parse_replay<P: AsRef<Path>>(
    game_constants: Arc<RwLock<serde_json::Value>>,
    wows_data: Arc<RwLock<WorldOfWarshipsData>>,
    replay_path: P,
    replay_sort: Arc<Mutex<SortOrder>>,
    background_task_sender: mpsc::Sender<BackgroundTask>,
    is_debug_mode: bool,
) -> Option<BackgroundTask> {
    let path = replay_path.as_ref();

    let replay_file: ReplayFile = ReplayFile::from_file(path).unwrap();
    let game_metadata = { wows_data.read().game_metadata.clone()? };
    let replay = Replay::new(replay_file, game_metadata);

    load_replay(game_constants, wows_data, Arc::new(RwLock::new(replay)), replay_sort, background_task_sender, is_debug_mode)
}

pub fn load_replay(
    game_constants: Arc<RwLock<serde_json::Value>>,
    wows_data: Arc<RwLock<WorldOfWarshipsData>>,
    replay: Arc<RwLock<Replay>>,
    replay_sort: Arc<Mutex<SortOrder>>,
    background_task_sender: mpsc::Sender<BackgroundTask>,
    is_debug_mode: bool,
) -> Option<BackgroundTask> {
    let game_version = { wows_data.read().game_version };

    let (tx, rx) = mpsc::channel();

    let _join_handle = std::thread::spawn(move || {
        let res = { replay.read().parse(game_version.to_string().as_str()) };
        let res = res.map(move |report| {
            {
                #[cfg(feature = "shipbuilds_debugging")]
                {
                    let wows_data_inner = wows_data.read();
                    let metadata_provider = wows_data_inner.game_metadata.as_ref().unwrap();
                    // Send the replay builds to the remote server
                    for player in report.player_entities() {
                        let client = reqwest::blocking::Client::new();
                        client
                            .post("http://shipbuilds.com/api/ship_builds")
                            .json(&crate::build_tracker::BuildTrackerPayload::build_from(
                                player,
                                player.player().unwrap().realm().to_owned(),
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

                replay_guard.build_ui_report(game_constants, wows_data, replay_sort, Some(background_task_sender), is_debug_mode);
            }
            BackgroundTaskCompletion::ReplayLoaded { replay }
        });

        let _ = tx.send(res);
    });

    Some(BackgroundTask { receiver: rx.into(), kind: BackgroundTaskKind::LoadingReplay })
}

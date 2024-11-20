use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
};

use parking_lot::RwLock;
use wows_replays::ReplayFile;
use wowsunpack::{
    data::{idx::FileNode, pkg::PkgFileLoader},
    game_params::{provider::GameMetadataProvider, types::Species},
};

use crate::{
    replay_parser::Replay,
    task::{BackgroundTask, BackgroundTaskCompletion, BackgroundTaskKind},
};

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
}

impl WorldOfWarshipsData {
    pub fn parse_replay<P: AsRef<Path>>(&self, replay_path: P) -> Option<BackgroundTask> {
        let path = replay_path.as_ref();

        let replay_file: ReplayFile = ReplayFile::from_file(path).unwrap();
        let game_metadata = self.game_metadata.clone()?;
        let replay = Replay::new(replay_file, game_metadata);

        self.load_replay(Arc::new(RwLock::new(replay)))
    }

    #[must_use]
    pub fn load_replay(&self, replay: Arc<RwLock<Replay>>) -> Option<BackgroundTask> {
        let game_version = self.game_version;

        let (tx, rx) = mpsc::channel();

        let metadata_provider = self.game_metadata.as_ref().unwrap().clone();
        let _join_handle = std::thread::spawn(move || {
            let res = { replay.read().parse(game_version.to_string().as_str()) };
            let res = res.map(move |report| {
                // // Send the replay builds to the remote server
                // for player in report.player_entities() {
                //     let client = reqwest::blocking::Client::new();
                //     client
                //         .post("http://192.168.1.215:5150/api/ship_builds")
                //         .json(&build_tracker::BuildTrackerPayload::build_from(
                //             player,
                //             player.player().unwrap().realm().to_owned(),
                //             report.version(),
                //             &metadata_provider,
                //         ))
                //         .send()
                //         .expect("failed to POST build data");
                // }
                {
                    let mut replay_guard = replay.write();
                    replay_guard.battle_report = Some(report);
                    replay_guard.assign_divs();
                }
                BackgroundTaskCompletion::ReplayLoaded { replay }
            });

            let _ = tx.send(res);
        });

        Some(BackgroundTask {
            receiver: rx,
            kind: BackgroundTaskKind::LoadingReplay,
        })
    }
}

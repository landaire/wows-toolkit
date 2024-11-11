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
    build_tracker,
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
    pub fn parse_live_replay(&self) -> Option<BackgroundTask> {
        let replays_dir = &self.replays_dir;
        let meta = replays_dir.join("tempArenaInfo.json");
        let replay = replays_dir.join("temp.wowsreplay");

        let meta_data = std::fs::read(meta);
        let replay_data = std::fs::read(replay);

        if meta_data.is_err() || replay_data.is_err() {
            return None;
        }

        let replay_file: ReplayFile = ReplayFile::from_decrypted_parts(meta_data.unwrap(), replay_data.unwrap()).unwrap();
        let game_metadata = self.game_metadata.clone()?;
        let replay = Replay::new(replay_file, game_metadata);

        self.load_replay(Arc::new(RwLock::new(replay)))
    }

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
                replay.write().battle_report = Some(report);
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

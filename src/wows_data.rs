use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
};

use egui::mutex::RwLock;

use wows_replays::{game_params::Species, ReplayFile};
use wowsunpack::{idx::FileNode, pkg::PkgFileLoader};

use crate::{
    game_params::GameMetadataProvider,
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

        let _join_handle = std::thread::spawn(move || {
            let res = { replay.read().parse(game_version.to_string().as_str()) };
            let res = res.map(move |report| {
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

use std::{
    collections::HashMap,
    fs::{read_dir, File},
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, TryRecvError},
        Arc,
    },
};

use egui::mutex::RwLock;
use gettext::Catalog;
use language_tags::LanguageTag;
use wows_replays::{game_params::Species, ReplayFile};
use wowsunpack::{
    idx::{self, FileNode},
    pkg::PkgFileLoader,
};

use crate::{app::WorldOfWarshipsData, error::ToolkitError, game_params::GameMetadataProvider, replay_parser::Replay};

pub struct ShipIcon {
    pub path: String,
    pub data: Vec<u8>,
}

pub struct BackgroundTask {
    pub receiver: mpsc::Receiver<Result<BackgroundTaskCompletion, ToolkitError>>,
    pub kind: BackgroundTaskKind,
}

#[derive(Clone, Copy)]
pub enum BackgroundTaskKind {
    LoadingData,
    LoadingReplay,
}

impl BackgroundTask {
    pub fn build_description(&self, ui: &mut egui::Ui) -> Option<Result<BackgroundTaskCompletion, ToolkitError>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => {
                match self.kind {
                    BackgroundTaskKind::LoadingData => {
                        ui.spinner();
                        ui.label("Loading game data...");
                    }
                    BackgroundTaskKind::LoadingReplay => {
                        ui.spinner();
                        ui.label("Loading replay...");
                    }
                }
                None
            }
            Err(TryRecvError::Disconnected) => {
                return Some(Err(ToolkitError::BackgroundTaskCompleted));
            }
        }
    }
}

pub enum BackgroundTaskCompletion {
    DataLoaded {
        new_dir: PathBuf,
        wows_data: WorldOfWarshipsData,
        replays: Option<HashMap<PathBuf, Arc<RwLock<Replay>>>>,
    },
    ReplayLoaded {
        replay: Arc<RwLock<Replay>>,
    },
}

fn replay_filepaths(wows_dir: &Path) -> Option<Vec<PathBuf>> {
    let replay_dir = wows_dir.join("replays");
    let mut files = Vec::new();

    if replay_dir.exists() {
        for file in std::fs::read_dir(&replay_dir).expect("failed to read replay dir").flatten() {
            if !file.file_type().expect("failed to get file type").is_file() {
                continue;
            }

            let file_path = file.path();

            if let Some("wowsreplay") = file_path.extension().map(|s| s.to_str().expect("failed to convert extension to str")) {
                if file.file_name() != "temp.wowsreplay" {
                    files.push(file_path);
                }
            }
        }
    }
    if !files.is_empty() {
        files.sort_by_key(|a| a.metadata().unwrap().created().unwrap());
        files.reverse();

        Some(files)
    } else {
        None
    }
}

fn load_ship_icons(file_tree: FileNode, pkg_loader: &PkgFileLoader) -> HashMap<Species, ShipIcon> {
    // Try loading ship icons
    let species = [
        Species::AirCarrier,
        Species::Battleship,
        Species::Cruiser,
        Species::Destroyer,
        Species::Submarine,
        Species::Auxiliary,
    ];

    let icons: HashMap<Species, ShipIcon> = HashMap::from_iter(species.iter().map(|species| {
        let path = format!("gui/fla/minimap/ship_icons/minimap_{}.svg", <&'static str>::from(species).to_ascii_lowercase());
        let icon_node = file_tree.find(&path).expect("failed to find file");

        let mut icon_data = Vec::with_capacity(icon_node.file_info().unwrap().unpacked_size as usize);
        icon_node.read_file(pkg_loader, &mut icon_data).expect("failed to read ship icon");

        (species.clone(), ShipIcon { path, data: icon_data })
    }));

    icons
}

pub fn load_wows_files(wows_directory: PathBuf, locale: &str) -> Result<BackgroundTaskCompletion, crate::error::ToolkitError> {
    let mut idx_files = Vec::new();
    let bin_dir = wows_directory.join("bin");
    if !wows_directory.exists() || !bin_dir.exists() {
        return Err(crate::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()));
    }

    let mut highest_number = None;
    for file in read_dir(wows_directory.join("bin"))? {
        if file.is_err() {
            continue;
        }

        let file = file.unwrap();
        if let Ok(ty) = file.file_type() {
            if ty.is_file() {
                continue;
            }

            if let Some(build_num) = file.file_name().to_str().and_then(|name| name.parse::<usize>().ok()) {
                if highest_number.is_none() || highest_number.map(|number| number < build_num).unwrap_or(false) {
                    highest_number = Some(build_num)
                }
            }
        }
    }

    if highest_number.is_none() {
        return Err(crate::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()));
    }

    let number = highest_number.unwrap();
    for file in read_dir(wows_directory.join("bin").join(format!("{}", number)).join("idx"))? {
        let file = file.unwrap();
        if file.file_type().unwrap().is_file() {
            let file_data = std::fs::read(file.path()).unwrap();
            let mut file = Cursor::new(file_data.as_slice());
            idx_files.push(idx::parse(&mut file).unwrap());
        }
    }

    let pkgs_path = wows_directory.join("res_packages");
    if !pkgs_path.exists() {
        return Err(crate::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()));
    }

    let pkg_loader = Arc::new(PkgFileLoader::new(pkgs_path));

    let file_tree = idx::build_file_tree(idx_files.as_slice());
    let files = file_tree.paths();

    let language_tag: LanguageTag = locale.parse().unwrap();
    let attempted_dirs = [locale, language_tag.primary_language(), "en"];
    let mut found_catalog = None;
    for dir in attempted_dirs {
        let localization_path = wows_directory.join(format!("bin/{}/res/texts/{}/LC_MESSAGES/global.mo", number, dir));
        if !localization_path.exists() {
            continue;
        }
        let global = File::open(localization_path).expect("failed to open localization file");
        let catalog = Catalog::parse(global).expect("could not parse catalog");
        found_catalog = Some(catalog);
        break;
    }

    // Try loading GameParams.data
    let metadata_provider = GameMetadataProvider::from_pkg(&file_tree, &pkg_loader, number).ok().map(|mut metadata_provider| {
        if let Some(catalog) = found_catalog {
            metadata_provider.set_translations(catalog)
        }

        Arc::new(metadata_provider)
    });

    let icons = load_ship_icons(file_tree.clone(), &pkg_loader);

    let data = WorldOfWarshipsData {
        game_metadata: metadata_provider.clone(),
        file_tree: Some(file_tree),
        pkg_loader: Some(pkg_loader),
        filtered_files: Some(files),
        current_replay: Default::default(),
        game_version: Some(number),
        ship_icons: Some(icons),
    };

    let replays = replay_filepaths(&wows_directory).map(|replays| {
        let iter = replays.into_iter().filter_map(|path| {
            // Filter out any replays that don't parse correctly
            let replay_file = ReplayFile::from_file(&path).ok()?;
            let replay = Arc::new(RwLock::new(Replay {
                replay_file,
                resource_loader: metadata_provider.clone().unwrap(),
                battle_report: None,
            }));

            Some((path, replay))
        });

        HashMap::from_iter(iter)
    });

    Ok(BackgroundTaskCompletion::DataLoaded {
        new_dir: wows_directory,
        wows_data: data,
        replays,
    })
}

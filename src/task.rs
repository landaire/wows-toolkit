use std::{
    collections::HashMap,
    fs::{read_dir, File},
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, TryRecvError},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

use egui::mutex::RwLock;
use gettext::Catalog;
use image::EncodableLayout;
use language_tags::LanguageTag;
use octocrab::models::repos::Asset;
use reqwest::Url;
use tokio::runtime::Runtime;
use tracing::{debug, error};
use wows_replays::ReplayFile;
use wowsunpack::{
    data::{
        idx::{self, FileNode},
        pkg::PkgFileLoader,
        Version,
    },
    game_params::{provider::GameMetadataProvider, types::Species},
};
use zip::ZipArchive;

use crate::{
    build_tracker,
    error::ToolkitError,
    game_params::load_game_params,
    replay_parser::Replay,
    wows_data::{ShipIcon, WorldOfWarshipsData},
};

pub struct DownloadProgress {
    downloaded: u64,
    total: u64,
}

pub struct BackgroundTask {
    pub receiver: mpsc::Receiver<Result<BackgroundTaskCompletion, ToolkitError>>,
    pub kind: BackgroundTaskKind,
}

pub enum BackgroundTaskKind {
    LoadingData,
    LoadingReplay,
    Updating {
        rx: mpsc::Receiver<DownloadProgress>,
        last_progress: Option<DownloadProgress>,
    },
}

impl BackgroundTask {
    pub fn build_description(&mut self, ui: &mut egui::Ui) -> Option<Result<BackgroundTaskCompletion, ToolkitError>> {
        match self.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => {
                match &mut self.kind {
                    BackgroundTaskKind::LoadingData => {
                        ui.spinner();
                        ui.label("Loading game data...");
                    }
                    BackgroundTaskKind::LoadingReplay => {
                        ui.spinner();
                        ui.label("Loading replay...");
                    }
                    BackgroundTaskKind::Updating { rx, last_progress } => {
                        match rx.try_recv() {
                            Ok(progress) => {
                                *last_progress = Some(progress);
                            }
                            Err(TryRecvError::Empty) => {}
                            Err(TryRecvError::Disconnected) => {}
                        }

                        if let Some(progress) = last_progress {
                            ui.add(egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32).text("Downloading Update"));
                        }
                    }
                }
                None
            }
            Err(TryRecvError::Disconnected) => Some(Err(ToolkitError::BackgroundTaskCompleted)),
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
    UpdateDownloaded(PathBuf),
}

fn replay_filepaths(replays_dir: &Path) -> Option<Vec<PathBuf>> {
    let mut files = Vec::new();

    if replays_dir.exists() {
        for file in std::fs::read_dir(&replays_dir).expect("failed to read replay dir").flatten() {
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

fn load_ship_icons(file_tree: FileNode, pkg_loader: &PkgFileLoader) -> HashMap<Species, Arc<ShipIcon>> {
    // Try loading ship icons
    let species = [
        Species::AirCarrier,
        Species::Battleship,
        Species::Cruiser,
        Species::Destroyer,
        Species::Submarine,
        Species::Auxiliary,
    ];

    let icons: HashMap<Species, Arc<ShipIcon>> = HashMap::from_iter(species.iter().map(|species| {
        let path = format!("gui/fla/minimap/ship_icons/minimap_{}.svg", <&'static str>::from(species).to_ascii_lowercase());
        let icon_node = file_tree.find(&path).expect("failed to find file");

        let mut icon_data = Vec::with_capacity(icon_node.file_info().unwrap().unpacked_size as usize);
        icon_node.read_file(pkg_loader, &mut icon_data).expect("failed to read ship icon");

        (species.clone(), Arc::new(ShipIcon { path, data: icon_data }))
    }));

    icons
}

fn current_build_from_preferences(path: &Path) -> Option<String> {
    let data = std::fs::read_to_string(path).ok()?;
    let start_of_node = data.find("<last_server_version>")?;
    let end_of_node = data[start_of_node..].find("</last_server_version>")?;
    let version_str = &data[start_of_node + "<last_server_version>".len()..(start_of_node + end_of_node)].trim();

    Some(version_str.to_string())
}

pub fn load_wows_files(wows_directory: PathBuf, locale: &str) -> Result<BackgroundTaskCompletion, crate::error::ToolkitError> {
    let mut idx_files = Vec::new();
    let bin_dir = wows_directory.join("bin");
    if !wows_directory.exists() || !bin_dir.exists() {
        return Err(crate::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()));
    }

    let mut latest_build = None;
    let mut replays_dir = wows_directory.join("replays");

    // Check to see if we can get a build from the preferences file
    let prefs_file = wows_directory.join("preferences.xml");
    if prefs_file.exists() {
        // Try getting the version string from the preferences file
        if let Some(version_str) = current_build_from_preferences(&prefs_file) {
            let parts: Vec<&str> = version_str.split(',').collect();
            if let Some(build_num) = parts.get(3) {
                latest_build = build_num.parse().ok();
            }

            // We want to build the version string without the build component to get the replays dir
            let friendly_build = parts[..=2].join(".");
            let friendly_build_with_extra_component = friendly_build.clone() + ".0";

            for temp_replays_dir in [replays_dir.join(friendly_build), replays_dir.join(friendly_build_with_extra_component)] {
                debug!("Looking for build-specific replays dir at {:?}", temp_replays_dir);
                if temp_replays_dir.exists() {
                    replays_dir = temp_replays_dir;
                    break;
                }
            }
        }
    }

    if latest_build.is_none() {
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
                    if latest_build.is_none() || latest_build.map(|number| number < build_num).unwrap_or(false) {
                        latest_build = Some(build_num)
                    }
                }
            }
        }
    }

    if latest_build.is_none() {
        return Err(crate::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()));
    }

    let number = latest_build.unwrap();
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
    let metadata_provider = load_game_params(&file_tree, &pkg_loader, number).ok().map(|mut metadata_provider| {
        if let Some(catalog) = found_catalog {
            metadata_provider.set_translations(catalog)
        }

        Arc::new(metadata_provider)
    });

    let icons = load_ship_icons(file_tree.clone(), &pkg_loader);

    let data = WorldOfWarshipsData {
        game_metadata: metadata_provider.clone(),
        file_tree: file_tree,
        pkg_loader: pkg_loader,
        filtered_files: files,
        game_version: number,
        ship_icons: icons,
        replays_dir: replays_dir.clone(),
    };

    let replays = replay_filepaths(&replays_dir).map(|replays| {
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

async fn download_update(tx: mpsc::Sender<DownloadProgress>, file: Url) -> Result<PathBuf, ToolkitError> {
    let mut body = reqwest::get(file).await?;

    let total = body.content_length().expect("body has no content-length");
    let mut downloaded = 0;
    let file_path = Path::new("wows_toolkit.tmp.exe");

    // We're going to be blocking here on I/O but it shouldn't matter since this
    // application doesn't really use async
    let mut zip_data = Vec::new();

    while let Some(chunk) = body.chunk().await? {
        downloaded += chunk.len();
        let _ = tx.send(DownloadProgress {
            downloaded: downloaded as u64,
            total,
        });

        zip_data.extend_from_slice(chunk.as_bytes());
    }

    let cursor = Cursor::new(zip_data.as_slice());

    let mut zip = ZipArchive::new(cursor)?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        if file.name().ends_with(".exe") {
            let mut exe_data = Vec::with_capacity(file.size() as usize);
            std::io::copy(&mut file, &mut exe_data)?;
            std::fs::write(file_path, exe_data.as_slice())?;
            break;
        }
    }

    Ok(file_path.to_path_buf())
}

pub fn start_download_update_task(runtime: &Runtime, release: &Asset) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();

    let (progress_tx, progress_rx) = mpsc::channel();
    let url = release.browser_download_url.clone();

    runtime.spawn(async move {
        let result = download_update(progress_tx, url).await.map(|path| BackgroundTaskCompletion::UpdateDownloaded(path));

        tx.send(result);
    });

    BackgroundTask {
        receiver: rx,
        kind: BackgroundTaskKind::Updating {
            rx: progress_rx,
            last_progress: None,
        },
    }
}

pub fn start_background_parsing_thread(rx: mpsc::Receiver<PathBuf>, wows_data: Arc<WorldOfWarshipsData>, should_send_replays: Arc<AtomicBool>) {
    debug!("starting background parsing thread");
    let _join_handle = std::thread::spawn(move || {
        let client = reqwest::blocking::Client::new();
        while let Some(path) = rx.recv().ok() {
            if !should_send_replays.load(Ordering::Relaxed) {
                continue;
            }

            let path = path.as_ref();

            // Files may be getting written to. If we fail to parse the replay,
            // let's try try to parse this at least 3 times.
            for _ in 0..3 {
                match ReplayFile::from_file(path) {
                    Ok(replay_file) => {
                        // We only send back random battles
                        if replay_file.meta.gameType != "RandomBattle" {
                            break;
                        }
                        let (metadata_provider, game_version) = { (wows_data.game_metadata.clone(), wows_data.game_version) };
                        if let Some(metadata_provider) = metadata_provider {
                            let replay = Replay::new(replay_file, Arc::clone(&metadata_provider));
                            match replay.parse(game_version.to_string().as_str()) {
                                Ok(report) => {
                                    // Send the replay builds to the remote server
                                    for player in report.player_entities() {
                                        // TODO: Bulk API
                                        let res = client
                                            .post("https://shipbuilds.com/api/ship_builds")
                                            .json(&build_tracker::BuildTrackerPayload::build_from(
                                                player,
                                                player.player().map(|player| player.realm().to_string()).unwrap(),
                                                report.version(),
                                                &metadata_provider,
                                            ))
                                            .send();
                                        if let Err(e) = res {
                                            error!("error sending request: {:?}", e)
                                        } else {
                                            break;
                                        }
                                    }
                                }
                                Err(e) => error!("error parsing background replay: {:?}", e),
                            }
                        } else {
                            break;
                        }
                    }
                    Err(e) => {
                        error!("error attempting to parse replay in background thread: {:?}", e);
                        thread::sleep(Duration::from_secs(5));
                    }
                }
            }
        }
    });
}

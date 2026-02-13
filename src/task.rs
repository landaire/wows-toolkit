use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::fs::read_dir;
use std::io::Cursor;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::TryRecvError;
use std::sync::mpsc::{self};
use std::thread;
use std::time::Duration;

use gettext::Catalog;
use image::EncodableLayout;
use jiff::Timestamp;
use language_tags::LanguageTag;
use octocrab::models::repos::Asset;
use parking_lot::Mutex;
use parking_lot::RwLock;
use reqwest::Url;
use rootcause::Report;

use rootcause::prelude::ResultExt;
use serde::Deserialize;
use serde::Serialize;
use tokio::runtime::Runtime;
use tracing::debug;
use tracing::error;
use tracing::warn;
use twitch_api::twitch_oauth2::AccessToken;
use twitch_api::twitch_oauth2::UserToken;
use wows_replays::ReplayFile;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::Version;
use wowsunpack::data::idx::FileNode;
use wowsunpack::data::idx::{self};
use wowsunpack::data::pkg::PkgFileLoader;
use wowsunpack::game_data;
use wowsunpack::game_params::types::Species;
use zip::ZipArchive;

use crate::WowsToolkitApp;

use crate::build_tracker;
use crate::error::ToolkitError;
use crate::game_params::load_game_params;
#[cfg(feature = "mod_manager")]
use crate::mod_manager::ModTaskCompletion;
#[cfg(feature = "mod_manager")]
use crate::mod_manager::load_mods_db;
use crate::plaintext_viewer::PlaintextFileViewer;
use crate::replay_export::FlattenedVehicle;
use crate::replay_export::Match;
use crate::twitch::Token;
use crate::twitch::TwitchState;
use crate::twitch::TwitchUpdate;
use crate::twitch::{self};
use crate::ui::player_tracker::PlayerTracker;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::SortOrder;
use crate::update_background_task;
use crate::wows_data::GameAsset;

use crate::wows_data::WorldOfWarshipsData;

pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
}

#[derive(Clone)]
#[allow(dead_code)]
pub enum ToastLevel {
    Success,
    Info,
    Warning,
    Error,
}

#[derive(Clone)]
pub struct ToastMessage {
    pub message: String,
    pub level: ToastLevel,
}

#[allow(dead_code)]
impl ToastMessage {
    pub fn success(message: impl Into<String>) -> Self {
        Self { message: message.into(), level: ToastLevel::Success }
    }

    pub fn info(message: impl Into<String>) -> Self {
        Self { message: message.into(), level: ToastLevel::Info }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self { message: message.into(), level: ToastLevel::Error }
    }
}

pub struct BackgroundTask {
    pub receiver: Option<mpsc::Receiver<Result<BackgroundTaskCompletion, Report>>>,
    pub kind: BackgroundTaskKind,
}

pub enum BackgroundTaskKind {
    LoadingData,
    LoadingReplay,
    // Updates only occur on Windows
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Updating {
        rx: mpsc::Receiver<DownloadProgress>,
        last_progress: Option<DownloadProgress>,
    },
    PopulatePlayerInspectorFromReplays,
    LoadingConstants,
    LoadingPersonalRatingData,
    #[cfg(feature = "mod_manager")]
    ModTask(Box<crate::mod_manager::ModTaskInfo>),
    UpdateTimedMessage(ToastMessage),
    OpenFileViewer(PlaintextFileViewer),
}

#[cfg(feature = "mod_manager")]
impl From<crate::mod_manager::ModTaskInfo> for BackgroundTaskKind {
    fn from(info: crate::mod_manager::ModTaskInfo) -> Self {
        Self::ModTask(Box::new(info))
    }
}

impl BackgroundTask {
    /// TODO: has a bug currently where if multiple tasks are running at the same time, the message looks a bit wonky
    pub fn build_description(&mut self, ui: &mut egui::Ui) -> Option<Result<BackgroundTaskCompletion, Report>> {
        if self.receiver.is_none() {
            return Some(Ok(BackgroundTaskCompletion::NoReceiver));
        }

        match self.receiver.as_ref().unwrap().try_recv() {
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
                            ui.add(
                                egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32)
                                    .text("Downloading Update"),
                            );
                        }
                    }
                    BackgroundTaskKind::PopulatePlayerInspectorFromReplays => {
                        ui.spinner();
                        ui.label("Populating player inspector from historical replays...");
                    }
                    BackgroundTaskKind::LoadingConstants => {
                        ui.spinner();
                        ui.label("Loading data constants...");
                    }
                    #[cfg(feature = "mod_manager")]
                    BackgroundTaskKind::ModTask(mod_task) => match mod_task.as_mut() {
                        crate::mod_manager::ModTaskInfo::LoadingModDatabase => {
                            ui.spinner();
                            ui.label("Loading mod database...");
                        }
                        crate::mod_manager::ModTaskInfo::DownloadingMod { mod_info, rx, last_progress } => {
                            match rx.try_recv() {
                                Ok(progress) => {
                                    *last_progress = Some(progress);
                                }
                                Err(TryRecvError::Empty) => {}
                                Err(TryRecvError::Disconnected) => {}
                            }

                            if let Some(progress) = last_progress {
                                ui.add(
                                    egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32)
                                        .text(format!("Downloading {}", mod_info.meta.name())),
                                );
                            }
                        }
                        crate::mod_manager::ModTaskInfo::InstallingMod { mod_info, rx, last_progress } => {
                            match rx.try_recv() {
                                Ok(progress) => {
                                    *last_progress = Some(progress);
                                }
                                Err(TryRecvError::Empty) => {}
                                Err(TryRecvError::Disconnected) => {}
                            }

                            if let Some(progress) = last_progress {
                                ui.add(
                                    egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32)
                                        .text(format!("Installing {}", mod_info.meta.name())),
                                );
                            }
                        }
                        crate::mod_manager::ModTaskInfo::UninstallingMod { mod_info, rx, last_progress } => {
                            match rx.try_recv() {
                                Ok(progress) => *last_progress = Some(progress),
                                Err(TryRecvError::Empty) => {}
                                Err(TryRecvError::Disconnected) => {}
                            }

                            if let Some(progress) = last_progress {
                                ui.add(
                                    egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32)
                                        .text(format!("Uninstalling {}", mod_info.meta.name())),
                                );
                            }
                        }
                    },
                    BackgroundTaskKind::LoadingPersonalRatingData
                    | BackgroundTaskKind::UpdateTimedMessage(_)
                    | BackgroundTaskKind::OpenFileViewer(_) => {
                        // do nothing
                    }
                }
                None
            }
            Err(TryRecvError::Disconnected) => Some(Err(ToolkitError::BackgroundTaskCompleted.into())),
        }
    }
}

pub enum BackgroundTaskCompletion {
    DataLoaded {
        new_dir: PathBuf,
        wows_data: Box<WorldOfWarshipsData>,
        replays: Option<HashMap<PathBuf, Arc<RwLock<Replay>>>>,
        available_builds: Vec<u32>,
    },
    ReplayLoaded {
        replay: Arc<RwLock<Replay>>,
        /// If true, don't update the current replay in the UI (used for batch session stats loading)
        skip_ui_update: bool,
        /// If true, add this replay to session stats
        track_session_stats: bool,
    },
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    UpdateDownloaded(PathBuf),
    PopulatePlayerInspectorFromReplays,
    ConstantsLoaded(serde_json::Value),
    PersonalRatingDataLoaded(crate::personal_rating::ExpectedValuesData),
    #[cfg(feature = "mod_manager")]
    ModManager(Box<crate::mod_manager::ModTaskCompletion>),
    NoReceiver,
}

#[cfg(feature = "mod_manager")]
impl From<ModTaskCompletion> for BackgroundTaskCompletion {
    fn from(completion: ModTaskCompletion) -> Self {
        Self::ModManager(Box::new(completion))
    }
}

impl std::fmt::Debug for BackgroundTaskCompletion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataLoaded { new_dir, wows_data: _, replays: _, available_builds } => f
                .debug_struct("DataLoaded")
                .field("new_dir", new_dir)
                .field("wows_data", &"<...>")
                .field("replays", &"<...>")
                .field("available_builds", available_builds)
                .finish(),
            Self::ReplayLoaded { replay: _, skip_ui_update, track_session_stats } => f
                .debug_struct("ReplayLoaded")
                .field("replay", &"<...>")
                .field("skip_ui_update", skip_ui_update)
                .field("track_session_stats", track_session_stats)
                .finish(),
            Self::UpdateDownloaded(arg0) => f.debug_tuple("UpdateDownloaded").field(arg0).finish(),
            Self::PopulatePlayerInspectorFromReplays => f.write_str("PopulatePlayerInspectorFromReplays"),
            Self::ConstantsLoaded(_) => f.write_str("ConstantsLoaded(_)"),
            Self::PersonalRatingDataLoaded(_) => f.write_str("PersonalRatingDataLoaded(_)"),
            #[cfg(feature = "mod_manager")]
            Self::ModManager(mod_manager_completion) => {
                f.write_fmt(format_args!("ModManager({:?})", mod_manager_completion))
            }
            Self::NoReceiver => f.debug_struct("NoReceiver").finish(),
        }
    }
}

fn replay_filepaths(replays_dir: &Path) -> Option<Vec<PathBuf>> {
    let mut files = Vec::new();

    if replays_dir.exists() {
        for file in std::fs::read_dir(replays_dir).expect("failed to read replay dir").flatten() {
            if !file.file_type().expect("failed to get file type").is_file() {
                continue;
            }

            let file_path = file.path();

            if let Some("wowsreplay") =
                file_path.extension().map(|s| s.to_str().expect("failed to convert extension to str"))
                && file.file_name() != "temp.wowsreplay"
            {
                files.push(file_path);
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

fn load_ribbon_icons(
    file_tree: &FileNode,
    pkg_loader: &PkgFileLoader,
    dir_path: &str,
) -> HashMap<String, Arc<GameAsset>> {
    let mut icons = HashMap::new();

    for (path, _) in file_tree.paths() {
        let path_str = path.to_string_lossy().replace('\\', "/");
        if !path_str.starts_with(dir_path) {
            continue;
        }

        // Extract the filename without extension as the key
        let Some(file_name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };

        let Ok(icon_node) = file_tree.find(&*path_str) else {
            continue;
        };

        let Some(file_info) = icon_node.file_info() else {
            continue;
        };

        let mut icon_data = Vec::with_capacity(file_info.unpacked_size as usize);
        if icon_node.read_file(pkg_loader, &mut icon_data).is_err() {
            continue;
        }

        icons.insert(file_name.to_string(), Arc::new(GameAsset { path: path_str.to_string(), data: icon_data }));
    }

    icons
}

fn load_ship_icons(file_tree: FileNode, pkg_loader: &PkgFileLoader) -> HashMap<Species, Arc<GameAsset>> {
    // Try loading ship icons
    let species = [
        Species::AirCarrier,
        Species::Battleship,
        Species::Cruiser,
        Species::Destroyer,
        Species::Submarine,
        Species::Auxiliary,
    ];

    let icons: HashMap<Species, Arc<GameAsset>> = HashMap::from_iter(species.iter().map(|species| {
        let path = wowsunpack::game_params::translations::ship_class_icon_path(species);

        let icon_node =
            file_tree.find(&path).unwrap_or_else(|_| panic!("failed to find file {}", <&'static str>::from(species)));

        let mut icon_data = Vec::with_capacity(icon_node.file_info().unwrap().unpacked_size as usize);
        icon_node.read_file(pkg_loader, &mut icon_data).expect("failed to read ship icon");

        (species.clone(), Arc::new(GameAsset { path, data: icon_data }))
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

/// Load game resources for a specific build number. This can be called for any build
/// that has a directory in `bin/`. Used both at startup (for the latest build) and
/// lazily when a replay from a different version is loaded.
pub fn load_wows_data_for_build(
    wows_directory: &Path,
    build: u32,
    locale: &str,
    fallback_constants: &serde_json::Value,
) -> Result<WorldOfWarshipsData, Report> {
    let game_patch = build as usize;
    let build_dir = wows_directory.join("bin").join(format!("{build}"));

    debug!("Loading game data for build {}", build);

    let mut idx_files = Vec::new();
    for file in read_dir(build_dir.join("idx")).context("failed to read idx directory")? {
        let file = file.unwrap();
        if file.file_type().unwrap().is_file() {
            let file_data = std::fs::read(file.path()).unwrap();
            let mut file = Cursor::new(file_data.as_slice());
            idx_files.push(idx::parse(&mut file).unwrap());
        }
    }

    let pkgs_path = wows_directory.join("res_packages");
    if !pkgs_path.exists() {
        return Err(crate::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()).into());
    }

    let pkg_loader = Arc::new(PkgFileLoader::new(pkgs_path));
    let file_tree = idx::build_file_tree(idx_files.as_slice());
    let files = file_tree.paths();

    let language_tag: LanguageTag = locale.parse().unwrap();
    let attempted_dirs = [locale, language_tag.primary_language(), "en"];
    let mut found_catalog = None;
    for dir in attempted_dirs {
        let localization_path = wows_directory.join(format!("bin/{build}/res/texts/{dir}/LC_MESSAGES/global.mo"));
        if !localization_path.exists() {
            continue;
        }
        let global = File::open(localization_path).expect("failed to open localization file");
        let catalog = Catalog::parse(global).expect("could not parse catalog");
        found_catalog = Some(catalog);
        break;
    }

    debug!("Loading GameParams for build {}", build);
    let metadata_provider = load_game_params(&file_tree, &pkg_loader, game_patch).ok().map(|mut metadata_provider| {
        if let Some(catalog) = found_catalog {
            metadata_provider.set_translations(catalog)
        }
        Arc::new(metadata_provider)
    });

    debug!("Loading icons for build {}", build);
    let icons = load_ship_icons(file_tree.clone(), &pkg_loader);
    let ribbon_icons =
        load_ribbon_icons(&file_tree, &pkg_loader, wowsunpack::game_params::translations::RIBBON_ICONS_DIR);
    let subribbon_icons =
        load_ribbon_icons(&file_tree, &pkg_loader, wowsunpack::game_params::translations::RIBBON_SUBICONS_DIR);
    let game_constants = Arc::new(GameConstants::from_pkg(&file_tree, &pkg_loader));

    // Load version-matched constants: try fetching from GitHub with walk-down fallback
    let (replay_constants, replay_constants_exact_match) = match fetch_versioned_constants_with_fallback(build) {
        Some((data, exact)) => (data, exact),
        None => (fallback_constants.clone(), false),
    };

    // Try to determine full version from preferences or leave as None for non-latest builds
    let full_version = None; // Will be set by caller for latest build

    Ok(WorldOfWarshipsData {
        game_metadata: metadata_provider,
        file_tree,
        pkg_loader,
        filtered_files: files,
        patch_version: game_patch,
        full_version,
        build_number: build,
        ship_icons: icons,
        ribbon_icons,
        subribbon_icons,
        achievement_icons: HashMap::new(),
        game_constants,
        replay_constants: Arc::new(RwLock::new(replay_constants)),
        replay_constants_exact_match,
        replays_dir: PathBuf::new(), // Set by caller
        build_dir,
    })
}

/// Try to load versioned constants from `constants_{build}.json` on disk.
fn load_versioned_constants_from_disk(build: u32) -> Option<serde_json::Value> {
    let filename = format!("constants_{build}.json");
    let storage_dir = eframe::storage_dir(crate::APP_NAME)?;
    let path = storage_dir.join(filename);
    if path.exists() {
        let data = std::fs::read(&path).ok()?;
        serde_json::from_slice(&data).ok()
    } else {
        None
    }
}

/// Save versioned constants to `constants_{build}.json` on disk.
fn save_versioned_constants(build: u32, data: &serde_json::Value) {
    if let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) {
        let filename = format!("constants_{build}.json");
        let path = storage_dir.join(filename);
        if let Ok(bytes) = serde_json::to_vec(data) {
            let _ = std::fs::write(path, bytes);
        }
    }
}

/// List available versioned constants builds from GitHub (data/versions/ directory).
fn list_available_constants_builds_from_github() -> Option<Vec<u32>> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
    rt.block_on(async {
        let items = octocrab::instance()
            .repos("padtrack", "wows-constants")
            .get_content()
            .path("data/versions")
            .r#ref("main")
            .send()
            .await
            .ok()?;
        let mut builds: Vec<u32> =
            items.items.iter().filter_map(|item| item.name.strip_suffix(".json")?.parse::<u32>().ok()).collect();
        builds.sort();
        Some(builds)
    })
}

/// Fetch a specific build's constants from GitHub and return the parsed JSON.
fn fetch_constants_from_github(build: u32) -> Option<serde_json::Value> {
    use http_body::Body;
    use http_body_util::BodyExt;
    use octocrab::params::repos::Reference;

    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
    rt.block_on(async {
        let path = format!("data/versions/{build}.json");
        let response = octocrab::instance()
            .repos("padtrack", "wows-constants")
            .raw_file(Reference::Branch("main".to_string()), &path)
            .await
            .ok()?;

        let mut body = response.into_body();
        let mut result = Vec::with_capacity(body.size_hint().exact().unwrap_or_default() as usize);

        while let Some(frame) = body.frame().await {
            match frame {
                Ok(frame) => {
                    if let Some(data) = frame.data_ref() {
                        result.extend_from_slice(data);
                    }
                }
                Err(_) => return None,
            }
        }

        serde_json::from_slice(&result).ok()
    })
}

/// Fetch versioned constants for a target build with walk-down fallback.
///
/// Strategy:
/// 1. Check local disk for exact build — return immediately if found (exact match)
/// 2. List available builds from GitHub (data/versions/)
/// 3. If exact build exists on server, fetch and cache it (exact match)
/// 4. Otherwise walk down to the nearest previous build, fetch and cache it (inexact)
/// 5. Return None if nothing works (caller uses latest as fallback)
///
/// Returns `(constants_data, is_exact_match)`.
pub fn fetch_versioned_constants_with_fallback(target_build: u32) -> Option<(serde_json::Value, bool)> {
    // 1. Check local disk for exact build
    if let Some(data) = load_versioned_constants_from_disk(target_build) {
        debug!("Loaded versioned constants for build {} from disk", target_build);
        return Some((data, true));
    }

    // 2. List available builds from GitHub
    let available_builds = match list_available_constants_builds_from_github() {
        Some(builds) => builds,
        None => {
            debug!("Failed to list available constants builds from GitHub");
            return None;
        }
    };

    // 3. Check if exact build exists on server
    if available_builds.contains(&target_build) {
        if let Some(data) = fetch_constants_from_github(target_build) {
            debug!("Fetched exact versioned constants for build {} from GitHub", target_build);
            save_versioned_constants(target_build, &data);
            return Some((data, true));
        }
    }

    // 4. Walk down to the nearest previous (lower) build — never use a higher build
    for &available_build in available_builds.iter().rev() {
        if available_build >= target_build {
            continue;
        }
        // Check disk first for this fallback build
        if let Some(data) = load_versioned_constants_from_disk(available_build) {
            debug!(
                "Using locally cached constants from build {} as fallback for build {}",
                available_build, target_build
            );
            // Cache under the target build name so we don't re-fetch next time
            save_versioned_constants(target_build, &data);
            return Some((data, false));
        }
        // Fetch from GitHub
        if let Some(data) = fetch_constants_from_github(available_build) {
            debug!("Fetched constants from build {} as fallback for build {}", available_build, target_build);
            // Cache both the source build and the target build
            save_versioned_constants(available_build, &data);
            save_versioned_constants(target_build, &data);
            return Some((data, false));
        }
    }

    // 5. Nothing worked
    debug!("No versioned constants available for build {} or any previous build", target_build);
    None
}

pub fn load_wows_files(
    wows_directory: PathBuf,
    locale: &str,
    fallback_constants: &serde_json::Value,
) -> Result<BackgroundTaskCompletion, Report> {
    let bin_dir = wows_directory.join("bin");
    if !wows_directory.exists() || !bin_dir.exists() {
        debug!("WoWs or WoWs bin directory does not exist");
        return Err(crate::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()).into());
    }

    // Discover all available builds
    let available_builds = game_data::list_available_builds(&wows_directory).map_err(|e| Report::new(e))?;

    if available_builds.is_empty() {
        return Err(crate::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()).into());
    }

    // Determine the latest build (from preferences or highest build number)
    let mut full_version = None;
    let mut latest_build = *available_builds.last().unwrap();
    let mut replays_dir = wows_directory.join("replays");

    let prefs_file = wows_directory.join("preferences.xml");
    if prefs_file.exists() {
        if let Some(version_str) = current_build_from_preferences(&prefs_file)
            && version_str.contains(',')
        {
            let full_build_info = Version::from_client_exe(&version_str);
            if available_builds.contains(&full_build_info.build) {
                latest_build = full_build_info.build;
            }

            let friendly_build =
                format!("{}.{}.{}.0", full_build_info.major, full_build_info.minor, full_build_info.patch);
            full_version = Some(full_build_info);

            for temp_replays_dir in [replays_dir.join(&friendly_build), replays_dir.join(friendly_build)] {
                debug!("Looking for build-specific replays dir at {:?}", temp_replays_dir);
                if temp_replays_dir.exists() {
                    replays_dir = temp_replays_dir;
                    break;
                }
            }
        }
    }

    // Load data for the latest build
    let mut data = load_wows_data_for_build(&wows_directory, latest_build, locale, fallback_constants)?;
    data.full_version = full_version;
    data.replays_dir = replays_dir.clone();

    let metadata_provider = data.game_metadata.clone();
    let game_constants = Arc::clone(&data.game_constants);

    debug!("Loading replays");
    let replays = replay_filepaths(&replays_dir).map(|replays| {
        let iter = replays.into_iter().filter_map(|path| {
            let replay_file = ReplayFile::from_file(&path).ok()?;
            let mut replay = Replay::new(replay_file, metadata_provider.clone().unwrap());
            replay.game_constants = Some(Arc::clone(&game_constants));
            let replay = Arc::new(RwLock::new(replay));

            Some((path, replay))
        });

        HashMap::from_iter(iter)
    });

    // Clean up stale caches for builds that no longer exist
    crate::game_params::cleanup_stale_caches(&available_builds);

    debug!("Sending background task completion");

    Ok(BackgroundTaskCompletion::DataLoaded {
        new_dir: wows_directory,
        wows_data: Box::new(data),
        replays,
        available_builds,
    })
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
async fn download_update(tx: mpsc::Sender<DownloadProgress>, file: Url) -> Result<PathBuf, Report> {
    let mut body = reqwest::get(file)
        .await
        .context("failed to get HTTP response for update file")?
        .error_for_status()
        .context("HTTP error status for update file")?;

    let total = body.content_length().expect("body has no content-length");
    let mut downloaded = 0;

    const NEW_FILE_NAME: &str = "wows_toolkit.tmp.exe";
    let new_exe_path = std::env::current_exe()
        .ok()
        .and_then(|p| Some(p.parent()?.join(NEW_FILE_NAME)))
        .unwrap_or_else(|| PathBuf::from(NEW_FILE_NAME));

    // We're going to be blocking here on I/O but it shouldn't matter since this
    // application doesn't really use async
    let mut zip_data = Vec::new();

    while let Some(chunk) = body.chunk().await.context("failed to get update body chunk")? {
        downloaded += chunk.len();
        let _ = tx.send(DownloadProgress { downloaded: downloaded as u64, total });

        zip_data.extend_from_slice(chunk.as_bytes());
    }

    let cursor = Cursor::new(zip_data.as_slice());

    let mut zip = ZipArchive::new(cursor).context("failed to create ZipArchive reader")?;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).context("failed to get zip inner file by index")?;
        if file.name().ends_with(".exe") {
            let mut out_file = std::fs::File::create(&new_exe_path)
                .context("failed to create update tmp file")
                .attach_with(|| format!("{new_exe_path:?}"))?;
            std::io::copy(&mut file, &mut out_file).context("failed to decompress update file to disk")?;
            break;
        }
    }

    Ok(new_exe_path)
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn start_download_update_task(runtime: &Runtime, release: &Asset) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();

    let (progress_tx, progress_rx) = mpsc::channel();
    let url = release.browser_download_url.clone();

    runtime.spawn(async move {
        let result = download_update(progress_tx, url).await.map(BackgroundTaskCompletion::UpdateDownloaded);

        let _ = tx.send(result);
    });

    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::Updating { rx: progress_rx, last_progress: None } }
}

async fn update_twitch_token(twitch_state: &RwLock<TwitchState>, token: &Token) {
    let client = twitch_state.read().client().clone();
    match UserToken::from_token(&client, AccessToken::from(token.oauth_token())).await {
        Ok(token) => {
            let mut state = twitch_state.write();
            state.token = Some(token);
        }
        Err(_e) => {}
    }
}

pub fn start_twitch_task(
    runtime: &Runtime,
    twitch_state: Arc<RwLock<TwitchState>>,
    monitored_channel: String,
    token: Option<Token>,
    mut token_rx: tokio::sync::mpsc::Receiver<TwitchUpdate>,
) {
    runtime.spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60 * 2));

        // Set the initial twitch token
        if let Some(token) = token {
            update_twitch_token(&twitch_state, &token).await;
        }

        let (client, token) = {
            let state = twitch_state.read();
            (state.client().clone(), state.token.clone())
        };
        let mut monitored_user_id = token.as_ref().map(|token| token.user_id.clone());
        if !monitored_channel.is_empty()
            && let Some(token) = token
            && let Ok(Some(user)) = client.get_user_from_login(&monitored_channel, &token).await
        {
            monitored_user_id = Some(user.id)
        }

        loop {
            let token_receive = token_rx.recv();

            tokio::select! {
                // Every 2 minutes we attempt to get the participants list
                _ = interval.tick() => {
                    let (client, token) = { let state = twitch_state.read(); (state.client().clone(), state.token.clone()) };
                    if let Some(token) = token
                        && let Some(monitored_user) = &monitored_user_id
                            && let Ok(chatters) = twitch::fetch_chatters(&client, monitored_user, &token).await {
                                let now = Timestamp::now();
                                let mut state = twitch_state.write();
                                for chatter in chatters {
                                    state.participants.entry(chatter).or_default().insert(now);
                                }
                            }
                }

                update = token_receive => {
                    if let Some(update) = update {
                        match update {
                            TwitchUpdate::Token(token) => {
                                let had_previous_token = { twitch_state.read().token_is_valid() };
                                update_twitch_token(&twitch_state, &token).await;

                                let (client, token) = { let state = twitch_state.read(); (state.client().clone(), state.token.clone()) };
                                if let Some(token) = &token
                                    && let Some(monitored_user) = &monitored_user_id
                                        && let Ok(chatters) = twitch::fetch_chatters(&client, monitored_user, token).await {
                                            let now = Timestamp::now();
                                            let mut state = twitch_state.write();
                                            for chatter in chatters {
                                                state.participants.entry(chatter).or_default().insert(now);
                                            }
                                        }

                                if !had_previous_token {
                                    // If we didn't have a previous token, but we did have a username to watch, update the username
                                    monitored_user_id = token.as_ref().map(|token| token.user_id.clone());
                                    if !monitored_channel.is_empty()
                                        && let Some(token) = token
                                            && let Ok(Some(user)) = client.get_user_from_login(&monitored_channel, &token).await {
                                                monitored_user_id = Some(user.id)
                                            }
                                }
                            },
                            TwitchUpdate::User(user_name) => {
                                let (client, token) = { let state = twitch_state.read(); (state.client().clone(), state.token.clone()) };
                                if let Some(token) = token
                                    && let Ok(Some(user)) = client.get_user_from_login(&user_name, &token).await {
                                        monitored_user_id = Some(user.id);
                                    }
                            },
                        }
                    }
                }
            }

            // Do a period cleanup of old viewers
            let mut state = twitch_state.write();
            let now = Timestamp::now();
            for timestamps in state.participants.values_mut() {
                // Retain only timestamps within the last 30 minutes
                timestamps.retain(|ts| *ts > (now - Duration::from_secs(60 * 30)));
            }
        }
    });
}

fn parse_replay_data_in_background(
    path: &Path,
    client: &reqwest::blocking::Client,
    replay_parsed_before: bool,
    data: &BackgroundParserThread,
) -> Result<(), ()> {
    // The parser lock serves to prevent file access issues when both the main
    // and background thread are attempting to parse some data. This technically
    // makes all parsers synchronous, but shouldn't be a big deal in practice.
    let _parser_lock = data.parser_lock.lock();

    // Files may be getting written to. If we fail to parse the replay,
    // let's try try to parse this at least 3 times.
    debug!("Sending replay data for: {:?}", path);
    'main_loop: for _ in 0..3 {
        match ReplayFile::from_file(path) {
            Ok(replay_file) => {
                debug!("replay parsed successfully");
                // We only send back random battles
                let game_type = replay_file.meta.gameType.clone();

                // Resolve version-matched data for this replay's build
                let replay_version = wowsunpack::data::Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
                let Some(wows_data_for_build) = data.wows_data_map.resolve(&replay_version) else {
                    warn!("Skipping replay {:?}: no data for build {}", path, replay_version.build);
                    return Ok(());
                };

                let (metadata_provider, game_version, gc) = {
                    let wows_data = wows_data_for_build.read();
                    (wows_data.game_metadata.clone(), wows_data.patch_version, wows_data.game_constants.clone())
                };
                if let Some(metadata_provider) = metadata_provider {
                    let mut replay = Replay::new(replay_file, Arc::clone(&metadata_provider));
                    replay.game_constants = Some(gc);
                    let mut build_uploaded_successfully = false;
                    match replay.parse(game_version.to_string().as_str()) {
                        Ok(report) => {
                            debug!("replay parsed successfully");
                            let is_valid_game_type_for_shipbuilds =
                                matches!(game_type.as_str(), "RandomBattle" | "RankedBattle");
                            if !is_valid_game_type_for_shipbuilds {
                                debug!("game type is: {}", &game_type);
                            }
                            if !replay_parsed_before {
                                debug!("we've never seen this replay before");
                                if data.should_send_replays && is_valid_game_type_for_shipbuilds {
                                    // Send the replay builds to the remote server
                                    for player in report.players() {
                                        #[cfg(not(feature = "shipbuilds_debugging"))]
                                        let url = "https://shipbuilds.com/api/ship_builds";
                                        #[cfg(feature = "shipbuilds_debugging")]
                                        let url = "http://192.168.1.215:3000/api/ship_builds";

                                        if let Some(payload) = build_tracker::BuildTrackerPayload::build_from(
                                            player,
                                            player.initial_state().realm().to_string(),
                                            report.version(),
                                            game_type.clone(),
                                            &metadata_provider,
                                        ) {
                                            // TODO: Bulk API
                                            let res = client.post(url).json(&payload).send();
                                            if let Err(e) = res {
                                                error!("error sending request: {:?}", e);
                                                if e.is_connect() {
                                                    break 'main_loop;
                                                }
                                            }
                                        } else {
                                            error!("no vehicle entity for player?");
                                        }
                                    }
                                    debug!("Successfully sent all builds");
                                }

                                data.player_tracker.write().update_from_replay(&replay);
                            }

                            // Update the player tracker
                            replay.battle_report = Some(report);
                            build_uploaded_successfully = true;
                        }
                        Err(e)
                            if e.downcast_current_context::<ToolkitError>()
                                .is_some_and(|e| matches!(e, ToolkitError::ReplayVersionMismatch { .. })) =>
                        {
                            return Ok(()); // We don't want to keep trying to parse this
                        }
                        Err(e) => {
                            error!("error parsing background replay: {:?}", e);
                        }
                    }

                    if let Some(battle_report) = replay.battle_report.as_ref() {
                        // We should only really be exporting data when the server-provided battle results
                        // are available. Otherwise the data isn't very reliable or interesting.
                        if battle_report.battle_results().is_some() {
                            // Create a dummy sender since we don't need to send background tasks from here
                            let (dummy_sender, _) = mpsc::channel();
                            let deps = crate::wows_data::ReplayDependencies {
                                wows_data_map: data.wows_data_map.clone(),
                                twitch_state: Arc::clone(&data.twitch_state),
                                replay_sort: Arc::new(Mutex::new(SortOrder::default())),
                                background_task_sender: dummy_sender,
                                is_debug_mode: data.is_debug,
                            };
                            replay.build_ui_report(&deps);

                            if data.data_export_settings.should_auto_export {
                                let export_path = data
                                    .data_export_settings
                                    .export_path
                                    .join(replay.better_file_name(&metadata_provider));
                                let export_path =
                                    export_path.with_extension(match data.data_export_settings.export_format {
                                        ReplayExportFormat::Json => "json",
                                        ReplayExportFormat::Cbor => "cbor",
                                        ReplayExportFormat::Csv => "csv",
                                    });

                                let transformed_data = Match::new(&replay, data.is_debug);

                                if let Err(e) = File::create(&export_path)
                                    .context("failed to create export file")
                                    .and_then(|file| match data.data_export_settings.export_format {
                                        ReplayExportFormat::Json => serde_json::to_writer(file, &transformed_data)
                                            .context("failed to write export file"),
                                        ReplayExportFormat::Cbor => serde_cbor::to_writer(file, &transformed_data)
                                            .context("failed to write export file"),
                                        ReplayExportFormat::Csv => {
                                            let mut writer =
                                                csv::WriterBuilder::new().has_headers(true).from_writer(file);
                                            let mut result = Ok(());
                                            for vehicle in transformed_data.vehicles {
                                                result = writer.serialize(FlattenedVehicle::from(vehicle));
                                                if result.is_err() {
                                                    break;
                                                }
                                            }

                                            result.context("failed to write export file")
                                        }
                                    })
                                {
                                    // fail gracefully
                                    error!("failed to write data export file: {:?}", e);
                                }
                            }
                        }
                    }

                    if build_uploaded_successfully {
                        return Ok(());
                    }
                } else {
                    return Err(());
                }
            }
            Err(e) => {
                error!("error attempting to parse replay in background thread: {:?}", e);
                thread::sleep(Duration::from_secs(5));
            }
        }
    }

    Err(())
}

#[derive(Copy, Clone, Default, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub enum ReplayExportFormat {
    #[default]
    Json,
    Cbor,
    Csv,
}

impl ReplayExportFormat {
    pub fn as_str(&self) -> &str {
        self.as_ref()
    }

    pub fn extension(&self) -> &str {
        match self {
            ReplayExportFormat::Json => "json",
            ReplayExportFormat::Cbor => "cbor",
            ReplayExportFormat::Csv => "csv",
        }
    }
}

impl AsRef<str> for ReplayExportFormat {
    fn as_ref(&self) -> &str {
        match self {
            ReplayExportFormat::Json => "JSON",
            ReplayExportFormat::Cbor => "CBOR",
            ReplayExportFormat::Csv => "CSV",
        }
    }
}

pub struct DataExportSettings {
    pub should_auto_export: bool,
    pub export_path: PathBuf,
    pub export_format: ReplayExportFormat,
}

pub enum ReplayBackgroundParserThreadMessage {
    /// A new replay has been written
    NewReplay(PathBuf),
    /// A replay has been modified. This probably indicates that the post-battle
    /// results have been written to the file.
    ModifiedReplay(PathBuf),
    ShouldSendReplaysToServer(bool),
    DataAutoExportSettingChange(DataExportSettings),
    DebugStateChange(bool),
}

pub struct BackgroundParserThread {
    pub rx: mpsc::Receiver<ReplayBackgroundParserThreadMessage>,
    pub sent_replays: Arc<RwLock<HashSet<String>>>,
    pub wows_data_map: crate::wows_data::WoWsDataMap,
    pub twitch_state: Arc<RwLock<TwitchState>>,
    pub should_send_replays: bool,
    pub data_export_settings: DataExportSettings,
    pub player_tracker: Arc<RwLock<PlayerTracker>>,
    pub is_debug: bool,
    pub parser_lock: Arc<Mutex<()>>,
}
pub fn start_background_parsing_thread(mut data: BackgroundParserThread) {
    debug!("starting background parsing thread");
    let _join_handle = std::thread::spawn(move || {
        let client = reqwest::blocking::Client::new();

        #[cfg(not(feature = "shipbuilds_debugging"))]
        {
            debug!("Attempting to prune old replay paths from settings");

            // Prune files that no longer exist to prevent the settings from growing too large
            let mut sent_replays = data.sent_replays.write();
            let mut to_remove = Vec::new();
            for file_path in &*sent_replays {
                if !Path::new(file_path).exists() {
                    to_remove.push(file_path.clone());
                    // do nothing
                }
            }

            for file_path in to_remove {
                sent_replays.remove(&file_path);
            }
        }

        {
            debug!("Attempting to enumerate replays directory to see if there are any new ones to send");
            let Some(replays_dir) =
                data.wows_data_map.with_builds(|builds| builds.values().next().map(|d| d.read().replays_dir.clone()))
            else {
                error!("No game data loaded, cannot enumerate replays directory");
                return;
            };

            // Try to see if we have any historical replays we can send
            match std::fs::read_dir(&replays_dir) {
                Ok(read_dir) => {
                    for file in read_dir.flatten() {
                        let path = file.path();
                        if path.extension().map(|ext| ext != "wowsreplay").unwrap_or(false)
                            || path.file_name().map(|name| name == "temp.wowsreplay").unwrap_or(false)
                        {
                            continue;
                        }

                        let path_str = path.to_string_lossy();
                        let already_recorded_replay = { data.sent_replays.read().contains(path_str.as_ref()) }
                            || cfg!(feature = "shipbuilds_debugging");

                        if !already_recorded_replay
                            && parse_replay_data_in_background(&path, &client, already_recorded_replay, &data).is_ok()
                        {
                            data.sent_replays.write().insert(path_str.into_owned());
                        }
                    }
                }
                Err(e) => {
                    error!("Error reading replays dir from background parsing thread: {:?}", e)
                }
            }
        }

        debug!("Beginning backgorund replay receive loop");
        while let Ok(message) = data.rx.recv() {
            match message {
                ReplayBackgroundParserThreadMessage::NewReplay(path) => {
                    let path_str = path.to_string_lossy();
                    let already_parsed_replay = { data.sent_replays.read().contains(path_str.as_ref()) };

                    debug!("Attempting to parse replay at {}", path_str);
                    if parse_replay_data_in_background(&path, &client, already_parsed_replay, &data).is_ok() {
                        data.sent_replays.write().insert(path_str.into_owned());
                    }
                }
                ReplayBackgroundParserThreadMessage::ModifiedReplay(path) => {
                    let path_str = path.to_string_lossy();
                    let already_parsed_replay = { data.sent_replays.read().contains(path_str.as_ref()) };

                    // For a modified replay, we will always re-parse it but never send it.
                    // TODO: this might export data multiple times?
                    let _ = parse_replay_data_in_background(&path, &client, already_parsed_replay, &data);
                }
                ReplayBackgroundParserThreadMessage::ShouldSendReplaysToServer(should_send) => {
                    data.should_send_replays = should_send;
                }
                ReplayBackgroundParserThreadMessage::DataAutoExportSettingChange(new_data_export_settings) => {
                    data.data_export_settings = new_data_export_settings;
                }
                ReplayBackgroundParserThreadMessage::DebugStateChange(new_debug_state) => {
                    data.is_debug = new_debug_state;
                }
            }
        }
    });
}

pub fn start_populating_player_inspector(
    replays: Vec<PathBuf>,
    wows_data_map: crate::wows_data::WoWsDataMap,
    player_tracker: Arc<RwLock<PlayerTracker>>,
) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for path in replays {
            match ReplayFile::from_file(&path) {
                Ok(replay_file) => {
                    let replay_version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
                    let Some(wows_data_for_build) = wows_data_map.resolve(&replay_version) else {
                        warn!("Skipping replay {:?}: no data for build {}", path, replay_version.build);
                        continue;
                    };

                    let (metadata_provider, game_version, gc) = {
                        let data = wows_data_for_build.read();
                        (data.game_metadata.clone(), data.patch_version, data.game_constants.clone())
                    };
                    if let Some(metadata_provider) = metadata_provider {
                        let mut replay = Replay::new(replay_file, Arc::clone(&metadata_provider));
                        replay.game_constants = Some(gc);
                        match replay.parse(game_version.to_string().as_str()) {
                            Ok(report) => {
                                replay.battle_report = Some(report);
                                player_tracker.write().update_from_replay(&replay);
                            }
                            Err(e) => {
                                warn!("error attempting to parse replay for replay inspector: {e:?}");
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("error attempting to open replay for replay inspector: {e:?}");
                }
            }
        }

        let _ = tx.send(Ok(BackgroundTaskCompletion::PopulatePlayerInspectorFromReplays));
    });

    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::PopulatePlayerInspectorFromReplays }
}

pub fn begin_startup_tasks(toolkit: &mut WowsToolkitApp, token_rx: tokio::sync::mpsc::Receiver<TwitchUpdate>) {
    start_twitch_task(
        &toolkit.runtime,
        Arc::clone(&toolkit.tab_state.twitch_state),
        toolkit.tab_state.settings.twitch_monitored_channel.clone(),
        toolkit.tab_state.settings.twitch_token.clone(),
        token_rx,
    );

    #[cfg(feature = "mod_manager")]
    update_background_task!(toolkit.tab_state.background_tasks, Some(load_mods_db()));

    let mut constants_path = PathBuf::from("constants.json");
    if let Some(storage_dir) = eframe::storage_dir(crate::APP_NAME) {
        constants_path = storage_dir.join(constants_path)
    }

    if constants_path.exists() {
        if let Ok(constants_data) = std::fs::read(&constants_path) {
            update_background_task!(toolkit.tab_state.background_tasks, Some(load_constants(constants_data)));
        } else {
            error!("failed to read constants file");
        }
    }

    // Load PR expected values from disk if available
    let pr_path = crate::personal_rating::get_expected_values_path();
    if pr_path.exists() {
        if let Ok(pr_data) = std::fs::read(&pr_path) {
            update_background_task!(toolkit.tab_state.background_tasks, Some(load_personal_rating_data(pr_data)));
        } else {
            error!("failed to read PR expected values file");
        }
    }
}

pub fn load_constants(constants: Vec<u8>) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result: Result<BackgroundTaskCompletion, Report> = serde_json::from_slice(&constants)
            .map(BackgroundTaskCompletion::ConstantsLoaded)
            .map_err(|err| Report::from(ToolkitError::from(err)));

        tx.send(result).expect("tx closed");
    });
    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingConstants }
}

pub fn load_personal_rating_data(data: Vec<u8>) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result: Result<BackgroundTaskCompletion, Report> = serde_json::from_slice(&data)
            .map(BackgroundTaskCompletion::PersonalRatingDataLoaded)
            .map_err(|err| Report::from(ToolkitError::from(err)));

        tx.send(result).expect("tx closed");
    });
    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingPersonalRatingData }
}

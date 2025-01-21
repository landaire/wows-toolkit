use std::{
    collections::{HashMap, HashSet},
    fs::{read_dir, File},
    io::Cursor,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc,
    },
    thread,
    time::Duration,
};

use anyhow::{anyhow, Context};
use flate2::read::GzDecoder;
use gettext::Catalog;
use glob::glob;
use http_body_util::BodyExt;
use image::EncodableLayout;
use language_tags::LanguageTag;
use octocrab::models::repos::Asset;
use parking_lot::{Mutex, RwLock};
use reqwest::Url;
use scopeguard::defer;
use tar::Archive;
use tokio::runtime::Runtime;
use tracing::{debug, error};
use twitch_api::twitch_oauth2::{AccessToken, UserToken};
use wows_replays::ReplayFile;
use wowsunpack::{
    data::{
        idx::{self, FileNode},
        pkg::PkgFileLoader,
    },
    game_params::types::Species,
};
use zip::ZipArchive;

use crate::{
    app::TimedMessage,
    build_tracker,
    error::ToolkitError,
    game_params::load_game_params,
    plaintext_viewer::PlaintextFileViewer,
    twitch::{self, Token, TwitchState, TwitchUpdate},
    ui::{
        mod_manager::{ModInfo, ModManagerIndex},
        player_tracker::PlayerTracker,
        replay_parser::Replay,
    },
    update_background_task,
    wows_data::{ShipIcon, WorldOfWarshipsData},
    WowsToolkitApp,
};

pub struct DownloadProgress {
    downloaded: u64,
    total: u64,
}

pub struct BackgroundTask {
    pub receiver: Option<mpsc::Receiver<Result<BackgroundTaskCompletion, ToolkitError>>>,
    pub kind: BackgroundTaskKind,
}

pub enum BackgroundTaskKind {
    LoadingData,
    LoadingReplay,
    Updating {
        rx: mpsc::Receiver<DownloadProgress>,
        last_progress: Option<DownloadProgress>,
    },
    PopulatePlayerInspectorFromReplays,
    LoadingConstants,
    LoadingModDatabase,
    DownloadingMod {
        mod_info: ModInfo,
        rx: mpsc::Receiver<DownloadProgress>,
        last_progress: Option<DownloadProgress>,
    },
    InstallingMod {
        mod_info: ModInfo,
        rx: mpsc::Receiver<DownloadProgress>,
        last_progress: Option<DownloadProgress>,
    },
    UninstallingMod {
        mod_info: ModInfo,
        rx: mpsc::Receiver<DownloadProgress>,
        last_progress: Option<DownloadProgress>,
    },
    UpdateTimedMessage(TimedMessage),
    OpenFileViewer(PlaintextFileViewer),
}

impl BackgroundTask {
    /// TODO: has a bug currently where if multiple tasks are running at the same time, the message looks a bit wonky
    pub fn build_description(&mut self, ui: &mut egui::Ui) -> Option<Result<BackgroundTaskCompletion, ToolkitError>> {
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
                            ui.add(egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32).text("Downloading Update"));
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
                    BackgroundTaskKind::LoadingModDatabase => {
                        ui.spinner();
                        ui.label("Loading mod database...");
                    }
                    BackgroundTaskKind::DownloadingMod { mod_info, rx, last_progress } => {
                        match rx.try_recv() {
                            Ok(progress) => {
                                *last_progress = Some(progress);
                            }
                            Err(TryRecvError::Empty) => {}
                            Err(TryRecvError::Disconnected) => {}
                        }

                        if let Some(progress) = last_progress {
                            ui.add(egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32).text(format!("Downloading {}", mod_info.meta.name())));
                        }
                    }
                    BackgroundTaskKind::InstallingMod { mod_info, rx, last_progress } => {
                        match rx.try_recv() {
                            Ok(progress) => {
                                *last_progress = Some(progress);
                            }
                            Err(TryRecvError::Empty) => {}
                            Err(TryRecvError::Disconnected) => {}
                        }

                        if let Some(progress) = last_progress {
                            ui.add(egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32).text(format!("Installing {}", mod_info.meta.name())));
                        }
                    }
                    BackgroundTaskKind::UninstallingMod { mod_info, rx, last_progress } => {
                        match rx.try_recv() {
                            Ok(progress) => *last_progress = Some(progress),
                            Err(TryRecvError::Empty) => {}
                            Err(TryRecvError::Disconnected) => {}
                        }

                        if let Some(progress) = last_progress {
                            ui.add(egui::ProgressBar::new(progress.downloaded as f32 / progress.total as f32).text(format!("Uninstalling {}", mod_info.meta.name())));
                        }
                    }
                    BackgroundTaskKind::UpdateTimedMessage(_) | BackgroundTaskKind::OpenFileViewer(_) => {
                        // do nothing
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
    PopulatePlayerInspectorFromReplays,
    ConstantsLoaded(serde_json::Value),
    ModDatabaseLoaded(ModManagerIndex),
    ModDownloaded(ModInfo),
    ModInstalled(ModInfo),
    ModUninstalled(ModInfo),
    NoReceiver,
}

impl std::fmt::Debug for BackgroundTaskCompletion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DataLoaded { new_dir, wows_data, replays } => f
                .debug_struct("DataLoaded")
                .field("new_dir", new_dir)
                .field("wows_data", &"<...>")
                .field("replays", &"<...>")
                .finish(),
            Self::ReplayLoaded { replay } => f.debug_struct("ReplayLoaded").field("replay", &"<...>").finish(),
            Self::UpdateDownloaded(arg0) => f.debug_tuple("UpdateDownloaded").field(arg0).finish(),
            Self::PopulatePlayerInspectorFromReplays => f.write_str("PopulatePlayerInspectorFromReplays"),
            Self::ConstantsLoaded(_) => f.write_str("ConstantsLoaded(_)"),
            Self::ModDatabaseLoaded(_) => f.write_str("ModDatabaseLoaded(_)"),
            Self::ModDownloaded(modi) => f.debug_struct("ModDownloaded").field("0", modi).finish(),
            Self::ModInstalled(modi) => f.debug_struct("ModInstalled").field("0", modi).finish(),
            Self::ModUninstalled(modi) => f.debug_struct("ModUninstalled").field("0", modi).finish(),
            Self::NoReceiver => f.debug_struct("NoReceiver").finish(),
        }
    }
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
        debug!("WoWs or WoWs bin directory does not exist");
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
    let build_dir = wows_directory.join("bin").join(format!("{}", number));
    for file in read_dir(&build_dir.join("idx"))? {
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

    debug!("Loading GameParams");

    // Try loading GameParams.data
    let metadata_provider = load_game_params(&file_tree, &pkg_loader, number).ok().map(|mut metadata_provider| {
        if let Some(catalog) = found_catalog {
            metadata_provider.set_translations(catalog)
        }

        Arc::new(metadata_provider)
    });

    debug!("Loading icons");
    let icons = load_ship_icons(file_tree.clone(), &pkg_loader);

    let data = WorldOfWarshipsData {
        game_metadata: metadata_provider.clone(),
        file_tree: file_tree,
        pkg_loader: pkg_loader,
        filtered_files: files,
        game_version: number,
        ship_icons: icons,
        replays_dir: replays_dir.clone(),
        build_dir,
    };

    debug!("Loading replays");
    let replays = replay_filepaths(&replays_dir).map(|replays| {
        let iter = replays.into_iter().filter_map(|path| {
            // Filter out any replays that don't parse correctly
            let replay_file = ReplayFile::from_file(&path).ok()?;
            let replay = Arc::new(RwLock::new(Replay::new(replay_file, metadata_provider.clone().unwrap())));

            Some((path, replay))
        });

        HashMap::from_iter(iter)
    });

    debug!("Sending background task completion");

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

        let _ = tx.send(result);
    });

    BackgroundTask {
        receiver: rx.into(),
        kind: BackgroundTaskKind::Updating {
            rx: progress_rx,
            last_progress: None,
        },
    }
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
        if !monitored_channel.is_empty() {
            if let Some(token) = token {
                if let Ok(Some(user)) = client.get_user_from_login(&monitored_channel, &token).await {
                    monitored_user_id = Some(user.id)
                }
            }
        }

        loop {
            let token_receive = token_rx.recv();

            tokio::select! {
                // Every 2 minutes we attempt to get the participants list
                _ = interval.tick() => {
                    let (client, token) = { let state = twitch_state.read(); (state.client().clone(), state.token.clone()) };
                    if let Some(token) = token {
                        if let Some(monitored_user) = &monitored_user_id {
                            if let Ok(chatters) = twitch::fetch_chatters(&client, monitored_user, &token).await {
                                let now = chrono::offset::Local::now();
                                let mut state = twitch_state.write();
                                for chatter in chatters {
                                    state.participants.entry(chatter).or_default().insert(now);
                                }
                            }
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
                                if let Some(token) = &token {
                                    if let Some(monitored_user) = &monitored_user_id {
                                        if let Ok(chatters) = twitch::fetch_chatters(&client, monitored_user, token).await {
                                            let now = chrono::offset::Local::now();
                                            let mut state = twitch_state.write();
                                            for chatter in chatters {
                                                state.participants.entry(chatter).or_default().insert(now);
                                            }
                                        }
                                    }
                                }

                                if !had_previous_token {
                                    // If we didn't have a previous token, but we did have a username to watch, update the username
                                    monitored_user_id = token.as_ref().map(|token| token.user_id.clone());
                                    if !monitored_channel.is_empty() {
                                        if let Some(token) = token {
                                            if let Ok(Some(user)) = client.get_user_from_login(&monitored_channel, &token).await {
                                                monitored_user_id = Some(user.id)
                                            }
                                        }
                                    }
                                }
                            },
                            TwitchUpdate::User(user_name) => {
                                let (client, token) = { let state = twitch_state.read(); (state.client().clone(), state.token.clone()) };
                                if let Some(token) = token {
                                    if let Ok(Some(user)) = client.get_user_from_login(&user_name, &token).await {
                                        monitored_user_id = Some(user.id);
                                    }
                                }
                            },
                        }
                    }
                }
            }

            // Do a period cleanup of old viewers
            let mut state = twitch_state.write();
            let now = chrono::offset::Local::now();
            for (_, timestamps) in &mut state.participants {
                // Retain only timestamps within the last 30 minutes
                timestamps.retain(|ts| *ts > (now - Duration::from_secs(60 * 30)));
            }
        }
    });
}

fn parse_replay_data_in_background(
    path: &Path,
    wows_data: &WorldOfWarshipsData,
    client: &reqwest::blocking::Client,
    should_send_replays: Arc<AtomicBool>,
    player_tracker: Arc<RwLock<PlayerTracker>>,
) -> Result<(), ()> {
    // Files may be getting written to. If we fail to parse the replay,
    // let's try try to parse this at least 3 times.
    debug!("Sending replay data for: {:?}", path);
    'main_loop: for _ in 0..3 {
        match ReplayFile::from_file(path) {
            Ok(replay_file) => {
                // We only send back random battles
                let game_type = replay_file.meta.gameType.clone();
                if !matches!(game_type.as_str(), "RandomBattle" | "RankedBattle") {
                    debug!("game type is: {}, not sending", &game_type);
                    break;
                }

                let (metadata_provider, game_version) = { (wows_data.game_metadata.clone(), wows_data.game_version) };
                if let Some(metadata_provider) = metadata_provider {
                    let mut replay = Replay::new(replay_file, Arc::clone(&metadata_provider));
                    match replay.parse(game_version.to_string().as_str()) {
                        Ok(report) => {
                            if should_send_replays.load(Ordering::Relaxed) {
                                // Send the replay builds to the remote server
                                for vehicle in report.player_entities() {
                                    #[cfg(not(feature = "shipbuilds_debugging"))]
                                    let url = "https://shipbuilds.com/api/ship_builds";
                                    #[cfg(feature = "shipbuilds_debugging")]
                                    let url = "http://192.168.1.215:3000/api/ship_builds";

                                    if let Some(player) = vehicle.player() {
                                        // TODO: Bulk API
                                        let res = client
                                            .post(url)
                                            .json(&build_tracker::BuildTrackerPayload::build_from(
                                                vehicle,
                                                player.realm().to_string(),
                                                report.version(),
                                                game_type.clone(),
                                                &metadata_provider,
                                            ))
                                            .send();
                                        if let Err(e) = res {
                                            error!("error sending request: {:?}", e);
                                            if e.is_connect() {
                                                break 'main_loop;
                                            }
                                        }
                                    } else {
                                        error!("no player for replay?");
                                    }
                                }
                                debug!("Successfully sent all builds");
                            }

                            // Update the player tracker
                            replay.battle_report = Some(report);
                            player_tracker.write().update_from_replay(&replay);

                            return Ok(());
                        }
                        Err(e) => {
                            error!("error parsing background replay: {:?}", e);
                        }
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

pub fn start_background_parsing_thread(
    rx: mpsc::Receiver<PathBuf>,
    sent_replays: Arc<RwLock<HashSet<String>>>,
    wows_data: Arc<RwLock<WorldOfWarshipsData>>,
    should_send_replays: Arc<AtomicBool>,
    player_tracker: Arc<RwLock<PlayerTracker>>,
) {
    debug!("starting background parsing thread");
    let _join_handle = std::thread::spawn(move || {
        let client = reqwest::blocking::Client::new();

        #[cfg(not(feature = "shipbuilds_debugging"))]
        {
            debug!("Attempting to prune old replay paths from settings");

            // Prune files that no longer exist to prevent the settings from growing too large
            let mut sent_replays = sent_replays.write();
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
            let wows_data = wows_data.read();

            // Try to see if we have any historical replays we can send
            match std::fs::read_dir(&wows_data.replays_dir) {
                Ok(read_dir) => {
                    for file in read_dir.flatten() {
                        let path = file.path();
                        if path.extension().map(|ext| ext != "wowsreplay").unwrap_or(false) || path.file_name().map(|name| name == "temp.wowsreplay").unwrap_or(false) {
                            continue;
                        }

                        let path_str = path.to_string_lossy();
                        let sent_replay = { sent_replays.read().contains(path_str.as_ref()) } || cfg!(feature = "shipbuilds_debugging");

                        if !sent_replay
                            && parse_replay_data_in_background(&path, &wows_data, &client, Arc::clone(&should_send_replays), Arc::clone(&player_tracker)).is_ok()
                        {
                            sent_replays.write().insert(path_str.into_owned());
                        }
                    }
                }
                Err(e) => {
                    error!("Error reading replays dir from background parsing thread: {:?}", e)
                }
            }
        }

        debug!("Beginning backgorund replay receive loop");
        while let Ok(path) = rx.recv() {
            let path_str = path.to_string_lossy();
            let sent_replay = { sent_replays.read().contains(path_str.as_ref()) };

            if !sent_replay {
                debug!("Attempting to send replay at {}", path_str);
                let wows_data = wows_data.read();
                if parse_replay_data_in_background(&path, &wows_data, &client, Arc::clone(&should_send_replays), Arc::clone(&player_tracker)).is_ok() {
                    sent_replays.write().insert(path_str.into_owned());
                }
            } else {
                debug!("Not sending replay as it's already been sent");
            }
        }
    });
}

pub fn start_populating_player_inspector(
    replays: Vec<PathBuf>,
    wows_data: Arc<RwLock<WorldOfWarshipsData>>,
    player_tracker: Arc<RwLock<PlayerTracker>>,
) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for path in replays {
            match ReplayFile::from_file(&path) {
                Ok(replay_file) => {
                    let wows_data = wows_data.read();
                    let (metadata_provider, game_version) = { (wows_data.game_metadata.clone(), wows_data.game_version) };
                    if let Some(metadata_provider) = metadata_provider {
                        let mut replay = Replay::new(replay_file, Arc::clone(&metadata_provider));
                        match replay.parse(game_version.to_string().as_str()) {
                            Ok(report) => {
                                replay.battle_report = Some(report);
                                player_tracker.write().update_from_replay(&replay);
                            }
                            Err(e) => {
                                println!("error attempting to parse replay for replay inspector: {:?}", e);
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("error attempting to open replay for replay inspector: {:?}", e);
                }
            }
        }

        let _ = tx.send(Ok(BackgroundTaskCompletion::PopulatePlayerInspectorFromReplays));
    });

    BackgroundTask {
        receiver: rx.into(),
        kind: BackgroundTaskKind::PopulatePlayerInspectorFromReplays,
    }
}

pub fn begin_startup_tasks(toolkit: &mut WowsToolkitApp, token_rx: tokio::sync::mpsc::Receiver<TwitchUpdate>) {
    start_twitch_task(
        &toolkit.runtime,
        Arc::clone(&toolkit.tab_state.twitch_state),
        toolkit.tab_state.settings.twitch_monitored_channel.clone(),
        toolkit.tab_state.settings.twitch_token.clone(),
        token_rx,
    );

    update_background_task!(toolkit.tab_state.background_tasks, Some(load_mods_db()));
}

pub fn load_constants(constants: Vec<u8>) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let result = serde_json::from_slice(&constants)
            .context("failed to deserialize constants file")
            .map(BackgroundTaskCompletion::ConstantsLoaded)
            .map_err(|err| err.into());

        tx.send(result);
    });
    BackgroundTask {
        receiver: rx.into(),
        kind: BackgroundTaskKind::LoadingConstants,
    }
}

pub fn load_mods_db() -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mods_db = std::fs::read_to_string("../mods.toml").unwrap();
        let result = toml::from_str::<ModManagerIndex>(&mods_db)
            .context("failed to deserialize mods db")
            .map(BackgroundTaskCompletion::ModDatabaseLoaded)
            .map_err(|err| err.into());

        tx.send(result);
    });
    BackgroundTask {
        receiver: rx.into(),
        kind: BackgroundTaskKind::LoadingModDatabase,
    }
}

async fn download_mod_tarball(mod_info: &ModInfo, tx: Sender<DownloadProgress>) -> anyhow::Result<Vec<u8>> {
    use http_body::Body;

    let Ok(url_parse) = Url::parse(mod_info.meta.repo_url()) else {
        eprintln!("failed to parse repo URL: {}", mod_info.meta.repo_url());
        return Err(anyhow!("Failed to download mod tarball"));
    };

    let mut repo_parts = url_parse.path().split('/').filter(|s| !s.is_empty());
    let Some(owner) = repo_parts.next() else {
        eprintln!("failed to get owner from repo URL: {}", mod_info.meta.repo_url());
        return Err(anyhow!("Failed to download mod tarball"));
    };
    let Some(repo) = repo_parts.next() else {
        eprintln!("failed to get repo from repo URL: {}", mod_info.meta.repo_url());
        return Err(anyhow!("Failed to download mod tarball"));
    };
    let octocrab = octocrab::instance();
    eprintln!("downloading tarball from {}/{}", owner, repo);
    eprintln!("commit: {}", mod_info.meta.commit());
    let repo = octocrab.repos(owner, repo);
    let mut response = repo.download_tarball(mod_info.meta.commit().to_string()).await?;

    if !response.status().is_success() {
        return Err(anyhow!("Failed to download mod tarball - server error"));
    }

    let body = response.body_mut();
    let total = body.size_hint().exact().unwrap_or_default() as usize;
    let mut result = Vec::with_capacity(total);

    // Iterate through all data chunks in the body
    while let Some(frame) = body.frame().await {
        match frame {
            Ok(frame) => {
                if let Some(data) = frame.data_ref() {
                    result.extend_from_slice(&data);
                    tx.send(DownloadProgress {
                        downloaded: result.len() as u64,
                        total: total as u64,
                    });
                }
            }
            Err(_) => Err(anyhow!("Error while downloading mod tarball"))?,
        }
    }

    Ok(result)
}

fn unpack_mod(tarball: &[u8], wows_data: Arc<RwLock<WorldOfWarshipsData>>, mod_info: &ModInfo, tx: Sender<DownloadProgress>) -> anyhow::Result<()> {
    let tar = GzDecoder::new(tarball);
    let mut archive = Archive::new(tar);

    let wows_dir = { wows_data.read().build_dir.join("res_mods") };

    let mut globs: Vec<_> = mod_info
        .meta
        .paths()
        .iter()
        .filter_map(|pat| match glob::Pattern::new(pat) {
            Ok(pat) => Some(pat),
            Err(e) => {
                eprintln!("failed to parse glob: {}", e);
                None
            }
        })
        .collect();
    if globs.is_empty() {
        eprintln!("using default glob");
        globs.push(glob::Pattern::new("*").unwrap());
    }

    let mut entries_count = 100;
    let mut paths = Vec::new();
    for (processed_files, entry) in archive.entries()?.enumerate() {
        defer! {
            let _ = tx.send(DownloadProgress {
                downloaded: processed_files as u64,
                total:  entries_count as u64,
            });
        };

        let mut entry = entry.context("entry")?;
        let mod_file_path = entry.path().context("path")?;

        // Tarballs are structured as:
        // <REPO_OWNER>-<REPO_NAME>-<REFERENCE>
        // <REPO_OWNER>-<REPO_NAME>-<REFERENCE>/<MOD_DATA>
        // pax_global_header

        // We want to strip out the first directory in the chain
        let mod_file_path = mod_file_path.components().skip(1).collect::<PathBuf>();
        if mod_file_path.components().count() == 0 {
            eprintln!("skipping repo metadata file");
            continue;
        }

        println!("processing {}", mod_file_path.display());

        // Ensure that this is a file we should be extracting
        if !globs.iter().any(|pat| pat.matches_path(&mod_file_path) || mod_file_path.starts_with(pat.as_str())) {
            eprintln!("Path did not match any glob: {}", mod_file_path.display());
            continue;
        }

        let target_path = wows_dir.join(mod_file_path);

        // Ensure that a mod didn't try to write to elsewhere on the filesystem
        if !std::path::absolute(&target_path).context("absolute path")?.starts_with(&wows_dir) {
            return Err(anyhow!("Mod tried to write to an invalid path: {}", target_path.display()));
        }

        println!("unpacking to {}", target_path.display());

        paths.push(target_path.clone());
        if entry.header().entry_type().is_dir() {
            std::fs::create_dir_all(&target_path).context(target_path.display().to_string())?;
            continue;
        }

        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).context(parent.display().to_string())?;
        }

        println!("writing file {}", target_path.display());

        entry.unpack(target_path)?;
    }

    let _ = std::fs::File::create(wows_dir.join("PnFModsLoader.py"));

    *mod_info.mod_paths.lock() = paths;

    Ok(())
}

fn install_mod(runtime: Arc<Runtime>, wows_data: Arc<RwLock<WorldOfWarshipsData>>, mod_info: ModInfo, tx: mpsc::Sender<BackgroundTask>) {
    eprintln!("downloading mod");
    let (download_task_tx, download_task_rx) = mpsc::channel();
    let (download_progress_tx, download_progress_rx) = mpsc::channel();
    let _ = tx.send(BackgroundTask {
        receiver: download_task_rx.into(),
        kind: BackgroundTaskKind::DownloadingMod {
            mod_info: mod_info.clone(),
            rx: download_progress_rx,
            last_progress: None,
        },
    });

    // TODO: Download pending mods in parallel?
    let tar_file = runtime.block_on(async { download_mod_tarball(&mod_info, download_progress_tx).await });
    eprintln!("downloaded");
    match tar_file {
        Ok(tar_file) => {
            download_task_tx.send(Ok(BackgroundTaskCompletion::ModDownloaded(mod_info.clone()))).unwrap();

            let (install_task_tx, install_task_rx) = mpsc::channel();
            let (install_progress_tx, install_progress_rx) = mpsc::channel();
            let _ = tx.send(BackgroundTask {
                receiver: install_task_rx.into(),
                kind: if mod_info.enabled {
                    BackgroundTaskKind::InstallingMod {
                        mod_info: mod_info.clone(),
                        rx: install_progress_rx,
                        last_progress: None,
                    }
                } else {
                    BackgroundTaskKind::UninstallingMod {
                        mod_info: mod_info.clone(),
                        rx: install_progress_rx,
                        last_progress: None,
                    }
                },
            });

            let unpack_result = unpack_mod(&tar_file, wows_data.clone(), &mod_info, install_progress_tx);
            eprintln!("unpack res: {:?}", unpack_result);

            install_task_tx.send(Ok(BackgroundTaskCompletion::ModInstalled(mod_info))).unwrap();
        }
        Err(e) => {
            eprintln!("{:?}", e);
        }
    }
}

fn uninstall_mod(runtime: Arc<Runtime>, wows_data: Arc<RwLock<WorldOfWarshipsData>>, mod_info: ModInfo, tx: mpsc::Sender<BackgroundTask>) -> anyhow::Result<()> {
    eprintln!("downloading mod");
    let (uninstall_task_tx, uninstall_task_rx) = mpsc::channel();
    let (uninstall_progress_tx, uninstall_progress_rx) = mpsc::channel();
    let _ = tx.send(BackgroundTask {
        receiver: uninstall_task_rx.into(),
        kind: BackgroundTaskKind::UninstallingMod {
            mod_info: mod_info.clone(),
            rx: uninstall_progress_rx,
            last_progress: None,
        },
    });

    let wows_dir = { wows_data.read().build_dir.join("res_mods") };
    let paths = { mod_info.mod_paths.lock().clone() };
    for (i, file) in paths.iter().enumerate() {
        if !file.starts_with(&wows_dir) {
            continue;
        }

        eprintln!("deleting {}", file.display());
        if file.is_dir() {
            std::fs::remove_dir_all(file);
        } else {
            std::fs::remove_file(file);
        }

        let _ = uninstall_progress_tx.send(DownloadProgress {
            downloaded: i as u64,
            total: paths.len() as u64,
        });
    }

    uninstall_task_tx.send(Ok(BackgroundTaskCompletion::ModUninstalled(mod_info))).unwrap();

    Ok(())
}

pub fn start_mod_manager_thread(
    runtime: Arc<Runtime>,
    wows_data: Arc<RwLock<WorldOfWarshipsData>>,
    receiver: mpsc::Receiver<ModInfo>,
    background_task_sender: mpsc::Sender<BackgroundTask>,
) {
    std::thread::spawn(move || {
        while let Ok(mod_info) = receiver.recv() {
            eprintln!("mod was changed: {:?}", mod_info.meta.name());

            if mod_info.enabled {
                eprintln!("installing mod: {:?}", mod_info.meta.name());
                install_mod(runtime.clone(), wows_data.clone(), mod_info.clone(), background_task_sender.clone());
            } else {
                eprintln!("uninstalling mod: {:?}", mod_info.meta.name());
                uninstall_mod(runtime.clone(), wows_data.clone(), mod_info.clone(), background_task_sender.clone());
            }
        }
    });
}

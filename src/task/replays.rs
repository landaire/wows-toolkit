use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::fs::read_dir;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self};
use std::thread;
use std::time::Duration;

use gettext::Catalog;
use language_tags::LanguageTag;
use parking_lot::Mutex;
use parking_lot::RwLock;
use rootcause::Report;
use rootcause::prelude::ResultExt;
use serde::Deserialize;
use serde::Serialize;
use std::borrow::Cow;
use tracing::debug;
use tracing::error;
use tracing::instrument;
use tracing::warn;
use wows_replays::ReplayFile;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::Version;
use wowsunpack::data::idx::{self, VfsEntry};
use wowsunpack::data::idx_vfs::IdxVfs;
use wowsunpack::data::wrappers::mmap::MmapPkgSource;
use wowsunpack::game_data;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

use crate::build_tracker;
use crate::error::ToolkitError;
use crate::game_params::load_game_params;
use crate::replay_export::FlattenedVehicle;
use crate::replay_export::Match;
use crate::twitch::TwitchState;
use crate::ui::player_tracker::PlayerTracker;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::SortOrder;
use crate::wows_data::GameAsset;
use crate::wows_data::WorldOfWarshipsData;

use super::BackgroundTask;
use super::BackgroundTaskCompletion;
use super::BackgroundTaskKind;

use crate::task::networking::load_versioned_constants_from_disk_with_fallback;

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

#[instrument(skip(vfs))]
pub fn load_ribbon_icons(vfs: &VfsPath, dir_path: &str) -> HashMap<String, Arc<GameAsset>> {
    let mut icons = HashMap::new();

    let Ok(dir) = vfs.join(dir_path) else { return icons };
    let Ok(entries) = dir.read_dir() else { return icons };
    for entry in entries {
        let filename = entry.filename();
        let file_stem = Path::new(&filename).file_stem().and_then(|s| s.to_str());
        let Some(file_name) = file_stem else { continue };
        let full_path = entry.as_str().trim_start_matches('/').to_string();
        let mut icon_data = Vec::new();
        if entry.open_file().and_then(|mut f| f.read_to_end(&mut icon_data).map_err(|e| e.into())).is_err() {
            continue;
        }
        icons.insert(file_name.to_string(), Arc::new(GameAsset { path: full_path, data: icon_data }));
    }

    icons
}

#[instrument(skip_all)]
pub fn load_ship_icons(vfs: &VfsPath) -> HashMap<Species, Arc<GameAsset>> {
    let species = [
        Species::AirCarrier,
        Species::Battleship,
        Species::Cruiser,
        Species::Destroyer,
        Species::Submarine,
        Species::Auxiliary,
    ];

    HashMap::from_iter(species.iter().filter_map(|species| {
        let path = wowsunpack::game_params::translations::ship_class_icon_path(species);
        let mut icon_data = Vec::new();
        vfs.join(&path).ok()?.open_file().ok()?.read_to_end(&mut icon_data).ok()?;
        Some((*species, Arc::new(GameAsset { path, data: icon_data })))
    }))
}

fn current_build_from_preferences(path: &Path) -> Option<String> {
    let data = std::fs::read_to_string(path).ok()?;
    let start_of_node = data.find("<last_server_version>")?;
    let end_of_node = data[start_of_node..].find("</last_server_version>")?;
    let version_str = &data[start_of_node + "<last_server_version>".len()..(start_of_node + end_of_node)].trim();

    Some(version_str.to_string())
}

/// Build `GameConstants` from VFS and merge in replay constants (CONSUMABLE_IDS, BATTLE_STAGES).
#[instrument(skip(vfs, replay_constants))]
pub fn build_game_constants(vfs: &VfsPath, replay_constants: &serde_json::Value, build: u32) -> GameConstants {
    let mut game_constants = GameConstants::from_vfs(vfs);
    if let Some(consumable_ids) = replay_constants.pointer("/CONSUMABLE_IDS").and_then(|ids| ids.as_object()) {
        let types = game_constants.common_mut().consumable_types_mut();
        for (key, value) in consumable_ids {
            let id = value.as_i64().expect("CONSUMABLE_IDS value is not a number") as i32;
            types.insert(id, Cow::Owned(key.clone()));
        }
    }
    if let Some(battle_stages) = replay_constants.pointer("/BATTLE_STAGES").and_then(|s| s.as_object()) {
        let stages = game_constants.common_mut().battle_stages_mut();
        let version = Version { major: 0, minor: 0, patch: 0, build };
        for (key, value) in battle_stages {
            if let Some(id) = value.as_i64()
                && let Some(stage) = wowsunpack::game_types::BattleStage::from_name(key, version).into_known()
            {
                stages.insert(id as i32, stage);
            }
        }
    }
    game_constants
}

/// Load game resources for a specific build number. This can be called for any build
/// that has a directory in `bin/`. Used both at startup (for the latest build) and
/// lazily when a replay from a different version is loaded.
#[instrument(skip(fallback_constants))]
pub fn load_wows_data_for_build(
    wows_directory: &Path,
    build: u32,
    locale: &str,
    fallback_constants: &serde_json::Value,
) -> Result<WorldOfWarshipsData, Report> {
    let game_patch = build as usize;
    let build_dir = wows_directory.join("bin").join(format!("{build}"));

    debug!("Loading game data for build {}", build);

    // Parse IDX files and build VFS
    let mut idx_files = Vec::new();
    for file in read_dir(build_dir.join("idx")).context("failed to read idx directory")? {
        let file = file.context("failed to read idx directory entry")?;
        if file.file_type().context("failed to get file type for idx entry")?.is_file() {
            let path = file.path();
            let file_data =
                std::fs::read(&path).context_with(|| format!("failed to read idx file {}", path.display()))?;
            idx_files
                .push(idx::parse(&file_data).context_with(|| format!("failed to parse idx file {}", path.display()))?);
        }
    }

    let pkgs_path = wows_directory.join("res_packages");
    if !pkgs_path.exists() {
        Err(crate::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()))
            .context("res_packages directory not found")?;
    }

    let pkg_source = MmapPkgSource::new(&pkgs_path);
    let idx_vfs = IdxVfs::new(pkg_source, &idx_files);
    let vfs = VfsPath::new(idx_vfs);

    // Build flat file list for the file browser
    let file_map = idx::build_file_tree(&idx_files);
    let filtered_files: Vec<(Arc<PathBuf>, VfsPath)> = file_map
        .iter()
        .filter(|(_, entry)| matches!(entry, VfsEntry::File { .. }))
        .filter_map(|(path_str, _)| {
            let vfs_path = vfs.join(path_str).ok()?;
            Some((Arc::new(PathBuf::from(path_str)), vfs_path))
        })
        .collect();

    // Load translations
    let language_tag: LanguageTag = locale
        .parse()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("invalid locale: {locale}")))
        .context_with(|| format!("failed to parse locale '{locale}'"))?;
    let attempted_dirs = [locale, language_tag.primary_language(), "en"];
    let mut found_catalog = None;
    for dir in attempted_dirs {
        let localization_path = wows_directory.join(format!("bin/{build}/res/texts/{dir}/LC_MESSAGES/global.mo"));
        if !localization_path.exists() {
            continue;
        }
        let global = File::open(&localization_path)
            .context_with(|| format!("failed to open localization file {}", localization_path.display()))?;
        let catalog = Catalog::parse(global)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e}")))
            .context_with(|| format!("failed to parse localization catalog {}", localization_path.display()))?;
        found_catalog = Some(catalog);
        break;
    }

    debug!("Loading GameParams for build {}", build);
    let metadata_provider = load_game_params(&vfs, game_patch).ok().map(|mut metadata_provider| {
        if let Some(catalog) = found_catalog {
            metadata_provider.set_translations(catalog)
        }
        Arc::new(metadata_provider)
    });

    debug!("Loading icons for build {}", build);
    let icons = load_ship_icons(&vfs);
    let ribbon_icons = load_ribbon_icons(&vfs, wowsunpack::game_params::translations::RIBBON_ICONS_DIR);
    let subribbon_icons = load_ribbon_icons(&vfs, wowsunpack::game_params::translations::RIBBON_SUBICONS_DIR);

    // Load version-matched constants from disk cache only (no network I/O).
    // If not cached, use fallback constants. The networking thread will fetch
    // updated constants from GitHub in the background.
    debug!("Loading versioned constants for build {}", build);
    let (replay_constants, replay_constants_exact_match) = match load_versioned_constants_from_disk_with_fallback(build)
    {
        Some((data, exact)) => (data, exact),
        None => (fallback_constants.clone(), false),
    };

    let game_constants = build_game_constants(&vfs, &replay_constants, build);
    let game_constants = Arc::new(game_constants);

    // Try to determine full version from preferences or leave as None for non-latest builds
    let full_version = None; // Will be set by caller for latest build

    Ok(WorldOfWarshipsData {
        game_metadata: metadata_provider,
        vfs,
        filtered_files,
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

#[instrument(skip(fallback_constants))]
pub fn load_wows_files(
    wows_directory: PathBuf,
    locale: &str,
    fallback_constants: &serde_json::Value,
) -> Result<BackgroundTaskCompletion, Report> {
    let bin_dir = wows_directory.join("bin");
    if !wows_directory.exists() || !bin_dir.exists() {
        debug!("WoWs or WoWs bin directory does not exist");
        Err(crate::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()))
            .context("World of Warships directory does not exist or is missing the bin/ folder")?;
    }

    // Discover all available builds
    let available_builds =
        game_data::list_available_builds(&wows_directory).context("failed to list available game builds")?;

    if available_builds.is_empty() {
        Err(crate::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()))
            .context("no game builds found in bin/ directory")?;
    }

    // Determine the latest build (from preferences or highest build number)
    let mut full_version = None;
    let mut latest_build = *available_builds.last().unwrap();
    let mut replays_dir = wows_directory.join("replays");

    let prefs_file = wows_directory.join("preferences.xml");
    if prefs_file.exists()
        && let Some(version_str) = current_build_from_preferences(&prefs_file)
        && version_str.contains(',')
    {
        let full_build_info = Version::from_client_exe(&version_str);
        if available_builds.contains(&full_build_info.build) {
            latest_build = full_build_info.build;
        }

        let friendly_build = format!("{}.{}.{}.0", full_build_info.major, full_build_info.minor, full_build_info.patch);
        full_version = Some(full_build_info);

        for temp_replays_dir in [replays_dir.join(&friendly_build), replays_dir.join(friendly_build)] {
            debug!("Looking for build-specific replays dir at {:?}", temp_replays_dir);
            if temp_replays_dir.exists() {
                replays_dir = temp_replays_dir;
                break;
            }
        }
    }

    // Load data for the latest build
    let mut data = load_wows_data_for_build(&wows_directory, latest_build, locale, fallback_constants)
        .context_with(|| format!("failed to load game data for build {latest_build}"))?;
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
            replay.source_path = Some(path.clone());
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
                    replay.source_path = Some(path.to_path_buf());
                    let mut build_uploaded_successfully = false;
                    match replay.parse(game_version.to_string().as_str()) {
                        Ok(report) => {
                            debug!("replay parsed successfully");
                            let battle_type =
                                wowsunpack::game_types::BattleType::from_value(&game_type, replay_version);
                            let is_valid_game_type_for_shipbuilds = matches!(
                                battle_type.known(),
                                Some(
                                    wowsunpack::game_types::BattleType::Random
                                        | wowsunpack::game_types::BattleType::Ranked
                                )
                            );
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
                                            game_type.to_string(),
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

        debug!("Beginning background replay receive loop");
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

#[instrument(skip_all, fields(replay_count = replays.len()))]
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
                        replay.source_path = Some(path.clone());
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

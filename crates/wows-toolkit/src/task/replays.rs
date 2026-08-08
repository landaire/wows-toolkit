use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;
use parking_lot::RwLock;
use rootcause::Report;
use rootcause::prelude::*;
use tracing::debug;
use tracing::error;
use tracing::instrument;
use tracing::warn;
use wows_replays::ReplayFile;
use wowsunpack::data::Version;
use wowsunpack::game_data;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

use crate::data::replay_reconcile::ParseOutcome;
use crate::data::settings::DataSharingMode;
use crate::data::wows_data::BuildData;
use crate::data::wows_data::GameAsset;
use crate::data::wows_data::ReplayDependencies;
use crate::data::wows_data::ReplayLoader;
use crate::tab_state::SharedPersistedState;
use crate::task::replay_upload::ReplayCount;
use crate::task::replay_upload::SendAllReplaysProgress;
use crate::task::replay_upload::SendReplayCachePolicy;
use crate::task::replay_upload::ShipBuildsUploadOutcome;
use crate::task::replay_upload::upload_parsed_replay;
use crate::twitch::TwitchState;
use crate::ui::player_tracker::PlayerTracker;
use crate::ui::replay_parser::ListedReplay;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::SortOrder;
use crate::util::error::ToolkitError;
use crate::util::replay_export::FlattenedVehicle;
use crate::util::replay_export::Match;

use super::BackgroundTask;
use super::BackgroundTaskCompletion;
use super::BackgroundTaskKind;
use super::IndexProgress;

use crate::task::networking::load_versioned_constants_from_disk;

pub fn replay_filepaths(replays_dir: &Path) -> Option<Vec<PathBuf>> {
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

/// Every `.wowsreplay` under `root`, recursively.
///
/// `temp.wowsreplay` is the file the game rewrites while a battle is running,
/// so it is excluded exactly as the live listing excludes it. Symlinks are not
/// followed: a directory the user picked may link back into its own ancestry,
/// which would loop the walk. Entries that cannot be read are skipped rather
/// than aborting, so one unreadable subdirectory does not lose the rest of a
/// dropped tree. The result is sorted so an ingest visits a directory in the
/// same order every time.
pub fn walk_replay_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .filter(|path| {
            path.extension().is_some_and(|extension| extension == "wowsreplay")
                && path.file_name().is_some_and(|name| name != "temp.wowsreplay")
        })
        .collect();
    files.sort();
    files
}

#[instrument(skip(vfs))]
pub fn load_ribbon_icons(
    vfs: &VfsPath,
    dir: wowsunpack::game_assets::GuiAssetDir,
    version: Option<&Version>,
) -> HashMap<String, Arc<GameAsset>> {
    let mut icons = HashMap::new();

    let Some(dir) = dir.resolve(vfs, version) else {
        // Absent in this build (e.g. the Flash-era GUI has no per-file ribbons).
        return icons;
    };

    let Ok(entries) = dir.read_dir() else {
        error!("failed to get directory entries");
        return icons;
    };

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

/// Load a single nation flag PNG from `gui/nation_flags/tiny/flag_{nation}.png`.
pub fn load_nation_flag(vfs: &VfsPath, nation: &str, version: Option<&Version>) -> Option<Arc<GameAsset>> {
    let resolved = wowsunpack::game_assets::GuiAsset::NationFlag(nation).resolve(vfs, version)?;
    let path = resolved.as_str().trim_start_matches('/').to_string();
    let mut data = Vec::new();
    resolved.open_file().ok()?.read_to_end(&mut data).ok()?;
    (!data.is_empty()).then(|| Arc::new(GameAsset { path, data }))
}

#[instrument(skip_all)]
pub fn load_ship_icons(vfs: &VfsPath, version: Option<&Version>) -> HashMap<Species, Arc<GameAsset>> {
    use wowsunpack::game_assets::GuiAsset;
    use wowsunpack::game_assets::ShipIconState;

    let species = [
        Species::AirCarrier,
        Species::Battleship,
        Species::Cruiser,
        Species::Destroyer,
        Species::Submarine,
        Species::Auxiliary,
    ];

    HashMap::from_iter(species.iter().filter_map(|species| {
        let resolved =
            GuiAsset::ShipClassIcon { species: *species, state: ShipIconState::Alive }.resolve(vfs, version)?;
        let path = resolved.as_str().trim_start_matches('/').to_string();
        let mut data = Vec::new();
        resolved.open_file().ok()?.read_to_end(&mut data).ok()?;
        Some((*species, Arc::new(GameAsset { path, data })))
    }))
}

/// Parse a dotted version string like `"0.6.13"` or `"15.1.0"` into a
/// [`Version`], attaching the build number. Missing components default to 0.
pub(crate) fn parse_dotted_version(version: &str, build: u32) -> Option<Version> {
    let mut parts = version.split('.').map(|p| p.trim().parse::<u32>().ok());
    let major = parts.next()??;
    let minor = parts.next().flatten().unwrap_or(0);
    let patch = parts.next().flatten().unwrap_or(0);
    Some(Version { major, minor, patch, build: std::num::NonZeroU32::new(build) })
}

fn current_build_from_preferences(path: &Path) -> Option<String> {
    let data = std::fs::read_to_string(path).ok()?;
    let start_of_node = data.find("<last_server_version>")?;
    let end_of_node = data[start_of_node..].find("</last_server_version>")?;
    let version_str = &data[start_of_node + "<last_server_version>".len()..(start_of_node + end_of_node)].trim();

    Some(version_str.to_string())
}

#[instrument(skip(fallback_constants))]
pub fn load_wows_files(
    wows_directory: PathBuf,
    locale: &str,
    fallback_constants: &serde_json::Value,
    auto_dump: bool,
    game_data_cache_dir: String,
) -> Result<BackgroundTaskCompletion, Report> {
    if !wows_directory.exists() {
        debug!("WoWs directory does not exist: {:?}", wows_directory);
        Err(crate::util::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()))
            .context("World of Warships directory does not exist")?;
    }

    // Check for telltale signs of a valid WoWs installation
    let has_exe = wows_directory.join("WorldOfWarships.exe").exists();
    let has_bin = wows_directory.join("bin").exists();
    let has_replays = wows_directory.join("replays").exists();

    if !has_exe && !has_bin && !has_replays {
        debug!("WoWs directory missing expected contents: {:?}", wows_directory);
        Err(crate::util::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()))
            .context("Invalid World of Warships directory. Make sure it's set to the game's root directory (containing WorldOfWarships.exe).")?;
    }

    if !has_bin {
        debug!("WoWs bin directory does not exist");
        Err(crate::util::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()))
            .context("World of Warships directory is missing the bin/ folder")?;
    }

    // Discover all available builds
    let available_builds =
        game_data::list_available_builds(&wows_directory).context("failed to list available game builds")?;

    if available_builds.is_empty() {
        Err(crate::util::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()))
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
        if let Some(full_build) = full_build_info.build_number() {
            if available_builds.contains(&full_build) {
                latest_build = full_build;
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
        } else {
            tracing::warn!(
                "preferences version {:?} lacks a build number; using highest installed build and default replays dir",
                version_str
            );
        }
    }

    // Load data for the latest build
    let mut data =
        BuildData::from_live_install(&wows_directory, latest_build, locale, fallback_constants, full_version)
            .context_with(|| format!("failed to load game data for build {latest_build}"))?;
    data.full_version = full_version;
    data.replays_dir = replays_dir.clone();

    debug!("Loading replays");
    let replays = replay_filepaths(&replays_dir).map(|replays| {
        // Metadata-only parse: reading, decrypting, and inflating packet data
        // here would make large directory scans take minutes instead of
        // seconds. Packet data is loaded when a replay is opened. The borrowed
        // parse avoids owning metadata fields the listing never keeps.
        let iter = replays.into_iter().filter_map(|path| {
            let blob = match ReplayFile::read_meta_blob(&path) {
                Ok(blob) => blob,
                Err(e) => {
                    error!("Failed to read replay header {}: {:?}", path.display(), e);
                    return None;
                }
            };
            match wows_replays::ReplayMetaRef::from_slice(&blob) {
                Ok(meta) => Some((path, Arc::new(ListedReplay::from_meta_ref(&meta)))),
                Err(e) => {
                    error!("Failed to parse replay {}: {:?}", path.display(), e);
                    None
                }
            }
        });

        HashMap::from_iter(iter)
    });

    // Clean up stale caches for builds that no longer exist
    crate::util::game_params::cleanup_stale_caches(&available_builds);

    // Auto-dump game data for this version so replays still work after a game update
    if auto_dump
        && let Some(ref fv) = data.full_version
        && let Some(dump_base) = game_data_dump_base_with_override(&game_data_cache_dir)
    {
        let version_str = format!("{}.{}.{}", fv.major, fv.minor, fv.patch);
        if let Some(fv_build) = fv.build_number() {
            if !wows_data_mgr::dump::dump_exists(&dump_base, &version_str, fv_build) {
                let game_dir = wows_directory.clone();
                let build = fv_build;
                let vs = version_str.clone();
                crate::util::thread::spawn_logged("auto-dump-game-data", move || {
                    if let Err(e) =
                        wows_data_mgr::dump::dump_renderer_data(&game_dir, build, &vs, &dump_base, None, true)
                    {
                        tracing::warn!("Auto-dump failed for {vs}_{build}: {e}");
                    } else {
                        // Copy constants.json into the dump if available on disk
                        if let Some(constants) = load_versioned_constants_from_disk(build) {
                            let dump_dir = wows_data_mgr::dump::dump_dir(&dump_base, &vs, build);
                            let dest = dump_dir.join("constants.json");
                            if let Ok(bytes) = serde_json::to_vec_pretty(&constants) {
                                let _ = std::fs::write(&dest, bytes);
                            }
                        }
                    }
                });
            }
        } else {
            tracing::warn!("preferences version {version_str} lacks a build number; skipping auto-dump of game data");
        }
    }

    debug!("Sending background task completion");

    Ok(BackgroundTaskCompletion::DataLoaded {
        new_dir: wows_directory,
        wows_data: Box::new(data),
        replays,
        available_builds,
    })
}

/// Returns the base directory for auto-dumped game data.
/// Uses the custom path from settings if set, otherwise the default app data location.
pub fn game_data_dump_base() -> Option<PathBuf> {
    // Try to read the custom path from settings (requires db to be loaded).
    // This is called from background threads that may not have access to TabState,
    // so we also accept it as a parameter in the dump trigger path.
    crate::storage_dir().map(|d| d.join("game_data"))
}

/// Returns the base directory for auto-dumped game data, preferring a custom path if set.
pub fn game_data_dump_base_with_override(custom_dir: &str) -> Option<PathBuf> {
    if !custom_dir.is_empty() {
        let p = PathBuf::from(custom_dir);
        if p.is_absolute() {
            return Some(p);
        }
    }
    game_data_dump_base()
}

/// Load game data from a previously dumped directory.
/// Used as a fallback when the live game install no longer has the build.
/// Cache a re-parsed provider's params into the build's override directory, so
/// the next load of this dump skips the parse.
///
/// Parsing GameParams from a dump's VFS costs seconds; decoding the cache costs
/// tens of milliseconds. Best-effort: a failure to write only means the next
/// load pays the parse again.
pub(crate) fn write_params_override(
    cas: &wows_data_mgr::cas_vfs::BuildCas,
    provider: &wowsunpack::game_params::provider::GameMetadataProvider,
) {
    use wowsunpack::game_params::types::GameParamProvider;

    let Some(path) = cas.derived_write_path("game_params.rkyv") else {
        return;
    };
    let params: Vec<_> = provider.params().iter().map(|param| Arc::unwrap_or_clone(Arc::clone(param))).collect();
    match wowsunpack::game_params::cache::save(&path, &params) {
        Ok(()) => debug!("wrote GameParams override to {}", path.display()),
        Err(e) => warn!("failed to write GameParams override to {}: {e}", path.display()),
    }
}

fn parse_replay_data_in_background(
    path: &Path,
    shipbuilds_client: &crate::data::shipbuilds::ShipBuildsClient,
    replay_parsed_before: bool,
    data: &BackgroundParserThread,
) -> ParseOutcome {
    // The parser lock serves to prevent file access issues when both the main
    // and background thread are attempting to parse some data. This technically
    // makes all parsers synchronous, but shouldn't be a big deal in practice.
    let _parser_lock = data.parser_lock.lock();

    // Files may be getting written to. If we fail to parse the replay,
    // let's try try to parse this at least 3 times.
    debug!("Sending replay data for: {:?}", path);
    for _ in 0..3 {
        match crate::data::wows_data::ReplayFileSnapshot::read(path) {
            Ok(snapshot) => {
                let crate::data::wows_data::ReplayFileSnapshot {
                    replay_file,
                    bytes: replay_bytes,
                    identity: replay_identity,
                } = snapshot;
                debug!("replay parsed successfully");
                // Resolve version-matched data for this replay's build
                let replay_version = wowsunpack::data::Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
                let Some(wows_data_for_build) = data.build_cache.resolve(&replay_version) else {
                    warn!(
                        "Skipping replay {:?}: no data for build {}",
                        path,
                        replay_version.build_number().map_or_else(|| "unknown".to_string(), |b| b.to_string())
                    );
                    return ParseOutcome::Transient;
                };

                let (metadata_provider, game_version, gc) = {
                    let wows_data = wows_data_for_build.read();
                    (wows_data.game_metadata.clone(), wows_data.patch_version, wows_data.game_constants.clone())
                };
                if let Some(metadata_provider) = metadata_provider {
                    // Populate cap layout cache in a separate thread so it
                    // cannot interfere with the background parser thread.
                    {
                        let key = crate::data::cap_layout::CapLayoutKey {
                            map_id: replay_file.meta.mapId,
                            scenario_config_id: replay_file.meta.scenarioConfigId,
                        };
                        let needs_extract = !data.cap_layout_db.lock().contains(&key);
                        if needs_extract {
                            let cap_path = path.to_path_buf();
                            let cap_provider = Arc::clone(&metadata_provider);
                            let cap_gc = Arc::clone(&gc);
                            let cap_db = Arc::clone(&data.cap_layout_db);
                            let cap_pool = data.db_pool.clone();
                            let cap_rt = data.tokio_runtime.clone();
                            crate::util::thread::spawn_logged("cap-layout-extract", move || {
                                if let Some(layout) = crate::data::cap_layout::extract_cap_layout_from_replay(
                                    &cap_path,
                                    cap_provider.as_ref(),
                                    Some(cap_gc.as_ref()),
                                ) {
                                    let mut db = cap_db.lock();
                                    if db.insert(layout.clone()) {
                                        debug!("added cap layout for ({}, {})", key.map_id, key.scenario_config_id);
                                        // Save to SQLite if available.
                                        if let (Some(pool), Some(rt)) = (&cap_pool, &cap_rt) {
                                            let _ = rt.block_on(
                                                crate::data::cap_layout::CapLayoutDb::save_layout_to_db(pool, &layout),
                                            );
                                        } else if let Some(cache_path) = crate::data::cap_layout::cache_path() {
                                            let _ = db.save(&cache_path);
                                        }
                                    }
                                }
                            });
                        }
                    }

                    let mut replay = Replay::new(replay_file, Arc::clone(&metadata_provider));
                    replay.game_constants = Some(gc);
                    replay.source_path = Some(path.to_path_buf());
                    let mut parsed_ok = false;
                    let mut upload_outcome = ShipBuildsUploadOutcome::Sent;
                    match replay.parse(game_version.to_string().as_str()) {
                        Ok(report) => {
                            if !replay_parsed_before {
                                upload_outcome = upload_parsed_replay(
                                    path,
                                    &replay,
                                    &report,
                                    metadata_provider.as_ref(),
                                    configured_data_sharing_mode(&data.persisted),
                                    shipbuilds_client,
                                    replay_bytes,
                                );

                                data.player_tracker.write().update_from_replay(&replay);
                            }

                            // Update the player tracker
                            replay.battle_report = Some(report);
                            parsed_ok = true;
                        }
                        Err(e)
                            if e.downcast_current_context::<ToolkitError>()
                                .is_some_and(|e| matches!(e, ToolkitError::ReplayVersionMismatch { .. })) =>
                        {
                            // The replay's version can't be parsed with this build's data.
                            // It is not malformed, so do not blacklist it.
                            return ParseOutcome::Transient;
                        }
                        Err(e) => {
                            error!("error parsing background replay: {:?}", e);
                        }
                    }

                    if let Some(battle_report) = replay.battle_report.as_ref() {
                        // Data export should only happen once server-provided battle results are
                        // available -- otherwise the exported data isn't reliable or interesting.
                        // Indexing, however, runs for any successfully-parsed replay: a
                        // results-absent (left-early) replay is still indexed with
                        // `results_available = false` and NULL server stats (see
                        // `replay_index::map_rows`), so it still shows up in rosters and
                        // recent-matches. Capture the flag now, since it's the last use of
                        // `battle_report` before `build_ui_report` needs `&mut replay`.
                        let results_available = battle_report.battle_results().is_some();

                        // Create a dummy sender since we don't need to send background tasks from here
                        let (dummy_sender, _) = egui_inbox::UiInbox::channel();
                        let deps = crate::data::wows_data::ReplayDependencies {
                            build_cache: data.build_cache.clone(),
                            shipbuilds_client: data.shipbuilds_client.clone(),
                            twitch_state: Arc::clone(&data.twitch_state),
                            replay_sort: Arc::new(Mutex::new(SortOrder::default())),
                            background_task_sender: dummy_sender,
                            is_debug_mode: data.is_debug,
                            personal_rating_data: Arc::clone(&data.personal_rating_data),
                            egui_ctx: data.egui_ctx.clone(),
                        };
                        replay.build_ui_report(&deps);

                        if let (Some(pool), Some(rt), Some(source_id)) =
                            (data.db_pool.as_ref(), data.tokio_runtime.as_ref(), data.index_source_id)
                        {
                            crate::data::replay_index::index_replay_blocking(
                                rt,
                                pool,
                                &replay,
                                source_id,
                                jiff::Timestamp::now(),
                                &data.personal_rating_data,
                            );
                        }

                        // TODO: this might export data multiple times. The gate is only
                        // `results_available`, not whether this replay has been parsed
                        // before, so any path that re-parses an already-exported replay
                        // rewrites its export file.
                        if results_available && data.data_export_settings.should_auto_export {
                            let export_path =
                                data.data_export_settings.export_path.join(replay.better_file_name(&metadata_provider));
                            let export_path =
                                export_path.with_extension(match data.data_export_settings.export_format {
                                    ReplayExportFormat::Json => "json",
                                    ReplayExportFormat::Cbor => "cbor",
                                    ReplayExportFormat::Csv => "csv",
                                });

                            let transformed_data = Match::new(&replay, data.is_debug);

                            if let Err(e) =
                                File::create(&export_path).context("failed to create export file").and_then(|file| {
                                    match data.data_export_settings.export_format {
                                        ReplayExportFormat::Json => serde_json::to_writer(file, &transformed_data)
                                            .context("failed to write export file"),
                                        ReplayExportFormat::Cbor => ciborium::into_writer(&transformed_data, file)
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
                                    }
                                })
                            {
                                // fail gracefully
                                error!("failed to write data export file: {:?}", e);
                            }
                        }
                    }

                    if parsed_ok {
                        return parse_outcome_for_upload(upload_outcome, replay_identity);
                    }
                } else {
                    // Game data for this build isn't loaded yet; retry next launch.
                    return ParseOutcome::Transient;
                }
            }
            Err(e) => {
                error!("error attempting to parse replay in background thread: {:?}", e);
                thread::sleep(Duration::from_secs(5));
            }
        }
    }

    ParseOutcome::HardFailure
}

fn parse_outcome_for_upload(
    outcome: ShipBuildsUploadOutcome,
    replay_identity: Option<crate::data::replay_reconcile::ReplayFileIdentity>,
) -> ParseOutcome {
    match outcome {
        ShipBuildsUploadOutcome::Skipped(crate::task::replay_upload::ReplayUploadSkipReason::IneligibleGameType) => {
            ParseOutcome::ParsedAndStableSkipped { identity: replay_identity }
        }
        ShipBuildsUploadOutcome::Skipped(crate::task::replay_upload::ReplayUploadSkipReason::SharingDisabled) => {
            ParseOutcome::ParsedAndDeferred
        }
        ShipBuildsUploadOutcome::Sent => ParseOutcome::ParsedAndSent,
        ShipBuildsUploadOutcome::TransientFailure => ParseOutcome::ParsedNotSent,
    }
}

pub use wows_toolkit_config::ReplayExportFormat;

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
    DataAutoExportSettingChange(DataExportSettings),
    DebugStateChange(bool),
    /// A match started, or the debug picker chose a replay to treat as one.
    /// Carries the file whose `onArenaStateReceived` names the roster.
    LiveMatchStarted {
        replay: PathBuf,
        build: Option<u32>,
        flush: crate::task::live_match_stats::FlushState,
        /// The match's key, matching `LiveMatch::started_at`. A scan queued
        /// behind a newer match's start still finishes; this lets its result
        /// be refused instead of landing on the wrong roster.
        started_at: jiff::Timestamp,
    },
}

pub struct BackgroundParserThread {
    pub rx: mpsc::Receiver<ReplayBackgroundParserThreadMessage>,
    pub sent_replays: Arc<RwLock<HashSet<String>>>,
    pub build_cache: crate::data::wows_data::BuildDataCache,
    pub shipbuilds_client: crate::data::shipbuilds::ShipBuildsClient,
    pub twitch_state: Arc<RwLock<TwitchState>>,
    pub persisted: SharedPersistedState,
    pub data_export_settings: DataExportSettings,
    pub player_tracker: Arc<RwLock<PlayerTracker>>,
    pub is_debug: bool,
    pub parser_lock: Arc<Mutex<()>>,
    pub cap_layout_db: Arc<Mutex<crate::data::cap_layout::CapLayoutDb>>,
    pub db_pool: Option<sqlx::SqlitePool>,
    pub tokio_runtime: Option<Arc<tokio::runtime::Runtime>>,
    pub personal_rating_data: Arc<RwLock<crate::util::personal_rating::PersonalRatingData>>,
    /// Cached id of the live replay-index source, resolved once at scan start so
    /// the live hook does not hit `ensure_default_source` per replay.
    pub index_source_id: Option<crate::db::index::rows::SourceId>,
    /// Replays that panicked or hard-errored during parsing, keyed by path + mtime
    /// and persisted so they are not retried every launch.
    pub unindexable: crate::data::replay_reconcile::Unindexable,
    /// Wakes the UI after work that mutated shared state (player tracker,
    /// sent-replay ledger) so the change shows without waiting for input.
    pub egui_ctx: egui::Context,
}

pub fn start_background_parsing_thread(mut data: BackgroundParserThread) -> std::thread::JoinHandle<Option<()>> {
    debug!("starting background parsing thread");
    crate::util::thread::spawn_logged("background-replay-parser", move || {
        // Built once so its arena cache and rate limiter survive across matches.
        let mut match_stats_client = crate::data::match_stats::MatchStatsClient::new(data.shipbuilds_client.clone());
        let mut stable_upload_skips = crate::data::replay_reconcile::StableUploadSkips::default();

        {
            debug!("Attempting to enumerate replays directory to see if there are any new ones to send");
            let Some(replays_dir) = data.build_cache.loaded_builds().first().map(|d| d.read().replays_dir.clone())
            else {
                error!("No game data loaded, cannot enumerate replays directory");
                return;
            };

            // Resolve the live index source once so the per-replay hook reuses it
            // instead of hitting ensure_default_source on every file.
            if let (Some(pool), Some(rt)) = (data.db_pool.as_ref(), data.tokio_runtime.as_ref()) {
                data.index_source_id = rt
                    .block_on(crate::db::index::query::ensure_default_source(
                        pool,
                        &replays_dir,
                        jiff::Timestamp::now(),
                    ))
                    .inspect_err(|e| warn!("failed to resolve replay index source: {e}"))
                    .ok();
            }

            // Load both ledgers once: which paths are already indexed for this
            // source, and the persistent set of files that previously failed.
            let indexed_paths: HashSet<String> =
                match (data.db_pool.as_ref(), data.tokio_runtime.as_ref(), data.index_source_id) {
                    (Some(pool), Some(rt), Some(src)) => {
                        rt.block_on(crate::db::index::query::record_paths_in_source(pool, src)).unwrap_or_default()
                    }
                    _ => HashSet::new(),
                };
            if let (Some(pool), Some(rt)) = (data.db_pool.as_ref(), data.tokio_runtime.as_ref()) {
                data.unindexable = rt.block_on(crate::data::replay_reconcile::Unindexable::load(pool));
                stable_upload_skips = rt.block_on(crate::data::replay_reconcile::StableUploadSkips::load(pool));
            }

            // Try to see if we have any historical replays we can send
            match std::fs::read_dir(&replays_dir) {
                Ok(read_dir) => {
                    let mut unindexable_dirty = false;
                    let mut stable_upload_skips_dirty = false;
                    for file in read_dir.flatten() {
                        let path = file.path();
                        if path.extension().map(|ext| ext != "wowsreplay").unwrap_or(false)
                            || path.file_name().map(|name| name == "temp.wowsreplay").unwrap_or(false)
                        {
                            continue;
                        }

                        let path_str = path.to_string_lossy();
                        if data.unindexable.contains(&path) {
                            continue;
                        }
                        let sent = data.sent_replays.read().contains(path_str.as_ref());
                        let indexed = indexed_paths.contains(path_str.as_ref());
                        let outcome = crate::data::replay_reconcile::reconcile_startup_one(
                            &path,
                            indexed,
                            sent,
                            configured_data_sharing_mode(&data.persisted),
                            &mut stable_upload_skips,
                            std::panic::AssertUnwindSafe(|| {
                                parse_replay_data_in_background(&path, &data.shipbuilds_client, sent, &data)
                            }),
                        );
                        stable_upload_skips_dirty |= outcome.stable_skip_changed;

                        match outcome.file {
                            // Mark sent only when the upload completed. Skipped and
                            // retryable uploads remain distinct but do not enter the ledger.
                            crate::data::replay_reconcile::FileOutcome::Parsed {
                                upload: crate::data::replay_reconcile::ParsedUploadDisposition::Sent,
                            } => {
                                if !sent {
                                    data.sent_replays.write().insert(path_str.into_owned());
                                }
                            }
                            // Only a hard parse failure or panic gets blacklisted.
                            crate::data::replay_reconcile::FileOutcome::HardFailure => {
                                if data.unindexable.insert(&path) {
                                    unindexable_dirty = true;
                                }
                            }
                            // Retryable (no game data yet) or already satisfied: no-op.
                            crate::data::replay_reconcile::FileOutcome::Transient
                            | crate::data::replay_reconcile::FileOutcome::Skipped
                            | crate::data::replay_reconcile::FileOutcome::Parsed { .. } => {}
                        }
                    }

                    if unindexable_dirty
                        && let (Some(pool), Some(rt)) = (data.db_pool.as_ref(), data.tokio_runtime.as_ref())
                        && let Err(e) = rt.block_on(data.unindexable.save(pool))
                    {
                        warn!("failed to persist unindexable replay set: {e}");
                    }
                    if stable_upload_skips_dirty
                        && let (Some(pool), Some(rt)) = (data.db_pool.as_ref(), data.tokio_runtime.as_ref())
                        && let Err(e) = rt.block_on(stable_upload_skips.save(pool))
                    {
                        warn!("failed to persist stable ShipBuilds upload skips: {e}");
                    }
                }
                Err(e) => {
                    error!("Error reading replays dir from background parsing thread: {:?}", e)
                }
            }

            data.egui_ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }

        debug!("Beginning background replay receive loop");
        while let Ok(message) = data.rx.recv() {
            match message {
                ReplayBackgroundParserThreadMessage::NewReplay(path) => {
                    let path_str = path.to_string_lossy();
                    let already_parsed_replay = { data.sent_replays.read().contains(path_str.as_ref()) };

                    if data.unindexable.contains(&path) {
                        debug!("Skipping blacklisted replay at {}", path_str);
                    } else {
                        debug!("Attempting to parse replay at {}", path_str);
                        // `indexed` is forced false so reconcile_one's indexed-and-sent skip
                        // cannot apply: a message on this channel is a request to parse this
                        // file now, not a candidate for the startup ledger's skip. Mark sent
                        // only when the parse fully completed and the upload succeeded;
                        // transient conditions are left for a later attempt. A hard failure or
                        // panic is caught by reconcile_one and blacklisted instead of unwinding
                        // through the receive loop.
                        let outcome = crate::data::replay_reconcile::reconcile_one(
                            &path,
                            false,
                            crate::data::replay_reconcile::UploadReconciliation::Pending,
                            std::panic::AssertUnwindSafe(|| {
                                parse_replay_data_in_background(
                                    &path,
                                    &data.shipbuilds_client,
                                    already_parsed_replay,
                                    &data,
                                )
                            }),
                        );
                        match outcome {
                            crate::data::replay_reconcile::FileOutcome::Parsed {
                                upload: crate::data::replay_reconcile::ParsedUploadDisposition::Sent,
                            } => {
                                data.sent_replays.write().insert(path_str.into_owned());
                            }
                            crate::data::replay_reconcile::FileOutcome::Parsed {
                                upload:
                                    crate::data::replay_reconcile::ParsedUploadDisposition::StableSkipped {
                                        identity: Some(identity),
                                    },
                            } => {
                                if stable_upload_skips.insert(identity)
                                    && let (Some(pool), Some(rt)) = (data.db_pool.as_ref(), data.tokio_runtime.as_ref())
                                    && let Err(e) = rt.block_on(stable_upload_skips.save(pool))
                                {
                                    warn!("failed to persist stable ShipBuilds upload skip: {e}");
                                }
                            }
                            crate::data::replay_reconcile::FileOutcome::HardFailure => {
                                if data.unindexable.insert(&path)
                                    && let (Some(pool), Some(rt)) = (data.db_pool.as_ref(), data.tokio_runtime.as_ref())
                                    && let Err(e) = rt.block_on(data.unindexable.save(pool))
                                {
                                    warn!("failed to persist unindexable replay set: {e}");
                                }
                            }
                            crate::data::replay_reconcile::FileOutcome::Transient
                            | crate::data::replay_reconcile::FileOutcome::Skipped => {}
                            crate::data::replay_reconcile::FileOutcome::Parsed { .. } => {}
                        }
                    }
                }
                ReplayBackgroundParserThreadMessage::ModifiedReplay(path) => {
                    let path_str = path.to_string_lossy();
                    let already_parsed_replay = { data.sent_replays.read().contains(path_str.as_ref()) };

                    if data.unindexable.contains(&path) {
                        debug!("Skipping blacklisted replay at {}", path_str);
                    } else {
                        // A modified replay always re-parses: `indexed` is forced false so
                        // reconcile_one's indexed-and-sent skip never applies, and the
                        // outcome is never used to mark the path sent. A hard failure or
                        // panic is caught by reconcile_one and blacklisted instead of
                        // unwinding through the receive loop.
                        let outcome = crate::data::replay_reconcile::reconcile_one(
                            &path,
                            false,
                            crate::data::replay_reconcile::UploadReconciliation::Pending,
                            std::panic::AssertUnwindSafe(|| {
                                parse_replay_data_in_background(
                                    &path,
                                    &data.shipbuilds_client,
                                    already_parsed_replay,
                                    &data,
                                )
                            }),
                        );
                        if let crate::data::replay_reconcile::FileOutcome::HardFailure = outcome
                            && data.unindexable.insert(&path)
                            && let (Some(pool), Some(rt)) = (data.db_pool.as_ref(), data.tokio_runtime.as_ref())
                            && let Err(e) = rt.block_on(data.unindexable.save(pool))
                        {
                            warn!("failed to persist unindexable replay set: {e}");
                        }
                    }
                }
                ReplayBackgroundParserThreadMessage::DataAutoExportSettingChange(new_data_export_settings) => {
                    data.data_export_settings = new_data_export_settings;
                }
                ReplayBackgroundParserThreadMessage::DebugStateChange(new_debug_state) => {
                    data.is_debug = new_debug_state;
                }
                ReplayBackgroundParserThreadMessage::LiveMatchStarted { replay, build, flush, started_at } => {
                    // A truncated, still-being-written packet stream is the likeliest
                    // panic source on this thread; a panic here must not take
                    // post-battle indexing and uploads down with it for the session.
                    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        crate::task::live_match_stats::resolve_and_fetch(
                            &replay,
                            build,
                            flush,
                            started_at,
                            &data.build_cache,
                            &data.player_tracker,
                            &mut match_stats_client,
                        );
                    }));
                    if let Err(payload) = outcome {
                        let panic_msg = crate::util::thread::panic_payload_to_string(&payload);
                        error!("live match stats scan panicked: {panic_msg}");
                        data.player_tracker.write().set_match_stats_for(
                            started_at,
                            crate::ui::player_tracker::MatchStatsState::Failed(
                                rust_i18n::t!("ui.player_tracker.roster_unavailable").into(),
                            ),
                        );
                    }
                }
            }

            // Deferred: live-match tailing delivers ModifiedReplay at the
            // game's flush rate, and the UI only shows the latest state.
            data.egui_ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
    })
}

#[instrument(skip_all, fields(replay_count = replays.len()))]
pub fn start_populating_player_inspector(
    replays: Vec<PathBuf>,
    build_cache: crate::data::wows_data::BuildDataCache,
    player_tracker: Arc<RwLock<PlayerTracker>>,
) -> BackgroundTask {
    let (tx, rx) = super::completion_channel();
    crate::util::thread::spawn_logged("player-inspector", move || {
        for path in replays {
            match ReplayFile::from_file(&path) {
                Ok(replay_file) => {
                    let replay_version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
                    let Some(wows_data_for_build) = build_cache.resolve(&replay_version) else {
                        warn!(
                            "Skipping replay {:?}: no data for build {}",
                            path,
                            replay_version.build_number().map_or_else(|| "unknown".to_string(), |b| b.to_string())
                        );
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SendAllReplaysBatchResult {
    attempted: ReplayCount,
    sent: ReplayCount,
    total: ReplayCount,
}

pub fn start_send_all_replays_to_shipbuilds(
    paths: BTreeSet<PathBuf>,
    cache_policy: SendReplayCachePolicy,
    deps: ReplayDependencies,
    persisted: SharedPersistedState,
    sent_replays: Arc<RwLock<HashSet<String>>>,
    db_pool: sqlx::SqlitePool,
    tokio_runtime: Arc<tokio::runtime::Runtime>,
) -> BackgroundTask {
    let (completion_tx, completion_rx) = super::completion_channel();
    let (progress_tx, progress_rx) = egui_inbox::UiInbox::channel();
    let total = ReplayCount(paths.len());

    crate::util::thread::spawn_logged("send-replays-to-shipbuilds", move || {
        let _ = progress_tx.send(SendAllReplaysProgress::new(ReplayCount(0), total));
        let result = run_send_all_replays_to_shipbuilds(
            paths,
            cache_policy,
            sent_replays.as_ref(),
            &progress_tx,
            &persisted,
            |path| upload_replay_path_to_shipbuilds(path, &deps, &persisted),
            |path| persist_sent_replay(&tokio_runtime, &db_pool, path),
        );

        let _ = completion_tx.send(Ok(BackgroundTaskCompletion::ReplaysSentToShipBuilds {
            attempted: result.attempted,
            sent: result.sent,
            total: result.total,
        }));
    });

    BackgroundTask {
        receiver: Some(completion_rx),
        kind: BackgroundTaskKind::SendingAllReplaysToShipBuilds { rx: progress_rx, last_progress: None },
    }
}

fn persist_sent_replay(
    tokio_runtime: &tokio::runtime::Runtime,
    db_pool: &sqlx::SqlitePool,
    path: &Path,
) -> Result<(), Report> {
    let path_text = path.to_string_lossy();
    tokio_runtime
        .block_on(wows_toolkit_config::queries::insert_sent_replay(db_pool, path_text.as_ref()))
        .into_report()
        .map_err(|error| error.attach(format!("path: {}", path.display())).into_dynamic())
}

#[allow(dead_code)]
fn upload_replay_path_to_shipbuilds(
    path: &Path,
    deps: &ReplayDependencies,
    persisted: &SharedPersistedState,
) -> ShipBuildsUploadOutcome {
    let loaded = match ReplayLoader::build_replay_from_path(deps, path.to_path_buf()) {
        Ok(replay) => replay,
        Err(error) => {
            error!("failed to load replay for ShipBuilds upload {}: {:?}", path.display(), error);
            return ShipBuildsUploadOutcome::TransientFailure;
        }
    };
    let game_version = loaded.wows_data.read().patch_version;
    let replay = loaded.replay.read();
    let report = match replay.parse(game_version.to_string().as_str()) {
        Ok(report) => report,
        Err(error) => {
            error!("failed to parse replay for ShipBuilds upload {}: {:?}", path.display(), error);
            return ShipBuildsUploadOutcome::TransientFailure;
        }
    };

    upload_parsed_replay(
        path,
        &replay,
        &report,
        replay.resource_loader.as_ref(),
        configured_data_sharing_mode(persisted),
        &deps.shipbuilds_client,
        loaded.bytes,
    )
}

fn configured_data_sharing_mode(persisted: &SharedPersistedState) -> DataSharingMode {
    persisted.read().settings.integrations.data_sharing_mode
}

fn run_send_all_replays_to_shipbuilds<U, P>(
    paths: BTreeSet<PathBuf>,
    cache_policy: SendReplayCachePolicy,
    sent_replays: &RwLock<HashSet<String>>,
    progress_tx: &egui_inbox::UiInboxSender<SendAllReplaysProgress>,
    persisted: &SharedPersistedState,
    mut upload: U,
    mut persist: P,
) -> SendAllReplaysBatchResult
where
    U: FnMut(&Path) -> ShipBuildsUploadOutcome,
    P: FnMut(&Path) -> Result<(), Report>,
{
    let total = ReplayCount(paths.len());
    let mut attempted = ReplayCount(0);
    let mut sent = ReplayCount(0);

    for (index, path) in paths.into_iter().enumerate() {
        let path_text = path.to_string_lossy();
        let ledger_contains = { sent_replays.read().contains(path_text.as_ref()) };
        if cache_policy.should_attempt(ledger_contains) {
            attempted.0 += 1;
            if configured_data_sharing_mode(persisted).shares_anything() {
                let processing = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match upload(&path) {
                    ShipBuildsUploadOutcome::Skipped(_) | ShipBuildsUploadOutcome::TransientFailure => {}
                    ShipBuildsUploadOutcome::Sent => {
                        sent.0 += 1;
                        sent_replays.write().insert(path_text.into_owned());
                        if let Err(error) = persist(&path) {
                            error!("ShipBuilds upload completed but sent-replay persistence failed: {:?}", error);
                        }
                    }
                }));
                if let Err(payload) = processing {
                    error!(
                        "ShipBuilds replay processing panicked for {}: {}; continuing the batch",
                        path.display(),
                        panic_payload_message(payload.as_ref())
                    );
                }
            }
        }

        let _ = progress_tx.send(SendAllReplaysProgress::new(ReplayCount(index + 1), total));
    }

    SendAllReplaysBatchResult { attempted, sent, total }
}

fn panic_payload_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "non-string panic payload".to_owned()
    }
}

/// Handles shared by every stage of an "Index all replays" reconciliation
/// pass: resolving game data per replay build, building UI reports, and
/// writing index rows.
#[derive(Clone)]
pub struct ReconcileIndexDeps {
    pub build_cache: crate::data::wows_data::BuildDataCache,
    pub shipbuilds_client: crate::data::shipbuilds::ShipBuildsClient,
    pub twitch_state: Arc<RwLock<TwitchState>>,
    pub db_pool: sqlx::SqlitePool,
    pub tokio_runtime: Arc<tokio::runtime::Runtime>,
    pub personal_rating_data: Arc<RwLock<crate::util::personal_rating::PersonalRatingData>>,
}

impl ReconcileIndexDeps {
    /// None until the database pool, tokio runtime, and game data have all
    /// finished loading; reconciliation cannot run before then.
    pub fn from_tab_state(tab_state: &crate::tab_state::TabState) -> Option<Self> {
        Some(Self {
            build_cache: tab_state.build_cache.clone()?,
            shipbuilds_client: tab_state.shipbuilds_client.clone(),
            twitch_state: Arc::clone(&tab_state.twitch_state),
            db_pool: tab_state.db_pool.clone()?,
            tokio_runtime: Arc::clone(tab_state.tokio_runtime.as_ref()?),
            personal_rating_data: Arc::clone(&tab_state.personal_rating_data),
        })
    }
}

/// Parse and index a single replay for the on-demand "Index all replays" pass.
///
/// Distinct from `parse_replay_data_in_background`: no upload, no player-tracker
/// update, no data export -- this only produces index rows, so it can run for
/// every historical replay without re-triggering side effects meant for newly
/// finished battles. Indexes any replay that parses successfully, whether or
/// not server battle results are present: results-absent (left-early) replays
/// are indexed with `results_available = false` and NULL server stats (see
/// `replay_index::map_rows`), and are upgraded in place if a later pass
/// re-indexes them once results have landed.
fn index_one_replay(
    path: &Path,
    deps: &ReconcileIndexDeps,
    source_id: crate::db::index::rows::SourceId,
) -> ParseOutcome {
    let replay_file = match ReplayFile::from_file(path) {
        Ok(f) => f,
        Err(e) => {
            error!("failed to parse replay {}: {:?}", path.display(), e);
            return ParseOutcome::HardFailure;
        }
    };

    let replay_version = wowsunpack::data::Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
    let Some(wows_data_for_build) = deps.build_cache.resolve(&replay_version) else {
        warn!(
            "Skipping replay {:?}: no data for build {}",
            path,
            replay_version.build_number().map_or_else(|| "unknown".to_string(), |b| b.to_string())
        );
        return ParseOutcome::Transient;
    };
    let (metadata_provider, game_version, gc) = {
        let wows_data = wows_data_for_build.read();
        (wows_data.game_metadata.clone(), wows_data.patch_version, wows_data.game_constants.clone())
    };
    let Some(metadata_provider) = metadata_provider else {
        return ParseOutcome::Transient;
    };

    let mut replay = Replay::new(replay_file, Arc::clone(&metadata_provider));
    replay.game_constants = Some(gc);
    replay.source_path = Some(path.to_path_buf());

    match replay.parse(game_version.to_string().as_str()) {
        Ok(report) => {
            replay.battle_report = Some(report);
        }
        Err(e)
            if e.downcast_current_context::<ToolkitError>()
                .is_some_and(|e| matches!(e, ToolkitError::ReplayVersionMismatch { .. })) =>
        {
            // Not malformed, just parsed with the wrong build's data. Retry
            // later rather than blacklisting.
            return ParseOutcome::Transient;
        }
        Err(e) => {
            error!("error indexing replay {:?}: {:?}", path, e);
            return ParseOutcome::HardFailure;
        }
    }

    if replay.battle_report.is_none() {
        return ParseOutcome::HardFailure;
    }

    let (dummy_sender, _) = egui_inbox::UiInbox::channel();
    let replay_deps = crate::data::wows_data::ReplayDependencies {
        build_cache: deps.build_cache.clone(),
        shipbuilds_client: deps.shipbuilds_client.clone(),
        twitch_state: Arc::clone(&deps.twitch_state),
        replay_sort: Arc::new(Mutex::new(SortOrder::default())),
        background_task_sender: dummy_sender,
        is_debug_mode: false,
        personal_rating_data: Arc::clone(&deps.personal_rating_data),
        // Dead-ended like the sender: these deps only serve build_ui_report,
        // which queues no background work and wakes nothing.
        egui_ctx: egui::Context::default(),
    };
    replay.build_ui_report(&replay_deps);

    crate::data::replay_index::index_replay_blocking(
        &deps.tokio_runtime,
        &deps.db_pool,
        &replay,
        source_id,
        jiff::Timestamp::now(),
        &deps.personal_rating_data,
    );

    ParseOutcome::ParsedAndSent
}

/// Spawn the on-demand "Index all replays" reconciliation pass.
///
/// This is a focused index-only backfill: it does not reuse the startup scan's
/// loop in `start_background_parsing_thread`, since that loop also drives
/// uploads and player-tracker updates and is entangled with the parser thread's
/// message loop. Instead it walks the replays directory directly and indexes
/// through `index_one_replay`, wrapped in `reconcile_one` for panic isolation
/// exactly like the startup pass.
///
/// When `force_reindex` is false, replays already recorded for the default
/// source are skipped (only gaps are filled). When true, already-indexed
/// replays are re-parsed and re-upserted too, so newly added index columns
/// (e.g. personal rating, disconnect state) backfill onto existing rows.
/// Either way, files in the persistent `Unindexable` blacklist are never
/// re-parsed.
pub fn start_reconcile_index(deps: ReconcileIndexDeps, force_reindex: bool, egui_ctx: egui::Context) -> BackgroundTask {
    let (tx, rx) = super::completion_channel();
    // Throttled: already-indexed files are skipped in a tight loop, so the
    // per-file progress sends can come thousands per second.
    let (progress_tx, progress_rx) =
        crate::ui_channel::throttled_channel(egui_ctx, std::time::Duration::from_millis(250));

    crate::util::thread::spawn_logged("reconcile-index", move || {
        let _ = tx.send(run_reconcile_index(deps, force_reindex, &progress_tx));
    });

    BackgroundTask {
        receiver: Some(rx),
        kind: BackgroundTaskKind::ReconcilingIndex { rx: progress_rx, last_progress: None },
    }
}

fn run_reconcile_index(
    deps: ReconcileIndexDeps,
    force_reindex: bool,
    progress_tx: &crate::ui_channel::ThrottledSender<IndexProgress>,
) -> Result<BackgroundTaskCompletion, Report> {
    let Some(replays_dir) = deps.build_cache.loaded_builds().first().map(|d| d.read().replays_dir.clone()) else {
        return Err(report!("no game data loaded, cannot enumerate replays directory"));
    };

    let now = jiff::Timestamp::now();
    let source_id = deps
        .tokio_runtime
        .block_on(crate::db::index::query::ensure_default_source(&deps.db_pool, &replays_dir, now))
        .map_err(|e| report!("failed to resolve replay index source: {e}"))?;

    let indexed_paths: HashSet<String> = deps
        .tokio_runtime
        .block_on(crate::db::index::query::record_paths_in_source(&deps.db_pool, source_id))
        .unwrap_or_default();
    let mut unindexable = deps.tokio_runtime.block_on(crate::data::replay_reconcile::Unindexable::load(&deps.db_pool));

    let files = replay_filepaths(&replays_dir).unwrap_or_default();
    let total = files.len();
    let mut indexed_count = 0usize;
    let mut unindexable_dirty = false;

    for (done, path) in files.into_iter().enumerate() {
        let _ = progress_tx.send(IndexProgress { done: done as u64, total: total as u64 });

        let path_str = path.to_string_lossy();
        if unindexable.contains(&path) {
            continue;
        }
        let already_indexed = !force_reindex && indexed_paths.contains(path_str.as_ref());

        // Upload reconciliation is satisfied: this task has no upload ledger of its own, so
        // the skip decision depends only on whether the replay is already indexed
        // (which `force_reindex` short-circuits to false so every non-blacklisted
        // file is re-parsed and re-upserted).
        let outcome = crate::data::replay_reconcile::reconcile_one(
            &path,
            already_indexed,
            crate::data::replay_reconcile::UploadReconciliation::Satisfied,
            std::panic::AssertUnwindSafe(|| index_one_replay(&path, &deps, source_id)),
        );

        match outcome {
            crate::data::replay_reconcile::FileOutcome::Parsed { .. } => indexed_count += 1,
            crate::data::replay_reconcile::FileOutcome::HardFailure => {
                if unindexable.insert(&path) {
                    unindexable_dirty = true;
                }
            }
            crate::data::replay_reconcile::FileOutcome::Transient
            | crate::data::replay_reconcile::FileOutcome::Skipped => {}
        }
    }

    let _ = progress_tx.send(IndexProgress { done: total as u64, total: total as u64 });

    if unindexable_dirty && let Err(e) = deps.tokio_runtime.block_on(unindexable.save(&deps.db_pool)) {
        warn!("failed to persist unindexable replay set: {e}");
    }

    Ok(BackgroundTaskCompletion::ReconcileIndexComplete { indexed: indexed_count, total })
}

/// Which source a summary load should read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceSelector {
    /// Resolve the live source, yielding an empty map when the indexer has not
    /// created it yet. Readers never create it.
    Live,
    /// A source already known to the caller.
    Explicit(crate::db::index::rows::SourceId),
}

/// Load the replay-listing row summaries for `selector`, tagged with `workspace`
/// so the completion can be routed to the listing that asked for it.
///
/// Resolving the source and running the query both happen on this thread; the
/// UI never blocks on the pool.
pub fn start_load_row_summaries(
    pool: sqlx::SqlitePool,
    tokio_runtime: Arc<tokio::runtime::Runtime>,
    selector: SourceSelector,
    workspace: crate::db::index::rows::WorkspaceId,
    generation: u64,
) -> BackgroundTask {
    let (tx, rx) = super::completion_channel();

    crate::util::thread::spawn_logged("load-row-summaries", move || {
        let result = tokio_runtime.block_on(async {
            let source = match selector {
                SourceSelector::Live => match crate::db::index::query::live_source_id(&pool).await? {
                    Some(id) => id,
                    None => return Ok(HashMap::new()),
                },
                SourceSelector::Explicit(id) => id,
            };
            crate::db::index::query::row_summaries_for_source(&pool, source).await
        });

        let completion = match result {
            Ok(summaries) => Ok(BackgroundTaskCompletion::RowSummariesLoaded { summaries, generation, workspace }),
            Err(e) => Err(rootcause::report!("failed to load replay row summaries: {e}")),
        };
        let _ = tx.send(completion);
    });

    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::LoadingRowSummaries { workspace } }
}

/// Resolve or create the index source for a scanned directory, and build a
/// [`Replay`] per file the scan grouped, for the workspace it was opened as.
///
/// The scan grouped those files by build, and the read visits one group at a
/// time: a dropped folder routinely spans game versions, the in-memory data map
/// holds only a couple of non-main builds at once, and resolving per replay
/// would evict and reload a build between neighbouring files.
pub fn start_read_directory(
    deps: crate::data::wows_data::ReplayDependencies,
    pool: sqlx::SqlitePool,
    tokio_runtime: Arc<tokio::runtime::Runtime>,
    workspace: crate::db::index::rows::WorkspaceId,
    scan: Box<crate::task::scan::DirectoryScan>,
) -> BackgroundTask {
    let (tx, rx) = super::completion_channel();
    // Throttled: the walk reports per file read, which is fast enough on a
    // large directory to repaint the listing at the disk's pace.
    let (update_tx, update_rx) =
        crate::ui_channel::throttled_channel(deps.egui_ctx.clone(), std::time::Duration::from_millis(250));

    crate::util::thread::spawn_logged("read-directory", move || {
        // The box carried the scan through the task channel; the read itself
        // owns it outright.
        let _ = tx.send(run_read_directory(deps, pool, tokio_runtime, workspace, *scan, &update_tx));
    });

    BackgroundTask { receiver: Some(rx), kind: BackgroundTaskKind::IngestingDirectory { workspace, rx: update_rx } }
}

/// How far a directory walk has got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestProgress {
    /// Files visited, whether they loaded or not.
    pub done: usize,
    /// Files the walk found before it started reading any of them.
    pub total: usize,
}

/// Which build's data is loading, and where it falls in the run. Named because
/// the position and the count are both `usize` and mean opposite things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildLoadPosition {
    pub index: usize,
    pub count: usize,
}

/// What an open directory is doing, reported above its listing.
#[derive(Debug, Clone, PartialEq)]
pub enum IngestStage {
    /// Reading each replay's header to count them and group them by build.
    Scanning(IngestProgress),
    /// Fetching game data for builds this machine does not have.
    Downloading(super::DownloadProgress),
    /// Loading one build's game data. Open-ended: the cost is a GameParams
    /// decode with no intermediate count to report.
    LoadingData { build: crate::task::BuildRequest, position: BuildLoadPosition },
    /// Reading and indexing the replays themselves.
    Reading(IngestProgress),
}

impl IngestStage {
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Scanning(_) => "ui.replay.listing_scanning",
            Self::Downloading(_) => "ui.replay.listing_downloading",
            Self::LoadingData { .. } => "ui.replay.listing_loading_data",
            Self::Reading(_) => "ui.replay.listing_reading",
        }
    }

    /// How far this stage has got, or `None` when it has nothing to report.
    pub fn fraction(&self) -> Option<f32> {
        let (done, total) = match self {
            Self::Scanning(progress) | Self::Reading(progress) => (progress.done as f64, progress.total as f64),
            Self::Downloading(progress) => (progress.downloaded as f64, progress.total as f64),
            Self::LoadingData { .. } => return None,
        };
        // A stage yet to find anything has not finished; dividing by its zero
        // total would report it as complete.
        Some(if total > 0.0 { (done / total) as f32 } else { 0.0 })
    }
}

/// One slice of a directory walk, delivered while the walk is still running so
/// the listing fills as replays are read rather than when the last one is done.
pub struct IngestBatch {
    /// The listing this slice belongs to. A batch whose workspace has closed is
    /// dropped: it belongs to nothing else.
    pub workspace: crate::db::index::rows::WorkspaceId,
    /// The index source the walk resolved, carried on every batch so the
    /// listing reads its row summaries against the right source from the first
    /// replay onwards rather than waiting for the walk to finish.
    pub source: crate::db::index::rows::SourceId,
    /// Only the metadata each row draws. A hydrated `Replay` pins its build's
    /// game data and packet stream, so the walk hands the listing the fields it
    /// needs and lets the read it made go.
    pub replays: HashMap<PathBuf, Arc<ListedReplay>>,
    pub progress: IngestProgress,
}

/// What a running directory walk sends the listing it is filling.
pub enum IngestUpdate {
    /// The files the walk found, sent before it reads any of them so entries
    /// the listing holds for replays that are no longer on disk leave with the
    /// files they name.
    Walked { workspace: crate::db::index::rows::WorkspaceId, paths: HashSet<PathBuf> },
    /// Replays read since the last update, with how far the walk has got.
    Batch(IngestBatch),
    /// Which stage the run has moved into, and how far that stage has got. A
    /// run of files the walk cannot read carries no replay, and would otherwise
    /// leave the count standing still, which is what a stalled walk looks like.
    Stage { workspace: crate::db::index::rows::WorkspaceId, stage: IngestStage },
}

/// Replays held back before a batch goes to the UI. Sized so a directory of
/// thousands of files does not wake the UI once per replay.
const INGEST_BATCH_SIZE: usize = 16;

/// How long a partial batch waits before going out anyway. Reading and indexing
/// a replay is slow enough that a size-only rule would hold the first rows back
/// for a noticeable time on a large directory.
const INGEST_FLUSH_INTERVAL: Duration = Duration::from_millis(500);

/// What the walk owes the UI at one point in its loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flush {
    /// Nothing: too few replays are held back to fill a batch, and the interval
    /// has not elapsed.
    Hold,
    /// The replays held back so far, carrying the walk's progress.
    Batch,
    /// The walk's progress on its own, because nothing has been read since the
    /// last update went out.
    Progress,
}

/// What the walk should send now.
fn flush_now(pending: usize, since_last_flush: Duration) -> Flush {
    if pending >= INGEST_BATCH_SIZE {
        Flush::Batch
    } else if since_last_flush < INGEST_FLUSH_INTERVAL {
        Flush::Hold
    } else if pending > 0 {
        Flush::Batch
    } else {
        Flush::Progress
    }
}

/// Why replays in a directory could not be loaded. Missing game data is
/// actionable -- the toolkit can offer to fetch it -- so it is tracked
/// separately from failures the user can do nothing about.
#[derive(Debug, Default, Clone)]
pub struct IngestFailures {
    pub missing_builds: BTreeMap<crate::task::BuildRequest, usize>,
    pub unreadable: usize,
    /// Replays that loaded and are listed, but whose index rows could not be
    /// written. Their rows fall back to the not-indexed placeholder, so they
    /// are worth reporting, but they are not missing from the listing and so
    /// are counted apart from the replays that never loaded.
    pub not_indexed: usize,
}

/// Rebuild the request a replay was asking for from what its failure carried.
///
/// The error reports the build and the `major.minor.patch` it came from as
/// text, which is what the download offer and the game-data repository are
/// keyed on. `None` when neither can be recovered, since nothing can be
/// offered for a build that cannot be named.
fn build_request_from_failure(build: u32, version: &str) -> Option<crate::task::BuildRequest> {
    let mut parts = version.split('.').filter_map(|part| part.trim().parse::<u32>().ok());
    crate::task::BuildRequest::new(Version {
        major: parts.next()?,
        // A version reported as `13` or `13.5` names the parts it leaves out as
        // zero, which is how the repository publishes them.
        minor: parts.next().unwrap_or(0),
        patch: parts.next().unwrap_or(0),
        build: std::num::NonZeroU32::new(build),
    })
}

impl IngestFailures {
    /// Attribute one failed replay. Anything that is not a recognisably
    /// missing build is counted as unreadable, since nothing can be offered
    /// for it.
    pub fn record(&mut self, report: &Report) {
        match report.downcast_current_context::<ToolkitError>() {
            Some(ToolkitError::ReplayBuildUnavailable { build, version, .. }) => {
                match build_request_from_failure(*build, version) {
                    Some(request) => *self.missing_builds.entry(request).or_default() += 1,
                    // A build nothing can be requested for is a replay nothing
                    // can be done about, which is what unreadable means here.
                    None => self.unreadable += 1,
                }
            }
            _ => self.unreadable += 1,
        }
    }

    /// Fold one group's failures into the directory's total.
    fn merge(&mut self, other: Self) {
        for (request, count) in other.missing_builds {
            *self.missing_builds.entry(request).or_default() += count;
        }
        self.unreadable += other.unreadable;
        self.not_indexed += other.not_indexed;
    }

    /// Attribute one replay whose read panicked. A recovered panic carries no
    /// report to classify, and nothing can be offered for it, so it counts as
    /// unreadable.
    pub fn record_panic(&mut self) {
        self.unreadable += 1;
    }

    /// Attribute one replay that is listed but could not be indexed.
    pub fn record_index_failure(&mut self) {
        self.not_indexed += 1;
    }

    /// Every replay the directory holds that did not load, for either reason.
    pub fn total(&self) -> usize {
        self.unreadable + self.missing_builds.values().sum::<usize>()
    }
}

/// A replay the walk read, with the build number of the game data its client
/// version resolved to. Indexing parses the replay against that build, and the
/// read has resolved it already.
struct ReadReplay {
    replay: Arc<RwLock<Replay>>,
    game_build: usize,
}

/// What a walk leaves behind once the replays it read have been sent.
#[derive(Debug, Default)]
struct WalkOutcome {
    failures: IngestFailures,
    /// Replays whose indexing panicked. The parse behind the panic is
    /// deterministic and expensive, so these are worth remembering rather than
    /// paying again on the next walk of the same directory.
    index_panics: Vec<PathBuf>,
    /// Set when an update could not be sent. The listing this was filling is
    /// gone, so the build groups after it have nobody to fill either, and each
    /// of those costs a game-data load and a parse per replay.
    channel_closed: bool,
}

impl WalkOutcome {
    /// Fold one group's result into the directory read's.
    fn absorb(&mut self, group: Self) {
        self.failures.merge(group.failures);
        self.index_panics.extend(group.index_panics);
        self.channel_closed |= group.channel_closed;
    }
}

/// The one build group a walk covers: the listing its replays belong to, and
/// where its files fall in the directory read they are part of.
#[derive(Debug, Clone, Copy)]
struct WalkSlice {
    workspace: crate::db::index::rows::WorkspaceId,
    source: crate::db::index::rows::SourceId,
    /// The directory's replays visited before this group. A count restarting at
    /// each build boundary reads as the run starting over.
    offset: usize,
    /// The replays the whole read stage will visit.
    total: usize,
}

/// What a directory read does with one build group.
///
/// These are the steps that touch the game data, the filesystem and the
/// database. They are parameters so the order the groups are visited in, the
/// counter that spans them and the skip branch are exercisable without any of
/// the three.
struct BuildSteps<'a, D, L, W> {
    /// Whether this machine has the build's data, without loading it.
    has_data: &'a D,
    /// Load the build's data, reporting whether it is now resident.
    load: &'a L,
    /// Read one build group's replays.
    walk: &'a W,
}

/// Read the replays the scan grouped, one build at a time.
///
/// Ascending build order and one group at a time are both load-bearing: the
/// data map holds only a couple of non-main builds at once, so a group split in
/// two would evict and reload its build, and resolution bridges constants
/// forward from an already-loaded older build, which requires the older build to
/// have been visited first.
fn read_build_groups<D, L, W>(
    scan: &crate::task::scan::DirectoryScan,
    workspace: crate::db::index::rows::WorkspaceId,
    source: crate::db::index::rows::SourceId,
    steps: BuildSteps<'_, D, L, W>,
    tx: &crate::ui_channel::ThrottledSender<IngestUpdate>,
) -> WalkOutcome
where
    D: Fn(&crate::task::BuildRequest) -> bool,
    L: Fn(&crate::task::BuildRequest) -> bool,
    W: Fn(Vec<PathBuf>, WalkSlice) -> WalkOutcome,
{
    let build_count = scan.by_build.len();
    // Not scan.total: the unreadable files are never opened again, so a bar over
    // total would stop short of full on any directory holding one.
    let to_read = scan.to_read();
    let mut outcome = WalkOutcome::default();
    let mut done = 0usize;

    // The scan already found these files; entries for replays no longer on disk
    // left with the scan's `Walked` update.
    for (position, (request, paths)) in scan.read_order().enumerate() {
        // The announce is cosmetic: it names the wait the load is about to be,
        // so a build with nothing to load has no wait to name. The load below
        // runs either way, and it alone decides what is read. Do not collapse
        // these -- gating the load on this check would put a cheap existence
        // test in charge of which replays reach the listing, and a check that
        // grew stricter than resolution would drop replays that load fine, with
        // nothing said anywhere.
        if (steps.has_data)(request) {
            let announced = tx.send(IngestUpdate::Stage {
                workspace,
                stage: IngestStage::LoadingData {
                    build: request.clone(),
                    position: BuildLoadPosition { index: position, count: build_count },
                },
            });
            if announced.is_err() {
                outcome.channel_closed = true;
                break;
            }
        }

        // Loading here, once per group, is what keeps this build's data
        // resident for every replay under it.
        if !(steps.load)(request) {
            // Its download failed or was declined. Its replays stay unread and
            // the rest of the directory is unaffected, so the count moves over
            // them rather than stopping short of full.
            *outcome.failures.missing_builds.entry(request.clone()).or_default() += paths.len();
            done += paths.len();
            let progress = IngestProgress { done, total: to_read };
            if tx.send(IngestUpdate::Stage { workspace, stage: IngestStage::Reading(progress) }).is_err() {
                outcome.channel_closed = true;
                break;
            }
            continue;
        }

        let slice = WalkSlice { workspace, source, offset: done, total: to_read };
        outcome.absorb((steps.walk)(paths.to_vec(), slice));
        done += paths.len();
        if outcome.channel_closed {
            break;
        }
    }

    // Every file the scan could not read is one the listing will not show.
    // The scan already classified why; the listing only reports the count.
    outcome.failures.unreadable += scan.unreadable.len();
    outcome
}

/// Read every path in `paths`, sending the replays that load as batches and
/// keeping the progress count moving through the ones that do not.
///
/// `read` and `index` are the steps that touch the filesystem, the game data
/// and the database. They are parameters so the gating, ordering and failure
/// handling around them are exercisable without any of the three.
fn ingest_walk<N, R, I>(
    paths: Vec<PathBuf>,
    slice: WalkSlice,
    needs_index: &N,
    read: &R,
    index: &I,
    tx: &crate::ui_channel::ThrottledSender<IngestUpdate>,
) -> WalkOutcome
where
    N: Fn(&Path) -> bool,
    R: Fn(&Path) -> Result<ReadReplay, Report>,
    I: Fn(&ReadReplay, crate::db::index::rows::SourceId) -> Result<(), Report>,
{
    let WalkSlice { workspace, source, offset, total } = slice;
    let mut outcome = WalkOutcome::default();
    let mut pending: HashMap<PathBuf, Arc<ListedReplay>> = HashMap::new();
    let mut last_flush = std::time::Instant::now();
    let group_size = paths.len();

    for (visited, path) in paths.into_iter().enumerate() {
        // One unreadable, malformed or version-orphaned file must cost only
        // itself. An uncaught parser panic would take this thread with it and
        // leave the rest of the directory unread.
        let read_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read(&path)));

        match read_result {
            Ok(Ok(built)) => {
                if needs_index(&path) {
                    // Indexing parses, so it carries the same panic risk the
                    // read does, and a replay that will not index is still
                    // worth listing.
                    let indexed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| index(&built, source)));
                    match indexed {
                        Ok(Ok(())) => {}
                        Ok(Err(e)) => {
                            outcome.failures.record_index_failure();
                            warn!("listing replay {} without an index row: {e}", path.display());
                        }
                        Err(payload) => {
                            outcome.failures.record_index_failure();
                            outcome.index_panics.push(path.clone());
                            let msg = crate::util::thread::panic_payload_to_string(&payload);
                            warn!("panic while indexing replay {}, listing it anyway: {msg}", path.display());
                        }
                    }
                }
                let listed = ListedReplay::from_meta(&built.replay.read().replay_file.meta);
                pending.insert(path, Arc::new(listed));
            }
            Ok(Err(e)) => {
                outcome.failures.record(&e);
                warn!("skipping replay {}: {e}", path.display());
            }
            Err(payload) => {
                outcome.failures.record_panic();
                let msg = crate::util::thread::panic_payload_to_string(&payload);
                warn!("panic while reading replay {}, skipping it: {msg}", path.display());
            }
        }

        let progress = IngestProgress { done: offset + visited + 1, total };
        let update = match flush_now(pending.len(), last_flush.elapsed()) {
            Flush::Hold => continue,
            Flush::Batch => {
                IngestUpdate::Batch(IngestBatch { workspace, source, replays: std::mem::take(&mut pending), progress })
            }
            Flush::Progress => IngestUpdate::Stage { workspace, stage: IngestStage::Reading(progress) },
        };
        if tx.send(update).is_err() {
            // Nothing is listening any more: the workspace closed or the app is
            // shutting down. The rest of the walk has no reader.
            outcome.channel_closed = true;
            return outcome;
        }
        last_flush = std::time::Instant::now();
    }

    let progress = IngestProgress { done: offset + group_size, total };
    let last = if pending.is_empty() {
        IngestUpdate::Stage { workspace, stage: IngestStage::Reading(progress) }
    } else {
        IngestUpdate::Batch(IngestBatch { workspace, source, replays: pending, progress })
    };
    if tx.send(last).is_err() {
        outcome.channel_closed = true;
    }

    outcome
}

fn run_read_directory(
    deps: crate::data::wows_data::ReplayDependencies,
    pool: sqlx::SqlitePool,
    tokio_runtime: Arc<tokio::runtime::Runtime>,
    workspace: crate::db::index::rows::WorkspaceId,
    scan: crate::task::scan::DirectoryScan,
    update_tx: &crate::ui_channel::ThrottledSender<IngestUpdate>,
) -> Result<BackgroundTaskCompletion, Report> {
    let root = &scan.root;
    let source = tokio_runtime
        .block_on(crate::db::index::query::ensure_source(
            &pool,
            &crate::ui::replay_parser::shorten_root(root),
            crate::db::index::rows::SourceKind::ImportedDir,
            root,
            jiff::Timestamp::now(),
        ))
        .map_err(|e| report!("failed to resolve replay index source for {}: {e}", root.display()))?;

    // The paths this source already has index rows for. Re-opening a directory
    // then costs a read per replay instead of a re-parse of the whole tree.
    let indexed_paths: HashSet<String> =
        tokio_runtime.block_on(crate::db::index::query::record_paths_in_source(&pool, source)).unwrap_or_default();

    // The replays a previous run found to be un-parseable. Their parse panics
    // reliably, so indexing them again costs the walk and yields nothing.
    let mut unindexable = tokio_runtime.block_on(crate::data::replay_reconcile::Unindexable::load(&pool));

    let read = |path: &Path| -> Result<ReadReplay, Report> {
        // The scan opened every one of these files. The retry loop in
        // `build_replay_from_path` answers the live watcher racing the game
        // flushing a replay, a race a directory read does not have.
        let (replay, wows_data) =
            crate::data::wows_data::ReplayLoader::build_replay_from_existing_file(&deps, path.to_path_buf())?;
        let game_build = wows_data.read().patch_version;
        Ok(ReadReplay { replay, game_build })
    };
    let index = |replay: &ReadReplay, source| index_ingested_replay(replay, &deps, &pool, &tokio_runtime, source);
    let needs_index =
        |path: &Path| !indexed_paths.contains(path.to_string_lossy().as_ref()) && !unindexable.contains(path);

    let has_data = |request: &crate::task::BuildRequest| deps.build_cache.has_data_for(request);
    let load = |request: &crate::task::BuildRequest| deps.build_cache.resolve(&request.version()).is_some();
    let walk = |paths, slice| ingest_walk(paths, slice, &needs_index, &read, &index, update_tx);

    let steps = BuildSteps { has_data: &has_data, load: &load, walk: &walk };
    let WalkOutcome { failures, index_panics, channel_closed: _ } =
        read_build_groups(&scan, workspace, source, steps, update_tx);

    let mut unindexable_dirty = false;
    for path in &index_panics {
        unindexable_dirty |= unindexable.insert(path);
    }
    if unindexable_dirty && let Err(e) = tokio_runtime.block_on(unindexable.save(&pool)) {
        warn!("failed to persist unindexable replay set: {e}");
    }

    if failures.total() > 0 {
        warn!(
            "ingest of {} skipped {} replay(s): {} unreadable, {} awaiting {} missing build(s)",
            root.display(),
            failures.total(),
            failures.unreadable,
            failures.missing_builds.values().sum::<usize>(),
            failures.missing_builds.len(),
        );
    }
    if failures.not_indexed > 0 {
        warn!("ingest of {} listed {} replay(s) it could not index", root.display(), failures.not_indexed);
    }

    Ok(BackgroundTaskCompletion::DirectoryIngested { workspace, source, failures })
}

/// Parse `replay` and write its index rows, so its listing row carries the same
/// damage, kills, division and outcome a live-parsed replay's does.
///
/// The parsed reports are dropped again once the rows are written: a directory
/// can hold thousands of replays, and a battle report per listed replay would
/// hold far more memory than the listing needs. The row data now lives in the
/// index, which is where the listing reads it from.
///
/// A parse that panics drops them too: the caller lists the replay either way,
/// and a listed replay holding a report both costs that memory and reads as
/// having battle results still pending.
fn index_ingested_replay(
    replay: &ReadReplay,
    deps: &crate::data::wows_data::ReplayDependencies,
    pool: &sqlx::SqlitePool,
    tokio_runtime: &tokio::runtime::Runtime,
    source: crate::db::index::rows::SourceId,
) -> Result<(), Report> {
    let mut guard = replay.replay.write();
    let expected_build = replay.game_build.to_string();

    reset_after(
        &mut *guard,
        |replay| {
            let report = replay.parse(&expected_build)?;
            replay.battle_report = Some(report);
            replay.build_ui_report(deps);
            crate::data::replay_index::index_replay_reporting(
                tokio_runtime,
                pool,
                replay,
                source,
                jiff::Timestamp::now(),
                &deps.personal_rating_data,
            )
        },
        |replay| {
            replay.battle_report = None;
            replay.ui_report = None;
        },
    )
}

/// Run `work` over `value` and `reset` it however `work` ended, panic included,
/// then hand the caller what `work` produced or the panic it raised.
///
/// The parse behind an indexing step panics on some replays, and the caller
/// lists the replay either way. Leaving the reset to the returning path would
/// list it holding everything the parse attached.
fn reset_after<V, T>(value: &mut V, work: impl FnOnce(&mut V) -> T, reset: impl FnOnce(&mut V)) -> T {
    let produced = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(value)));
    reset(value);
    match produced {
        Ok(produced) => produced,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    mod send_all {
        use std::collections::HashSet;

        use super::*;
        use crate::task::replay_upload::ReplayCount;
        use crate::task::replay_upload::SendAllReplaysProgress;
        use crate::task::replay_upload::SendReplayCachePolicy;
        use crate::task::replay_upload::ShipBuildsUploadOutcome;

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum TestOutcome {
            Skipped,
            Sent,
            TransientFailure,
        }

        struct TestBatchResult {
            attempted: ReplayCount,
            sent: ReplayCount,
            progress: Vec<SendAllReplaysProgress>,
            sent_paths: HashSet<String>,
            persisted_paths: Vec<PathBuf>,
        }

        #[cfg(feature = "logging")]
        #[derive(Clone, Default)]
        struct CapturedLog(Arc<std::sync::Mutex<Vec<u8>>>);

        #[cfg(feature = "logging")]
        impl std::io::Write for CapturedLog {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap_or_else(|error| error.into_inner()).extend_from_slice(buf);
                Ok(buf.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        #[cfg(feature = "logging")]
        impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CapturedLog {
            type Writer = Self;

            fn make_writer(&'a self) -> Self::Writer {
                self.clone()
            }
        }

        #[cfg(feature = "logging")]
        fn captured_log(body: impl FnOnce()) -> String {
            let captured = CapturedLog::default();
            let subscriber =
                tracing_subscriber::fmt().with_writer(captured.clone()).with_ansi(false).without_time().finish();
            tracing::subscriber::with_default(subscriber, body);
            let bytes = captured.0.lock().unwrap_or_else(|error| error.into_inner()).clone();
            String::from_utf8_lossy(&bytes).into_owned()
        }

        fn path(name: &str) -> PathBuf {
            PathBuf::from(name)
        }

        fn progress(completed: usize, total: usize) -> SendAllReplaysProgress {
            SendAllReplaysProgress::new(ReplayCount(completed), ReplayCount(total))
        }

        fn ledger<const N: usize>(paths: [&str; N]) -> Arc<RwLock<HashSet<String>>> {
            Arc::new(RwLock::new(paths.into_iter().map(str::to_owned).collect()))
        }

        fn sharing(mode: DataSharingMode) -> SharedPersistedState {
            let persisted = Arc::new(crate::tab_state::TrackedPersistedState::default());
            persisted.write().settings.integrations.data_sharing_mode = mode;
            persisted
        }

        fn run_test_batch<F>(
            paths: BTreeSet<PathBuf>,
            policy: SendReplayCachePolicy,
            sent_replays: Arc<RwLock<HashSet<String>>>,
            mut upload: F,
        ) -> TestBatchResult
        where
            F: FnMut(&Path) -> TestOutcome,
        {
            let (progress_tx, progress_rx) = egui_inbox::UiInbox::channel();
            let mut persisted_paths = Vec::new();
            let persisted = sharing(DataSharingMode::Replays);
            let completion = run_send_all_replays_to_shipbuilds(
                paths,
                policy,
                sent_replays.as_ref(),
                &progress_tx,
                &persisted,
                |path| match upload(path) {
                    TestOutcome::Skipped => ShipBuildsUploadOutcome::Skipped(
                        crate::task::replay_upload::ReplayUploadSkipReason::IneligibleGameType,
                    ),
                    TestOutcome::Sent => ShipBuildsUploadOutcome::Sent,
                    TestOutcome::TransientFailure => ShipBuildsUploadOutcome::TransientFailure,
                },
                |path| {
                    persisted_paths.push(path.to_path_buf());
                    Ok(())
                },
            );
            drop(progress_tx);

            TestBatchResult {
                attempted: completion.attempted,
                sent: completion.sent,
                progress: progress_rx.read_without_ctx().collect(),
                sent_paths: sent_replays.read().clone(),
                persisted_paths,
            }
        }

        #[test]
        fn batch_advances_for_cached_skipped_failed_and_sent_paths() {
            let paths = BTreeSet::from([path("cached"), path("failed"), path("sent")]);
            let result = run_test_batch(paths, SendReplayCachePolicy::UseLedger, ledger(["cached"]), |path| {
                if path.ends_with("failed") { TestOutcome::TransientFailure } else { TestOutcome::Sent }
            });

            assert_eq!(result.attempted, ReplayCount(2));
            assert_eq!(result.sent, ReplayCount(1));
            assert_eq!(result.progress, vec![progress(1, 3), progress(2, 3), progress(3, 3)]);
            assert_eq!(result.sent_paths, HashSet::from(["cached".to_owned(), "sent".to_owned()]));
        }

        #[test]
        fn ignore_ledger_attempts_and_records_an_already_cached_path() {
            let result = run_test_batch(
                BTreeSet::from([path("cached")]),
                SendReplayCachePolicy::IgnoreLedger,
                ledger(["cached"]),
                |_| TestOutcome::Sent,
            );

            assert_eq!(result.attempted, ReplayCount(1));
            assert_eq!(result.sent, ReplayCount(1));
            assert_eq!(result.progress.last(), Some(&progress(1, 1)));
            assert_eq!(result.sent_paths, HashSet::from(["cached".to_owned()]));
            assert_eq!(result.persisted_paths, vec![path("cached")]);
        }

        #[test]
        fn ineligible_path_advances_without_entering_the_sent_ledger() {
            let result = run_test_batch(
                BTreeSet::from([path("ineligible")]),
                SendReplayCachePolicy::UseLedger,
                ledger([]),
                |_| TestOutcome::Skipped,
            );

            assert_eq!(result.attempted, ReplayCount(1));
            assert_eq!(result.sent, ReplayCount(0));
            assert_eq!(result.progress, vec![progress(1, 1)]);
            assert!(result.sent_paths.is_empty());
        }

        #[test]
        fn persistence_failure_is_attempted_and_does_not_stop_progress() {
            let sent_replays = ledger([]);
            let (progress_tx, progress_rx) = egui_inbox::UiInbox::channel();
            let persistence_attempts = std::cell::Cell::new(0);
            let persisted = sharing(DataSharingMode::Replays);
            let completion = run_send_all_replays_to_shipbuilds(
                BTreeSet::from([path("sent")]),
                SendReplayCachePolicy::UseLedger,
                sent_replays.as_ref(),
                &progress_tx,
                &persisted,
                |_| ShipBuildsUploadOutcome::Sent,
                |_| {
                    persistence_attempts.set(persistence_attempts.get() + 1);
                    Err(report!("database unavailable"))
                },
            );
            drop(progress_tx);

            assert_eq!(completion.attempted, ReplayCount(1));
            assert_eq!(completion.sent, ReplayCount(1));
            assert_eq!(progress_rx.read_without_ctx().last(), Some(progress(1, 1)));
            assert_eq!(persistence_attempts.get(), 1);
            assert_eq!(&*sent_replays.read(), &HashSet::from(["sent".to_owned()]));
        }

        #[test]
        fn disconnected_progress_receiver_does_not_stop_the_batch() {
            let sent_replays = ledger([]);
            let (progress_tx, progress_rx) = egui_inbox::UiInbox::channel();
            drop(progress_rx);
            let persisted = sharing(DataSharingMode::Replays);

            let completion = run_send_all_replays_to_shipbuilds(
                BTreeSet::from([path("first"), path("second")]),
                SendReplayCachePolicy::UseLedger,
                sent_replays.as_ref(),
                &progress_tx,
                &persisted,
                |_| ShipBuildsUploadOutcome::Sent,
                |_| Ok(()),
            );

            assert_eq!(completion.attempted, ReplayCount(2));
            assert_eq!(completion.sent, ReplayCount(2));
            assert_eq!(sent_replays.read().len(), 2);
        }

        #[test]
        fn a_panicking_replay_does_not_abort_the_remaining_batch() {
            let result = crate::test_utils::with_silenced_panic_hook(|| {
                run_test_batch(
                    BTreeSet::from([path("panic"), path("sent")]),
                    SendReplayCachePolicy::UseLedger,
                    ledger([]),
                    |path| {
                        if path.ends_with("panic") {
                            panic!("bad replay");
                        }
                        TestOutcome::Sent
                    },
                )
            });
            assert_eq!(result.attempted, ReplayCount(2));
            assert_eq!(result.sent, ReplayCount(1));
            assert_eq!(result.progress, vec![progress(1, 2), progress(2, 2)]);
            assert_eq!(result.sent_paths, HashSet::from(["sent".to_owned()]));
        }

        #[test]
        #[cfg(feature = "logging")]
        fn a_panicking_replay_logs_its_path_and_payload() {
            let log = crate::test_utils::with_silenced_panic_hook(|| {
                captured_log(|| {
                    let (progress_tx, _inbox) = egui_inbox::UiInbox::channel();
                    let persisted = sharing(DataSharingMode::Replays);
                    run_send_all_replays_to_shipbuilds(
                        BTreeSet::from([path("panic-payload.wowsreplay")]),
                        SendReplayCachePolicy::UseLedger,
                        ledger([]).as_ref(),
                        &progress_tx,
                        &persisted,
                        |_| panic!("payload evidence"),
                        |_| Ok(()),
                    );
                })
            });

            assert!(log.contains("panic-payload.wowsreplay"), "{log}");
            assert!(log.contains("payload evidence"), "{log}");
        }

        #[test]
        fn a_panicking_persistence_attempt_does_not_abort_the_remaining_batch() {
            let sent_replays = ledger([]);
            let (progress_tx, progress_rx) = egui_inbox::UiInbox::channel();
            let persisted = sharing(DataSharingMode::Replays);
            let completion = crate::test_utils::with_silenced_panic_hook(|| {
                run_send_all_replays_to_shipbuilds(
                    BTreeSet::from([path("first"), path("second")]),
                    SendReplayCachePolicy::UseLedger,
                    sent_replays.as_ref(),
                    &progress_tx,
                    &persisted,
                    |_| ShipBuildsUploadOutcome::Sent,
                    |path| {
                        if path.ends_with("first") {
                            panic!("persistence panic");
                        }
                        Ok(())
                    },
                )
            });
            drop(progress_tx);

            assert_eq!(completion.attempted, ReplayCount(2));
            assert_eq!(completion.sent, ReplayCount(2));
            assert_eq!(progress_rx.read_without_ctx().collect::<Vec<_>>(), vec![progress(1, 2), progress(2, 2)]);
            assert_eq!(&*sent_replays.read(), &HashSet::from(["first".to_owned(), "second".to_owned()]));
        }

        #[test]
        fn revoking_consent_stops_later_paths_before_the_upload_callback() {
            let persisted = Arc::new(crate::tab_state::TrackedPersistedState::default());
            persisted.write().settings.integrations.data_sharing_mode = DataSharingMode::Replays;
            let sent_replays = ledger([]);
            let (progress_tx, progress_rx) = egui_inbox::UiInbox::channel();
            let mut upload_paths = Vec::new();
            let mut persisted_paths = Vec::new();

            let completion = run_send_all_replays_to_shipbuilds(
                BTreeSet::from([path("first"), path("second")]),
                SendReplayCachePolicy::UseLedger,
                sent_replays.as_ref(),
                &progress_tx,
                &persisted,
                |path| {
                    upload_paths.push(path.to_path_buf());
                    persisted.write().settings.integrations.data_sharing_mode = DataSharingMode::Off;
                    ShipBuildsUploadOutcome::Sent
                },
                |path| {
                    persisted_paths.push(path.to_path_buf());
                    Ok(())
                },
            );
            drop(progress_tx);

            assert_eq!(completion.attempted, ReplayCount(2));
            assert_eq!(completion.sent, ReplayCount(1));
            assert_eq!(upload_paths, vec![path("first")]);
            assert_eq!(persisted_paths, vec![path("first")]);
            assert_eq!(progress_rx.read_without_ctx().last(), Some(progress(2, 2)));
            assert_eq!(&*sent_replays.read(), &HashSet::from(["first".to_owned()]));
        }

        #[test]
        fn a_successful_send_path_is_persisted_in_sqlite() {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let pool = runtime
                .block_on(sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:"))
                .unwrap();
            runtime
                .block_on(sqlx::query("CREATE TABLE sent_replays (replay_path TEXT PRIMARY KEY)").execute(&pool))
                .unwrap();

            persist_sent_replay(&runtime, &pool, Path::new("persisted.wowsreplay")).unwrap();

            assert_eq!(
                runtime.block_on(wows_toolkit_config::queries::get_all_sent_replays(&pool)).unwrap(),
                vec!["persisted.wowsreplay".to_owned()]
            );
        }

        #[test]
        fn an_empty_worker_reports_initial_progress_and_completion() {
            let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
            let pool =
                runtime.block_on(SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:")).unwrap();
            runtime
                .block_on(sqlx::query("CREATE TABLE sent_replays (replay_path TEXT PRIMARY KEY)").execute(&pool))
                .unwrap();
            let (background_task_sender, _inbox) = egui_inbox::UiInbox::channel();
            let deps = ReplayDependencies {
                build_cache: crate::data::wows_data::BuildDataCache::new(
                    PathBuf::new(),
                    "en".to_owned(),
                    String::new(),
                ),
                shipbuilds_client: crate::data::shipbuilds::ShipBuildsClient::new().unwrap(),
                twitch_state: Arc::new(RwLock::new(crate::twitch::TwitchState::default())),
                replay_sort: Arc::new(Mutex::new(SortOrder::default())),
                background_task_sender,
                is_debug_mode: false,
                personal_rating_data: Arc::new(RwLock::new(crate::util::personal_rating::PersonalRatingData::new())),
                egui_ctx: egui::Context::default(),
            };

            let mut task = start_send_all_replays_to_shipbuilds(
                BTreeSet::new(),
                SendReplayCachePolicy::UseLedger,
                deps,
                Arc::new(crate::tab_state::TrackedPersistedState::default()),
                ledger([]),
                pool,
                runtime,
            );
            let progress_rx = match task.kind {
                BackgroundTaskKind::SendingAllReplaysToShipBuilds { rx, .. } => rx,
                _ => panic!("unexpected task kind"),
            };
            assert_eq!(
                crate::test_utils::recv_inbox_timeout(&progress_rx, Duration::from_secs(5)).unwrap(),
                progress(0, 0)
            );
            let completion =
                crate::test_utils::recv_completion_timeout(&task.receiver.take().unwrap(), Duration::from_secs(5))
                    .unwrap()
                    .unwrap();
            assert!(matches!(
                completion,
                BackgroundTaskCompletion::ReplaysSentToShipBuilds {
                    attempted: ReplayCount(0),
                    sent: ReplayCount(0),
                    total: ReplayCount(0),
                }
            ));
        }
    }

    #[test]
    fn upload_outcome_preserves_skip_reasons_sent_and_retryable_states() {
        assert_eq!(
            parse_outcome_for_upload(
                ShipBuildsUploadOutcome::Skipped(
                    crate::task::replay_upload::ReplayUploadSkipReason::IneligibleGameType,
                ),
                None,
            ),
            ParseOutcome::ParsedAndStableSkipped { identity: None }
        );
        assert_eq!(
            parse_outcome_for_upload(
                ShipBuildsUploadOutcome::Skipped(crate::task::replay_upload::ReplayUploadSkipReason::SharingDisabled,),
                None,
            ),
            ParseOutcome::ParsedAndDeferred
        );
        assert_eq!(parse_outcome_for_upload(ShipBuildsUploadOutcome::Sent, None), ParseOutcome::ParsedAndSent);
        assert_eq!(
            parse_outcome_for_upload(ShipBuildsUploadOutcome::TransientFailure, None),
            ParseOutcome::ParsedNotSent
        );
    }

    #[test]
    fn startup_reconciliation_observes_consent_revoked_between_files() {
        let persisted = Arc::new(crate::tab_state::TrackedPersistedState::default());
        persisted.write().settings.integrations.data_sharing_mode = DataSharingMode::Replays;
        assert_eq!(configured_data_sharing_mode(&persisted), DataSharingMode::Replays);

        persisted.write().settings.integrations.data_sharing_mode = DataSharingMode::Off;
        assert_eq!(configured_data_sharing_mode(&persisted), DataSharingMode::Off);
        assert_eq!(
            crate::data::replay_reconcile::startup_upload_reconciliation(
                configured_data_sharing_mode(&persisted),
                false,
                false,
            ),
            crate::data::replay_reconcile::UploadReconciliation::Satisfied
        );
    }

    #[test]
    fn startup_preserves_an_offline_sent_path_through_the_next_sqlite_save() {
        let directory = tempfile::tempdir().unwrap();
        let offline = directory.path().join("offline").join("sent.wowsreplay");
        assert!(!offline.exists());
        let offline_text = offline.to_string_lossy().into_owned();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let pool = runtime
            .block_on(sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:"))
            .unwrap();
        runtime.block_on(sqlx::migrate!("../wows-toolkit-config/migrations").run(&pool)).unwrap();
        runtime.block_on(wows_toolkit_config::queries::insert_sent_replay(&pool, &offline_text)).unwrap();

        let mut first_launch = crate::tab_state::TabState::default();
        runtime.block_on(crate::db::load::load_tab_state_from_db(&pool, &mut first_launch)).unwrap();
        let sent_replays = Arc::clone(&first_launch.sent_replays);
        assert_eq!(&*sent_replays.read(), &HashSet::from([offline_text.clone()]));
        let (_tx, rx) = mpsc::channel();
        let data = BackgroundParserThread {
            egui_ctx: egui::Context::default(),
            rx,
            sent_replays: Arc::clone(&sent_replays),
            build_cache: crate::data::wows_data::BuildDataCache::new(PathBuf::new(), "en".to_owned(), String::new()),
            shipbuilds_client: first_launch.shipbuilds_client.clone(),
            twitch_state: Arc::clone(&first_launch.twitch_state),
            persisted: Arc::clone(&first_launch.persisted),
            data_export_settings: DataExportSettings {
                should_auto_export: false,
                export_path: PathBuf::new(),
                export_format: ReplayExportFormat::Json,
            },
            player_tracker: Arc::clone(&first_launch.player_tracker),
            is_debug: false,
            parser_lock: Arc::clone(&first_launch.parser_lock),
            cap_layout_db: Arc::clone(&first_launch.cap_layout_db),
            db_pool: Some(pool.clone()),
            tokio_runtime: Some(Arc::clone(&runtime)),
            personal_rating_data: Arc::clone(&first_launch.personal_rating_data),
            index_source_id: None,
            unindexable: crate::data::replay_reconcile::Unindexable::default(),
        };

        start_background_parsing_thread(data).join().unwrap();
        runtime.block_on(crate::db::migrate_ron::save_tab_state_to_db(&pool, &first_launch)).unwrap();

        let mut second_launch = crate::tab_state::TabState::default();
        runtime.block_on(crate::db::load::load_tab_state_from_db(&pool, &mut second_launch)).unwrap();

        assert_eq!(&*sent_replays.read(), &HashSet::from([offline_text.clone()]));
        assert_eq!(&*second_launch.sent_replays.read(), &HashSet::from([offline_text]));
    }

    /// Creates `root/relative`, including any parent directories.
    fn touch(root: &Path, relative: &str) -> PathBuf {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("a relative path under root always has a parent")).unwrap();
        std::fs::write(&path, b"not a real replay").unwrap();
        path
    }

    fn walked(root: &Path) -> BTreeSet<PathBuf> {
        walk_replay_files(root).into_iter().collect()
    }

    /// The exact set of paths, not just how many: a walk that recursed but
    /// returned the wrong three files would pass a count-only assertion.
    #[test]
    fn the_walk_finds_replays_in_nested_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let top = touch(root, "top.wowsreplay");
        let nested = touch(root, "sub/nested.wowsreplay");
        let deeper = touch(root, "sub/deeper/still/deeper.wowsreplay");

        assert_eq!(walked(root), BTreeSet::from([top, nested, deeper]));
    }

    /// The in-progress file the game keeps rewriting is never a listed replay.
    /// A legitimate sibling is present so an implementation that returns
    /// nothing at all cannot pass this.
    #[test]
    fn the_walk_excludes_temp_wowsreplay_but_keeps_its_siblings() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "temp.wowsreplay");
        touch(root, "sub/temp.wowsreplay");
        let real = touch(root, "real.wowsreplay");
        let nested_real = touch(root, "sub/real.wowsreplay");

        assert_eq!(walked(root), BTreeSet::from([real, nested_real]));
    }

    #[test]
    fn the_walk_excludes_non_replay_extensions() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(root, "notes.txt");
        touch(root, "tempArenaInfo.json");
        touch(root, "archive.wowsreplay.zip");
        touch(root, "noextension");
        let real = touch(root, "real.wowsreplay");

        assert_eq!(walked(root), BTreeSet::from([real]));
    }

    #[test]
    fn the_walk_of_an_empty_directory_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub/deeper")).unwrap();
        assert!(walk_replay_files(dir.path()).is_empty());
    }

    /// A directory the user picked may no longer exist by the time the ingest
    /// thread walks it. That must read as no replays, not a panic.
    #[test]
    fn the_walk_of_a_missing_directory_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(walk_replay_files(&dir.path().join("does-not-exist")).is_empty());
    }

    async fn mem_pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("../wows-toolkit-config/migrations").run(&pool).await.unwrap();
        pool
    }

    /// `SourceSelector::Live` reads the source the indexer has created, but the
    /// indexer runs on its own schedule and may not have created it yet when a
    /// listing asks for summaries. That must read as an empty map, not an error.
    #[test]
    fn source_selector_live_with_no_source_yields_an_empty_map() {
        let rt = Arc::new(tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap());
        let pool = rt.block_on(mem_pool());

        let task =
            start_load_row_summaries(pool, rt, SourceSelector::Live, crate::db::index::rows::WorkspaceId::LIVE, 0);
        let completion = crate::test_utils::recv_completion_timeout(&task.receiver.unwrap(), Duration::from_secs(5))
            .unwrap()
            .unwrap();
        match completion {
            BackgroundTaskCompletion::RowSummariesLoaded { summaries, .. } => {
                assert!(summaries.is_empty(), "Live with no live source must yield an empty map, not an error");
            }
            other => panic!("unexpected completion: {other:?}"),
        }
    }

    /// The workspace passed in must round-trip unchanged through both the task
    /// kind (read while the load is in flight) and the completion (read once it
    /// finishes), so the caller can route the result to the right listing.
    /// `WorkspaceId(7)` rather than `WorkspaceId::LIVE` so a stray default value
    /// cannot make this pass by coincidence.
    #[test]
    fn start_load_row_summaries_tags_the_task_and_completion_with_the_given_workspace() {
        let rt = Arc::new(tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap());
        let pool = rt.block_on(mem_pool());
        let workspace = crate::db::index::rows::WorkspaceId(7);

        let task = start_load_row_summaries(pool, rt, SourceSelector::Live, workspace, 0);
        match &task.kind {
            BackgroundTaskKind::LoadingRowSummaries { workspace: tagged } => {
                assert_eq!(*tagged, workspace, "the task kind must carry the workspace passed in");
            }
            _ => panic!("unexpected task kind: not LoadingRowSummaries"),
        }

        let completion = crate::test_utils::recv_completion_timeout(&task.receiver.unwrap(), Duration::from_secs(5))
            .unwrap()
            .unwrap();
        match completion {
            BackgroundTaskCompletion::RowSummariesLoaded { workspace: tagged, .. } => {
                assert_eq!(tagged, workspace, "the completion must carry the same workspace passed in");
            }
            other => panic!("unexpected completion: {other:?}"),
        }
    }

    /// Built the way `build_replay_from_path` builds it, so the downcast under
    /// test is the one production exercises.
    fn missing_build_report(build: u32, version: &str) -> rootcause::Report {
        ToolkitError::ReplayBuildUnavailable { build, version: version.to_string(), replay_path: None }.into()
    }

    /// The request that failure names, as the offer keys it.
    fn missing_request(build: u32, version: &str) -> crate::task::BuildRequest {
        build_request_from_failure(build, version).expect("a build and a parseable version are requestable")
    }

    #[test]
    fn a_missing_build_report_is_classified_as_a_missing_build() {
        let mut failures = IngestFailures::default();
        failures.record(&missing_build_report(9_876, "13.5.0"));
        assert_eq!(failures.unreadable, 0);
        assert_eq!(failures.missing_builds, BTreeMap::from([(missing_request(9_876, "13.5.0"), 1)]));
    }

    /// A pre-0.10 replay reports build 0, which is no build at all. Nothing can
    /// be offered for it, so it is one of the replays nothing can be done about
    /// rather than a row in the download prompt.
    #[test]
    fn a_failure_naming_no_build_is_counted_as_unreadable() {
        let mut failures = IngestFailures::default();
        failures.record(&missing_build_report(0, "0.6.13"));
        assert_eq!(failures.unreadable, 1);
        assert!(failures.missing_builds.is_empty());
    }

    /// Each group is walked on its own and reports its own failures, and the
    /// prompt is built from the directory's total: two groups short of the same
    /// build must add up rather than one replacing the other.
    #[test]
    fn merging_two_groups_adds_up_the_replays_waiting_on_each_build() {
        let mut directory = IngestFailures::default();
        directory.record(&missing_build_report(9_876, "13.5.0"));
        directory.record_index_failure();

        let mut group = IngestFailures::default();
        group.record(&missing_build_report(9_876, "13.5.0"));
        group.record(&missing_build_report(9_877, "13.6.0"));
        group.record(&report!("file is truncated"));

        directory.merge(group);

        assert_eq!(
            directory.missing_builds,
            BTreeMap::from([(missing_request(9_876, "13.5.0"), 2), (missing_request(9_877, "13.6.0"), 1)])
        );
        assert_eq!(directory.unreadable, 1);
        assert_eq!(directory.not_indexed, 1);
    }

    #[test]
    fn an_unrelated_report_is_classified_as_unreadable() {
        let mut failures = IngestFailures::default();
        failures.record(&rootcause::report!("file is truncated"));
        assert_eq!(failures.unreadable, 1);
        assert!(failures.missing_builds.is_empty());
    }

    #[test]
    fn several_replays_needing_one_build_coalesce_into_one_entry() {
        let mut failures = IngestFailures::default();
        for _ in 0..3 {
            failures.record(&missing_build_report(9_876, "13.5.0"));
        }
        assert_eq!(failures.missing_builds, BTreeMap::from([(missing_request(9_876, "13.5.0"), 3)]));
        assert_eq!(failures.unreadable, 0);
    }

    #[test]
    fn different_builds_stay_separate() {
        let mut failures = IngestFailures::default();
        failures.record(&missing_build_report(9_876, "13.5.0"));
        failures.record(&missing_build_report(9_877, "13.6.0"));
        assert_eq!(failures.missing_builds.len(), 2);
        assert_eq!(failures.missing_builds.values().copied().collect::<Vec<_>>(), vec![1, 1]);
    }

    /// `build_replay_from_path` hangs a hint off the report before returning
    /// it. The classification has to survive that, or every real ingest would
    /// count missing builds as unreadable while these tests still passed.
    #[test]
    fn an_attachment_does_not_hide_the_missing_build() {
        let mut failures = IngestFailures::default();
        failures
            .record(&missing_build_report(9_876, "13.5.0").attach("try installing the matching game client version"));
        assert_eq!(failures.unreadable, 0);
        assert_eq!(failures.missing_builds.values().sum::<usize>(), 1);
    }

    /// A parser panic is recovered per file and carries no report to
    /// classify, so it has its own entry point. It still has to land in the
    /// same bucket as an unreadable file and leave the actionable missing
    /// builds untouched.
    #[test]
    fn a_recovered_panic_is_counted_as_unreadable() {
        let mut failures = IngestFailures::default();
        failures.record(&missing_build_report(9_876, "13.5.0"));
        failures.record_panic();
        assert_eq!(failures.unreadable, 1);
        assert_eq!(failures.missing_builds.values().sum::<usize>(), 1);
        assert_eq!(failures.total(), 2);
    }

    #[test]
    fn a_mixed_directory_reports_both_kinds() {
        let mut failures = IngestFailures::default();
        failures.record(&missing_build_report(9_876, "13.5.0"));
        failures.record(&rootcause::report!("file is truncated"));
        assert_eq!(failures.unreadable, 1);
        assert_eq!(failures.missing_builds.values().sum::<usize>(), 1);
    }

    /// A replay that reads but does not index is still listed, so it is not one
    /// of the replays the directory skipped. Counting it in `total()` would
    /// report it as missing from a listing it is actually in.
    #[test]
    fn an_index_failure_is_counted_apart_from_the_replays_that_did_not_load() {
        let mut failures = IngestFailures::default();
        failures.record(&rootcause::report!("file is truncated"));
        failures.record_index_failure();
        failures.record_index_failure();

        assert_eq!(failures.not_indexed, 2);
        assert_eq!(failures.unreadable, 1, "an index failure is not an unreadable file");
        assert!(failures.missing_builds.is_empty());
        assert_eq!(failures.total(), 1, "only the replays that did not load count as skipped");
    }

    const WALK_WORKSPACE: crate::db::index::rows::WorkspaceId = crate::db::index::rows::WorkspaceId(7);
    const WALK_SOURCE: crate::db::index::rows::SourceId = crate::db::index::rows::SourceId(42);

    /// A minimal but real `Replay`: an empty-params `GameMetadataProvider` (no
    /// VFS needed) backing a hand-built `ReplayMeta` round-tripped through
    /// `ReplayFile::from_decrypted_parts`, the same entry point the app uses
    /// for a loaded replay's raw JSON.
    fn test_replay() -> Arc<RwLock<Replay>> {
        let meta = wows_replays::ReplayMeta {
            matchGroup: None,
            gameMode: 0,
            gameType: None,
            clientVersionFromExe: "0,0,0,0".to_string(),
            scenarioUiCategoryId: None,
            mapDisplayName: String::new(),
            mapId: 0,
            clientVersionFromXml: String::new(),
            weatherParams: None,
            duration: 0,
            gameLogic: None,
            name: String::new(),
            scenario: String::new(),
            playerID: wows_replays::types::AccountId(0),
            vehicles: Vec::new(),
            playersPerTeam: 0,
            dateTime: String::new(),
            mapName: String::new(),
            playerName: String::new(),
            scenarioConfigId: 0,
            teamsCount: 0,
            logic: None,
            playerVehicle: String::new(),
            battleDuration: None,
        };
        let meta_json = serde_json::to_vec(&meta).expect("ReplayMeta serializes");
        let replay_file = ReplayFile::from_decrypted_parts(meta_json, Vec::new())
            .expect("a ReplayMeta we just serialized parses back");
        let resource_loader = Arc::new(
            wowsunpack::game_params::provider::GameMetadataProvider::from_params_no_specs(Vec::new())
                .expect("an empty param list is always valid"),
        );
        Arc::new(RwLock::new(Replay::new(replay_file, resource_loader)))
    }

    /// A replay the read step produced, tagged with `game_build` so a test can
    /// tell one of them from another wherever the walk hands it on.
    fn read_replay(game_build: usize) -> ReadReplay {
        ReadReplay { replay: test_replay(), game_build }
    }

    /// Walk `paths` with steps the test supplies, returning everything the walk
    /// sent, in the order it sent it, and what it left behind. The group is the
    /// whole run here, so it starts at nothing and its total is its own length.
    fn run_walk<N, R, I>(paths: &[&str], needs_index: N, read: R, index: I) -> (Vec<IngestUpdate>, WalkOutcome)
    where
        N: Fn(&Path) -> bool,
        R: Fn(&Path) -> Result<ReadReplay, Report>,
        I: Fn(&ReadReplay, crate::db::index::rows::SourceId) -> Result<(), Report>,
    {
        run_walk_within(paths, 0, paths.len(), needs_index, read, index)
    }

    /// Walk `paths` as one group of a run that has already visited `offset`
    /// files and holds `total` in all.
    fn run_walk_within<N, R, I>(
        paths: &[&str],
        offset: usize,
        total: usize,
        needs_index: N,
        read: R,
        index: I,
    ) -> (Vec<IngestUpdate>, WalkOutcome)
    where
        N: Fn(&Path) -> bool,
        R: Fn(&Path) -> Result<ReadReplay, Report>,
        I: Fn(&ReadReplay, crate::db::index::rows::SourceId) -> Result<(), Report>,
    {
        let (tx, rx) = crate::ui_channel::throttled_channel(egui::Context::default(), Duration::ZERO);
        let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let slice = WalkSlice { workspace: WALK_WORKSPACE, source: WALK_SOURCE, offset, total };
        let outcome = ingest_walk(paths, slice, &needs_index, &read, &index, &tx);
        drop(tx);
        (rx.read_without_ctx().collect(), outcome)
    }

    /// Every count a walk reported, whichever update carried it.
    fn progress(updates: &[IngestUpdate]) -> Vec<IngestProgress> {
        updates
            .iter()
            .filter_map(|update| match update {
                IngestUpdate::Batch(batch) => Some(batch.progress),
                IngestUpdate::Stage { stage: IngestStage::Reading(progress), .. } => Some(*progress),
                _ => None,
            })
            .collect()
    }

    /// Every replay the walk sent to the listing, whichever batch carried it.
    fn listed(updates: &[IngestUpdate]) -> BTreeSet<PathBuf> {
        updates
            .iter()
            .filter_map(|update| match update {
                IngestUpdate::Batch(batch) => Some(batch.replays.keys().cloned()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Indexing is what puts a listing row's damage, kills and outcome on the
    /// screen: an imported directory whose replays are never indexed shows the
    /// not-indexed placeholder on every row. It has to happen once per replay
    /// read, against the source the walk resolved for that directory.
    #[test]
    fn each_replay_the_walk_reads_is_indexed_against_the_source_the_walk_resolved() {
        let indexed = std::cell::RefCell::new(Vec::new());
        let (updates, outcome) = run_walk(
            &["a.wowsreplay", "b.wowsreplay"],
            |_| true,
            |path| Ok(read_replay(if path == Path::new("a.wowsreplay") { 11 } else { 22 })),
            |replay, source| {
                indexed.borrow_mut().push((replay.game_build, source));
                Ok(())
            },
        );

        assert_eq!(
            indexed.into_inner(),
            vec![(11, WALK_SOURCE), (22, WALK_SOURCE)],
            "both replays must be indexed, against the source the walk resolved"
        );
        assert_eq!(
            listed(&updates),
            BTreeSet::from([PathBuf::from("a.wowsreplay"), PathBuf::from("b.wowsreplay")]),
            "both replays must reach the listing"
        );
        assert_eq!(outcome.failures.not_indexed, 0);
    }

    /// Re-opening a directory must cost a read per replay, not a re-parse of
    /// the whole tree, so a replay the source already has rows for is listed
    /// without being indexed again.
    #[test]
    fn a_replay_the_gate_rejects_is_listed_without_being_indexed() {
        let indexed = std::cell::RefCell::new(Vec::new());
        let (updates, _) = run_walk(
            &["a.wowsreplay", "b.wowsreplay"],
            |path| path == Path::new("b.wowsreplay"),
            |path| Ok(read_replay(if path == Path::new("a.wowsreplay") { 11 } else { 22 })),
            |replay, _| {
                indexed.borrow_mut().push(replay.game_build);
                Ok(())
            },
        );

        assert_eq!(indexed.into_inner(), vec![22], "the gated replay must not be indexed again");
        assert_eq!(
            listed(&updates),
            BTreeSet::from([PathBuf::from("a.wowsreplay"), PathBuf::from("b.wowsreplay")]),
            "a replay that was not indexed by this walk is still listed"
        );
    }

    /// A replay whose index rows cannot be written is still a replay the user
    /// dropped a directory to see. It is listed, with its failure counted apart
    /// from the files that never loaded.
    #[test]
    fn a_replay_whose_indexing_fails_is_still_listed() {
        let (updates, outcome) =
            run_walk(&["a.wowsreplay"], |_| true, |_| Ok(read_replay(11)), |_, _| Err(report!("no database")));

        assert_eq!(listed(&updates), BTreeSet::from([PathBuf::from("a.wowsreplay")]));
        assert_eq!(outcome.failures.not_indexed, 1);
        assert_eq!(outcome.failures.total(), 0, "a listed replay is not a replay the walk skipped");
        assert!(outcome.index_panics.is_empty(), "an error is not a panic and must not be blacklisted");
    }

    /// A parse that panics is deterministic, so the walk reports it for the
    /// blacklist the other index paths read. The replay is still listed: it was
    /// read, and only its index rows are missing.
    #[test]
    fn a_replay_whose_indexing_panics_is_listed_and_reported_for_the_blacklist() {
        let (updates, outcome) = run_walk(
            &["a.wowsreplay", "b.wowsreplay"],
            |_| true,
            |_| Ok(read_replay(11)),
            |_, _| panic!("parser exploded"),
        );

        assert_eq!(
            listed(&updates),
            BTreeSet::from([PathBuf::from("a.wowsreplay"), PathBuf::from("b.wowsreplay")]),
            "a replay whose indexing panicked is still listed, and the walk carries on"
        );
        assert_eq!(outcome.failures.not_indexed, 2);
        assert_eq!(
            outcome.index_panics,
            vec![PathBuf::from("a.wowsreplay"), PathBuf::from("b.wowsreplay")],
            "both panicking replays must be reported so the next walk skips their parse"
        );
    }

    /// One file the walk cannot read costs only itself: it is not listed, it is
    /// classified for the failure report, and the files after it are still read.
    #[test]
    fn a_file_that_cannot_be_read_is_not_listed_and_does_not_stop_the_walk() {
        let (updates, outcome) = run_walk(
            &["a.wowsreplay", "b.wowsreplay", "c.wowsreplay"],
            |_| true,
            |path| match path.file_name().and_then(|name| name.to_str()) {
                Some("a.wowsreplay") => Err(missing_build_report(9_876, "13.5.0")),
                Some("b.wowsreplay") => panic!("parser exploded"),
                _ => Ok(read_replay(11)),
            },
            |_, _| Ok(()),
        );

        assert_eq!(
            listed(&updates),
            BTreeSet::from([PathBuf::from("c.wowsreplay")]),
            "only the file that read must be listed, and it must still be reached"
        );
        assert_eq!(outcome.failures.missing_builds.values().sum::<usize>(), 1);
        assert_eq!(outcome.failures.unreadable, 1);
    }

    /// The walk reads a replay to index it, then hands the listing only the
    /// metadata its row draws. Passing the hydrated replay on -- in the batch,
    /// or in what the walk leaves behind -- would keep that build's
    /// `GameMetadataProvider` and packet stream resident for as long as the
    /// directory is listed, which is what makes a directory spanning many
    /// builds hold all of them at once.
    #[test]
    fn the_walk_does_not_hand_the_listing_the_replay_it_read() {
        let read_replays: std::cell::RefCell<Vec<std::sync::Weak<RwLock<Replay>>>> =
            std::cell::RefCell::new(Vec::new());
        let (updates, _outcome) = run_walk(
            &["a.wowsreplay", "b.wowsreplay"],
            |_| true,
            |_| {
                let built = read_replay(11);
                read_replays.borrow_mut().push(Arc::downgrade(&built.replay));
                Ok(built)
            },
            |_, _| Ok(()),
        );

        assert_eq!(
            listed(&updates),
            BTreeSet::from([PathBuf::from("a.wowsreplay"), PathBuf::from("b.wowsreplay")]),
            "both replays must reach the listing, or the assertion below holds vacuously"
        );
        let read_replays = read_replays.into_inner();
        assert_eq!(read_replays.len(), 2, "the read step must actually have built a replay for each file");
        for (index, replay) in read_replays.iter().enumerate() {
            assert!(
                replay.upgrade().is_none(),
                "replay {index} was still alive after its batch went out: the walk retained what it read"
            );
        }
    }

    /// The listing retains exactly the files it is told the directory holds,
    /// and the scan tells it once, before any group is read. A walk naming its
    /// own group's files would drop every other build's rows from the listing.
    #[test]
    fn a_group_walk_does_not_claim_the_directorys_file_set() {
        let (updates, _) = run_walk(&["a.wowsreplay"], |_| true, |_| Ok(read_replay(11)), |_, _| Ok(()));

        assert!(
            !updates.iter().any(|update| matches!(update, IngestUpdate::Walked { .. })),
            "naming the files found belongs to the scan, which sees the whole directory"
        );
    }

    /// A group is one slice of a run that spans several, so its count carries
    /// on from what came before it and is measured against the directory. A
    /// count restarting per group reads as the run starting over.
    #[test]
    fn a_group_counts_from_where_the_run_had_got_to() {
        let (updates, _) =
            run_walk_within(&["b.wowsreplay", "c.wowsreplay"], 2, 5, |_| true, |_| Ok(read_replay(11)), |_, _| Ok(()));

        assert_eq!(
            progress(&updates).last(),
            Some(&IngestProgress { done: 4, total: 5 }),
            "two files after two already visited is four of five, not two of two"
        );
    }

    /// The population that fails to read is contiguous -- a whole game
    /// version's replays share a missing build, and the walk visits files in
    /// date order -- so a walk that only speaks when it has a replay in hand
    /// shows one frozen count next to a spinner for the entire run.
    #[test]
    fn a_walk_reading_nothing_reports_progress_before_it_finishes() {
        let (updates, _) = run_walk(
            &["a.wowsreplay", "b.wowsreplay"],
            |_| true,
            |_| {
                std::thread::sleep(INGEST_FLUSH_INTERVAL + Duration::from_millis(50));
                Err(report!("no game data for this build"))
            },
            |_, _| Ok(()),
        );

        let mid_walk = updates.iter().any(|update| {
            matches!(
                update,
                IngestUpdate::Stage { workspace, stage: IngestStage::Reading(progress) }
                    if *workspace == WALK_WORKSPACE && *progress == IngestProgress { done: 1, total: 2 }
            )
        });
        assert!(mid_walk, "the count must move while the walk is still running, not only when it ends");
    }

    /// The reports a parse attaches are dropped again once the index rows are
    /// written, and a parse that panics has attached them too. The panic still
    /// has to reach the caller, which counts it and lists the replay anyway.
    #[test]
    fn a_panicking_step_resets_before_its_panic_reaches_the_caller() {
        let mut held = Some("battle report");

        let outcome: Result<(), _> = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            reset_after(
                &mut held,
                |held| {
                    *held = Some("battle report");
                    panic!("parser exploded")
                },
                |held| *held = None,
            )
        }));

        assert!(outcome.is_err(), "the panic must still reach the caller");
        assert_eq!(held, None, "the reset must have run before the panic resumed");
    }

    #[test]
    fn a_step_that_returns_normally_resets_and_yields_what_it_produced() {
        let mut held = Some("battle report");
        let produced = reset_after(&mut held, |_| 7, |held| *held = None);

        assert_eq!(produced, 7, "the caller must get what the step produced");
        assert_eq!(held, None, "the reset must run on the returning path too");
    }

    /// Replays that fail to load are contiguous in a walk -- the files are
    /// visited in date order and a whole game version's worth of them shares a
    /// missing build -- so a rule that only ever speaks when it has a replay in
    /// hand leaves the count frozen for the whole run, which is what a stalled
    /// walk looks like. Once the interval has elapsed the walk reports progress
    /// with nothing to carry it.
    #[test]
    fn a_walk_holding_no_replays_reports_progress_once_the_interval_elapses() {
        assert_eq!(flush_now(0, Duration::ZERO), Flush::Hold);
        assert_eq!(flush_now(0, INGEST_FLUSH_INTERVAL), Flush::Progress);
        assert_eq!(flush_now(0, INGEST_FLUSH_INTERVAL * 10), Flush::Progress);
    }

    /// Size is what keeps a fast walk from waking the UI per replay, so a full
    /// batch goes out without waiting for the interval.
    #[test]
    fn a_full_batch_flushes_before_the_interval_elapses() {
        assert_eq!(flush_now(INGEST_BATCH_SIZE, Duration::ZERO), Flush::Batch);
    }

    /// The interval is what keeps a slow walk from holding replays back until
    /// enough of them accumulate: one replay is enough once it has elapsed, and
    /// not before.
    #[test]
    fn a_partial_batch_waits_for_the_interval() {
        assert_eq!(flush_now(1, Duration::ZERO), Flush::Hold);
        assert_eq!(flush_now(1, INGEST_FLUSH_INTERVAL), Flush::Batch);
    }

    /// A scan of `files`, each named by a string and reporting a build, or no
    /// readable header at all when its build is `None`. No filesystem and no
    /// game install behind it.
    fn scan_for(files: &[(&str, Option<u32>)]) -> crate::task::scan::DirectoryScan {
        let owned: Vec<(String, Option<u32>)> = files.iter().map(|(n, b)| ((*n).to_string(), *b)).collect();
        crate::task::scan::scan_paths(
            PathBuf::from("root"),
            owned.iter().map(|(n, _)| PathBuf::from(n)).collect(),
            |p| {
                owned.iter().find(|(n, _)| Path::new(n) == p).and_then(|(_, b)| {
                    b.map(|build| Version { major: 15, minor: 0, patch: 0, build: std::num::NonZeroU32::new(build) })
                })
            },
            |_| true,
            |_| std::ops::ControlFlow::Continue(()),
            std::num::NonZeroUsize::MIN,
        )
    }

    /// Every path one group's walk was handed, and where that group was told
    /// the run had got to.
    #[derive(Debug, PartialEq, Eq)]
    struct WalkCall {
        paths: Vec<PathBuf>,
        offset: usize,
        total: usize,
    }

    /// What a directory read did, as seen from its injected steps: which builds
    /// it tried to load and in what order, what it handed each group's walk, and
    /// everything it sent.
    struct ReadRun {
        loaded: Vec<u32>,
        walks: Vec<WalkCall>,
        updates: Vec<IngestUpdate>,
        outcome: WalkOutcome,
    }

    /// Run the real group loop over `scan`, with `has_data` answering whatever
    /// `load` will. That is what a machine whose cheap check and loader agree
    /// looks like, which is every case but the one
    /// [`run_read_with`] exists for.
    fn run_read<L, W>(scan: &crate::task::scan::DirectoryScan, load: L, walk: W) -> ReadRun
    where
        L: Fn(&crate::task::BuildRequest) -> bool,
        W: Fn(Vec<PathBuf>, WalkSlice) -> WalkOutcome,
    {
        run_read_with(scan, &load, &load, walk)
    }

    /// Run the real group loop over `scan` with steps the test supplies.
    ///
    /// `has_data` is the cheap check the announce is gated on, `load` decides
    /// whether each build's data comes up, and `walk` stands in for reading a
    /// group's replays. The first two are separate parameters so a test can
    /// make them disagree.
    fn run_read_with<D, L, W>(scan: &crate::task::scan::DirectoryScan, has_data: D, load: L, walk: W) -> ReadRun
    where
        D: Fn(&crate::task::BuildRequest) -> bool,
        L: Fn(&crate::task::BuildRequest) -> bool,
        W: Fn(Vec<PathBuf>, WalkSlice) -> WalkOutcome,
    {
        let loaded = std::cell::RefCell::new(Vec::new());
        let walks = std::cell::RefCell::new(Vec::new());
        let (tx, rx) = crate::ui_channel::throttled_channel(egui::Context::default(), Duration::ZERO);

        let record_load = |request: &crate::task::BuildRequest| {
            loaded.borrow_mut().push(request.build_u32());
            load(request)
        };
        let record_walk = |paths: Vec<PathBuf>, slice: WalkSlice| {
            walks.borrow_mut().push(WalkCall { paths: paths.clone(), offset: slice.offset, total: slice.total });
            walk(paths, slice)
        };

        let steps = BuildSteps { has_data: &has_data, load: &record_load, walk: &record_walk };
        let outcome = read_build_groups(scan, WALK_WORKSPACE, WALK_SOURCE, steps, &tx);
        drop(tx);

        ReadRun {
            loaded: loaded.into_inner(),
            walks: walks.into_inner(),
            updates: rx.read_without_ctx().collect(),
            outcome,
        }
    }

    /// Every count the run reported, whichever update carried it.
    fn read_progress(updates: &[IngestUpdate]) -> Vec<IngestProgress> {
        updates
            .iter()
            .filter_map(|update| match update {
                IngestUpdate::Stage { stage: IngestStage::Reading(progress), .. } => Some(*progress),
                IngestUpdate::Batch(batch) => Some(batch.progress),
                _ => None,
            })
            .collect()
    }

    /// Each build's data is loaded once, at the head of its group, and every
    /// replay of that build is read under that one load. Loading per replay, or
    /// splitting a build across two visits, lets a directory spanning more
    /// builds than the data map holds evict and reload between neighbouring
    /// files. Ascending order is what lets an older build's constants bridge
    /// forward to a newer one and never the other way round.
    #[test]
    fn each_build_is_resolved_once_for_its_whole_group() {
        let scan = scan_for(&[("a", Some(100)), ("b", Some(90)), ("c", Some(100)), ("d", Some(90))]);

        let run = run_read(&scan, |_| true, |_, _| WalkOutcome::default());

        assert_eq!(run.loaded, vec![90, 100], "each build loads exactly once, in ascending build order");
        assert_eq!(
            run.walks.iter().map(|call| call.paths.clone()).collect::<Vec<_>>(),
            vec![vec![PathBuf::from("b"), PathBuf::from("d")], vec![PathBuf::from("a"), PathBuf::from("c")]],
            "each build's replays are read in one visit, never interleaved with another build's"
        );
    }

    /// The counter spans the directory rather than each group, so it advances
    /// monotonically across build boundaries instead of resetting at each one,
    /// and reaches full when the last group ends.
    #[test]
    fn the_read_counter_spans_the_whole_directory() {
        let scan = scan_for(&[("a", Some(100)), ("b", Some(90)), ("c", Some(100)), ("d", Some(90))]);

        let run = run_read(&scan, |_| true, |_, _| WalkOutcome::default());

        assert_eq!(
            run.walks,
            vec![
                WalkCall { paths: vec![PathBuf::from("b"), PathBuf::from("d")], offset: 0, total: 4 },
                WalkCall { paths: vec![PathBuf::from("a"), PathBuf::from("c")], offset: 2, total: 4 },
            ],
            "the second group starts from where the first left off, against the directory's own total"
        );
        let last = run.walks.last().expect("two groups were walked");
        assert_eq!(last.offset + last.paths.len(), scan.to_read(), "the last group ends at full");
    }

    /// A build whose data will not come up costs its own replays and nothing
    /// else: they are reported against that build so the count of what is
    /// waiting on it is right, the counter moves over them so the bar still
    /// reaches full, and the builds after it are read normally.
    #[test]
    fn a_build_whose_data_will_not_load_is_skipped_without_stopping_the_read() {
        let scan = scan_for(&[("a", Some(100)), ("b", Some(90)), ("c", Some(90))]);

        let run = run_read(&scan, |request| request.build_u32() != 90, |_, _| WalkOutcome::default());

        assert_eq!(
            run.walks,
            vec![WalkCall { paths: vec![PathBuf::from("a")], offset: 2, total: 3 }],
            "only the build that loaded is walked, and it starts past the two replays that were skipped"
        );
        assert_eq!(
            run.outcome.failures.missing_builds.values().copied().collect::<Vec<_>>(),
            vec![2],
            "both replays of the build that would not load are reported against it"
        );
        assert_eq!(
            run.outcome.failures.missing_builds.keys().map(crate::task::BuildRequest::build_u32).collect::<Vec<_>>(),
            vec![90],
            "and against that build, not another"
        );
        assert!(
            read_progress(&run.updates).contains(&IngestProgress { done: 2, total: 3 }),
            "the count must move over a skipped build, or the bar stops short of full"
        );
    }

    /// A build with no data on this machine is skipped, not loaded, so
    /// announcing a load for it names work that never happens.
    #[test]
    fn a_build_with_no_data_is_not_announced_as_loading() {
        let scan = scan_for(&[("a", Some(100)), ("b", Some(90))]);

        let run = run_read(&scan, |request| request.build_u32() == 100, |_, _| WalkOutcome::default());

        assert_eq!(announced_builds(&run.updates), vec![100], "only the build that is actually loaded is announced");
    }

    /// The announce is a hint about what to say; the load is what decides what
    /// is read. A build the cheap availability check does not know about, but
    /// whose data loads anyway, must still have its replays read. Gating the
    /// load on that check would let it drop replays that load perfectly well,
    /// silently, the moment it grew stricter than resolution.
    #[test]
    fn a_build_the_availability_check_missed_is_still_read_when_its_data_loads() {
        let scan = scan_for(&[("a", Some(100))]);

        let run = run_read_with(&scan, |_| false, |_| true, |_, _| WalkOutcome::default());

        assert_eq!(run.loaded, vec![100], "the load must be attempted whatever the cheap check answered");
        assert_eq!(
            run.walks,
            vec![WalkCall { paths: vec![PathBuf::from("a")], offset: 0, total: 1 }],
            "a build whose data loads must have its replays read"
        );
        assert!(
            run.outcome.failures.missing_builds.is_empty(),
            "a build that loaded is not one the directory is waiting on"
        );
        assert!(announced_builds(&run.updates).is_empty(), "and it is read without being announced");
    }

    /// The builds a run said out loud it was loading data for.
    fn announced_builds(updates: &[IngestUpdate]) -> Vec<u32> {
        updates
            .iter()
            .filter_map(|update| match update {
                IngestUpdate::Stage { stage: IngestStage::LoadingData { build, .. }, .. } => Some(build.build_u32()),
                _ => None,
            })
            .collect()
    }

    /// The files the scan could not read are files the listing will never show.
    /// The scan already classified why; the read only adds their count, and a
    /// read that dropped it would report a directory as fully listed when it is
    /// not.
    #[test]
    fn the_files_the_scan_could_not_read_are_counted_as_unreadable() {
        let scan = scan_for(&[("a", Some(100)), ("bad", None), ("worse", None)]);
        assert_eq!(scan.unreadable.len(), 2, "the fixture must actually hold unreadable files");

        let run = run_read(&scan, |_| true, |_, _| WalkOutcome::default());

        assert_eq!(run.outcome.failures.unreadable, 2);
        assert_eq!(scan.to_read(), 1, "an unreadable file is not one the read stage visits");
    }

    /// A walk whose updates cannot be sent has lost its listing, and every
    /// build after it costs a full game-data load and a parse per replay for a
    /// listing nobody is watching.
    #[test]
    fn a_lost_listing_stops_the_read_before_the_next_build_loads() {
        let scan = scan_for(&[("a", Some(100)), ("b", Some(90))]);

        let run = run_read(&scan, |_| true, |_, _| WalkOutcome { channel_closed: true, ..WalkOutcome::default() });

        assert_eq!(run.loaded, vec![90], "the build after the one that lost its listing must not be loaded");
        assert_eq!(run.walks.len(), 1, "and its replays must not be read");
        assert!(run.outcome.channel_closed);
    }

    /// Every stage needs its own label. A shared key would report a download as a
    /// read, which is the one thing the staging exists to distinguish.
    #[test]
    fn every_stage_has_its_own_key() {
        let stages = [
            IngestStage::Scanning(IngestProgress { done: 0, total: 1 }),
            IngestStage::Downloading(crate::task::DownloadProgress { downloaded: 0, total: 1 }),
            IngestStage::LoadingData { build: test_request(), position: BuildLoadPosition { index: 0, count: 1 } },
            IngestStage::Reading(IngestProgress { done: 0, total: 1 }),
        ];
        let keys: BTreeSet<&str> = stages.iter().map(|stage| stage.key()).collect();
        assert_eq!(keys.len(), stages.len());
    }

    /// Only the data load is open-ended. Reporting a determinate stage as
    /// indefinite loses a bar the user can read progress from.
    #[test]
    fn only_the_data_load_is_indefinite() {
        assert!(
            IngestStage::LoadingData { build: test_request(), position: BuildLoadPosition { index: 0, count: 1 } }
                .fraction()
                .is_none()
        );
        assert_eq!(IngestStage::Reading(IngestProgress { done: 1, total: 4 }).fraction(), Some(0.25));
    }

    /// A stage that has not started reads as zero, not as finished. A total of
    /// zero would otherwise divide into a full bar.
    ///
    /// `Downloading` is set the moment the task is spawned, before planning has
    /// found a single object to fetch, so a zero total is what it reports for
    /// as long as that takes.
    #[test]
    fn an_empty_stage_reads_as_zero_not_complete() {
        assert_eq!(IngestStage::Scanning(IngestProgress { done: 0, total: 0 }).fraction(), Some(0.0));
        assert_eq!(IngestStage::Reading(IngestProgress { done: 0, total: 0 }).fraction(), Some(0.0));
        assert_eq!(
            IngestStage::Downloading(crate::task::DownloadProgress { downloaded: 0, total: 0 }).fraction(),
            Some(0.0)
        );
    }

    fn test_request() -> crate::task::BuildRequest {
        crate::task::BuildRequest::new(Version { major: 15, minor: 0, patch: 0, build: std::num::NonZeroU32::new(100) })
            .expect("build is present")
    }
}

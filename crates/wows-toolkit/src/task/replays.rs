use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::fs::read_dir;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use gettext::Catalog;
use language_tags::LanguageTag;
use parking_lot::Mutex;
use parking_lot::RwLock;
use rootcause::Report;
use rootcause::prelude::*;
use tracing::debug;
use tracing::error;
use tracing::instrument;
use tracing::warn;
use wows_replays::ReplayFile;
use wows_replays::game_constants::GameConstants;
use wowsunpack::data::Version;
use wowsunpack::data::idx;
use wowsunpack::data::idx_vfs::IdxVfs;
use wowsunpack::data::wrappers::mmap::MmapPkgSource;
use wowsunpack::game_data;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

use crate::data::replay_reconcile::ParseOutcome;
use crate::data::settings::DataSharingMode;
use crate::data::wows_data::GameAsset;
use crate::data::wows_data::WorldOfWarshipsData;
use crate::task::replay_upload::ReplayUploadAction;
use crate::task::replay_upload::decide_upload_action;
use crate::twitch::TwitchState;
use crate::ui::player_tracker::PlayerTracker;
use crate::ui::replay_parser::ListedReplay;
use crate::ui::replay_parser::Replay;
use crate::ui::replay_parser::SortOrder;
use crate::util::build_tracker;
use crate::util::error::ToolkitError;
use crate::util::game_params::load_game_params;
use crate::util::replay_export::FlattenedVehicle;
use crate::util::replay_export::Match;

use super::BackgroundTask;
use super::BackgroundTaskCompletion;
use super::BackgroundTaskKind;
use super::IndexProgress;

use crate::task::networking::load_versioned_constants_from_disk;
use crate::task::networking::load_versioned_constants_from_disk_with_fallback;

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
fn parse_dotted_version(version: &str, build: u32) -> Option<Version> {
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

/// Build `GameConstants` from VFS and merge in replay constants (CONSUMABLE_IDS, BATTLE_STAGES).
#[instrument(skip(vfs, replay_constants))]
pub fn build_game_constants(
    vfs: &VfsPath,
    replay_constants: &serde_json::Value,
    version: Option<Version>,
) -> GameConstants {
    let mut game_constants = GameConstants::from_vfs(vfs);
    game_constants.merge_replay_constants(replay_constants, version.unwrap_or_default());
    // The replay's own client version is authoritative for consumable ids: their
    // ordering shifts across versions, so resolve them against the static per-version
    // table rather than the latest layout. Applied last so it wins over any bridged
    // (newer-build) constants merged above.
    if let Some(version) = version {
        wowsunpack::game_constants::apply_version_consumables(game_constants.common_mut(), version);
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
    version: Option<Version>,
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
        Err(crate::util::error::ToolkitError::InvalidWowsDirectory(wows_directory.to_path_buf()))
            .context("res_packages directory not found")?;
    }

    let pkg_source = MmapPkgSource::new(&pkgs_path);
    let idx_vfs = IdxVfs::new(pkg_source, &idx_files);
    let vfs = VfsPath::new(idx_vfs);

    // Load translations
    // WoWs locale codes use underscores (e.g. "zh_tw", "pt_br") but BCP 47
    // language tags use hyphens. Normalize before parsing.
    let bcp47 = locale.replace('_', "-");
    let primary_lang = bcp47
        .parse::<LanguageTag>()
        .map(|tag| tag.primary_language().to_string())
        .unwrap_or_else(|_| locale.to_string());
    let attempted_dirs = [locale, &primary_lang, "en"];
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
    let metadata_provider = load_game_params(&vfs, game_patch).ok().map(|metadata_provider| {
        if let Some(catalog) = found_catalog {
            metadata_provider.set_translations(catalog)
        }
        Arc::new(metadata_provider)
    });

    debug!("Loading icons for build {}", build);
    // The semantic version isn't resolved at this point for a live install, so
    // the resolver gets `None` and picks the newest layout with its fallbacks.
    let icons = load_ship_icons(&vfs, None);
    let ribbon_icons = load_ribbon_icons(&vfs, wowsunpack::game_assets::GuiAssetDir::Ribbons, None);
    let subribbon_icons = load_ribbon_icons(&vfs, wowsunpack::game_assets::GuiAssetDir::SubRibbons, None);

    // Load version-matched constants from disk cache only (no network I/O).
    // If not cached, use fallback constants. The networking thread will fetch
    // updated constants from GitHub in the background.
    debug!("Loading versioned constants for build {}", build);
    let (replay_constants, replay_constants_exact_match) = match load_versioned_constants_from_disk_with_fallback(build)
    {
        Some((data, exact)) => (data, exact),
        None => (fallback_constants.clone(), false),
    };

    let game_constants = build_game_constants(&vfs, &replay_constants, version);
    let game_constants = Arc::new(game_constants);

    // Try to determine full version from preferences or leave as None for non-latest builds
    let full_version = version; // Set by caller (replay version) when known

    Ok(WorldOfWarshipsData {
        game_metadata: metadata_provider,
        vfs,
        patch_version: game_patch,
        full_version,
        build_number: build,
        ship_icons: icons,
        ribbon_icons,
        subribbon_icons,
        achievement_icons: HashMap::new(),
        consumable_icons: HashMap::new(),
        crew_skill_icons: HashMap::new(),
        modernization_icons: HashMap::new(),
        signal_flag_icons: HashMap::new(),
        game_constants,
        replay_constants: Arc::new(RwLock::new(replay_constants)),
        replay_constants_exact_match,
        replays_dir: PathBuf::new(), // Set by caller
        build_dir,
        dump_dir: None,
    })
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
    let mut data = load_wows_data_for_build(&wows_directory, latest_build, locale, fallback_constants, full_version)
        .context_with(|| format!("failed to load game data for build {latest_build}"))?;
    data.full_version = full_version;
    data.replays_dir = replays_dir.clone();

    debug!("Loading replays");
    let replays = replay_filepaths(&replays_dir).map(|replays| {
        let iter = replays.into_iter().filter_map(|path| match ReplayFile::from_file(&path) {
            Ok(replay_file) => Some((path, Arc::new(ListedReplay::from_meta(&replay_file.meta)))),
            Err(e) => {
                error!("Failed to parse replay {}: {:?}", path.display(), e);
                None
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
pub fn load_wows_data_from_dump(
    dump_dir: &Path,
    build: u32,
    locale: &str,
    fallback_constants: &serde_json::Value,
    replay_version: Option<Version>,
) -> Result<WorldOfWarshipsData, Report> {
    use wowsunpack::game_params::provider::GameMetadataProvider;

    debug!("Loading game data from dump: {}", dump_dir.display());

    let cas = wows_data_mgr::cas_vfs::BuildCas::open(dump_dir)
        .ok_or_else(|| report!("metadata.toml not found in dump: {}", dump_dir.display()))?;
    let vfs = cas.vfs();

    // Recover the semantic version from the dump's metadata so version-aware
    // asset resolution works. Build numbers differ across servers (e.g. the
    // China client) for the same major.minor.patch, so the version is the
    // server-independent key.
    let full_version = parse_dotted_version(&cas.metadata().version, build);

    // Load translations from dump
    let bcp47 = locale.replace('_', "-");
    let primary_lang = bcp47
        .parse::<LanguageTag>()
        .map(|tag| tag.primary_language().to_string())
        .unwrap_or_else(|_| locale.to_string());
    let attempted_dirs = [locale, &primary_lang, "en"];
    let mut found_catalog = None;
    for dir in attempted_dirs {
        let Some(mo_path) = cas.derived_path(&format!("translations/{dir}/LC_MESSAGES/global.mo")) else {
            continue;
        };
        if let Ok(file) = File::open(&mo_path)
            && let Ok(catalog) = Catalog::parse(file)
        {
            found_catalog = Some(catalog);
            break;
        }
    }

    // Load GameParams: prefer the dump's rkyv cache when it carries a current
    // header, otherwise fall back to re-parsing from the dump's VFS (this
    // happens for caches generated before the current cache format version,
    // including any pre-WUGP-magic dumps in wows-replay-data).
    let rkyv_path = cas.derived_path("game_params.rkyv");
    let cached_params = rkyv_path.as_deref().and_then(wowsunpack::game_params::cache::load);
    let metadata_provider = match cached_params {
        Some(params) => {
            debug!("Loaded GameParams from rkyv cache");
            // Pair the fast rkyv params with entity specs parsed from the dump's
            // VFS (`scripts/entity_defs`). The specs are required to parse replay
            // packets (BasePlayerCreate indexes them by entity type); without them
            // any dump-loaded replay -- i.e. any build not in the live install --
            // panics in the packet parser.
            GameMetadataProvider::from_params_with_vfs(params, &vfs)
                .map_err(|e| report!("Failed to build GameMetadataProvider from dump: {e:?}"))?
        }
        None => {
            debug!("Falling back to GameParams.data in dump VFS (rkyv missing or stale)");
            GameMetadataProvider::from_vfs(&vfs)
                .map_err(|e| report!("Failed to load GameParams from dump VFS: {e:?}"))?
        }
    };
    if let Some(catalog) = found_catalog {
        metadata_provider.set_translations(catalog);
    }
    let metadata_provider = Some(Arc::new(metadata_provider));

    // Load icons, using the semantic version recovered from the dump if present.
    let version = full_version.as_ref();
    let icons = load_ship_icons(&vfs, version);
    let ribbon_icons = load_ribbon_icons(&vfs, wowsunpack::game_assets::GuiAssetDir::Ribbons, version);
    let subribbon_icons = load_ribbon_icons(&vfs, wowsunpack::game_assets::GuiAssetDir::SubRibbons, version);

    // Load constants: try dump dir first, then disk cache, then fallback
    let (replay_constants, replay_constants_exact_match) = {
        let dump_constants_path = dump_dir.join("constants.json");
        if dump_constants_path.exists() {
            if let Ok(data) = std::fs::read(&dump_constants_path)
                && let Ok(json) = serde_json::from_slice::<serde_json::Value>(&data)
            {
                (json, true)
            } else {
                (fallback_constants.clone(), false)
            }
        } else {
            match load_versioned_constants_from_disk_with_fallback(build) {
                Some((data, exact)) => (data, exact),
                None => (fallback_constants.clone(), false),
            }
        }
    };
    // Prefer the replay's own version; fall back to the version recovered from the dump.
    let constants_version = replay_version.or(full_version);
    let game_constants = build_game_constants(&vfs, &replay_constants, constants_version);

    Ok(WorldOfWarshipsData {
        game_metadata: metadata_provider,
        vfs,
        patch_version: build as usize,
        full_version,
        build_number: build,
        ship_icons: icons,
        ribbon_icons,
        subribbon_icons,
        achievement_icons: HashMap::new(),
        consumable_icons: HashMap::new(),
        crew_skill_icons: HashMap::new(),
        modernization_icons: HashMap::new(),
        signal_flag_icons: HashMap::new(),
        game_constants: Arc::new(game_constants),
        replay_constants: Arc::new(RwLock::new(replay_constants)),
        replay_constants_exact_match,
        replays_dir: PathBuf::new(),
        build_dir: dump_dir.to_path_buf(),
        dump_dir: Some(dump_dir.to_path_buf()),
    })
}

fn parse_replay_data_in_background(
    path: &Path,
    client: &reqwest::blocking::Client,
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
        match ReplayFile::from_file(path) {
            Ok(replay_file) => {
                debug!("replay parsed successfully");
                // We only send back random battles
                let game_type = replay_file.meta.gameType.clone().unwrap_or_default();

                // Resolve version-matched data for this replay's build
                let replay_version = wowsunpack::data::Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
                let Some(wows_data_for_build) = data.wows_data_map.resolve(&replay_version) else {
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
                    let mut upload_transient = false;
                    match replay.parse(game_version.to_string().as_str()) {
                        Ok(report) => {
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
                                let self_confirmed_non_test = report
                                    .players()
                                    .iter()
                                    .find(|p| p.relation().is_self())
                                    .and_then(|p| p.vehicle().vehicle())
                                    .map(|v| !v.is_test_ship())
                                    .unwrap_or(false);

                                match decide_upload_action(
                                    data.data_sharing_mode,
                                    is_valid_game_type_for_shipbuilds,
                                    self_confirmed_non_test,
                                ) {
                                    ReplayUploadAction::Skip => {}
                                    ReplayUploadAction::BuildData => {
                                        for player in report.players().iter().filter(|player| !player.is_bot()) {
                                            let Some(realm) = player.initial_state().realm() else {
                                                continue;
                                            };
                                            #[cfg(not(feature = "shipbuilds_debugging"))]
                                            let url = "https://shipbuilds.com/api/ship_builds";
                                            #[cfg(feature = "shipbuilds_debugging")]
                                            let url = "http://192.168.1.215:3000/api/ship_builds";

                                            if let Some(payload) = build_tracker::BuildTrackerPayload::build_from(
                                                player,
                                                realm.to_string(),
                                                report.version(),
                                                game_type.to_string(),
                                                &metadata_provider,
                                            ) {
                                                let res = client.post(url).json(&payload).send();
                                                if let Err(e) = res {
                                                    error!("error sending request: {:?}", e);
                                                    if e.is_connect() {
                                                        upload_transient = true;
                                                        break;
                                                    }
                                                }
                                            } else {
                                                error!("no vehicle entity for player?");
                                            }
                                        }
                                        debug!("Successfully sent all builds");
                                    }
                                    ReplayUploadAction::RawReplay => {
                                        #[cfg(not(feature = "shipbuilds_debugging"))]
                                        let url = "https://shipbuilds.com/api/replays";
                                        #[cfg(feature = "shipbuilds_debugging")]
                                        let url = "http://192.168.1.215:3000/api/replays";

                                        match std::fs::read(path) {
                                            Ok(bytes) => {
                                                let res = client
                                                    .post(url)
                                                    .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
                                                    .body(bytes)
                                                    .send();
                                                if let Err(e) = res {
                                                    error!("error sending replay: {:?}", e);
                                                    if e.is_connect() {
                                                        upload_transient = true;
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                error!("failed to read replay file for upload {:?}: {:?}", path, e)
                                            }
                                        }
                                    }
                                }

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
                            // Not a malformed replay: don't blacklist, just stop retrying.
                            return ParseOutcome::ParsedAndSent;
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
                        let (dummy_sender, _) = mpsc::channel();
                        let deps = crate::data::wows_data::ReplayDependencies {
                            wows_data_map: data.wows_data_map.clone(),
                            twitch_state: Arc::clone(&data.twitch_state),
                            replay_sort: Arc::new(Mutex::new(SortOrder::default())),
                            background_task_sender: dummy_sender,
                            is_debug_mode: data.is_debug,
                            personal_rating_data: Arc::clone(&data.personal_rating_data),
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
                        // Indexing already happened above (independent of upload). The
                        // upload either completed or hit a transient error; only the
                        // former marks the file sent.
                        return if upload_transient {
                            ParseOutcome::ParsedNotSent
                        } else {
                            ParseOutcome::ParsedAndSent
                        };
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
    DataSharingModeChanged(DataSharingMode),
    DataAutoExportSettingChange(DataExportSettings),
    DebugStateChange(bool),
}

pub struct BackgroundParserThread {
    pub rx: mpsc::Receiver<ReplayBackgroundParserThreadMessage>,
    pub sent_replays: Arc<RwLock<HashSet<String>>>,
    pub wows_data_map: crate::data::wows_data::WoWsDataMap,
    pub twitch_state: Arc<RwLock<TwitchState>>,
    pub data_sharing_mode: DataSharingMode,
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
}

pub fn start_background_parsing_thread(mut data: BackgroundParserThread) {
    debug!("starting background parsing thread");
    let _join_handle = crate::util::thread::spawn_logged("background-replay-parser", move || {
        let client = crate::util::http::blocking_client().expect("failed to build HTTP client");

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
            let Some(replays_dir) = data.wows_data_map.loaded_builds().first().map(|d| d.read().replays_dir.clone())
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
            }

            // Try to see if we have any historical replays we can send
            match std::fs::read_dir(&replays_dir) {
                Ok(read_dir) => {
                    let mut unindexable_dirty = false;
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
                        let sent = { data.sent_replays.read().contains(path_str.as_ref()) }
                            || cfg!(feature = "shipbuilds_debugging");
                        let indexed = indexed_paths.contains(path_str.as_ref());

                        let outcome = crate::data::replay_reconcile::reconcile_one(
                            &path,
                            indexed,
                            sent,
                            std::panic::AssertUnwindSafe(|| {
                                parse_replay_data_in_background(&path, &client, sent, &data)
                            }),
                        );

                        match outcome {
                            // Parsed: mark sent only when the upload also completed. A
                            // transient upload failure (sent == false) is left for retry
                            // but must not be blacklisted -- it was indexed regardless.
                            crate::data::replay_reconcile::FileOutcome::Parsed { sent: upload_sent } => {
                                if upload_sent && !sent {
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
                            | crate::data::replay_reconcile::FileOutcome::Skipped => {}
                        }
                    }

                    if unindexable_dirty
                        && let (Some(pool), Some(rt)) = (data.db_pool.as_ref(), data.tokio_runtime.as_ref())
                        && let Err(e) = rt.block_on(data.unindexable.save(pool))
                    {
                        warn!("failed to persist unindexable replay set: {e}");
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
                            already_parsed_replay,
                            std::panic::AssertUnwindSafe(|| {
                                parse_replay_data_in_background(&path, &client, already_parsed_replay, &data)
                            }),
                        );
                        match outcome {
                            crate::data::replay_reconcile::FileOutcome::Parsed { sent: true } => {
                                data.sent_replays.write().insert(path_str.into_owned());
                            }
                            crate::data::replay_reconcile::FileOutcome::HardFailure => {
                                if data.unindexable.insert(&path)
                                    && let (Some(pool), Some(rt)) = (data.db_pool.as_ref(), data.tokio_runtime.as_ref())
                                    && let Err(e) = rt.block_on(data.unindexable.save(pool))
                                {
                                    warn!("failed to persist unindexable replay set: {e}");
                                }
                            }
                            crate::data::replay_reconcile::FileOutcome::Parsed { sent: false }
                            | crate::data::replay_reconcile::FileOutcome::Transient
                            | crate::data::replay_reconcile::FileOutcome::Skipped => {}
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
                            already_parsed_replay,
                            std::panic::AssertUnwindSafe(|| {
                                parse_replay_data_in_background(&path, &client, already_parsed_replay, &data)
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
                ReplayBackgroundParserThreadMessage::DataSharingModeChanged(mode) => {
                    data.data_sharing_mode = mode;
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
    wows_data_map: crate::data::wows_data::WoWsDataMap,
    player_tracker: Arc<RwLock<PlayerTracker>>,
) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    crate::util::thread::spawn_logged("player-inspector", move || {
        for path in replays {
            match ReplayFile::from_file(&path) {
                Ok(replay_file) => {
                    let replay_version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);
                    let Some(wows_data_for_build) = wows_data_map.resolve(&replay_version) else {
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
    wows_data_map: &crate::data::wows_data::WoWsDataMap,
    twitch_state: &Arc<RwLock<TwitchState>>,
    db_pool: &sqlx::SqlitePool,
    tokio_runtime: &tokio::runtime::Runtime,
    personal_rating_data: &Arc<RwLock<crate::util::personal_rating::PersonalRatingData>>,
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
    let Some(wows_data_for_build) = wows_data_map.resolve(&replay_version) else {
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

    let (dummy_sender, _) = mpsc::channel();
    let deps = crate::data::wows_data::ReplayDependencies {
        wows_data_map: wows_data_map.clone(),
        twitch_state: Arc::clone(twitch_state),
        replay_sort: Arc::new(Mutex::new(SortOrder::default())),
        background_task_sender: dummy_sender,
        is_debug_mode: false,
        personal_rating_data: Arc::clone(personal_rating_data),
    };
    replay.build_ui_report(&deps);

    crate::data::replay_index::index_replay_blocking(
        tokio_runtime,
        db_pool,
        &replay,
        source_id,
        jiff::Timestamp::now(),
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
pub fn start_reconcile_index(
    wows_data_map: crate::data::wows_data::WoWsDataMap,
    twitch_state: Arc<RwLock<TwitchState>>,
    db_pool: sqlx::SqlitePool,
    tokio_runtime: Arc<tokio::runtime::Runtime>,
    personal_rating_data: Arc<RwLock<crate::util::personal_rating::PersonalRatingData>>,
    force_reindex: bool,
) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    let (progress_tx, progress_rx) = mpsc::channel();

    crate::util::thread::spawn_logged("reconcile-index", move || {
        let _ = tx.send(run_reconcile_index(
            wows_data_map,
            twitch_state,
            db_pool,
            tokio_runtime,
            personal_rating_data,
            force_reindex,
            &progress_tx,
        ));
    });

    BackgroundTask {
        receiver: Some(rx),
        kind: BackgroundTaskKind::ReconcilingIndex { rx: progress_rx, last_progress: None },
    }
}

fn run_reconcile_index(
    wows_data_map: crate::data::wows_data::WoWsDataMap,
    twitch_state: Arc<RwLock<TwitchState>>,
    db_pool: sqlx::SqlitePool,
    tokio_runtime: Arc<tokio::runtime::Runtime>,
    personal_rating_data: Arc<RwLock<crate::util::personal_rating::PersonalRatingData>>,
    force_reindex: bool,
    progress_tx: &mpsc::Sender<IndexProgress>,
) -> Result<BackgroundTaskCompletion, Report> {
    let Some(replays_dir) = wows_data_map.loaded_builds().first().map(|d| d.read().replays_dir.clone()) else {
        return Err(report!("no game data loaded, cannot enumerate replays directory"));
    };

    let now = jiff::Timestamp::now();
    let source_id = tokio_runtime
        .block_on(crate::db::index::query::ensure_default_source(&db_pool, &replays_dir, now))
        .map_err(|e| report!("failed to resolve replay index source: {e}"))?;

    let indexed_paths: HashSet<String> = tokio_runtime
        .block_on(crate::db::index::query::record_paths_in_source(&db_pool, source_id))
        .unwrap_or_default();
    let mut unindexable = tokio_runtime.block_on(crate::data::replay_reconcile::Unindexable::load(&db_pool));

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

        // `sent` is forced true: this task has no upload ledger of its own, so
        // the skip decision depends only on whether the replay is already indexed
        // (which `force_reindex` short-circuits to false so every non-blacklisted
        // file is re-parsed and re-upserted).
        let outcome = crate::data::replay_reconcile::reconcile_one(
            &path,
            already_indexed,
            true,
            std::panic::AssertUnwindSafe(|| {
                index_one_replay(
                    &path,
                    &wows_data_map,
                    &twitch_state,
                    &db_pool,
                    &tokio_runtime,
                    &personal_rating_data,
                    source_id,
                )
            }),
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

    if unindexable_dirty && let Err(e) = tokio_runtime.block_on(unindexable.save(&db_pool)) {
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
    let (tx, rx) = mpsc::channel();

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

/// Walk `root`, resolve or create its index source, and build a [`Replay`] per
/// file it holds, for the workspace `root` was opened as.
///
/// The build is resolved per replay rather than once for the directory: a
/// dropped folder routinely spans game versions, and the single-latest-build
/// shortcut `load_wows_files` takes would panic on the first replay from a
/// version that is not the newest one installed.
pub fn start_ingest_directory(
    deps: crate::data::wows_data::ReplayDependencies,
    pool: sqlx::SqlitePool,
    tokio_runtime: Arc<tokio::runtime::Runtime>,
    workspace: crate::db::index::rows::WorkspaceId,
    root: PathBuf,
) -> BackgroundTask {
    let (tx, rx) = mpsc::channel();
    let (update_tx, update_rx) = mpsc::channel();

    crate::util::thread::spawn_logged("ingest-directory", move || {
        let _ = tx.send(run_ingest_directory(deps, pool, tokio_runtime, workspace, root, &update_tx));
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
    /// How far the walk has got, with no replay to carry it. A run of files the
    /// walk cannot read would otherwise leave the count standing still, which
    /// is what a stalled walk looks like.
    Progress { workspace: crate::db::index::rows::WorkspaceId, progress: IngestProgress },
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

/// A game build some replay needed and the toolkit does not have installed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MissingBuild {
    pub build: u32,
    /// The replay's `major.minor.patch` version, which is what the game-data
    /// repository is keyed on when no exact build match is published.
    pub version: String,
}

/// Why replays in a directory could not be loaded. Missing game data is
/// actionable -- the toolkit can offer to fetch it -- so it is tracked
/// separately from failures the user can do nothing about.
#[derive(Debug, Default, Clone)]
pub struct IngestFailures {
    pub missing_builds: BTreeMap<MissingBuild, usize>,
    pub unreadable: usize,
    /// Replays that loaded and are listed, but whose index rows could not be
    /// written. Their rows fall back to the not-indexed placeholder, so they
    /// are worth reporting, but they are not missing from the listing and so
    /// are counted apart from the replays that never loaded.
    pub not_indexed: usize,
}

impl IngestFailures {
    /// Attribute one failed replay. Anything that is not a recognisably
    /// missing build is counted as unreadable, since nothing can be offered
    /// for it.
    pub fn record(&mut self, report: &Report) {
        match report.downcast_current_context::<ToolkitError>() {
            Some(ToolkitError::ReplayBuildUnavailable { build, version, .. }) => {
                let missing = MissingBuild { build: *build, version: version.clone() };
                *self.missing_builds.entry(missing).or_default() += 1;
            }
            _ => self.unreadable += 1,
        }
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
}

/// Read every path in `paths`, sending the replays that load as batches and
/// keeping the progress count moving through the ones that do not.
///
/// `read` and `index` are the steps that touch the filesystem, the game data
/// and the database. They are parameters so the gating, ordering and failure
/// handling around them are exercisable without any of the three.
fn ingest_walk<N, R, I>(
    paths: Vec<PathBuf>,
    workspace: crate::db::index::rows::WorkspaceId,
    source: crate::db::index::rows::SourceId,
    needs_index: N,
    read: R,
    index: I,
    tx: &mpsc::Sender<IngestUpdate>,
) -> WalkOutcome
where
    N: Fn(&Path) -> bool,
    R: Fn(&Path) -> Result<ReadReplay, Report>,
    I: Fn(&ReadReplay, crate::db::index::rows::SourceId) -> Result<(), Report>,
{
    let total = paths.len();
    let mut outcome = WalkOutcome::default();
    let mut pending: HashMap<PathBuf, Arc<ListedReplay>> = HashMap::new();
    let mut last_flush = std::time::Instant::now();

    let found: HashSet<PathBuf> = paths.iter().cloned().collect();
    if tx.send(IngestUpdate::Walked { workspace, paths: found }).is_err() {
        return outcome;
    }

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

        let progress = IngestProgress { done: visited + 1, total };
        let update = match flush_now(pending.len(), last_flush.elapsed()) {
            Flush::Hold => continue,
            Flush::Batch => {
                IngestUpdate::Batch(IngestBatch { workspace, source, replays: std::mem::take(&mut pending), progress })
            }
            Flush::Progress => IngestUpdate::Progress { workspace, progress },
        };
        if tx.send(update).is_err() {
            // Nothing is listening any more: the workspace closed or the app is
            // shutting down. The rest of the walk has no reader.
            return outcome;
        }
        last_flush = std::time::Instant::now();
    }

    let progress = IngestProgress { done: total, total };
    let _ = tx.send(if pending.is_empty() {
        IngestUpdate::Progress { workspace, progress }
    } else {
        IngestUpdate::Batch(IngestBatch { workspace, source, replays: pending, progress })
    });

    outcome
}

fn run_ingest_directory(
    deps: crate::data::wows_data::ReplayDependencies,
    pool: sqlx::SqlitePool,
    tokio_runtime: Arc<tokio::runtime::Runtime>,
    workspace: crate::db::index::rows::WorkspaceId,
    root: PathBuf,
    update_tx: &mpsc::Sender<IngestUpdate>,
) -> Result<BackgroundTaskCompletion, Report> {
    let source = tokio_runtime
        .block_on(crate::db::index::query::ensure_source(
            &pool,
            &crate::ui::replay_parser::shorten_root(&root),
            crate::db::index::rows::SourceKind::ImportedDir,
            &root,
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
        let (replay, wows_data) =
            crate::data::wows_data::ReplayLoader::build_replay_from_path(&deps, path.to_path_buf())?;
        let game_build = wows_data.read().patch_version;
        Ok(ReadReplay { replay, game_build })
    };
    let index = |replay: &ReadReplay, source| index_ingested_replay(replay, &deps, &pool, &tokio_runtime, source);
    let needs_index =
        |path: &Path| !indexed_paths.contains(path.to_string_lossy().as_ref()) && !unindexable.contains(path);

    let WalkOutcome { failures, index_panics } =
        ingest_walk(walk_replay_files(&root), workspace, source, needs_index, read, index, update_tx);

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
        let completion = task.receiver.unwrap().recv().unwrap().unwrap();
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

        let completion = task.receiver.unwrap().recv().unwrap().unwrap();
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

    #[test]
    fn a_missing_build_report_is_classified_as_a_missing_build() {
        let mut failures = IngestFailures::default();
        failures.record(&missing_build_report(9_876, "13.5.0"));
        assert_eq!(failures.unreadable, 0);
        assert_eq!(
            failures.missing_builds,
            BTreeMap::from([(MissingBuild { build: 9_876, version: "13.5.0".into() }, 1)])
        );
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
        assert_eq!(
            failures.missing_builds,
            BTreeMap::from([(MissingBuild { build: 9_876, version: "13.5.0".into() }, 3)])
        );
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
    /// sent, in the order it sent it, and what it left behind.
    fn run_walk<N, R, I>(paths: &[&str], needs_index: N, read: R, index: I) -> (Vec<IngestUpdate>, WalkOutcome)
    where
        N: Fn(&Path) -> bool,
        R: Fn(&Path) -> Result<ReadReplay, Report>,
        I: Fn(&ReadReplay, crate::db::index::rows::SourceId) -> Result<(), Report>,
    {
        let (tx, rx) = mpsc::channel();
        let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let outcome = ingest_walk(paths, WALK_WORKSPACE, WALK_SOURCE, needs_index, read, index, &tx);
        drop(tx);
        (rx.into_iter().collect(), outcome)
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

    /// The listing is merged into, never replaced, so a replay deleted between
    /// two walks of the same directory would otherwise outlive its file. The
    /// walk names the files it found before reading any of them, which is what
    /// lets the listing drop the rest.
    #[test]
    fn the_walk_names_the_files_it_found_before_it_sends_any_replay() {
        let (updates, _) = run_walk(&["a.wowsreplay"], |_| true, |_| Ok(read_replay(11)), |_, _| Ok(()));

        match updates.first() {
            Some(IngestUpdate::Walked { workspace, paths }) => {
                assert_eq!(*workspace, WALK_WORKSPACE);
                assert_eq!(*paths, HashSet::from([PathBuf::from("a.wowsreplay")]));
            }
            _ => panic!("the walk must announce the files it found before anything else"),
        }
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
                IngestUpdate::Progress { workspace, progress }
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
}

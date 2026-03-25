use clap::Parser;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use rootcause::prelude::*;
use std::cell::Cell;
use std::fs::File;
use std::path::PathBuf;
use tracing::info;
use tracing::warn;
use wowsunpack::data::Version;
use wowsunpack::game_data;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::vfs::VfsPath;

use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::game_constants::GameConstants;

use wows_minimap_renderer::assets::load_building_icons;
use wows_minimap_renderer::assets::load_consumable_icons;
use wows_minimap_renderer::assets::load_death_cause_icons;
use wows_minimap_renderer::assets::load_flag_icons;
use wows_minimap_renderer::assets::load_game_fonts;
use wows_minimap_renderer::assets::load_map_image;
use wows_minimap_renderer::assets::load_map_info;
use wows_minimap_renderer::assets::load_packed_image;
use wows_minimap_renderer::assets::load_plane_icons;
use wows_minimap_renderer::assets::load_powerup_icons;
use wows_minimap_renderer::assets::load_ship_icons;
use wows_minimap_renderer::config::RendererConfig;
use wows_minimap_renderer::drawing::ImageTarget;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_minimap_renderer::video::DumpMode;
use wows_minimap_renderer::video::RenderStage;
use wows_minimap_renderer::video::VideoEncoder;

/// Generates a minimap timelapse video from a WoWS replay
#[derive(Parser)]
#[command(name = "Minimap Renderer")]
struct Args {
    /// Path to the World of Warships game directory
    #[arg(short = 'g', long = "game", conflicts_with = "extracted_dir", required_unless_present_any = ["generate_config", "check_encoder", "extracted_dir"])]
    game_dir: Option<PathBuf>,

    /// Path to pre-extracted renderer data directory (alternative to --game)
    #[arg(long, conflicts_with = "game_dir", required_unless_present_any = ["generate_config", "check_encoder", "game_dir"])]
    extracted_dir: Option<PathBuf>,

    /// Output MP4 file path
    #[arg(short, long, required_unless_present_any = ["generate_config", "check_encoder"])]
    output: Option<PathBuf>,

    /// Dump a single frame as PNG instead of rendering video (specify frame number, 'mid' for midpoint or 'last' for last frame)
    #[arg(long, conflicts_with = "dump_frames")]
    dump_frame: Option<String>,

    /// Dump all frames as PNGs instead of rendering video (output flag must specify directory where files will be placed)
    #[arg(long, conflicts_with = "dump_frame")]
    dump_frames: bool,

    /// Hide player names above ship icons
    #[arg(long)]
    no_player_names: bool,

    /// Hide ship names above ship icons
    #[arg(long)]
    no_ship_names: bool,

    /// Hide capture point zones
    #[arg(long)]
    no_capture_points: bool,

    /// Hide building markers
    #[arg(long)]
    no_buildings: bool,

    /// Hide turret direction indicators
    #[arg(long)]
    no_turret_direction: bool,

    /// Hide selected armament/ammo type below ship icons
    #[arg(long)]
    no_armament: bool,

    /// Show position trail heatmap (rainbow coloring)
    #[arg(long)]
    show_trails: bool,

    /// Hide trails for dead ships
    #[arg(long)]
    no_dead_trails: bool,

    /// Show speed-based trails (blue=slow, red=fast)
    #[arg(long)]
    show_speed_trails: bool,

    /// Show ship config range circles (detection, battery, etc.)
    #[arg(long)]
    show_ship_config: bool,

    /// Path to TOML config file
    #[arg(long)]
    config: Option<PathBuf>,

    /// Print default TOML config to stdout and exit
    #[arg(long)]
    generate_config: bool,

    /// Check encoder availability (GPU/CPU) and exit
    #[arg(long)]
    check_encoder: bool,

    /// Use CPU encoder (openh264) instead of GPU
    #[arg(long)]
    cpu: bool,

    /// Disable progress bar and use log output instead
    #[arg(long)]
    no_progress: bool,

    /// Path to a constants JSON file (from wows-constants repo) to override
    /// consumable IDs, battle stages, etc.
    #[arg(short = 'c', long = "constants")]
    constants: Option<PathBuf>,

    /// The replay file to process
    #[arg(required_unless_present_any = ["generate_config", "check_encoder"])]
    replay: Option<PathBuf>,
}

fn main() -> Result<(), Report> {
    let args = Args::parse();

    tracing_subscriber::fmt().with_target(false).with_writer(std::io::stderr).init();

    // Handle --generate-config before anything else
    if args.generate_config {
        print!("{}", RendererConfig::generate_default_toml());
        return Ok(());
    }

    // Handle --check-encoder
    if args.check_encoder {
        let status = wows_minimap_renderer::check_encoder();
        print!("{status}");
        return Ok(());
    }

    let output = args.output.as_ref().expect("output is required");
    let replay_path = args.replay.as_ref().expect("replay is required");

    let dump_mode = match args.dump_frame.as_deref() {
        Some("mid") => Some(DumpMode::Midpoint),
        Some("last") => Some(DumpMode::Last),
        Some(n) => Some(DumpMode::Frame(n.parse::<usize>().expect("invalid frame number"))),
        None => None,
    };

    info!("Parsing replay");
    let replay_file = ReplayFile::from_file(replay_path)?;
    let replay_version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);

    // Load game data from either a full game install or pre-extracted directory
    let (vfs_owned, specs, game_params, controller_game_params) = if let Some(ref extracted) = args.extracted_dir {
        let resolved = resolve_extracted_dir(extracted, &replay_version)?;
        load_from_extracted(&resolved, &replay_version)?
    } else {
        let game_dir = args.game_dir.as_ref().expect("game directory is required");
        load_from_game_dir(game_dir, &replay_version)?
    };
    let vfs = &vfs_owned;

    info!("Loading fonts and icons");
    let game_fonts = load_game_fonts(vfs);
    let ship_icons = load_ship_icons(vfs);
    let plane_icons = load_plane_icons(vfs);
    let building_icons = load_building_icons(vfs);
    let consumable_icons = load_consumable_icons(vfs);
    let death_cause_icons = load_death_cause_icons(vfs, wows_minimap_renderer::assets::ICON_SIZE);
    let powerup_icons = load_powerup_icons(vfs, wows_minimap_renderer::assets::ICON_SIZE);
    let flag_icons = load_flag_icons(vfs);

    // Load game constants from game data (falls back to hardcoded defaults per-field)
    let mut game_constants = GameConstants::from_vfs(vfs);
    if let Some(ref constants_path) = args.constants {
        let data = std::fs::read_to_string(constants_path)
            .unwrap_or_else(|e| panic!("Failed to read constants file {}: {e}", constants_path.display()));
        let json: serde_json::Value =
            serde_json::from_str(&data).unwrap_or_else(|e| panic!("Failed to parse constants JSON: {e}"));
        game_constants.merge_replay_constants(&json, replay_version.build);
        info!("Merged replay constants from {}", constants_path.display());
    }

    if let Some(mode_name) = game_constants.game_mode_name(replay_file.meta.gameMode as i32) {
        info!(mode = %mode_name, id = replay_file.meta.gameMode, "Game mode");
    }

    // Load map image and metadata from game files
    let map_name = &replay_file.meta.mapName;
    let map_image = load_map_image(map_name, vfs);
    let map_info = load_map_info(map_name, vfs);

    let game_duration = replay_file.meta.duration as f32;

    // Load config: --config path > exe-adjacent minimap_renderer.toml > defaults
    let mut config = if let Some(config_path) = &args.config {
        RendererConfig::load(config_path)?
    } else {
        // Try exe-adjacent config file
        let exe_config = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("minimap_renderer.toml")));
        match exe_config {
            Some(path) if path.exists() => {
                info!(path = ?path, "Loading config");
                RendererConfig::load(&path)?
            }
            _ => RendererConfig::default(),
        }
    };
    config.apply_cli_overrides(&wows_minimap_renderer::config::CliOverrides {
        no_player_names: args.no_player_names,
        no_ship_names: args.no_ship_names,
        no_capture_points: args.no_capture_points,
        no_buildings: args.no_buildings,
        no_turret_direction: args.no_turret_direction,
        no_armament: args.no_armament,
        show_trails: args.show_trails,
        no_dead_trails: args.no_dead_trails,
        show_speed_trails: args.show_speed_trails,
        show_ship_config: args.show_ship_config,
    });
    let mut options = config.into_render_options();
    options.ship_config_visibility = wows_minimap_renderer::ShipConfigVisibility::SelfOnly;

    let mut target = ImageTarget::with_stats_panel(
        map_image,
        game_fonts.clone(),
        ship_icons,
        plane_icons,
        building_icons,
        consumable_icons,
        death_cause_icons,
        powerup_icons,
        options.show_stats_panel,
    );

    // Load self player's ship silhouette for the stats panel
    let self_silhouette = replay_file.meta.vehicles.iter().find(|v| v.relation == 0).and_then(|v| {
        let param = GameParamProvider::game_param_by_id(&game_params, v.shipId)?;
        let path = format!("gui/ships_silhouettes/{}.png", param.index());
        let img = load_packed_image(&path, vfs)?;
        Some(img.into_rgba8())
    });

    let mut renderer = MinimapRenderer::new(map_info.clone(), &game_params, replay_version, options);
    renderer.set_fonts(game_fonts);
    renderer.set_flag_icons(flag_icons);
    if let Some(sil) = self_silhouette {
        renderer.set_self_silhouette(sil);
    }

    let (cw, ch) = target.canvas_size();
    let mut encoder = VideoEncoder::new(output.to_str().unwrap(), dump_mode, args.dump_frames, game_duration, cw, ch);
    if args.cpu {
        encoder.set_prefer_cpu(true);
    }
    // Initialize the encoder eagerly so startup logs appear before the
    // progress bar. Skip for dump modes which don't encode video.
    if args.dump_frame.is_none() {
        encoder.init()?;
    }

    let use_progress_bar = !args.no_progress;
    let progress_bar = if use_progress_bar {
        let pb = ProgressBar::new(0);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{msg} [{bar:40}] {pos}/{len} ({eta})")
                .expect("valid progress template")
                .progress_chars("=> "),
        );
        pb.set_message("Encoding");
        let pb_clone = pb.clone();
        let current_stage = Cell::new(RenderStage::Encoding);
        encoder.set_progress_callback(move |p| {
            if p.stage != current_stage.get() {
                current_stage.set(p.stage);
                pb_clone.set_position(0);
                pb_clone.set_message(match p.stage {
                    RenderStage::Encoding => "Encoding",
                    RenderStage::Muxing => "Muxing",
                });
            }
            pb_clone.set_length(p.total);
            pb_clone.set_position(p.current);
        });
        Some(pb)
    } else {
        let last_reported = Cell::new(RenderStage::Encoding);
        encoder.set_progress_callback(move |p| {
            if p.stage != last_reported.get() {
                last_reported.set(p.stage);
                info!(stage = ?p.stage, total = p.total, "Starting stage");
            }
            if p.current % 100 == 0 || p.current == p.total {
                info!(stage = ?p.stage, frame = p.current, total = p.total, "Progress");
            }
        });
        None
    };

    // Pre-scan packets to find the last clock for accurate progress reporting.
    {
        let mut scan_parser = wows_replays::packet2::Parser::new(&specs);
        let mut scan_remaining = &replay_file.packet_data[..];
        let mut last_clock = wows_replays::types::GameClock(0.0);
        while !scan_remaining.is_empty() {
            match scan_parser.parse_packet(&mut scan_remaining) {
                Ok(packet) => {
                    last_clock = wows_replays::types::GameClock(packet.clock.0.max(last_clock.0));
                }
                Err(_) => break,
            }
        }
        if last_clock.seconds() > 0.0 {
            encoder.set_battle_duration(last_clock);
        }
    }

    let mut controller = BattleController::new(&replay_file.meta, &controller_game_params, Some(&game_constants));

    let mut parser = wows_replays::packet2::Parser::new(&specs);
    let mut remaining = &replay_file.packet_data[..];
    let mut prev_clock = wows_replays::types::GameClock(0.0);

    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).map_err(|e| report!("Packet parse error: {e:?}"))?;

        // Render when clock changes (all prev_clock packets have been processed)
        if packet.clock != prev_clock && prev_clock.seconds() > 0.0 {
            renderer.populate_players(&controller);
            renderer.update_squadron_info(&controller);
            renderer.update_ship_abilities(&controller);
            encoder.advance_clock(prev_clock, &controller, &mut renderer, &mut target);
            prev_clock = packet.clock;
        } else if prev_clock.seconds() == 0.0 {
            prev_clock = packet.clock;
        }

        // Process the packet to update state
        controller.process(&packet);
    }

    // Render final tick
    if prev_clock.seconds() > 0.0 {
        renderer.populate_players(&controller);
        renderer.update_squadron_info(&controller);
        renderer.update_ship_abilities(&controller);
        encoder.advance_clock(prev_clock, &controller, &mut renderer, &mut target);
    }

    controller.finish();
    encoder.finish(&controller, &mut renderer, &mut target)?;

    if let Some(pb) = progress_bar {
        pb.finish_and_clear();
    }

    info!("Done");
    Ok(())
}

type LoadedGameData =
    (VfsPath, Vec<wowsunpack::rpc::entitydefs::EntitySpec>, GameMetadataProvider, GameMetadataProvider);

/// Load game data from a full WoWS game installation.
fn load_from_game_dir(game_dir: &std::path::Path, replay_version: &Version) -> Result<LoadedGameData, Report> {
    info!(build = %replay_version.build, "Loading game data");
    let resources = game_data::load_game_resources(game_dir, replay_version).map_err(|e| report!("{e}"))?;
    let vfs = resources.vfs;
    let specs = resources.specs;

    info!("Loading game params");
    let mut game_params =
        GameMetadataProvider::from_vfs(&vfs).map_err(|e| report!("Failed to load GameParams: {e:?}"))?;
    let mut controller_game_params =
        GameMetadataProvider::from_vfs(&vfs).map_err(|e| report!("Failed to load GameParams for controller: {e:?}"))?;

    let mo_path = game_data::translations_path(game_dir, replay_version.build);
    load_translations(&mo_path, &mut game_params, &mut controller_game_params);

    Ok((vfs, specs, game_params, controller_game_params))
}

struct ExtractedMetadata {
    version: String,
    build: u32,
}

fn read_metadata(path: &std::path::Path) -> Option<ExtractedMetadata> {
    let contents = std::fs::read_to_string(path.join("metadata.toml")).ok()?;
    let table: toml::Table = contents.parse().ok()?;
    Some(ExtractedMetadata {
        version: table.get("version")?.as_str()?.to_string(),
        build: table.get("build")?.as_integer()? as u32,
    })
}

/// Resolve the extracted data directory. If the user passed a parent directory
/// containing version subdirectories (e.g. `15.1.0_11965230/`), auto-detect the
/// right one. If they passed the version dir itself, use it directly.
fn resolve_extracted_dir(path: &std::path::Path, replay_version: &Version) -> Result<PathBuf, Report> {
    if !path.exists() {
        bail!("Extracted data directory does not exist: {}", path.display());
    }

    // If the path itself contains metadata.toml, it's already the version dir
    if let Some(meta) = read_metadata(path) {
        if meta.build != replay_version.build {
            bail!(
                "Extracted data is build {} ({}) but replay is build {}. \
                 Entity definitions will not match. Use extracted data for the correct build.",
                meta.build,
                meta.version,
                replay_version.build
            );
        }
        return Ok(path.to_path_buf());
    }

    // Otherwise, scan for version subdirectories
    let mut candidates: Vec<(PathBuf, ExtractedMetadata)> = Vec::new();
    let entries = std::fs::read_dir(path).attach_with(|| format!("Failed to read directory: {}", path.display()))?;

    for entry in entries.flatten() {
        let sub = entry.path();
        if let Some(meta) = read_metadata(&sub) {
            candidates.push((sub, meta));
        }
    }

    if candidates.is_empty() {
        bail!(
            "No extracted game data found in {}. Expected either a version directory \
             (containing metadata.toml, vfs/, game_params.rkyv) or a parent directory \
             containing version subdirectories (e.g. 15.1.0_11965230/).",
            path.display()
        );
    }

    // Try to match by build number first
    if let Some(matched) = candidates.iter().find(|(_, m)| m.build == replay_version.build) {
        info!("Matched extracted data for build {}: {}", replay_version.build, matched.0.display());
        return Ok(matched.0.clone());
    }

    // No match — fail with available versions
    if candidates.len() == 1 {
        let (_, ref meta) = candidates[0];
        bail!(
            "No exact build match for replay (build {}). Only available: {} (build {}). \
             Download or extract the correct build.",
            replay_version.build,
            meta.version,
            meta.build
        );
    }

    let available: Vec<String> = candidates.iter().map(|(_, m)| format!("{} (build {})", m.version, m.build)).collect();
    bail!(
        "No extracted data matches replay build {}. Available versions in {}: {}",
        replay_version.build,
        path.display(),
        available.join(", ")
    );
}

/// Load game data from a pre-extracted renderer data directory.
fn load_from_extracted(extracted_dir: &std::path::Path, _replay_version: &Version) -> Result<LoadedGameData, Report> {
    use std::borrow::Cow;
    use std::io::Read;
    use wowsunpack::data::DataFileWithCallback;
    use wowsunpack::rpc::entitydefs::parse_scripts;
    use wowsunpack::vfs::impls::physical::PhysicalFS;

    info!("Loading from extracted directory: {}", extracted_dir.display());

    let vfs_root = extracted_dir.join("vfs");
    if !vfs_root.exists() {
        bail!("VFS directory not found: {}", vfs_root.display());
    }
    let vfs = VfsPath::new(PhysicalFS::new(&vfs_root));

    // Load entity specs from VFS
    info!("Loading entity specs");
    let specs = {
        let vfs_ref = &vfs;
        let loader = DataFileWithCallback::new(move |path: &str| {
            let mut data = Vec::new();
            vfs_ref.join(path)?.open_file()?.read_to_end(&mut data)?;
            Ok(Cow::Owned(data))
        });
        parse_scripts(&loader).map_err(|e| report!("Failed to parse entity specs: {e:?}"))?
    };

    // Load GameParams from rkyv cache
    let rkyv_path = extracted_dir.join("game_params.rkyv");
    info!("Loading game params from rkyv cache");
    let rkyv_data = std::fs::read(&rkyv_path).attach_with(|| format!("Failed to read {}", rkyv_path.display()))?;
    let params: Vec<Param> = rkyv::from_bytes::<Vec<Param>, rkyv::rancor::Error>(&rkyv_data)
        .map_err(|e| report!("Failed to deserialize GameParams: {e}"))?;

    // Use from_params_no_specs since we already have specs separately
    let mut game_params = GameMetadataProvider::from_params_no_specs(params.clone())
        .map_err(|e| report!("Failed to build GameMetadataProvider: {e:?}"))?;
    let mut controller_game_params = GameMetadataProvider::from_params_no_specs(params)
        .map_err(|e| report!("Failed to build controller GameMetadataProvider: {e:?}"))?;

    // Load translations
    let mo_path = extracted_dir.join("translations/en/LC_MESSAGES/global.mo");
    load_translations(&mo_path, &mut game_params, &mut controller_game_params);

    Ok((vfs, specs, game_params, controller_game_params))
}

fn load_translations(
    mo_path: &std::path::Path,
    game_params: &mut GameMetadataProvider,
    controller_game_params: &mut GameMetadataProvider,
) {
    if mo_path.exists() {
        if let Ok(file) = File::open(mo_path)
            && let Ok(catalog) = gettext::Catalog::parse(file)
        {
            game_params.set_translations(catalog);
            if let Ok(file2) = File::open(mo_path)
                && let Ok(catalog2) = gettext::Catalog::parse(file2)
            {
                controller_game_params.set_translations(catalog2);
            }
        } else {
            warn!(path = ?mo_path, "Failed to parse translations");
        }
    } else {
        warn!(path = ?mo_path, "Translations not found, ship names will be unavailable");
    }
}

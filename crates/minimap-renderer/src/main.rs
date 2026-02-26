use clap::{App, Arg};
use indicatif::{ProgressBar, ProgressStyle};
use rootcause::prelude::*;
use std::cell::Cell;
use std::fs::File;
use std::path::Path;
use tracing::{info, warn};
use wowsunpack::data::Version;
use wowsunpack::game_data;
use wowsunpack::game_params::provider::GameMetadataProvider;

use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::BattleController;
use wows_replays::game_constants::GameConstants;

use wows_minimap_renderer::assets::{
    load_consumable_icons, load_death_cause_icons, load_flag_icons, load_game_fonts, load_map_image, load_map_info,
    load_plane_icons, load_powerup_icons, load_ship_icons,
};
use wows_minimap_renderer::config::RendererConfig;
use wows_minimap_renderer::drawing::ImageTarget;
use wows_minimap_renderer::renderer::MinimapRenderer;
use wows_minimap_renderer::video::{DumpMode, RenderStage, VideoEncoder};

fn main() -> Result<(), Report> {
    let matches = App::new("Minimap Renderer")
        .about("Generates a minimap timelapse video from a WoWS replay")
        .arg(
            Arg::with_name("GAME_DIRECTORY")
                .help("Path to the World of Warships game directory")
                .short("g")
                .long("game")
                .takes_value(true)
                .required_unless_one(&["GENERATE_CONFIG", "CHECK_ENCODER"]),
        )
        .arg(
            Arg::with_name("OUTPUT")
                .help("Output MP4 file path")
                .short("o")
                .long("output")
                .takes_value(true)
                .required_unless_one(&["GENERATE_CONFIG", "CHECK_ENCODER"]),
        )
        .arg(
            Arg::with_name("DUMP_FRAME")
                .help("Dump a single frame as PNG instead of rendering video (specify frame number or 'mid' for midpoint)")
                .long("dump-frame")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("NO_PLAYER_NAMES")
                .help("Hide player names above ship icons")
                .long("no-player-names"),
        )
        .arg(
            Arg::with_name("NO_SHIP_NAMES")
                .help("Hide ship names above ship icons")
                .long("no-ship-names"),
        )
        .arg(
            Arg::with_name("NO_CAPTURE_POINTS")
                .help("Hide capture point zones")
                .long("no-capture-points"),
        )
        .arg(
            Arg::with_name("NO_BUILDINGS")
                .help("Hide building markers")
                .long("no-buildings"),
        )
        .arg(
            Arg::with_name("NO_TURRET_DIRECTION")
                .help("Hide turret direction indicators")
                .long("no-turret-direction"),
        )
        .arg(
            Arg::with_name("NO_ARMAMENT")
                .help("Hide selected armament/ammo type below ship icons")
                .long("no-armament"),
        )
        .arg(
            Arg::with_name("SHOW_TRAILS")
                .help("Show position trail heatmap (rainbow coloring)")
                .long("show-trails"),
        )
        .arg(
            Arg::with_name("NO_DEAD_TRAILS")
                .help("Hide trails for dead ships")
                .long("no-dead-trails"),
        )
        .arg(
            Arg::with_name("SHOW_SPEED_TRAILS")
                .help("Show speed-based trails (blue=slow, red=fast)")
                .long("show-speed-trails"),
        )
        .arg(
            Arg::with_name("SHOW_SHIP_CONFIG")
                .help("Show ship config range circles (detection, battery, etc.)")
                .long("show-ship-config"),
        )
        .arg(
            Arg::with_name("CONFIG")
                .help("Path to TOML config file")
                .long("config")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("GENERATE_CONFIG")
                .help("Print default TOML config to stdout and exit")
                .long("generate-config"),
        )
        .arg(
            Arg::with_name("CHECK_ENCODER")
                .help("Check encoder availability (GPU/CPU) and exit")
                .long("check-encoder"),
        )
        .arg(
            Arg::with_name("CPU")
                .help("Use CPU encoder (openh264) instead of GPU")
                .long("cpu"),
        )
        .arg(
            Arg::with_name("NO_PROGRESS")
                .help("Disable progress bar and use log output instead")
                .long("no-progress"),
        )
        .arg(
            Arg::with_name("REPLAY")
                .help("The replay file to process")
                .required_unless_one(&["GENERATE_CONFIG", "CHECK_ENCODER"])
                .index(1),
        )
        .get_matches();

    tracing_subscriber::fmt().with_target(false).init();

    // Handle --generate-config before anything else
    if matches.is_present("GENERATE_CONFIG") {
        print!("{}", RendererConfig::generate_default_toml());
        return Ok(());
    }

    // Handle --check-encoder
    if matches.is_present("CHECK_ENCODER") {
        let status = wows_minimap_renderer::check_encoder();
        print!("{status}");
        return Ok(());
    }

    let game_dir = matches.value_of("GAME_DIRECTORY").unwrap();
    let output = matches.value_of("OUTPUT").unwrap();
    let replay_path = matches.value_of("REPLAY").unwrap();

    let dump_mode = match matches.value_of("DUMP_FRAME") {
        Some("mid") => Some(DumpMode::Midpoint),
        Some("last") => Some(DumpMode::Last),
        Some(n) => Some(DumpMode::Frame(n.parse::<usize>().expect("invalid frame number"))),
        None => None,
    };

    info!("Parsing replay");
    let replay_file = ReplayFile::from_file(&std::path::PathBuf::from(replay_path))?;
    let replay_version = Version::from_client_exe(&replay_file.meta.clientVersionFromExe);

    info!(build = %replay_version.build, "Loading game data");
    let wows_dir = Path::new(game_dir);
    let resources = game_data::load_game_resources(wows_dir, &replay_version).map_err(|e| report!("{e}"))?;
    let vfs = &resources.vfs;
    let specs = &resources.specs;

    info!("Loading game params");
    let mut game_params =
        GameMetadataProvider::from_vfs(vfs).map_err(|e| report!("Failed to load GameParams: {e:?}"))?;
    let mut controller_game_params =
        GameMetadataProvider::from_vfs(vfs).map_err(|e| report!("Failed to load GameParams for controller: {e:?}"))?;

    // Load translations for ship name localization
    let mo_path = game_data::translations_path(wows_dir, replay_version.build);
    if mo_path.exists() {
        let catalog =
            gettext::Catalog::parse(File::open(&mo_path)?).map_err(|e| report!("Failed to parse global.mo: {e:?}"))?;
        game_params.set_translations(catalog);
        // Also load translations for the controller (bot name/chat translation)
        let catalog2 = gettext::Catalog::parse(File::open(&mo_path)?)
            .map_err(|e| report!("Failed to parse global.mo for controller: {e:?}"))?;
        controller_game_params.set_translations(catalog2);
    } else {
        warn!(path = ?mo_path, "Translations not found, ship names will be unavailable");
    }

    info!("Loading fonts and icons");
    let game_fonts = load_game_fonts(vfs);
    let ship_icons = load_ship_icons(vfs);
    let plane_icons = load_plane_icons(vfs);
    let consumable_icons = load_consumable_icons(vfs);
    let death_cause_icons = load_death_cause_icons(vfs, wows_minimap_renderer::assets::ICON_SIZE);
    let powerup_icons = load_powerup_icons(vfs, wows_minimap_renderer::assets::ICON_SIZE);
    let flag_icons = load_flag_icons(vfs);

    // Load game constants from game data (falls back to hardcoded defaults per-field)
    let game_constants = GameConstants::from_vfs(vfs);

    if let Some(mode_name) = game_constants.game_mode_name(replay_file.meta.gameMode as i32) {
        info!(mode = %mode_name, id = replay_file.meta.gameMode, "Game mode");
    }

    // Load map image and metadata from game files
    let map_name = &replay_file.meta.mapName;
    let map_image = load_map_image(map_name, vfs);
    let map_info = load_map_info(map_name, vfs);

    let game_duration = replay_file.meta.duration as f32;

    let mut target = ImageTarget::new(
        map_image,
        game_fonts.clone(),
        ship_icons,
        plane_icons,
        consumable_icons,
        death_cause_icons,
        powerup_icons,
    );

    // Load config: --config path > exe-adjacent minimap_renderer.toml > defaults
    let mut config = if let Some(config_path) = matches.value_of("CONFIG") {
        RendererConfig::load(Path::new(config_path))?
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
    config.apply_cli_overrides(&matches);
    let mut options = config.into_render_options();
    options.ship_config_visibility = wows_minimap_renderer::ShipConfigVisibility::SelfOnly;

    let mut renderer = MinimapRenderer::new(map_info.clone(), &game_params, replay_version, options);
    renderer.set_fonts(game_fonts);
    renderer.set_flag_icons(flag_icons);

    let mut encoder = VideoEncoder::new(output, dump_mode, game_duration);
    if matches.is_present("CPU") {
        encoder.set_prefer_cpu(true);
    }
    // Initialize the encoder eagerly so startup logs appear before the
    // progress bar. Skip for dump modes which don't encode video.
    if !matches.is_present("DUMP_FRAME") {
        encoder.init()?;
    }

    let use_progress_bar = !matches.is_present("NO_PROGRESS");
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
        let mut scan_parser = wows_replays::packet2::Parser::new(specs);
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

    let mut parser = wows_replays::packet2::Parser::new(specs);
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

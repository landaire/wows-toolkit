//! Bakes a clip of a replay into the track file the web demo plays.
//!
//! ```text
//! export_demo_track <replay> <game dir> <output>
//!     [--from SECONDS] [--to SECONDS] [--interval SECONDS] [--activity]
//! ```
//!
//! `--from` and `--to` are seconds since the battle started. `--activity`
//! prints what happens in each half minute of the battle, for choosing a clip.
//! Player names are written as pseudonyms, and chat is left out: it is free
//! text pseudonyms cannot anonymize.

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use image::RgbaImage;
use wows_core::pseudonym::NameDisplay;
use wows_minimap_renderer::assets;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipConfigVisibility;
use wows_minimap_renderer::error::TrackFileError;
use wows_minimap_renderer::toggles::TOGGLES;
use wows_minimap_renderer::track_file::OfferedToggle;
use wows_minimap_renderer::track_file::RgbaIcon;
use wows_minimap_renderer::track_file::TrackFile;
use wows_minimap_renderer::track_file::TrackIcons;
use wows_replays::types::ElapsedClock;
use wows_replays::types::GameClock;
use wows_toolkit_replay_data::error::ReplayLoadError;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::minimap_preview::PreviewError;
use wows_toolkit_replay_data::minimap_preview::bake_track;
use wows_toolkit_replay_data::replay_render::options::playback_options;

const USAGE: &str = "usage: export_demo_track <replay> <game dir> <output> \
                     [--from SECONDS] [--to SECONDS] [--interval SECONDS] [--activity]";

/// The rosters' toggle widens the canvas into gutters the demo does not draw,
/// and chat is not baked.
const WITHHELD_TOGGLES: &[&str] = &["renderer-opt-team-rosters", "renderer-opt-chat"];

/// The size the desktop egui viewer loads these icon sets at.
const SMALL_ICON_SIZE: u32 = 16;

#[derive(Debug, thiserror::Error)]
enum ExportError {
    #[error("{USAGE}")]
    Usage,
    #[error("unknown argument {argument:?}\n{USAGE}")]
    UnknownArgument { argument: String },
    #[error("{flag} takes a number of seconds, got {value:?}")]
    BadSeconds { flag: &'static str, value: String },
    #[error("--interval takes a number of seconds above zero, got {value:?}")]
    BadInterval { value: String },
    #[error("the replay could not be baked")]
    Bake(#[from] PreviewError),
    #[error("the replay's version {version} names no build")]
    NoBuild { version: String },
    #[error("game data for build {build} did not load")]
    GameData {
        build: u32,
        #[source]
        source: ReplayLoadError,
    },
    #[error("the game data has no art for map {map}")]
    NoMapArt { map: String },
    #[error("the map art could not be encoded")]
    MapPng(#[source] image::ImageError),
    #[error("no frames fall inside the requested window")]
    EmptyClip,
    #[error("the translation catalog has no English text for {key}")]
    NoLabel { key: &'static str },
    #[error("the track file could not be encoded")]
    TrackFile(#[from] TrackFileError),
    #[error("{path} could not be written")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

struct Args {
    replay: PathBuf,
    game_dir: PathBuf,
    output: PathBuf,
    from: Option<ElapsedClock>,
    to: Option<ElapsedClock>,
    /// Game seconds per baked frame.
    frame_interval: f32,
    activity: bool,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self, ExportError> {
        let mut positional = Vec::new();
        let mut from = None;
        let mut to = None;
        let mut frame_interval = 0.5;
        let mut activity = false;

        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--from" => from = Some(ElapsedClock(seconds("--from", args.next())?)),
                "--to" => to = Some(ElapsedClock(seconds("--to", args.next())?)),
                "--interval" => frame_interval = interval(args.next())?,
                "--activity" => activity = true,
                flag if flag.starts_with("--") => return Err(ExportError::UnknownArgument { argument }),
                _ => positional.push(PathBuf::from(argument)),
            }
        }

        let [replay, game_dir, output] = <[PathBuf; 3]>::try_from(positional).map_err(|_| ExportError::Usage)?;

        Ok(Self { replay, game_dir, output, from, to, frame_interval, activity })
    }
}

fn seconds(flag: &'static str, value: Option<String>) -> Result<f32, ExportError> {
    let value = value.ok_or(ExportError::Usage)?;

    value
        .parse::<f32>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .ok_or(ExportError::BadSeconds { flag, value })
}

/// A frame interval: zero would draw frames without end, and a negative or
/// infinite one is no interval at all.
fn interval(value: Option<String>) -> Result<f32, ExportError> {
    let value = value.ok_or(ExportError::Usage)?;

    value
        .parse::<f32>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .ok_or(ExportError::BadInterval { value })
}

/// Icons sorted by key, so the same bake writes the same file.
fn icon_list(icons: HashMap<String, RgbaImage>, keep: impl Fn(&str) -> bool) -> Vec<(String, RgbaIcon)> {
    let mut list: Vec<(String, RgbaIcon)> = icons
        .into_iter()
        .filter(|(key, _)| keep(key))
        .map(|(key, image)| {
            let (width, height) = image.dimensions();
            (key, RgbaIcon { width, height, rgba: image.into_raw() })
        })
        .collect();
    list.sort_by(|a, b| a.0.cmp(&b.0));

    list
}

/// The plane, consumable and powerup icons the frames name. Those sets hold
/// hundreds of icons a clip never shows; the rest are small and kept whole.
#[derive(Default)]
struct UsedIcons {
    planes: BTreeSet<String>,
    consumables: BTreeSet<String>,
    powerups: BTreeSet<String>,
}

impl UsedIcons {
    fn of(frames: &[Vec<DrawCommand>]) -> Self {
        let mut used = Self::default();
        for command in frames.iter().flatten() {
            match command {
                DrawCommand::Plane { icon_key, .. } => {
                    used.planes.insert(icon_key.clone());
                }
                DrawCommand::ConsumableIcons { icon_keys, .. } => {
                    used.consumables.extend(icon_keys.iter().cloned());
                }
                DrawCommand::BuffZone { marker_name: Some(marker), .. } => {
                    used.powerups.insert(marker.clone());
                }
                DrawCommand::TeamBuffs { friendly_buffs, enemy_buffs } => {
                    used.powerups.extend(friendly_buffs.iter().chain(enemy_buffs).map(|(marker, _)| marker.clone()));
                }
                _ => {}
            }
        }

        used
    }
}

/// Prints, per half minute of the battle, how much fighting the frames show:
/// shells and torpedoes in flight, ships sunk, and the recording player's HP.
fn print_activity(frames: &[Vec<DrawCommand>], clocks: &[GameClock], battle_start: GameClock) {
    const WINDOW_SECONDS: f32 = 30.0;

    println!("window  frames  shells  torps  planes  sunk  self_hp");
    let mut window_start = None;
    let mut counts = [0usize; 4];
    let mut self_hp = (None, None);
    let mut sunk_before = 0usize;

    let flush = |start: f32, frames: usize, counts: &[usize; 4], self_hp: (Option<f32>, Option<f32>)| {
        let hp = match self_hp {
            (Some(first), Some(last)) => format!("{:.0}% -> {:.0}%", first * 100.0, last * 100.0),
            _ => "-".to_owned(),
        };
        println!(
            "{:02}:{:02}   {frames:>6}  {:>6}  {:>5}  {:>6}  {:>4}  {hp}",
            (start / 60.0) as u32,
            (start % 60.0) as u32,
            counts[0],
            counts[1],
            counts[2],
            counts[3],
        );
    };

    let mut frames_in_window = 0;
    for (commands, clock) in frames.iter().zip(clocks) {
        let elapsed = clock.seconds() - battle_start.seconds();
        if elapsed < 0.0 {
            continue;
        }
        let start = (elapsed / WINDOW_SECONDS).floor() * WINDOW_SECONDS;
        if window_start != Some(start) {
            if let Some(previous) = window_start {
                flush(previous, frames_in_window, &counts, self_hp);
            }
            window_start = Some(start);
            counts = [0; 4];
            self_hp = (None, None);
            frames_in_window = 0;
        }
        frames_in_window += 1;

        let self_entity = commands.iter().find_map(|command| match command {
            DrawCommand::Ship { entity_id, is_self: true, .. } => Some(*entity_id),
            _ => None,
        });
        let mut sunk = 0usize;
        for command in commands {
            match command {
                DrawCommand::ShotTracer { .. } => counts[0] += 1,
                DrawCommand::Torpedo { .. } => counts[1] += 1,
                DrawCommand::Plane { .. } => counts[2] += 1,
                DrawCommand::DeadShip { .. } => sunk += 1,
                DrawCommand::HealthBar { entity_id, fraction, .. } if Some(*entity_id) == self_entity => {
                    self_hp.0.get_or_insert(*fraction);
                    self_hp.1 = Some(*fraction);
                }
                _ => {}
            }
        }
        counts[3] += sunk.saturating_sub(sunk_before);
        sunk_before = sunk_before.max(sunk);
    }
    if let Some(start) = window_start {
        flush(start, frames_in_window, &counts, self_hp);
    }
}

fn export(args: &Args) -> Result<(), ExportError> {
    let game_data = GameDataCache::new(args.game_dir.clone());
    let cancel = AtomicBool::new(false);

    let mut options = playback_options();
    // The demo draws the map and its HUD only, and the recording player's
    // ranges rather than every ship's, which would bury the map.
    options.show_team_rosters = false;
    options.show_stats_panel = false;
    options.ship_config_visibility = ShipConfigVisibility::SelfOnly;
    options.player_names = NameDisplay::Pseudonym;
    options.show_chat = false;

    // The window is cut after the bake, so the budget has to hold the whole
    // battle at this interval rather than thinning it.
    let budget = (3600.0 / args.frame_interval) as usize;
    let baked = bake_track(&args.replay, &[], &game_data, &cancel, budget, args.frame_interval, options)?;

    if args.activity {
        print_activity(&baked.frames, &baked.clocks, baked.battle_start);
    }

    let from = args.from.map_or(f32::NEG_INFINITY, ElapsedClock::seconds);
    let to = args.to.map_or(f32::INFINITY, ElapsedClock::seconds);
    let (frames, clocks): (Vec<Vec<DrawCommand>>, Vec<GameClock>) = baked
        .frames
        .into_iter()
        .zip(baked.clocks)
        .filter(|(_, clock)| (from..=to).contains(&(clock.seconds() - baked.battle_start.seconds())))
        .unzip();
    if frames.is_empty() {
        return Err(ExportError::EmptyClip);
    }

    let build = baked.version.build.ok_or_else(|| ExportError::NoBuild { version: baked.version.to_path() })?.get();
    let loaded = game_data
        .get_or_load_build_for(build, Some(&baked.version))
        .map_err(|source| ExportError::GameData { build, source })?;
    let vfs = loaded.vfs();
    let version = Some(&baked.version);

    let map_art = assets::load_map_image(&baked.map_name, vfs)
        .ok_or_else(|| ExportError::NoMapArt { map: baked.map_name.clone() })?;
    let mut map_png = Vec::new();
    map_art.write_to(&mut std::io::Cursor::new(&mut map_png), image::ImageFormat::Png).map_err(ExportError::MapPng)?;

    let used = UsedIcons::of(&frames);
    let icons = TrackIcons {
        ships: icon_list(assets::load_ship_icons(vfs, version), |_| true),
        planes: icon_list(assets::load_plane_icons(vfs, version), |key| used.planes.contains(key)),
        buildings: icon_list(assets::load_building_icons(vfs, version), |_| true),
        consumables: icon_list(assets::load_consumable_icons(vfs, version), |key| used.consumables.contains(key)),
        death_causes: icon_list(assets::load_death_cause_icons(vfs, SMALL_ICON_SIZE, version), |_| true),
        powerups: icon_list(assets::load_powerup_icons(vfs, SMALL_ICON_SIZE, version), |key| {
            used.powerups.contains(key)
        }),
    };

    let translations = wt_translations::TranslationsBackend::load();
    let toggles = TOGGLES
        .iter()
        .filter(|toggle| !WITHHELD_TOGGLES.contains(&toggle.id))
        .map(|toggle| {
            let label = rust_i18n::Backend::translate(&translations, "en", toggle.label_key)
                .ok_or(ExportError::NoLabel { key: toggle.label_key })?;

            Ok(OfferedToggle { id: toggle.id.to_owned(), label: label.to_owned() })
        })
        .collect::<Result<Vec<_>, ExportError>>()?;

    let track = TrackFile {
        frames,
        clocks,
        battle_start: baked.battle_start,
        battle_end: baked.battle_end,
        map_png,
        icons,
        toggles,
    };
    let bytes = track.encode()?;
    std::fs::write(&args.output, &bytes).map_err(|source| ExportError::Write { path: args.output.clone(), source })?;

    println!("wrote {} frames, {} bytes, to {}", track.frames.len(), bytes.len(), args.output.display());

    Ok(())
}

fn main() -> std::process::ExitCode {
    let result = Args::parse(std::env::args().skip(1)).and_then(|args| export(&args));
    let Err(error) = result else {
        return std::process::ExitCode::SUCCESS;
    };

    eprintln!("export_demo_track: {error}");
    let mut source = std::error::Error::source(&error);
    while let Some(cause) = source {
        eprintln!("  caused by: {cause}");
        source = cause.source();
    }

    std::process::ExitCode::FAILURE
}

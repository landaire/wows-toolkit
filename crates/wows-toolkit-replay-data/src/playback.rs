//! A replay played back on its own minimap, independent of the front end that
//! shows it: the baked track and where playback stands in it, the layers a
//! viewport offers, the per-ship trail and range filters, and the video
//! export and batch render.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use wows_minimap_renderer::EncoderKind;
use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::RenderOptions;
use wows_minimap_renderer::SavedRenderOptions;
use wows_minimap_renderer::VideoCodec;
use wows_minimap_renderer::config::should_draw_command;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_minimap_renderer::draw_command::ShipConfigVisibility;
use wows_minimap_renderer::encoder::EncoderStatus;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wowsunpack::data::Version;

use crate::load::GameDataCache;
use crate::minimap::PreviewError;
use crate::minimap::SharedPreviewRenderer;

/// How many frames of a battle the track keeps.
///
/// Past this the sink halves what it holds and samples half as often, so a
/// twenty-minute battle costs what a five-minute one does. Twelve hundred
/// frames is two minutes of game time at the bake rate below, and a whole
/// twenty-minute battle once the stride has doubled four times.
pub const TRACK_BUDGET: usize = 1200;

/// How much game time one baked frame covers.
pub const BAKE_INTERVAL: f32 = 0.5;

/// How often a viewport advances while playing, before the speed multiplier.
/// One frame of game time per tick at 1x.
pub const TICK: Duration = Duration::from_millis(500);

/// The speeds a transport offers, which are the egui renderer's own
/// (`PLAYBACK_SPEEDS`). A battle runs twenty minutes, so the useful range is
/// well above real time.
pub const SPEEDS: [f32; 6] = [1.0, 5.0, 10.0, 20.0, 40.0, 60.0];

/// The speed a viewport opens at, as the egui renderer opens at. Real time is
/// too slow to watch a battle through.
pub const DEFAULT_SPEED: f32 = 20.0;

/// How far the skip controls and the left and right keys move, in seconds of
/// game time. The egui renderer's own step.
pub const SEEK_STEP: f32 = 10.0;

/// How near a right-click has to land to have meant a ship, in map pixels.
/// A ship icon is about this wide.
pub const PICK_RADIUS: f32 = 14.0;

/// How much of a zoom one pixel of wheel travel is. The egui renderer's own
/// rate, so the wheel feels the same in every front end.
pub const ZOOM_PER_PIXEL: f32 = 0.01;

/// Where the map's corner sits before a track says otherwise: below the strip
/// the renderer reserves for the score bar and the timer.
pub const DEFAULT_MAP_ORIGIN: (f32, f32) = (0.0, HUD_HEIGHT as f32);

/// A ship showing none of its ranges.
pub const NO_RANGES: ShipConfigFilter = ShipConfigFilter {
    detection: false,
    main_battery: false,
    secondary_battery: false,
    torpedo: false,
    radar: false,
    hydro: false,
};

/// A ship showing all of them.
pub const ALL_RANGES: ShipConfigFilter = ShipConfigFilter {
    detection: true,
    main_battery: true,
    secondary_battery: true,
    torpedo: true,
    radar: true,
    hydro: true,
};

/// What a drawing tool draws with until the reader says otherwise: the white
/// and the width the egui toolbar opens on.
pub const DEFAULT_INK: [u8; 4] = [255, 255, 255, 255];
pub const DEFAULT_NIB: f32 = 2.0;

/// What the bracket keys hold the nib between, as the egui board does.
pub const MIN_NIB: f32 = 1.0;
pub const MAX_NIB: f32 = 8.0;

/// How many changes back an annotation undo can reach.
pub const HISTORY_DEPTH: usize = 32;

/// How long a ping stays on the map, and how wide its ring grows in screen
/// points.
pub const PING_SECONDS: f32 = 1.0;
pub const PING_RADIUS: f32 = 26.0;

/// How big a rotation handle is drawn, and how far above the shape it sits,
/// in screen points at any zoom. The egui renderer's own figures.
pub const HANDLE_RADIUS: f32 = 5.0;
pub const HANDLE_DISTANCE: f32 = 25.0;

/// How many ships a ship search offers at once.
pub const SHIP_MATCHES: usize = 10;

/// What a drawing tool draws with, as the egui toolbar offers it: a stable
/// name and the colour.
pub const INKS: &[(&str, [u8; 4])] = &[
    ("white", [255, 255, 255, 255]),
    ("gray", [160, 160, 160, 255]),
    ("red", [230, 50, 50, 255]),
    ("orange", [240, 140, 30, 255]),
    ("yellow", [240, 230, 50, 255]),
    ("green", [50, 200, 50, 255]),
    ("blue", [50, 120, 230, 255]),
    ("purple", [180, 60, 230, 255]),
    ("pink", [255, 130, 180, 255]),
];

/// The ships that can be placed on the map, as the egui toolbar offers them,
/// short name first.
pub const SPECIES: &[(&str, &str)] =
    &[("DD", "Destroyer"), ("CA", "Cruiser"), ("BB", "Battleship"), ("CV", "AirCarrier"), ("SS", "Submarine")];

/// What a placed ship is tinted by which side it is on, `0xRRGGBB`.
pub const FRIENDLY_TINT: u32 = 0x4CE8AA;
pub const ENEMY_TINT: u32 = 0xFE4D2A;

/// A baked battle: the commands of every kept frame, and when each was drawn.
pub struct Track {
    pub frames: Vec<Vec<DrawCommand>>,
    pub clocks: Vec<GameClock>,
    /// When the battle proper began. Recording starts at the loading screen,
    /// so the clock a frame carries runs ahead of the one the game showed by
    /// this much.
    pub battle_start: GameClock,
    /// When the battle ended, if the replay ran that far.
    pub battle_end: Option<GameClock>,
    /// How wide the map is in the game's own world units, which is what turns
    /// a ship's range in metres into a distance on the minimap.
    pub space_size: f32,
    /// The build this was recorded on, which some of a ship's ranges are
    /// gated on.
    pub version: Version,
    /// Where the map's top-left corner sits in these frames.
    pub map_origin: (f32, f32),
    /// The map's space name, which a peer watching this playback loads art by.
    pub space: String,
}

/// The ship a right-click picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShipPick {
    pub entity_id: EntityId,
    pub player_name: String,
}

impl Track {
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// The last frame's index; zero for a track of one frame or none.
    pub fn last(&self) -> usize {
        self.len().saturating_sub(1)
    }

    /// The game time `index` was drawn at, in seconds. Zero past the end,
    /// which only a track with no frames reaches.
    pub fn seconds_at(&self, index: usize) -> f32 {
        self.clocks.get(index).map_or(0.0, |clock| clock.seconds())
    }

    /// How far into the battle `index` is, which is the clock the game showed.
    pub fn elapsed_at(&self, index: usize) -> f32 {
        self.seconds_at(index) - self.battle_start.seconds()
    }

    /// The last frame drawn at or before `seconds` of game time.
    ///
    /// Clocks ascend, so this is a partition point. A time before the first
    /// frame lands on that frame rather than nowhere.
    pub fn frame_at(&self, seconds: f32) -> usize {
        self.clocks.partition_point(|clock| clock.seconds() <= seconds).saturating_sub(1)
    }

    /// Where a clock sits along the track, as a fraction of its length.
    ///
    /// `None` when the track is too short to have a length to sit along.
    pub fn position_of(&self, clock: GameClock) -> Option<f32> {
        let last = self.len().checked_sub(1).filter(|last| *last > 0)?;
        Some((self.frame_at(clock.seconds()) as f32 / last as f32).clamp(0.0, 1.0))
    }

    /// The marks on a seek bar for where the battle began and ended, as
    /// fractions of its length. A replay records from the loading screen on
    /// and often past the last shot, so the battle proper is a stretch inside
    /// the track rather than the whole of it.
    pub fn battle_marks(&self) -> Vec<f32> {
        [Some(self.battle_start), self.battle_end].into_iter().flatten().filter_map(|clock| self.position_of(clock)).collect()
    }

    /// The ship drawn nearest map point `at` in frame `index`, if one is close
    /// enough to have been meant. Reads the frame on screen rather than the
    /// whole track: a right-click picks what the reader can see.
    pub fn ship_near(&self, index: usize, at: (f32, f32)) -> Option<ShipPick> {
        let commands = self.frames.get(index)?;
        let mut best: Option<(f32, ShipPick)> = None;
        for command in commands {
            let (pos, player_name, entity_id) = match command {
                DrawCommand::Ship { pos, player_name, entity_id, .. }
                | DrawCommand::DeadShip { pos, player_name, entity_id, .. } => (pos, player_name, entity_id),
                _ => continue,
            };
            let Some(name) = player_name.clone() else { continue };
            let away = ((pos.x - at.0).powi(2) + (pos.y - at.1).powi(2)).sqrt();
            if away > PICK_RADIUS {
                continue;
            }
            if best.as_ref().is_none_or(|(nearest, _)| away < *nearest) {
                best = Some((away, ShipPick { entity_id: *entity_id, player_name: name }));
            }
        }

        best.map(|(_, pick)| pick)
    }

    /// The ships drawn in frame `index`, by player name.
    pub fn drawn_ships(&self, index: usize) -> Vec<String> {
        let Some(commands) = self.frames.get(index) else { return Vec::new() };
        commands
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Ship { player_name, .. } | DrawCommand::DeadShip { player_name, .. } => player_name.clone(),
                _ => None,
            })
            .collect()
    }

    /// What the team advantage label of frame `index` is made of, when it
    /// shows one.
    pub fn advantage_reading(&self, index: usize) -> Option<AdvantageReading> {
        let commands = self.frames.get(index)?;
        let breakdown = commands.iter().find_map(|command| match command {
            DrawCommand::TeamAdvantage { level: Some(_), breakdown, .. } => Some(breakdown),
            _ => None,
        })?;
        let reading = match wows_minimap_renderer::advantage::breakdown_rows(breakdown) {
            None => AdvantageReading::TeamEliminated,
            Some(rows) => AdvantageReading::Rows {
                rows: rows.into_iter().map(|row| (row.label_key, row.contribution)).collect(),
                hp_incomplete: !breakdown.hp_data_reliable,
            },
        };

        Some(reading)
    }
}

/// What a hover over the advantage label reads, as translation keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdvantageReading {
    /// One team has nothing left to weigh.
    TeamEliminated,
    /// Each factor's translation key and what it contributes, and whether the
    /// health figures behind them were incomplete.
    Rows { rows: Vec<(&'static str, String)>, hp_incomplete: bool },
}

/// Where `speed` sits in the ladder; the first rung for a speed not on it.
pub fn speed_index(speed: f32) -> usize {
    SPEEDS.iter().position(|offered| (offered - speed).abs() < f32::EPSILON).unwrap_or_default()
}

/// A speed as a transport labels it: no trailing zero on a whole one.
pub fn speed_label(speed: f32) -> String {
    if (speed - speed.round()).abs() < f32::EPSILON {
        format!("{}x", speed.round() as i32)
    } else {
        format!("{speed}x")
    }
}

/// Seconds of game time as the clock the game shows, zero-padded as the egui
/// renderer pads it.
pub fn mmss(seconds: f32) -> String {
    let seconds = seconds.max(0.0) as u32;
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// Whether the reader has asked for this ship's trail or this range.
///
/// The global switches decide whether a layer is drawn at all; this decides
/// whose, which is what the per-ship menu changes.
pub fn per_ship_allows(
    command: &DrawCommand,
    trail_hidden: &HashSet<String>,
    ship_ranges: &HashMap<String, ShipConfigFilter>,
) -> bool {
    match command {
        DrawCommand::PositionTrail { player_name, .. } => {
            player_name.as_ref().is_none_or(|name| !trail_hidden.contains(name))
        }
        DrawCommand::ShipConfigCircle { player_name, kind, .. } => {
            ship_ranges.get(player_name).is_some_and(|filter| filter.is_enabled(kind))
        }
        _ => true,
    }
}

/// What a playback viewport draws when it opens.
///
/// Everything it baked except the layers that are asked for rather than
/// assumed: the rosters take a gutter either side of the map, and the ranges
/// would cover it.
pub fn playback_options_hidden() -> RenderOptions {
    RenderOptions { show_ship_config: false, show_team_rosters: false, ..playback_options() }
}

/// What a playback viewport bakes.
///
/// The hover preview's set plus every ship's range circles. They are per-frame
/// and small, so keeping them costs little, and a viewport that did not bake
/// them could not offer them at all. Trails are still left out: a trail
/// command carries every point so far, so baking one per frame would cost the
/// square of the track's length.
pub fn playback_options() -> RenderOptions {
    RenderOptions {
        show_ship_config: true,
        // The rosters' own commands carry fixed coordinates, so baking them
        // costs a roster per frame and nothing else; the canvas they need is
        // the target's business, not the bake's.
        show_team_rosters: true,
        // Every ship's, so the reader can choose whose to look at afterwards
        // rather than re-baking to change their mind.
        ship_config_visibility: ShipConfigVisibility::Filtered(std::sync::Arc::new(|_entity| Some(ALL_RANGES))),
        ..wows_minimap_renderer::frame_track::bake_options()
    }
}

/// Walks `path`'s battle once, keeping every frame's draw commands and the
/// renderer they are drawn through.
///
/// `alts` are other recordings of the same battle, read alongside it: the map
/// then shows what the primary's team could not see.
pub fn bake(
    path: &Path,
    alts: &[PathBuf],
    game_data: &GameDataCache,
    cancel: &AtomicBool,
) -> Result<(Track, SharedPreviewRenderer), PreviewError> {
    let baked =
        crate::minimap::bake_track(path, alts, game_data, cancel, TRACK_BUDGET, BAKE_INTERVAL, playback_options())?;
    let track = Track {
        frames: baked.frames,
        clocks: baked.clocks,
        battle_start: baked.battle_start,
        battle_end: baked.battle_end,
        space_size: baked.space_size,
        version: baked.version,
        map_origin: (baked.map_origin.0 as f32, baked.map_origin.1 as f32),
        space: baked.map_name,
    };

    Ok((track, baked.renderer))
}

/// One layer switch a viewport offers: a stable id, its translation key, what
/// it reads, and what it sets. The `bool` is whether dead ships are drawn,
/// which gates a command rather than a layer and so is not one of
/// [`RenderOptions`].
pub struct LayerToggle {
    pub id: &'static str,
    pub label: &'static str,
    pub read: fn(&RenderOptions, bool) -> bool,
    pub write: fn(&mut RenderOptions, &mut bool, bool),
}

/// What a viewport can be told to draw.
///
/// Only the commands the track actually holds. A playback bake adds the team
/// rosters and every ship's range circles to `bake_options`; the stats panel
/// is still left out, because its silhouettes are one of the loads a bake
/// skips to stay cheap, so a switch for it would draw an empty gutter and is
/// not offered. Position trails are derived at raster time rather than baked.
pub const LAYER_TOGGLES: &[LayerToggle] = &[
    LayerToggle {
        id: "renderer-opt-hp-bars",
        label: "ui.renderer.settings.hp_bars",
        read: |o, _| o.show_hp_bars,
        write: |o, _, v| o.show_hp_bars = v,
    },
    LayerToggle {
        id: "renderer-opt-tracers",
        label: "ui.renderer.settings.tracers",
        read: |o, _| o.show_tracers,
        write: |o, _, v| o.show_tracers = v,
    },
    LayerToggle {
        id: "renderer-opt-torpedoes",
        label: "ui.renderer.settings.torpedoes",
        read: |o, _| o.show_torpedoes,
        write: |o, _, v| o.show_torpedoes = v,
    },
    LayerToggle {
        id: "renderer-opt-planes",
        label: "ui.renderer.settings.planes",
        read: |o, _| o.show_planes,
        write: |o, _, v| o.show_planes = v,
    },
    LayerToggle {
        id: "renderer-opt-smoke",
        label: "ui.renderer.settings.smoke",
        read: |o, _| o.show_smoke,
        write: |o, _, v| o.show_smoke = v,
    },
    LayerToggle {
        id: "renderer-opt-consumables",
        label: "ui.renderer.settings.consumables",
        read: |o, _| o.show_consumables,
        write: |o, _, v| o.show_consumables = v,
    },
    LayerToggle {
        id: "renderer-opt-capture-points",
        label: "ui.renderer.settings.capture_points",
        read: |o, _| o.show_capture_points,
        write: |o, _, v| o.show_capture_points = v,
    },
    LayerToggle {
        id: "renderer-opt-buildings",
        label: "ui.renderer.settings.buildings",
        read: |o, _| o.show_buildings,
        write: |o, _, v| o.show_buildings = v,
    },
    LayerToggle {
        id: "renderer-opt-player-names",
        label: "ui.renderer.settings.player_names",
        read: |o, _| o.show_player_names,
        write: |o, _, v| o.show_player_names = v,
    },
    LayerToggle {
        id: "renderer-opt-ship-names",
        label: "ui.renderer.settings.ship_names",
        read: |o, _| o.show_ship_names,
        write: |o, _, v| o.show_ship_names = v,
    },
    LayerToggle {
        id: "renderer-opt-dead-ships",
        label: "ui.renderer.settings.dead_ships",
        read: |_, dead| dead,
        write: |_, dead, v| *dead = v,
    },
    LayerToggle {
        id: "renderer-opt-dead-ship-names",
        label: "ui.renderer.settings.dead_ship_names",
        read: |o, _| o.show_dead_ship_names,
        write: |o, _, v| o.show_dead_ship_names = v,
    },
    LayerToggle {
        id: "renderer-opt-camera-direction",
        label: "ui.renderer.settings.camera_direction",
        read: |o, _| o.show_camera_direction,
        write: |o, _, v| o.show_camera_direction = v,
    },
    LayerToggle {
        id: "renderer-opt-armament",
        label: "ui.renderer.settings.armament",
        read: |o, _| o.show_armament,
        write: |o, _, v| o.show_armament = v,
    },
    LayerToggle {
        id: "renderer-opt-score",
        label: "ui.renderer.settings.score_label",
        read: |o, _| o.show_score,
        write: |o, _, v| o.show_score = v,
    },
    LayerToggle {
        id: "renderer-opt-timer",
        label: "ui.renderer.settings.timer",
        read: |o, _| o.show_timer,
        write: |o, _, v| o.show_timer = v,
    },
    LayerToggle {
        id: "renderer-opt-kill-feed",
        label: "ui.renderer.settings.kill_feed",
        read: |o, _| o.show_kill_feed,
        write: |o, _, v| o.show_kill_feed = v,
    },
    LayerToggle {
        id: "renderer-opt-chat",
        label: "ui.renderer.settings.chat_label",
        read: |o, _| o.show_chat,
        write: |o, _, v| o.show_chat = v,
    },
    LayerToggle {
        id: "renderer-opt-battle-result",
        label: "ui.renderer.settings.battle_result",
        read: |o, _| o.show_battle_result,
        write: |o, _, v| o.show_battle_result = v,
    },
    LayerToggle {
        id: "renderer-opt-buffs",
        label: "ui.renderer.settings.buff_counters",
        read: |o, _| o.show_buffs,
        write: |o, _, v| o.show_buffs = v,
    },
    LayerToggle {
        id: "renderer-opt-trails",
        label: "ui.renderer.settings.heat_trail",
        read: |o, _| o.show_trails,
        write: |o, _, v| o.show_trails = v,
    },
    LayerToggle {
        id: "renderer-opt-ship-ranges",
        label: "ui.renderer.settings.ship_ranges",
        read: |o, _| o.show_ship_config,
        write: |o, _, v| o.show_ship_config = v,
    },
    LayerToggle {
        id: "renderer-opt-advantage",
        label: "ui.renderer.settings.team_advantage",
        read: |o, _| o.show_advantage,
        write: |o, _, v| o.show_advantage = v,
    },
    LayerToggle {
        id: "renderer-opt-team-rosters",
        label: "ui.renderer.settings.team_rosters",
        read: |o, _| o.show_team_rosters,
        // The rosters sit in gutters either side of the map, which the stats
        // panel wants for itself, so turning one on clears the other as the
        // egui checkboxes do.
        write: |o, _, v| {
            o.show_team_rosters = v;
            if v {
                o.show_stats_panel = false;
            }
        },
    },
];

/// What an export is encoded with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExportSettings {
    /// Encode in software even where the GPU could do it.
    pub prefer_cpu: bool,
    /// Start the video at the loading screen rather than at the battle.
    pub include_pre_battle: bool,
    /// The codec to encode with. `None` lets the encoder pick the best one it
    /// can, which is what most readers want.
    pub codec: Option<VideoCodec>,
}

/// How far an export has got, in frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportProgress {
    pub done: u64,
    pub total: u64,
    pub stage: ExportStage,
}

/// Which part of an export is running.
///
/// Muxing happens after the last frame and can take a noticeable while on a
/// long battle, so saying so is the difference between a bar that has stopped
/// and one that is nearly done.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportStage {
    Encoding,
    Muxing,
}

impl ExportStage {
    /// What a progress line calls it, as a translation key worded as the egui
    /// renderer words it.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Encoding => "ui.renderer.encoding",
            Self::Muxing => "ui.renderer.muxing",
        }
    }
}

/// Why an export stopped.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// The encoder takes a UTF-8 path, and this one is not.
    #[error("the output path {path:?} cannot be handed to the encoder")]
    PathUnusable { path: PathBuf },
    #[error("{0}")]
    Encoder(rootcause::Report<wows_minimap_renderer::error::VideoError>),
    /// A track with nothing in it, which would make a video of nothing.
    #[error("there are no frames to export")]
    NoFrames,
}

/// What this machine can encode with, probed once.
///
/// The probe builds a GPU device to ask it, which costs long enough to notice,
/// and the answer cannot change while the app runs.
pub fn encoder_status() -> &'static EncoderStatus {
    static STATUS: LazyLock<EncoderStatus> = LazyLock::new(wows_minimap_renderer::check_encoder);
    &STATUS
}

/// Whether an export has to be encoded in software.
///
/// A codec the GPU cannot encode falls back rather than failing: picking AV1
/// is a choice of codec, not a demand for the GPU.
pub fn must_use_cpu(settings: &ExportSettings, status: &EncoderStatus) -> bool {
    if settings.prefer_cpu || !status.gpu_available() {
        return true;
    }
    match settings.codec {
        Some(codec) => !status.supports(EncoderKind::Gpu, codec),
        None => false,
    }
}

/// What an export is encoded with, as the reader last saved it.
pub fn export_settings_of(saved: &SavedRenderOptions) -> ExportSettings {
    ExportSettings {
        prefer_cpu: saved.prefer_cpu_encoder,
        codec: saved.video_codec,
        include_pre_battle: saved.include_pre_battle,
    }
}

/// What a batch draws of what it baked, and whether dead ships are among it.
///
/// The reader's saved layers, less the two a viewport holds off for the same
/// reasons: the stats panel's ship silhouettes are a load the bake skips, so its
/// gutter would be empty and the kill feed and the chat that share it would go
/// unfilmed; and which ships have their range circles drawn is a per-ship choice
/// a batch is never given, so drawing every ship's would bury the map.
pub fn batch_options(defaults: &SavedRenderOptions) -> (RenderOptions, bool) {
    let mut options = playback_options();
    defaults.apply_to(&mut options);
    options.show_stats_panel = false;
    options.show_ship_config = false;
    (options, defaults.show_dead_ships)
}

/// The frames of `track` an export covers.
///
/// A replay records the loading screen and the countdown before the battle
/// proper; unless the reader asked for them, an export starts where the battle
/// did.
pub fn frames_to_export<'a>(track: &'a Track, settings: &ExportSettings) -> &'a [Vec<DrawCommand>] {
    if settings.include_pre_battle {
        return &track.frames;
    }
    let from = track.frame_at(track.battle_start.seconds());
    track.frames.get(from..).unwrap_or(&track.frames)
}

/// The commands of each frame an export writes: what is drawn on screen, not
/// everything that was baked, so a layer the reader turned off does not come
/// back in the file.
pub fn filtered_frames(
    frames: &[Vec<DrawCommand>],
    options: &RenderOptions,
    show_dead_ships: bool,
    trail_hidden: &HashSet<String>,
    ship_ranges: &HashMap<String, ShipConfigFilter>,
) -> Vec<Vec<DrawCommand>> {
    frames
        .iter()
        .map(|frame| {
            frame
                .iter()
                .filter(|command| should_draw_command(command, options, show_dead_ships))
                .filter(|command| per_ship_allows(command, trail_hidden, ship_ranges))
                .cloned()
                .collect()
        })
        .collect()
}

/// Rasterises a baked track and encodes it to `output`.
///
/// Blocking: it draws every frame and waits on the encoder. The renderer is
/// held for the whole encode, which is why a viewport refuses its transport
/// while one runs. `progress` is told after every frame and once more when
/// muxing starts.
pub fn encode_track(
    renderer: &SharedPreviewRenderer,
    frames: &[Vec<DrawCommand>],
    output: &Path,
    settings: ExportSettings,
    mut progress: impl FnMut(ExportProgress),
) -> Result<(), ExportError> {
    if frames.is_empty() {
        return Err(ExportError::NoFrames);
    }
    let duration_seconds = (frames.len().saturating_sub(1)) as f32 * BAKE_INTERVAL;
    let mut drawing = renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (width, height) = drawing.canvas_size();

    let Some(path) = output.to_str() else {
        return Err(ExportError::PathUnusable { path: output.to_owned() });
    };
    let mut encoder =
        wows_minimap_renderer::VideoEncoder::new(Some(path), None, false, duration_seconds, width, height);
    encoder.set_prefer_cpu(must_use_cpu(&settings, encoder_status()));
    encoder.set_codec(match settings.codec {
        Some(codec) => wows_minimap_renderer::video::CodecChoice::Explicit(codec),
        None => wows_minimap_renderer::video::CodecChoice::Auto,
    });
    encoder.init().map_err(ExportError::Encoder)?;

    let total = frames.len() as u64;
    for (index, commands) in frames.iter().enumerate() {
        let image = drawing.render(commands);
        encoder.submit_frame(&image).map_err(ExportError::Encoder)?;
        progress(ExportProgress { done: index as u64 + 1, total, stage: ExportStage::Encoding });
    }
    // Said before rather than after, because finishing is the part that takes
    // a while with nothing else to show for it.
    progress(ExportProgress { done: total, total, stage: ExportStage::Muxing });

    encoder.finish_submitted().map_err(ExportError::Encoder)
}

/// Which replay of how many a batch has reached.
///
/// `done` counts the ones behind it, so the first report reads as none done of
/// however many there are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchStep {
    pub done: usize,
    pub total: usize,
    /// The replay being read, by its file stem.
    pub replay: String,
}

/// What a batch render managed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatchOutcome {
    /// The files written.
    pub written: Vec<PathBuf>,
    /// The replays that failed.
    pub failed: Vec<PathBuf>,
}

/// Renders each replay in `paths` to its own file under `out_dir`, without
/// opening a viewport for any of them.
///
/// Sequential, because each encode holds a GPU device and the machine has one:
/// two at once would contend for it and finish no sooner. A replay that fails is
/// reported and the rest still run, since one unreadable file in a marked set
/// should not lose the other forty.
///
/// `defaults` is what the reader saved as what a renderer opens with, so a
/// batch writes what a viewport would have shown rather than the built-in set.
/// `report` is told which replay the batch has reached before it is read.
/// Blocking: a front end runs it on its own executor.
pub fn batch_export(
    paths: Vec<PathBuf>,
    game_data: &GameDataCache,
    out_dir: &Path,
    defaults: &SavedRenderOptions,
    cancel: &AtomicBool,
    mut report: impl FnMut(BatchStep),
) -> BatchOutcome {
    let settings = export_settings_of(defaults);
    let (options, show_dead_ships) = batch_options(defaults);
    let mut outcome = BatchOutcome::default();
    let total = paths.len();

    for (index, path) in paths.into_iter().enumerate() {
        let named = path.file_stem().unwrap_or(path.as_os_str()).to_string_lossy().into_owned();
        report(BatchStep { done: index, total, replay: named });
        let Some(stem) = path.file_stem() else {
            outcome.failed.push(path);
            continue;
        };
        let output = out_dir.join(stem).with_extension("mp4");

        let (track, renderer) = match bake(&path, &[], game_data, cancel) {
            Ok(baked) => baked,
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "batch render: the replay did not bake");
                outcome.failed.push(path);
                continue;
            }
        };
        // The bake holds every layer it could; which of them are drawn is what
        // the saved defaults decide, as they do in a viewport.
        let frames = filtered_frames(
            frames_to_export(&track, &settings),
            &options,
            show_dead_ships,
            &HashSet::new(),
            &HashMap::new(),
        );
        match encode_track(&renderer, &frames, &output, settings, |_| {}) {
            Ok(()) => outcome.written.push(output),
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "batch render: the encode failed");
                outcome.failed.push(path);
            }
        }
    }

    outcome
}

/// A place to render to that outlives this process, for the clipboard.
///
/// `None` when no temporary directory could be made, which is the only way
/// this fails.
pub fn temporary_output(name: &str) -> Option<PathBuf> {
    let dir = tempfile::Builder::new().prefix("wt-render-").tempdir().ok()?;
    let path = dir.path().join(name);
    // Kept: the clipboard will hold this path, and a directory removed on drop
    // would leave it pointing at nothing.
    let _ = dir.keep();

    Some(path)
}

/// A directory a batch can write into that outlives this process, for the
/// clipboard.
pub fn temporary_batch_dir() -> Option<PathBuf> {
    let dir = tempfile::Builder::new().prefix("wt-batch-").tempdir().ok()?;
    // Kept, as `temporary_output` keeps its own.
    Some(dir.keep())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(clocks: &[f32]) -> Track {
        Track {
            frames: vec![Vec::new(); clocks.len()],
            clocks: clocks.iter().copied().map(GameClock).collect(),
            battle_start: GameClock(0.0),
            battle_end: None,
            space_size: 1400.0,
            version: Version::base(99, 0, 0),
            map_origin: DEFAULT_MAP_ORIGIN,
            space: String::new(),
        }
    }

    #[test]
    fn a_clock_lands_on_the_last_frame_drawn_at_or_before_it() {
        let track = track(&[0.0, 0.5, 1.0, 1.5]);

        assert_eq!(track.frame_at(-3.0), 0, "before the first frame is the first frame");
        assert_eq!(track.frame_at(0.7), 1);
        assert_eq!(track.frame_at(1.5), 3);
        assert_eq!(track.frame_at(99.0), 3, "past the end is the last frame");
    }

    #[test]
    fn battle_marks_sit_where_the_battle_began_and_ended() {
        let mut track = track(&[0.0, 10.0, 20.0, 30.0, 40.0, 50.0]);
        track.battle_start = GameClock(10.0);
        assert_eq!(track.battle_marks(), vec![0.2], "an end the replay never recorded has no mark");

        track.battle_end = Some(GameClock(40.0));
        assert_eq!(track.battle_marks(), vec![0.2, 0.8]);
        assert_eq!(track.elapsed_at(2), 10.0, "elapsed reads from the battle's start");
    }

    #[test]
    fn labels_read_as_the_transport_shows_them() {
        assert_eq!(speed_label(20.0), "20x");
        assert_eq!(speed_label(0.5), "0.5x");
        assert_eq!(mmss(-4.0), "00:00");
        assert_eq!(mmss(754.9), "12:34");
        assert_eq!(speed_index(40.0), 4);
    }

    #[test]
    fn the_per_ship_filters_admit_only_what_was_asked_for() {
        use wows_minimap_renderer::draw_command::ShipConfigCircleKind;

        let trail = |name: &str| DrawCommand::PositionTrail {
            entity_id: EntityId::from(1u32),
            player_name: Some(name.to_owned()),
            points: Vec::new(),
        };
        let circle = |name: &str| DrawCommand::ShipConfigCircle {
            entity_id: EntityId::from(1u32),
            pos: wows_minimap_renderer::MinimapPos { x: 0.0, y: 0.0 },
            radius_px: 10.0,
            color: [0, 0, 0],
            alpha: 1.0,
            dashed: false,
            label: None,
            kind: ShipConfigCircleKind::Detection,
            player_name: name.to_owned(),
            is_self: false,
        };
        let hidden: HashSet<String> = ["hidden_player".to_owned()].into();
        let ranges: HashMap<String, ShipConfigFilter> = [("ranged_player".to_owned(), ALL_RANGES)].into();

        assert!(per_ship_allows(&trail("shown_player"), &hidden, &ranges));
        assert!(!per_ship_allows(&trail("hidden_player"), &hidden, &ranges));
        assert!(per_ship_allows(&circle("ranged_player"), &hidden, &ranges));
        assert!(!per_ship_allows(&circle("unranged_player"), &hidden, &ranges), "a ship nobody asked for");
    }

    #[test]
    fn a_batch_draws_the_saved_layers_without_the_stats_panel_or_ranges() {
        let saved = SavedRenderOptions { show_stats_panel: true, show_ship_config: true, ..SavedRenderOptions::default() };

        let (options, _) = batch_options(&saved);

        assert!(!options.show_stats_panel);
        assert!(!options.show_ship_config);
    }

    #[test]
    fn layer_ids_are_unique() {
        let ids: HashSet<&str> = LAYER_TOGGLES.iter().map(|toggle| toggle.id).collect();
        assert_eq!(ids.len(), LAYER_TOGGLES.len());
    }
}

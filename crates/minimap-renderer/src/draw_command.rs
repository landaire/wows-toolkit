use std::sync::Arc;

#[cfg(feature = "rendering")]
use image::RgbaImage;
use wowsunpack::game_types::{AdvantageLevel, BattleResult, FinishType};
use wows_replays::analyzer::decoder::DeathCause;
use wows_replays::analyzer::decoder::Recognized;
use wows_replays::types::ElapsedClock;
use wows_replays::types::EntityId;
use wows_replays::types::PlaneId;

use crate::map_data::MinimapPos;

/// How a ship should be rendered based on its visibility state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ShipVisibility {
    /// Ship is directly visible (Position packets). Solid fill.
    Visible,
    /// Ship is detected on minimap but not directly rendered. Outline only.
    MinimapOnly,
    /// Ship has gone undetected. Gray, semi-transparent at last known position.
    Undetected,
}

/// Kind of ship configuration circle for filtering and grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ShipConfigCircleKind {
    Detection,
    MainBattery,
    SecondaryBattery,
    TorpedoRange,
    Radar,
    Hydro,
}

/// Per-range-type visibility filter for ship configuration circles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ShipConfigFilter {
    pub detection: bool,
    pub main_battery: bool,
    pub secondary_battery: bool,
    pub torpedo: bool,
    pub radar: bool,
    pub hydro: bool,
}

impl ShipConfigFilter {
    /// Returns true if the given circle kind is enabled in this filter.
    pub fn is_enabled(&self, kind: &ShipConfigCircleKind) -> bool {
        match kind {
            ShipConfigCircleKind::Detection => self.detection,
            ShipConfigCircleKind::MainBattery => self.main_battery,
            ShipConfigCircleKind::SecondaryBattery => self.secondary_battery,
            ShipConfigCircleKind::TorpedoRange => self.torpedo,
            ShipConfigCircleKind::Radar => self.radar,
            ShipConfigCircleKind::Hydro => self.hydro,
        }
    }

    /// Returns a filter with all range types enabled.
    pub fn all_enabled() -> Self {
        Self { detection: true, main_battery: true, secondary_battery: true, torpedo: true, radar: true, hydro: true }
    }

    /// Returns true if any range type is enabled.
    pub fn any_enabled(&self) -> bool {
        self.detection || self.main_battery || self.secondary_battery || self.torpedo || self.radar || self.hydro
    }
}

/// Controls which ships have their config circles rendered.
///
/// The callback in `Filtered` receives an entity ID and returns:
/// - `Some(filter)` to show circles matching the filter for that entity
/// - `None` to hide all circles for that entity
#[derive(Default)]
pub enum ShipConfigVisibility {
    /// Only show the replay owner's config circles (all range types). Default.
    #[default]
    SelfOnly,
    /// Use a callback to determine per-ship visibility and per-range filtering.
    /// The callback receives the entity ID and returns an optional filter.
    Filtered(Arc<dyn Fn(EntityId) -> Option<ShipConfigFilter> + Send + Sync>),
}

impl ShipConfigVisibility {
    /// Returns the filter for a ship, or None if circles should be hidden.
    pub fn filter_for(&self, is_self: bool, entity_id: EntityId) -> Option<ShipConfigFilter> {
        match self {
            ShipConfigVisibility::SelfOnly => {
                if is_self {
                    Some(ShipConfigFilter::all_enabled())
                } else {
                    None
                }
            }
            ShipConfigVisibility::Filtered(cb) => cb(entity_id),
        }
    }
}

impl Clone for ShipConfigVisibility {
    fn clone(&self) -> Self {
        match self {
            Self::SelfOnly => Self::SelfOnly,
            Self::Filtered(cb) => Self::Filtered(Arc::clone(cb)),
        }
    }
}

impl std::fmt::Debug for ShipConfigVisibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SelfOnly => write!(f, "SelfOnly"),
            Self::Filtered(_) => write!(f, "Filtered(<callback>)"),
        }
    }
}

/// Hint for which font to use when rendering text.
///
/// Allows render backends (egui, ImageTarget) to select the correct font
/// without needing access to `GameFonts` directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum FontHint {
    /// Use the primary UI font (Warhelios Bold).
    #[default]
    Primary,
    /// Use the fallback font at the given index in `GameFonts::fallbacks`.
    Fallback(usize),
}

/// A single chat message entry for the chat overlay.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ChatEntry {
    /// Clan tag (e.g. "CLAN"), empty if none
    pub clan_tag: String,
    /// Clan color as RGB, or None to use team color
    pub clan_color: Option<[u8; 3]>,
    /// Player name
    pub player_name: String,
    /// Team color for the player name
    pub team_color: [u8; 3],
    /// Ship species for icon lookup (e.g. "Destroyer")
    pub ship_species: Option<String>,
    /// Localized ship name (e.g. "Shimakaze")
    pub ship_name: Option<String>,
    /// Chat message text
    pub message: String,
    /// Color for the message text (reflects the chat channel)
    pub message_color: [u8; 3],
    /// Opacity (0.0 = fully faded, 1.0 = fully visible)
    pub opacity: f32,
    /// Which font to use for the message text (primary or CJK fallback).
    pub font_hint: FontHint,
}

/// A single entry in the kill feed.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct KillFeedEntry {
    /// Killer's player name
    pub killer_name: String,
    /// Killer's ship species (e.g. "Destroyer") for icon lookup
    pub killer_species: Option<String>,
    /// Killer's localized ship name (e.g. "Shimakaze")
    pub killer_ship_name: Option<String>,
    /// Killer's team color
    pub killer_color: [u8; 3],
    /// Victim's player name
    pub victim_name: String,
    /// Victim's ship species for icon lookup
    pub victim_species: Option<String>,
    /// Victim's localized ship name
    pub victim_ship_name: Option<String>,
    /// Victim's team color
    pub victim_color: [u8; 3],
    /// How the victim died
    pub cause: Recognized<DeathCause>,
}

/// A high-level draw command emitted by the renderer.
///
/// The renderer reads game state and produces a sequence of these commands.
/// A `RenderTarget` implementation consumes them to produce visual output,
/// whether that's a software-rendered image or GPU draw calls.
///
/// All visual properties (colors, opacity, etc.) are fully resolved by the renderer,
/// so backends don't need to duplicate game logic.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum DrawCommand {
    /// Artillery tracer line segment
    ShotTracer { from: MinimapPos, to: MinimapPos, color: [u8; 3] },
    /// Torpedo dot
    Torpedo { pos: MinimapPos, color: [u8; 3] },
    /// Smoke puff circle (alpha blended)
    Smoke { pos: MinimapPos, radius: i32, color: [u8; 3], alpha: f32 },
    /// Ship with icon, rotation, color, visibility
    Ship {
        entity_id: EntityId,
        pos: MinimapPos,
        yaw: f32,
        /// Species name for icon lookup (e.g. "Destroyer")
        species: Option<String>,
        /// Tint color. None = use the icon's native colors (for last_visible/invisible variants)
        color: Option<[u8; 3]>,
        visibility: ShipVisibility,
        opacity: f32,
        /// Whether this is the player's own ship (uses `_self` icon variant)
        is_self: bool,
        /// Player name to render above the icon
        player_name: Option<String>,
        /// Localized ship name to render above the icon (below player name)
        ship_name: Option<String>,
        /// Whether this ship is a detected teammate (ally visible but not self)
        is_detected_teammate: bool,
        /// Override color for player name based on selected armament
        /// (e.g. orange=HE, light blue=AP, green=torp). None = default white.
        name_color: Option<[u8; 3]>,
    },
    /// Health bar above a ship
    HealthBar {
        entity_id: EntityId,
        pos: MinimapPos,
        fraction: f32,
        fill_color: [u8; 3],
        background_color: [u8; 3],
        background_alpha: f32,
    },
    /// Dead ship marker
    DeadShip {
        entity_id: EntityId,
        pos: MinimapPos,
        yaw: f32,
        species: Option<String>,
        /// Tint color. None = use the icon's native colors
        color: Option<[u8; 3]>,
        is_self: bool,
        /// Player name to render above the icon
        player_name: Option<String>,
        /// Localized ship name to render above the icon (below player name)
        ship_name: Option<String>,
    },
    /// Arms race buff zone circle
    BuffZone {
        pos: MinimapPos,
        /// Zone radius in pixels
        radius: i32,
        /// Team color (green/red/white)
        color: [u8; 3],
        /// Fill transparency
        alpha: f32,
        /// Marker name for icon lookup (e.g. "damage_active")
        marker_name: Option<String>,
    },
    /// Capture zone circle with team coloring and letter label
    CapturePoint {
        pos: MinimapPos,
        /// Zone radius in pixels
        radius: i32,
        /// Team color (green/red/white) for the owning team
        color: [u8; 3],
        /// Fill transparency
        alpha: f32,
        /// Zone label (e.g. "A", "B", "C")
        label: String,
        /// Capture progress 0.0..1.0 (0 = no capture in progress)
        progress: f32,
        /// Color of the invading team (shown as progress arc)
        invader_color: Option<[u8; 3]>,
        /// Pre-selected flag icon for base-type capture points (drawn instead of text label)
        #[cfg(feature = "rendering")]
        #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
        flag_icon: Option<RgbaImage>,
    },
    /// Turret direction indicator line from ship center
    TurretDirection {
        entity_id: EntityId,
        pos: MinimapPos,
        /// Turret yaw in radians (world-space, already includes ship heading)
        yaw: f32,
        color: [u8; 3],
        /// Line length in pixels
        length: i32,
    },
    /// Building dot on the minimap
    Building { pos: MinimapPos, color: [u8; 3], is_alive: bool },
    /// Local weather zone (squall/storm) — semi-transparent gray circle
    WeatherZone {
        pos: MinimapPos,
        /// Zone radius in pixels
        radius: i32,
    },
    /// Plane icon
    Plane {
        plane_id: PlaneId,
        owner_entity_id: EntityId,
        pos: MinimapPos,
        /// Icon key for lookup (e.g. "controllable/fighter_he_enemy")
        icon_key: String,
        /// Owner player name to render above the icon
        player_name: Option<String>,
        /// Owner's localized ship name to render above the icon
        ship_name: Option<String>,
    },
    /// Consumable detection radius circle (radar, hydro, etc.)
    ConsumableRadius {
        entity_id: EntityId,
        pos: MinimapPos,
        /// Radius in pixels
        radius_px: i32,
        /// Circle color (team-colored: green for friendly, red for enemy)
        color: [u8; 3],
        /// Fill transparency
        alpha: f32,
    },
    /// Fighter patrol radius circle (filled only, no outline)
    PatrolRadius {
        plane_id: PlaneId,
        pos: MinimapPos,
        /// Radius in pixels
        radius_px: i32,
        /// Circle color (team-colored)
        color: [u8; 3],
        /// Fill transparency
        alpha: f32,
    },
    /// Active consumable icons laid out horizontally below a ship
    ConsumableIcons {
        entity_id: EntityId,
        pos: MinimapPos,
        /// Icon keys for lookup (e.g. "PCY019_RLSSearch")
        icon_keys: Vec<String>,
        /// True for self/allies, false for enemies (affects tint color)
        is_friendly: bool,
        /// Whether a health bar is rendered below this ship (affects vertical offset)
        has_hp_bar: bool,
    },
    /// Ship configuration range circle (detection, main battery, secondary, radar, hydro)
    ShipConfigCircle {
        entity_id: EntityId,
        pos: MinimapPos,
        /// Radius in minimap pixels
        radius_px: f32,
        color: [u8; 3],
        alpha: f32,
        /// Whether circle should be dashed (detection) or solid
        dashed: bool,
        /// Label text (e.g. "12.0 km")
        label: Option<String>,
        kind: ShipConfigCircleKind,
        /// Player name for filtering per-ship
        player_name: String,
        /// Whether this is the replay player's own ship
        is_self: bool,
    },
    /// Position trail showing historical movement as colored dots
    PositionTrail {
        entity_id: EntityId,
        /// Player name for filtering trails per-ship
        player_name: Option<String>,
        /// Points with interpolated colors (oldest=blue, newest=red)
        points: Vec<(MinimapPos, [u8; 3])>,
    },
    /// Team buff indicators below the score bar (arms race)
    TeamBuffs {
        /// Friendly team buffs: (marker_name, count), sorted by sorting field
        friendly_buffs: Vec<(String, u32)>,
        /// Enemy team buffs: (marker_name, count), sorted by sorting field
        enemy_buffs: Vec<(String, u32)>,
    },
    /// Score bar
    ScoreBar {
        team0: i32,
        team1: i32,
        team0_color: [u8; 3],
        team1_color: [u8; 3],
        /// Win score threshold (from BattleLogic, typically 1000)
        max_score: i32,
        /// Time-to-win for team 0 (e.g. "5:32"), or None if no caps
        team0_timer: Option<String>,
        /// Time-to-win for team 1 (e.g. "3:15"), or None if no caps
        team1_timer: Option<String>,
        /// Which team has the advantage and at what level. None = even or disabled.
        /// Tuple is (level, team_index) where team_index is 0 or 1.
        advantage: Option<(AdvantageLevel, u8)>,
    },
    /// Team advantage indicator (shown in score bar area)
    TeamAdvantage {
        /// Advantage level, or None if even
        level: Option<AdvantageLevel>,
        /// Color for the label (advantaged team's color)
        color: [u8; 3],
        /// Detailed breakdown for tooltip display
        breakdown: crate::advantage::AdvantageBreakdown,
    },
    /// Game timer (during battle)
    Timer {
        /// Seconds remaining in the match (from BattleLogic timeLeft), if available
        time_remaining: Option<i64>,
        /// Seconds elapsed since battle started (excludes pre-battle countdown)
        elapsed: ElapsedClock,
    },
    /// Pre-battle countdown overlay (large centered number before battle starts)
    PreBattleCountdown { seconds: i64 },
    /// Kill feed entries with rich data
    KillFeed { entries: Vec<KillFeedEntry> },
    /// Chat overlay on the left side of the minimap
    ChatOverlay { entries: Vec<ChatEntry> },
    /// Battle result overlay (shown at end of match)
    BattleResultOverlay {
        /// The battle outcome (Victory, Defeat, Draw)
        result: BattleResult,
        /// How the battle ended
        finish_type: Option<Recognized<FinishType>>,
        /// Glow/shadow color behind the text
        color: [u8; 3],
        /// If true, subtitle is drawn above the main text; otherwise below.
        subtitle_above: bool,
    },
}

impl DrawCommand {
    /// Returns true if this is a HUD overlay element (score bar, timer, kill feed, etc.)
    /// that should be drawn unclipped, outside the map viewport area.
    pub fn is_hud(&self) -> bool {
        matches!(
            self,
            Self::ScoreBar { .. }
                | Self::Timer { .. }
                | Self::PreBattleCountdown { .. }
                | Self::KillFeed { .. }
                | Self::BattleResultOverlay { .. }
                | Self::TeamBuffs { .. }
                | Self::TeamAdvantage { .. }
                | Self::ChatOverlay { .. }
        )
    }
}

/// Trait for rendering backends that consume `DrawCommand`s.
///
/// Implementations produce visual output from high-level draw commands.
/// The software image renderer and a future GPU renderer both implement this.
pub trait RenderTarget {
    /// Prepare a fresh frame (clear canvas, draw background map + grid).
    fn begin_frame(&mut self);

    /// Execute a single draw command.
    fn draw(&mut self, cmd: &DrawCommand);

    /// Finalize the current frame. After this call, the frame is ready to read/encode.
    fn end_frame(&mut self);
}

use std::sync::Arc;

#[cfg(feature = "rendering")]
use image::RgbaImage;
use wows_replays::analyzer::decoder::DeathCause;
use wows_replays::analyzer::decoder::Recognized;
use wows_replays::types::ElapsedClock;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::GameParamId;
use wows_replays::types::PlaneId;
use wowsunpack::game_types::AdvantageLevel;
use wowsunpack::game_types::BattleResult;
use wowsunpack::game_types::FinishType;
pub use wowsunpack::game_types::Ribbon;

use crate::map_data::MinimapPos;

/// The type of building icon to display on the minimap.
///
/// Derived from the building's Species in GameParams.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum BuildingIconType {
    Airbase,
    AirDefence,
    Artillery,
    Generator,
    Radar,
    Station,
    Supply,
    Tower,
}

impl BuildingIconType {
    /// Icon file base name (e.g. `"airbase"`, `"air_defence"`).
    pub fn icon_name(&self) -> &'static str {
        match self {
            Self::Airbase => "airbase",
            Self::AirDefence => "air_defence",
            Self::Artillery => "artillery",
            Self::Generator => "generator",
            Self::Radar => "radar",
            Self::Station => "station",
            Self::Supply => "supply",
            Self::Tower => "tower",
        }
    }
}

/// The relation/state of a building for icon selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum BuildingRelation {
    Ally,
    Enemy,
    Neutral,
    Dead,
    SuppressedAlly,
    SuppressedEnemy,
    SuppressedNeutral,
}

impl BuildingRelation {
    /// Icon file suffix (e.g. `"ally"`, `"suppressed_enemy"`).
    pub fn icon_suffix(&self) -> &'static str {
        match self {
            Self::Ally => "ally",
            Self::Enemy => "enemy",
            Self::Neutral => "neutral",
            Self::Dead => "dead",
            Self::SuppressedAlly => "suppressed_ally",
            Self::SuppressedEnemy => "suppressed_enemy",
            Self::SuppressedNeutral => "suppressed_neutral",
        }
    }
}

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

impl PartialEq for ShipConfigVisibility {
    fn eq(&self, other: &Self) -> bool {
        matches!((self, other), (Self::SelfOnly, Self::SelfOnly) | (Self::Filtered(_), Self::Filtered(_)))
    }
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
    /// True if killer is self or ally
    pub killer_is_friendly: bool,
    /// Victim's player name
    pub victim_name: String,
    /// Victim's ship species for icon lookup
    pub victim_species: Option<String>,
    /// Victim's localized ship name
    pub victim_ship_name: Option<String>,
    /// Victim's team color
    pub victim_color: [u8; 3],
    /// True if victim is self or ally
    pub victim_is_friendly: bool,
    /// How the victim died
    pub cause: Recognized<DeathCause>,
}

/// A single ribbon type with its accumulated count.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct RibbonCount {
    pub ribbon: Ribbon,
    pub count: usize,
    /// Localized display name (resolved via translate_ribbon, falls back to English).
    pub display_name: String,
}

/// An entry in the merged activity feed (kills + chat), sorted by game clock.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct ActivityFeedEntry {
    pub clock: GameClock,
    pub kind: ActivityFeedKind,
}

/// The payload of an activity feed entry.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ActivityFeedKind {
    Kill(KillFeedEntry),
    Chat(ChatEntry),
}

/// A single weapon-group damage entry for the stats panel breakdown.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct DamageBreakdownEntry {
    /// Short display label (e.g. "MAIN", "TORP", "FIRE")
    pub label: String,
    /// Accumulated enemy damage for this weapon group
    pub damage: f64,
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
        /// Whether this player is currently disconnected (latest connection
        /// event before the current clock is `Disconnected`). Draws a red
        /// outline around the icon, mirroring `is_detected_teammate`.
        is_disconnected: bool,
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
    /// Building marker on the minimap (fort, AA battery, airbase, etc.)
    Building {
        pos: MinimapPos,
        color: [u8; 3],
        is_alive: bool,
        icon_type: Option<BuildingIconType>,
        relation: BuildingRelation,
    },
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
    /// Stats panel background
    StatsPanel { x: i32, width: i32 },
    /// Ship silhouette with HP overlay in the stats panel
    StatsSilhouette {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        /// Ship GameParamId — renderers resolve to localized name
        ship_param_id: Option<GameParamId>,
        hp_fraction: f32,
        hp_current: f32,
        hp_max: f32,
        /// Player name to display above the silhouette.
        player_name: Option<String>,
        /// Clan tag (e.g. "CLAN"), empty string or None if none.
        clan_tag: Option<String>,
        /// Clan color as RGB, or None to use default white.
        clan_color: Option<[u8; 3]>,
        /// Ship name to display below the player name.
        ship_name: Option<String>,
        #[cfg(feature = "rendering")]
        #[cfg_attr(feature = "rkyv", rkyv(with = rkyv::with::Skip))]
        silhouette: Option<RgbaImage>,
    },
    /// Damage breakdown numbers in the stats panel
    StatsDamage {
        x: i32,
        y: i32,
        width: i32,
        /// Per-weapon-group enemy damage breakdown (sorted by damage desc)
        breakdowns: Vec<DamageBreakdownEntry>,
        damage_spotting: f64,
        spotting_breakdowns: Vec<DamageBreakdownEntry>,
        damage_potential: f64,
        potential_breakdowns: Vec<DamageBreakdownEntry>,
    },
    /// Compact ribbon summary in the stats panel
    StatsRibbons { x: i32, y: i32, width: i32, ribbons: Vec<RibbonCount> },
    /// Merged kill feed + chat activity log in the stats panel
    StatsActivityFeed { x: i32, y: i32, width: i32, height: i32, entries: Vec<ActivityFeedEntry> },
    /// Per-team roster panel: list of ships with HP, name, and consumable slots.
    /// Positioned in a gutter beside the map (left or right depending on `side`).
    TeamRoster { side: RosterSide, x: i32, y: i32, width: i32, height: i32, rows: Vec<RosterRow> },
}

/// Which gutter a [`DrawCommand::TeamRoster`] sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum RosterSide {
    /// Player's own team (rendered left of the map in the default layout).
    Friendly,
    /// Opposing team (rendered right of the map).
    Enemy,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct RosterRow {
    /// Per-session entity ID for this player's ship. Acts as a stable lookup
    /// key for the desktop renderer's per-player build snapshot, so the build
    /// popover can resolve hovered rows back to a `ResolvedBuild`.
    pub entity_id: EntityId,
    /// Raw arena-side team id for this player. The desktop renderer uses this
    /// to decide whether the build popover may show enemy loadouts (only when
    /// at least one merged replay's recording player is on the same team).
    pub team_id: i64,
    pub player_name: String,
    pub clan_tag: Option<String>,
    pub clan_color: Option<[u8; 3]>,
    pub ship_name: String,
    pub ship_param_id: Option<GameParamId>,
    /// Lookup key for the ship class icon (DD/CA/BB/CV/SS). Matches a key in
    /// the renderer's `ship_icons` texture map (the species name).
    pub class_icon_key: Option<String>,
    /// Parsed ship species, used as the primary roster sort key so the order
    /// follows the in-game class hierarchy (CV/BB/CA/DD/SS) rather than the
    /// alphabetical fall-back of `class_icon_key`.
    pub species: Option<wowsunpack::game_params::types::Species>,
    pub hp_current: f32,
    pub hp_max: f32,
    /// Portion of missing HP that the ship can still restore via Repair Party
    /// (regen-crew limit minus already-regenerated). Drawn as a darker segment
    /// in the HP bar between current HP and permanent damage.
    pub hp_healable: f32,
    pub is_dead: bool,
    /// Highlight this row (player's own ship, or own division-mate).
    pub is_self: bool,
    /// True while the ship is currently visible to the opposing team. Drives
    /// the yellow name highlight.
    pub is_spotted: bool,
    /// True if the most recent connection event for this player at the current
    /// clock is `Disconnected`. Drives the red outline on the icon.
    pub is_disconnected: bool,
    /// Number of kills the player has scored at the current clock.
    pub kills: u32,
    /// Total damage dealt by the player at the current clock.
    pub damage_dealt: f32,
    /// Seconds elapsed since this player's last observed damage event, or
    /// `None` if they have never dealt damage. UI fades the recent-damage
    /// indicator over this duration.
    pub seconds_since_damage: Option<f32>,
    pub consumables: Vec<RosterConsumable>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub struct RosterConsumable {
    /// Lookup key in the consumable icon map (Ability param index, e.g.
    /// `"PCY009_CrashCrewPremium"`).
    pub icon_key: String,
    /// User-facing display name. Localized when game translations are
    /// available, otherwise the raw consumable type.
    pub display_name: String,
    /// Localized description (the in-game tooltip text). Empty when no
    /// translation is available.
    pub description: String,
    pub total_charges: ChargeCount,
    pub charges_used: u32,
    pub work_time_secs: f32,
    pub reload_time_secs: f32,
    /// Seconds of activation remaining, or `None` when not active.
    pub active_remaining_secs: Option<f32>,
}

/// Mirror of `wowsunpack::game_types::ChargeCount` for the draw command
/// layer. Kept local so this crate avoids a full wowsunpack dep when
/// built with `rendering` off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "rkyv", derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize))]
pub enum ChargeCount {
    Unlimited,
    Finite(u32),
}

impl ChargeCount {
    pub fn remaining(self, used: u32) -> Self {
        match self {
            Self::Unlimited => Self::Unlimited,
            Self::Finite(n) => Self::Finite(n.saturating_sub(used)),
        }
    }
}

#[cfg(feature = "rendering")]
impl From<wowsunpack::game_types::ChargeCount> for ChargeCount {
    fn from(value: wowsunpack::game_types::ChargeCount) -> Self {
        match value {
            wowsunpack::game_types::ChargeCount::Unlimited => Self::Unlimited,
            wowsunpack::game_types::ChargeCount::Finite(n) => Self::Finite(n),
        }
    }
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
                | Self::StatsPanel { .. }
                | Self::StatsSilhouette { .. }
                | Self::StatsDamage { .. }
                | Self::StatsRibbons { .. }
                | Self::StatsActivityFeed { .. }
                | Self::TeamRoster { .. }
        )
    }

    /// Returns true if this is a stats-panel element that should be rendered
    /// in the side panel rather than on the main canvas.
    pub fn is_stats(&self) -> bool {
        matches!(
            self,
            Self::StatsPanel { .. }
                | Self::StatsSilhouette { .. }
                | Self::StatsDamage { .. }
                | Self::StatsRibbons { .. }
                | Self::StatsActivityFeed { .. }
                | Self::TeamRoster { .. }
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

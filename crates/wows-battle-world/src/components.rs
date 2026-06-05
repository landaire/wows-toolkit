//! ECS components representing per-entity battle state.

use bevy_ecs::prelude::*;
use wows_replays::Rc;
use wows_replays::analyzer::battle_controller::DeathInfo;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::battle_controller::VehicleProps;
use wows_replays::analyzer::battle_controller::state::ActiveConsumable;
use wows_replays::analyzer::battle_controller::state::ActivePlane;
use wows_replays::analyzer::battle_controller::state::ActiveWard;
use wows_replays::analyzer::battle_controller::state::BuffZoneState;
use wows_replays::analyzer::battle_controller::state::BuildingEntity;
use wows_replays::analyzer::battle_controller::state::CapturePointState;
use wows_replays::analyzer::battle_controller::state::ConsumableInventory;
use wows_replays::analyzer::battle_controller::state::LocalWeatherZone;
use wows_replays::analyzer::battle_controller::state::SmokeScreenEntity;
use wows_replays::analyzer::decoder::ArtillerySalvo;
use wows_replays::analyzer::decoder::TorpedoData;
use wows_replays::types::AvatarId;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::GameParamId;
use wows_replays::types::NormalizedPos;
use wows_replays::types::TeamId;
use wows_replays::types::WorldPos;
use wowsunpack::game_params::types::BigWorldDistance;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_types::PlaneId;
use wowsunpack::game_types::WorldPos2D;

use crate::units::Degrees;
use crate::units::Radians;
use crate::units::VisibilityFlags;

// -- Kind markers (zero-sized, tag the archetype) --

/// Marks a vehicle (ship) entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Vehicle;

/// Marks a building/structure entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Building;

/// Marks a smoke screen entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct SmokeScreen;

/// Marks an active plane squadron entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Plane;

/// Marks a fighter patrol ward entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Ward;

/// Marks a capture point (interactive zone) entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct CapturePoint;

/// Marks a buff zone (arms race powerup drop) entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct BuffZone;

/// Marks a local weather zone entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct WeatherZone;

/// Marks a projectile (artillery salvo or torpedo) entity.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Projectile;

/// Coarse entity kind, derived from the marker component present on the entity.
///
/// Used for read-side comparison and reporting; not stored as a component itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntityKind {
    Vehicle,
    Building,
    SmokeScreen,
}

// -- Identity --

/// Game-assigned entity id, present on every spawned ECS entity.
#[derive(Component, Debug, Clone, Copy)]
pub struct GameId(pub EntityId);

// -- Positional state --

/// Last known world-space position and orientation.
#[derive(Component, Debug, Clone)]
pub struct Transform3d {
    pub pos: WorldPos,
    pub yaw: Radians,
    pub pitch: Radians,
    pub roll: Radians,
    pub last_updated: GameClock,
}

/// Last known minimap position and visibility state.
#[derive(Component, Debug, Clone)]
pub struct MinimapPlacement {
    pub pos: NormalizedPos,
    /// Heading in degrees.
    pub heading: Degrees,
    pub visible: bool,
    /// Bitmask of special detection reasons (radar, hydro, etc.).
    pub visibility_flags: VisibilityFlags,
    /// True when the entity is invisible (e.g. submarine submerged).
    pub is_invisible: bool,
    pub last_updated: GameClock,
}

// -- Vehicle state --

/// Full server-authoritative vehicle property state.
#[derive(Component, Debug, Clone)]
pub struct VehicleState(pub VehicleProps);

/// Aggregated record for one vehicle: captain ref, damage, death, results, frags.
///
/// Send+Sync (required by bevy Component) holds only if the `arc` feature makes `wows_replays::Rc = Arc`.
#[derive(Component, Debug, Clone)]
pub struct VehicleRecord {
    /// Captain `Param`, if resolved.
    pub captain: Option<Rc<Param>>,
    pub damage: f64,
    pub death: Option<DeathInfo>,
    /// End-of-battle results blob, mirroring `VehicleEntity.results_info`.
    pub results: Option<serde_json::Value>,
    pub frags: Vec<DeathInfo>,
}

/// Main battery turret orientation and ammo selection.
#[derive(Component, Debug, Clone)]
pub struct Aim {
    /// Per-turret yaws (group 0 only, relative to ship heading).
    pub turret_yaws: Vec<Radians>,
    /// World-space gun aim yaw, from `targetLocalPos`.
    pub target_yaw: Option<Radians>,
    /// Currently selected ammo `GameParamId`, artillery only.
    pub selected_ammo: Option<GameParamId>,
}

/// Consumable activation log and slot inventory for one vehicle.
#[derive(Component, Debug, Clone)]
pub struct Consumables {
    pub active: Vec<ActiveConsumable>,
    pub slots: Vec<ConsumableInventory>,
}

/// Link to the shared `Player` record for this vehicle.
///
/// Send+Sync (required by bevy Component) holds only if the `arc` feature makes `wows_replays::Rc = Arc`.
#[derive(Component, Debug, Clone)]
pub struct PlayerLink(pub Rc<Player>);

/// Current visibility state from Vehicle entity properties.
///
/// Kept separate from VehicleState so minimap rendering does not require full
/// vehicle-property parsing to be wired up.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct VehicleVisibility {
    pub visibility_flags: crate::units::VisibilityFlags,
    pub is_invisible: bool,
    pub team_id: i8,
}

// -- Non-vehicle entity state --

/// State for a building entity (drops the redundant id field; `GameId` carries it).
#[derive(Component, Debug, Clone)]
pub struct BuildingState {
    pub position: WorldPos,
    pub is_alive: bool,
    pub is_hidden: bool,
    pub is_suppressed: bool,
    pub team_id: TeamId,
    pub params_id: GameParamId,
}

impl From<&BuildingEntity> for BuildingState {
    fn from(b: &BuildingEntity) -> Self {
        Self {
            position: b.position,
            is_alive: b.is_alive,
            is_hidden: b.is_hidden,
            is_suppressed: b.is_suppressed,
            team_id: TeamId::from(b.team_id),
            params_id: b.params_id,
        }
    }
}

/// State for a smoke screen entity (drops the redundant id field).
#[derive(Component, Debug, Clone)]
pub struct SmokeScreenState {
    pub radius: BigWorldDistance,
    pub position: WorldPos,
    pub points: Vec<WorldPos>,
}

impl From<&SmokeScreenEntity> for SmokeScreenState {
    fn from(s: &SmokeScreenEntity) -> Self {
        Self { radius: s.radius, position: s.position, points: s.points.clone() }
    }
}

/// State for an active plane squadron (drops the redundant `plane_id`; `GameId` carries entity id).
#[derive(Component, Debug, Clone)]
pub struct PlaneState {
    pub plane_id: PlaneId,
    pub owner_id: EntityId,
    pub team_id: TeamId,
    pub params_id: GameParamId,
    pub position: WorldPos2D,
    pub last_updated: GameClock,
}

impl From<&ActivePlane> for PlaneState {
    fn from(p: &ActivePlane) -> Self {
        Self {
            plane_id: p.plane_id,
            owner_id: p.owner_id,
            team_id: TeamId::from(p.team_id),
            params_id: p.params_id,
            position: p.position,
            last_updated: p.last_updated,
        }
    }
}

/// State for a fighter patrol ward.
#[derive(Component, Debug, Clone)]
pub struct WardState {
    pub plane_id: PlaneId,
    pub position: WorldPos,
    pub radius: BigWorldDistance,
    pub owner_id: EntityId,
}

impl From<&ActiveWard> for WardState {
    fn from(w: &ActiveWard) -> Self {
        Self { plane_id: w.plane_id, position: w.position, radius: w.radius, owner_id: w.owner_id }
    }
}

// -- Zone state wrappers --

/// Current state of a capture point, carried in full.
#[derive(Component, Debug, Clone)]
pub struct CapturePointData(pub CapturePointState);

/// Current state of a buff zone (arms race powerup drop).
#[derive(Component, Debug, Clone)]
pub struct BuffZoneData(pub BuffZoneState);

/// State of a local weather zone.
#[derive(Component, Debug, Clone)]
pub struct WeatherZoneData(pub LocalWeatherZone);

// -- Projectile state --

/// Projectile state: either an artillery salvo or a torpedo.
///
/// A single `Projectile`-tagged entity holds one of these variants.
#[derive(Component, Debug, Clone)]
pub enum ProjectileState {
    Artillery {
        salvo: ArtillerySalvo,
        fired_at: GameClock,
        avatar_id: AvatarId,
    },
    Torpedo {
        torpedo: TorpedoData,
        launched_at: GameClock,
        updated_at: GameClock,
        avatar_id: AvatarId,
    },
}

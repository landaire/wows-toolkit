//! Entity lifecycle ingestion: EntityCreate, EntityLeave, arena-state seeding.

use std::str::FromStr as _;

use bevy_ecs::world::World;
use tracing::debug;
use tracing::warn;
use wows_replays::analyzer::battle_controller::EntityType;
use wows_replays::analyzer::battle_controller::VehicleProps;
use wows_replays::analyzer::decoder::PlayerStateData;
use wows_replays::game_constants::GameConstants;
use wows_replays::packet2::EntityCreatePacket;
use wowsunpack::rpc::typedefs::ArgValue;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::GameParamId;
use wows_replays::types::TeamId;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::types::BigWorldDistance;
use wowsunpack::game_types::WorldPos;

use crate::components::{
    Building, BuildingState, BuffZone, BuffZoneData, CapturePoint, CapturePointData, GameId,
    SmokeScreen, SmokeScreenState, Transform3d, Vehicle, VehicleState, WeatherZone, WeatherZoneData,
};
use crate::resources::{CapturePointOrder, EntityIndex, InteractiveZoneIndex};

/// Handle an EntityCreate packet.
pub fn handle_entity_create<G: ResourceLoader>(
    clock: GameClock,
    packet: &EntityCreatePacket<'_>,
    world: &mut World,
    resources: &G,
    constants: &GameConstants,
    version: Version,
) {
    let entity_type = match EntityType::from_str(packet.entity_type) {
        Ok(et) => et,
        Err(_) => {
            warn!("unknown entity type: {}", packet.entity_type);
            return;
        }
    };

    match entity_type {
        EntityType::Vehicle => handle_vehicle_create(packet, world, resources, constants, version),
        EntityType::Building => handle_building_create(clock, packet, world),
        EntityType::SmokeScreen => handle_smoke_create(packet, world),
        EntityType::BattleLogic => handle_battle_logic_create(packet, world, constants, version),
        EntityType::InteractiveZone => {
            handle_interactive_zone_create(packet, world, constants, version);
        }
        EntityType::BattleEntity => {
            debug!("BattleEntity create (entity_id={})", packet.entity_id);
        }
    }
}

fn handle_vehicle_create<G: ResourceLoader>(
    packet: &EntityCreatePacket<'_>,
    world: &mut World,
    _resources: &G,
    constants: &GameConstants,
    version: Version,
) {
    let props = VehicleProps::from_create_props(&packet.props, version, constants);
    let entity = spawn_or_get(world, packet.entity_id);
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(Vehicle);
        e.insert(VehicleState(props));
    }
}

fn handle_building_create(_clock: GameClock, packet: &EntityCreatePacket<'_>, world: &mut World) {
    let mut is_alive = true;
    let mut is_hidden = false;
    let mut is_suppressed = false;
    let mut team_id: i8 = 0;
    let mut params_id: u32 = 0;

    if let Some(v) = packet.props.get("isAlive") {
        is_alive = v.uint_8_ref().map(|v| *v != 0).unwrap_or(true);
    }
    if let Some(v) = packet.props.get("isHidden") {
        is_hidden = v.uint_8_ref().map(|v| *v != 0).unwrap_or(false);
    }
    if let Some(v) = packet.props.get("isSuppressed") {
        is_suppressed = v.uint_8_ref().map(|v| *v != 0).unwrap_or(false);
    }
    if let Some(v) = packet.props.get("teamId") {
        team_id = v.int_8_ref().copied().unwrap_or(0);
    }
    if let Some(v) = packet.props.get("paramsId") {
        params_id = v.uint_32_ref().copied().unwrap_or(0);
    }

    let position = WorldPos { x: packet.position.x, y: packet.position.y, z: packet.position.z };
    let state = BuildingState {
        position,
        is_alive,
        is_hidden,
        is_suppressed,
        team_id: TeamId::from(team_id as i64),
        params_id: GameParamId::from(params_id),
    };

    let entity = spawn_or_get(world, packet.entity_id);
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(Building);
        e.insert(state);
    }
}

fn handle_smoke_create(packet: &EntityCreatePacket<'_>, world: &mut World) {
    let radius = BigWorldDistance::from(
        packet.props.get("radius").and_then(|v| v.float_32_ref().copied()).unwrap_or(0.0),
    );
    let position = WorldPos { x: packet.position.x, y: packet.position.y, z: packet.position.z };
    let state = SmokeScreenState { radius, position, points: vec![position] };

    let entity = spawn_or_get(world, packet.entity_id);
    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(SmokeScreen);
        e.insert(state);
    }
}

fn handle_battle_logic_create(
    packet: &EntityCreatePacket<'_>,
    world: &mut World,
    constants: &GameConstants,
    version: Version,
) {
    debug!("BattleLogic create (entity_id={})", packet.entity_id);

    // Legacy control points (pre-InteractiveZone clients, e.g. 0.9.10):
    // seed capture_points from state.controlPoints if no InteractiveZone has
    // populated them yet.
    if world.resource::<CapturePointOrder>().0.is_empty()
        && let Some(state) = packet.props.get("state")
            && let Some(state_dict) = as_dict(state)
            && let Some(ArgValue::Array(control_points)) = state_dict.get("controlPoints")
        {
            let cps: Vec<_> = control_points
                .iter()
                .enumerate()
                .filter_map(|(idx, entry)| {
                    parse_legacy_control_point(idx, entry, constants, version)
                })
                .collect();
            for cp in cps {
                let cp_entity = world.spawn(()).id();
                if let Ok(mut e) = world.get_entity_mut(cp_entity) {
                    e.insert(CapturePoint);
                    e.insert(CapturePointData(cp));
                }
                world.resource_mut::<CapturePointOrder>().0.push(cp_entity);
            }
        }
}

fn handle_interactive_zone_create(
    packet: &EntityCreatePacket<'_>,
    world: &mut World,
    constants: &GameConstants,
    version: Version,
) {
    use wows_replays::analyzer::battle_controller::state::{
        BuffZoneState, CapturePointState, ControlPointType, InteractiveZoneType,
    };
    use wows_replays::analyzer::decoder::Recognized;

    let position = WorldPos { x: packet.position.x, y: packet.position.y, z: packet.position.z };
    let radius = packet.props.get("radius").and_then(|v| v.float_32_ref().copied()).unwrap_or(0.0);
    let team_id = packet.props.get("teamId").and_then(|v| v.as_i64()).unwrap_or(-1);

    let zone_type: Option<Recognized<InteractiveZoneType>> =
        packet.props.get("type").and_then(|v| v.as_i64()).and_then(|id| {
            InteractiveZoneType::from_id(id as i32, constants.battle(), version)
        });
    let is_weather = zone_type.as_ref().and_then(|r| r.known().copied())
        == Some(InteractiveZoneType::WeatherZone);

    if is_weather {
        let name = decode_name(packet.props.get("name"));
        let wz = wows_replays::analyzer::battle_controller::state::LocalWeatherZone {
            name,
            position,
            radius,
            params_id: GameParamId::default(),
            entity_id: Some(packet.entity_id),
        };
        let entity = spawn_or_get(world, packet.entity_id);
        if let Ok(mut e) = world.get_entity_mut(entity) {
            e.insert(WeatherZone);
            e.insert(WeatherZoneData(wz));
        }
        return;
    }

    // Non-weather: capture point or buff zone.
    let mut cp_index: Option<usize> = None;
    let mut cp_type: Option<Recognized<ControlPointType>> = None;
    let mut has_invaders = false;
    let mut invader_team: i64 = -1;
    let mut progress: f64 = 0.0;
    let mut both_inside = false;
    let mut is_enabled = true;

    if let Some(cs) = packet.props.get("componentsState")
        && let Some(cs_dict) = as_dict(cs)
    {
        if let Some(cp) = cs_dict.get("controlPoint")
            && let Some(cp_dict) = as_dict(cp)
        {
            if let Some(idx) = cp_dict.get("index") {
                cp_index = idx.as_i64().map(|v| v as usize);
            }
            if let Some(t) = cp_dict.get("type") {
                cp_type = t.as_i64().and_then(|id| {
                    ControlPointType::from_id(id as i32, constants.battle(), version)
                });
            }
        }
        if let Some(cl) = cs_dict.get("captureLogic")
            && let Some(cl_dict) = as_dict(cl)
        {
            has_invaders = cl_dict.get("hasInvaders").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
            invader_team = cl_dict.get("invaderTeam").and_then(|v| v.as_i64()).unwrap_or(-1);
            progress =
                cl_dict.get("progress").and_then(|v| v.float_32_ref()).map(|f| *f as f64).unwrap_or(0.0);
            both_inside = cl_dict.get("bothInside").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
            is_enabled = cl_dict.get("isEnabled").and_then(|v| v.as_i64()).unwrap_or(1) != 0;
        }
    }

    if let Some(idx) = cp_index {
        let cp_state = CapturePointState {
            index: idx,
            position: Some(position),
            radius,
            control_point_type: cp_type,
            team_id,
            invader_team,
            progress: (progress, 0.0),
            has_invaders,
            both_inside,
            is_enabled,
        };

        let entity = spawn_or_get(world, packet.entity_id);
        if let Ok(mut e) = world.get_entity_mut(entity) {
            e.insert(CapturePoint);
            e.insert(CapturePointData(cp_state));
        }

        let order_len = world.resource::<CapturePointOrder>().0.len();
        if order_len <= idx {
            // Grow the ordered vec; use a placeholder entity for gaps.
            let placeholder = world.spawn(()).id();
            for _ in order_len..=idx {
                world.resource_mut::<CapturePointOrder>().0.push(placeholder);
            }
        }
        let entity = world.resource::<EntityIndex>().get(packet.entity_id).unwrap();
        world.resource_mut::<CapturePointOrder>().0[idx] = entity;
        world.resource_mut::<InteractiveZoneIndex>().0.insert(packet.entity_id, idx);
    } else {
        // Buff zone (arms race powerup drop).
        let bz_state = BuffZoneState {
            entity_id: packet.entity_id,
            position,
            radius,
            team_id,
            is_active: is_enabled,
            drop_params_id: None,
        };
        let entity = spawn_or_get(world, packet.entity_id);
        if let Ok(mut e) = world.get_entity_mut(entity) {
            e.insert(BuffZone);
            e.insert(BuffZoneData(bz_state));
        }
        // Register in InteractiveZoneIndex with sentinel usize::MAX, matching original.
        world.resource_mut::<InteractiveZoneIndex>().0.insert(packet.entity_id, usize::MAX);
    }
}

/// Seed Vehicle entities for every participant listed in OnArenaStateReceived.
///
/// Mirrors BattleController::seed_vehicles_from_arena_state: pre-creates entities
/// so even ships that never enter the AoI have a Vehicle component.
pub fn seed_vehicles_from_arena_state<'a, G: ResourceLoader>(
    players: impl Iterator<Item = &'a PlayerStateData>,
    world: &mut World,
    _resources: &G,
    constants: &GameConstants,
    version: Version,
) {
    let players: Vec<&PlayerStateData> = players.collect();
    for player in players {
        let entity_id = player.entity_id();
        if world.resource::<EntityIndex>().get(entity_id).is_some() {
            continue;
        }

        let args = arena_state_to_args(player);
        let props = VehicleProps::from_create_props(&args, version, constants);

        let entity = spawn_or_get(world, entity_id);
        if let Ok(mut e) = world.get_entity_mut(entity) {
            e.insert(Vehicle);
            e.insert(VehicleState(props));
        }
    }
}

/// Handle EntityLeave.
///
/// Policy (mirrors BattleController):
/// - SmokeScreen entity: despawn and remove from EntityIndex.
/// - BuffZone entity: despawn and remove from EntityIndex.
/// - Vehicle/Building: keep the ECS entity; only remove its Transform3d component
///   so stale world-position rendering stops. MinimapPlacement is kept.
pub fn handle_entity_leave(entity_id: EntityId, world: &mut World) {
    let ecs_entity = world.resource::<EntityIndex>().get(entity_id);

    let is_smoke = ecs_entity
        .and_then(|e| world.get_entity(e).ok())
        .map(|er| er.contains::<SmokeScreen>())
        .unwrap_or(false);
    let is_buff = ecs_entity
        .and_then(|e| world.get_entity(e).ok())
        .map(|er| er.contains::<BuffZone>())
        .unwrap_or(false);

    if is_smoke || is_buff {
        if let Some(entity) = world.resource_mut::<EntityIndex>().remove(entity_id)
            && world.get_entity(entity).is_ok() {
                world.despawn(entity);
            }
        return;
    }

    // Vehicles and buildings: remove only Transform3d, keeping MinimapPlacement.
    if let Some(ecs_entity) = world.resource::<EntityIndex>().get(entity_id)
        && let Ok(mut er) = world.get_entity_mut(ecs_entity) {
            er.remove::<Transform3d>();
        }
}

fn spawn_or_get(world: &mut World, id: EntityId) -> bevy_ecs::entity::Entity {
    if let Some(entity) = world.resource::<EntityIndex>().get(id) {
        return entity;
    }
    let entity = world.spawn((GameId(id),)).id();
    world.resource_mut::<EntityIndex>().insert(id, entity);
    entity
}

fn as_dict<'a, 'b>(
    v: &'a ArgValue<'b>,
) -> Option<&'a std::collections::HashMap<&'b str, ArgValue<'b>>> {
    match v {
        ArgValue::FixedDict(d) => Some(d),
        ArgValue::NullableFixedDict(Some(d)) => Some(d),
        _ => None,
    }
}

fn decode_name(v: Option<&ArgValue<'_>>) -> String {
    match v {
        Some(ArgValue::Array(arr)) => {
            let bytes: Vec<u8> = arr.iter().filter_map(|v| v.as_i64().map(|i| i as u8)).collect();
            String::from_utf8(bytes).unwrap_or_default()
        }
        Some(ArgValue::String(s)) => String::from_utf8_lossy(s).into_owned(),
        _ => String::new(),
    }
}

fn arena_state_to_args(
    player: &PlayerStateData,
) -> std::collections::HashMap<&'static str, ArgValue<'static>> {
    let mut args = std::collections::HashMap::new();
    if player.max_health() > 0 {
        args.insert("maxHealth", ArgValue::Float32(player.max_health() as f32));
    }
    if let Some(blob) = player.ship_config_dump() {
        args.insert("shipConfig", ArgValue::Blob(blob));
    }
    args.insert("teamId", ArgValue::Int8(player.team_id() as i8));
    args.insert("isAlive", ArgValue::Uint8(if player.is_alive() { 1 } else { 0 }));
    args.insert("isBot", ArgValue::Uint8(if player.is_bot() { 1 } else { 0 }));
    args
}

fn parse_legacy_control_point(
    idx: usize,
    entry: &ArgValue<'_>,
    constants: &GameConstants,
    version: Version,
) -> Option<wows_replays::analyzer::battle_controller::state::CapturePointState> {
    use wows_replays::analyzer::battle_controller::state::{CapturePointState, ControlPointType};

    let dict = as_dict(entry)?;
    let position = match dict.get("position") {
        Some(ArgValue::Vector2((x, z))) => Some(WorldPos { x: *x, y: 0.0, z: *z }),
        Some(ArgValue::Array(p)) if p.len() >= 2 => {
            Some(WorldPos { x: p[0].as_f32().unwrap_or(0.0), y: 0.0, z: p[1].as_f32().unwrap_or(0.0) })
        }
        _ => None,
    };
    let radius = dict.get("radius").and_then(|v| v.as_f32()).unwrap_or(0.0);
    let team_id = dict.get("teamId").and_then(|v| v.as_i64()).unwrap_or(-1);
    let invader_team = dict.get("invaderTeam").and_then(|v| v.as_i64()).unwrap_or(-1);
    let has_invaders = dict.get("hasInvaders").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
    let both_inside = dict.get("bothInside").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
    let is_enabled = dict.get("isEnabled").and_then(|v| v.as_i64()).unwrap_or(1) != 0;
    let control_point_type = dict
        .get("controlPointType")
        .and_then(|v| v.as_i64())
        .and_then(|id| ControlPointType::from_id(id as i32, constants.battle(), version));
    let progress = match dict.get("progress") {
        Some(ArgValue::Array(p)) if p.len() >= 2 => {
            (p[0].as_f32().unwrap_or(0.0) as f64, p[1].as_f32().unwrap_or(0.0) as f64)
        }
        _ => (0.0, 0.0),
    };
    Some(CapturePointState {
        index: idx,
        position,
        radius,
        control_point_type,
        team_id,
        invader_team,
        progress,
        has_invaders,
        both_inside,
        is_enabled,
    })
}

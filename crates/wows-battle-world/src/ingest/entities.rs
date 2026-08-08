//! Entity lifecycle ingestion: EntityCreate, EntityLeave, arena-state seeding.

use std::str::FromStr as _;

use bevy_ecs::world::World;
use tracing::debug;
use tracing::warn;
use wows_replays::Rc;
use wows_replays::analyzer::battle_controller::ConnectionChangeInfo;
use wows_replays::analyzer::battle_controller::ConnectionChangeKind;
use wows_replays::analyzer::battle_controller::EntityType;
use wows_replays::analyzer::battle_controller::Player;
use wows_replays::analyzer::battle_controller::VehicleProps;
use wows_replays::analyzer::decoder::PlayerStateData;
use wows_replays::game_constants::GameConstants;
use wows_replays::packet2::EntityCreatePacket;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::GameParamId;
use wows_replays::types::Relation;
use wows_replays::types::TeamId;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_params::types::BigWorldDistance;
use wowsunpack::game_types::WorldPos;
use wowsunpack::rpc::typedefs::ArgValue;

use crate::components::BuffZone;
use crate::components::BuffZoneData;
use crate::components::Building;
use crate::components::BuildingState;
use crate::components::Captain;
use crate::components::CapturePoint;
use crate::components::CapturePointData;
use crate::components::Division;
use crate::components::GameId;
use crate::components::PlayerLink;
use crate::components::SmokeScreen;
use crate::components::SmokeScreenState;
use crate::components::Transform3d;
use crate::components::Vehicle;
use crate::components::VehicleState;
use crate::components::WeatherZone;
use crate::components::WeatherZoneData;
use crate::resources::BURN_MASK;
use crate::resources::BURNING_FLAGS_PROPERTY;
use crate::resources::BurnFlagsObserved;
use crate::resources::BurnStateChange;
use crate::resources::BurnStateLog;
use crate::resources::CapturePointOrder;
use crate::resources::EntityIndex;
use crate::resources::InteractiveZoneIndex;
use crate::resources::InteractiveZoneRef;
use crate::resources::KillLog;
use crate::resources::MetadataPlayers;
use crate::resources::PendingDropParams;
use crate::resources::PlayerIndex;
use crate::resources::PresenceLog;
use crate::resources::PresenceWindow;
use crate::resources::WeatherZoneOrder;

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
        EntityType::Vehicle => handle_vehicle_create(clock, packet, world, resources, constants, version),
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
    clock: GameClock,
    packet: &EntityCreatePacket<'_>,
    world: &mut World,
    resources: &G,
    constants: &GameConstants,
    version: Version,
) {
    let props = VehicleProps::from_create_props(&packet.props, version, constants);
    let captain_id = props.crew_modifiers_compact_params().params_id();
    let captain = if captain_id.raw() != 0 {
        resources.game_param_by_id(captain_id).or_else(|| {
            warn!("failed to get captain param for id={}", captain_id);
            None
        })
    } else {
        None
    };
    // Snapshot player link before borrowing the entity mutably.
    let player_rc = world.resource::<PlayerIndex>().0.get(&packet.entity_id).cloned();
    let entity = spawn_or_get(world, packet.entity_id);

    // This create replaces VehicleState wholesale, so the burn bits it carries
    // are diffed here instead of by the EntityProperty path. Without it the
    // presence window opened below would certify a range whose burn baseline
    // was never recorded, and a mask reconstructed from BurnStateLog would
    // report an already-alight section as free. A vehicle with no VehicleState
    // yet has no observed burning sections, so an absent component is a zero
    // baseline, matching the default VehicleProps this create would replace.
    let previous_burn = world
        .get_entity(entity)
        .ok()
        .and_then(|er| er.get::<VehicleState>().map(|vs| vs.0.burning_flags() & BURN_MASK))
        .unwrap_or(0);
    let current_burn = props.burning_flags() & BURN_MASK;
    if packet.props.contains_key(BURNING_FLAGS_PROPERTY) {
        world.resource_mut::<BurnFlagsObserved>().0 = true;
    }

    if let Ok(mut e) = world.get_entity_mut(entity) {
        e.insert(Vehicle);
        e.insert(VehicleState(props));
        e.insert(Captain(captain));
        // Attach player link when the player was registered via NewPlayerSpawnedInBattle
        // before this EntityCreate arrived.
        if let Some(rc) = player_rc
            && !e.contains::<PlayerLink>()
        {
            e.insert(PlayerLink(rc));
        }
    }

    // Ordered before open_presence so the baseline precedes the window it
    // makes sound.
    if previous_burn != current_burn {
        world.resource_mut::<BurnStateLog>().0.push(BurnStateChange {
            victim: packet.entity_id,
            clock,
            previous: previous_burn,
            current: current_burn,
        });
    }
    open_presence(world, packet.entity_id, clock);
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
        // teamId is always present in building EntityCreate packets; the 0 fallback is
        // effectively unreachable and matches the old controller's Default state.
        team_id = v.int_8_ref().copied().unwrap_or(0);
    }
    if let Some(v) = packet.props.get("paramsId") {
        // 0 maps to GameParamId::default() (no param); correct when key absent (mirrors old controller).
        params_id = v.uint_32_ref().copied().unwrap_or(0);
    }

    let position = WorldPos::new(packet.position.x, packet.position.y, packet.position.z);
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
    let radius =
        BigWorldDistance::from(packet.props.get("radius").and_then(|v| v.float_32_ref().copied()).unwrap_or(0.0));
    let position = WorldPos::new(packet.position.x, packet.position.y, packet.position.z);
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

    // Seed TeamScores, ScoringRules, and LocalWeatherZones from BattleLogic state.
    super::zones::seed_battle_logic_state(&packet.props, world);

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
            .filter_map(|(idx, entry)| parse_legacy_control_point(idx, entry, constants, version))
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
    use wows_replays::analyzer::battle_controller::state::BuffZoneState;
    use wows_replays::analyzer::battle_controller::state::CapturePointState;
    use wows_replays::analyzer::battle_controller::state::ControlPointType;
    use wows_replays::analyzer::battle_controller::state::InteractiveZoneType;
    use wows_replays::analyzer::decoder::Recognized;

    let position = WorldPos::new(packet.position.x, packet.position.y, packet.position.z);
    let radius = packet.props.get("radius").and_then(|v| v.float_32_ref().copied()).unwrap_or(0.0);
    // -1 is the game's own "no owning team" encoding for zones (mirrors old controller and game protocol).
    let team_id = packet.props.get("teamId").and_then(|v| v.as_i64()).unwrap_or(-1);

    let zone_type: Option<Recognized<InteractiveZoneType>> = packet
        .props
        .get("type")
        .and_then(|v| v.as_i64())
        .and_then(|id| InteractiveZoneType::from_id(id as i32, constants.battle(), version));
    let is_weather = zone_type.as_ref().and_then(|r| r.known().copied()) == Some(InteractiveZoneType::WeatherZone);

    if is_weather {
        let name = decode_name(packet.props.get("name"));

        // Try to match against a WeatherZoneData already seeded from BattleLogic state.
        // If found (by name, no entity_id yet), update it in-place instead of creating a duplicate.
        let matched: Option<bevy_ecs::entity::Entity> = {
            let mut q = world.query::<(bevy_ecs::entity::Entity, &WeatherZoneData)>();
            let mut found = None;
            for (ecs_entity, data) in q.iter(world) {
                if data.0.name == name && data.0.entity_id.is_none() {
                    found = Some(ecs_entity);
                    break;
                }
            }
            found
        };

        if let Some(ecs_entity) = matched {
            // Link the ECS entity to the game entity id and update position/radius.
            world.resource_mut::<EntityIndex>().insert(packet.entity_id, ecs_entity);
            if let Ok(mut e) = world.get_entity_mut(ecs_entity)
                && let Some(mut data) = e.get_mut::<WeatherZoneData>()
            {
                data.0.entity_id = Some(packet.entity_id);
                data.0.position = position;
                data.0.radius = radius;
            }
        } else {
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
            world.resource_mut::<WeatherZoneOrder>().0.push(entity);
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
                cp_type = t.as_i64().and_then(|id| ControlPointType::from_id(id as i32, constants.battle(), version));
            }
        }
        if let Some(cl) = cs_dict.get("captureLogic")
            && let Some(cl_dict) = as_dict(cl)
        {
            has_invaders = cl_dict.get("hasInvaders").and_then(|v| v.as_i64()).unwrap_or(0) != 0;
            // -1 is the game's own "no invader" encoding (documented in decode.rs and mirrors old controller).
            invader_team = cl_dict.get("invaderTeam").and_then(|v| v.as_i64()).unwrap_or(-1);
            progress = cl_dict.get("progress").and_then(|v| v.float_32_ref()).map(|f| *f as f64).unwrap_or(0.0);
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
            // Fill index gaps with default CapturePointData so capture_points()
            // returns vec length == max_index+1 (mirrors the original's while-push).
            for gap in order_len..idx {
                let gap_entity = world.spawn(()).id();
                if let Ok(mut e) = world.get_entity_mut(gap_entity) {
                    e.insert(CapturePoint);
                    let default_state = CapturePointState { index: gap, ..Default::default() };
                    e.insert(CapturePointData(default_state));
                }
                world.resource_mut::<CapturePointOrder>().0.push(gap_entity);
            }
            // Reserve the slot for idx; overwritten immediately below.
            let slot_entity = world.resource::<EntityIndex>().get(packet.entity_id).unwrap();
            world.resource_mut::<CapturePointOrder>().0.push(slot_entity);
        }
        let entity = world.resource::<EntityIndex>().get(packet.entity_id).unwrap();
        world.resource_mut::<CapturePointOrder>().0[idx] = entity;
        world.resource_mut::<InteractiveZoneIndex>().0.insert(packet.entity_id, InteractiveZoneRef::CapturePoint(idx));
    } else {
        // Buff zone (arms race powerup drop).
        // Apply any drop params that arrived before this entity was created.
        let drop_params_id = world.resource::<PendingDropParams>().0.get(&packet.entity_id).copied();
        let bz_state = BuffZoneState {
            entity_id: packet.entity_id,
            position,
            radius,
            team_id,
            is_active: is_enabled,
            drop_params_id,
        };
        let entity = spawn_or_get(world, packet.entity_id);
        if let Ok(mut e) = world.get_entity_mut(entity) {
            e.insert(BuffZone);
            e.insert(BuffZoneData(bz_state));
        }
        world.resource_mut::<InteractiveZoneIndex>().0.insert(packet.entity_id, InteractiveZoneRef::BuffZone);
    }
}

/// Seed Vehicle entities and Player records for every participant in OnArenaStateReceived.
///
/// Mirrors BattleController's OnArenaStateReceived arm: builds Player objects from the
/// arena roster, inserts them into PlayerIndex, attaches PlayerLink, and pushes the
/// initial connection record when `is_connected()` (mirrors controller.rs ~3264-3270).
pub fn seed_vehicles_from_arena_state<'a, G: ResourceLoader>(
    players: impl Iterator<Item = &'a PlayerStateData>,
    clock: GameClock,
    world: &mut World,
    resources: &G,
    constants: &GameConstants,
    version: Version,
) {
    let players: Vec<&PlayerStateData> = players.collect();

    // Snapshot metadata players to avoid borrowing world twice.
    let metadata: Vec<_> = world.resource::<MetadataPlayers>().0.clone();

    for player in &players {
        let entity_id = player.entity_id();

        // Deliberately no presence window opened here: OnArenaStateReceived lists
        // every match participant, including ships the recording client's AOI
        // never detects and for which no EntityCreate ever arrives (see
        // gather_replay_facts's doc comment in merged.rs). Opening a window on
        // the seed would report such a ship as observed for the whole match,
        // which is exactly backwards. Presence is proven only by EntityCreate.

        // Build Player if not already in the index.
        if !world.resource::<PlayerIndex>().0.contains_key(&entity_id) {
            let meta = metadata.iter().find(|m| m.id() == player.player_id()).or_else(|| {
                let name = player.username();
                if name.is_empty() { None } else { metadata.iter().find(|m| m.name() == name) }
            });

            match meta {
                None => {
                    warn!("could not map arena player to metadata player (player_id={})", player.player_id());
                }
                Some(meta) => {
                    if let Some(battle_player) = Player::from_arena_player(player, meta.as_ref(), resources) {
                        // Mirror controller.rs ~3252-3270: check if the vehicle
                        // was already in entities_by_id and had a frag against it.
                        let player_has_died = world
                            .resource::<EntityIndex>()
                            .get(entity_id)
                            .is_some_and(|_| world.resource::<KillLog>().0.iter().any(|kill| kill.victim == entity_id));
                        if player.is_connected() {
                            battle_player.connection_change_info_mut().push(ConnectionChangeInfo::new(
                                clock.to_duration(),
                                ConnectionChangeKind::Connected,
                                player_has_died,
                            ));
                        }
                        let battle_player = Rc::new(battle_player);
                        world.resource_mut::<PlayerIndex>().0.insert(entity_id, battle_player);
                    }
                }
            }
        }

        // Pre-create the vehicle entity if not already present.
        if world.resource::<EntityIndex>().get(entity_id).is_some() {
            // Attach PlayerLink if we just built a Player for an already-existing entity.
            if let Some(player_rc) = world.resource::<PlayerIndex>().0.get(&entity_id).cloned() {
                let ecs_entity = world.resource::<EntityIndex>().get(entity_id).unwrap();
                if let Ok(mut e) = world.get_entity_mut(ecs_entity)
                    && !e.contains::<PlayerLink>()
                {
                    e.insert(PlayerLink(player_rc));
                }
            }
            continue;
        }

        let ship_config_dump = player.ship_config_dump();
        let args = arena_state_to_args(player, ship_config_dump.as_deref());
        let mut props = VehicleProps::from_create_props(&args, version, constants);
        // Arena state does not broadcast live health; seed from max so HP is full
        // instead of 0 until the first EntityProperty(health) arrives.
        props.seed_initial_health();

        let captain_id = props.crew_modifiers_compact_params().params_id();
        let captain = if captain_id.raw() != 0 {
            resources.game_param_by_id(captain_id).or_else(|| {
                warn!("failed to get captain param for id={}", captain_id);
                None
            })
        } else {
            None
        };

        // Snapshot player_rc before entity_mut borrow.
        let player_rc = world.resource::<PlayerIndex>().0.get(&entity_id).cloned();
        let entity = spawn_or_get(world, entity_id);
        if let Ok(mut e) = world.get_entity_mut(entity) {
            e.insert(Vehicle);
            e.insert(VehicleState(props));
            e.insert(Captain(captain));
            if let Some(player_rc) = player_rc {
                e.insert(PlayerLink(player_rc));
            }
        }
    }

    derive_divisions(world);
}

/// Attach `Division` components, reconstructing the in-game per-team division labels.
///
/// Within each team the distinct non-zero prebattle ids are sorted ascending and
/// labelled A, B, C... matching the game's per-team division `sign` (letter =
/// `'A' + sign`); the replay's per-player `preBattleSign` is server-zeroed, so the
/// label is rebuilt from prebattle-id order. Idempotent: re-running overwrites the
/// component, so repeated arena-state seeding stays consistent.
fn derive_divisions(world: &mut World) {
    // Snapshot (entity, team, prebattle_id) for every vehicle whose player is in a division.
    let mut members: Vec<(bevy_ecs::entity::Entity, i64, i64)> = Vec::new();
    {
        let mut query =
            world.query_filtered::<(bevy_ecs::entity::Entity, &PlayerLink), bevy_ecs::query::With<Vehicle>>();
        for (entity, link) in query.iter(world) {
            let state = link.0.initial_state();
            let prebattle_id = state.division_id();
            if prebattle_id > 0 {
                members.push((entity, state.team_id(), prebattle_id));
            }
        }
    }

    // Per team, sort the distinct prebattle ids ascending; position becomes the letter.
    let mut ids_by_team: std::collections::HashMap<i64, Vec<i64>> = std::collections::HashMap::new();
    for (_, team, prebattle_id) in &members {
        let ids = ids_by_team.entry(*team).or_default();
        if !ids.contains(prebattle_id) {
            ids.push(*prebattle_id);
        }
    }
    let mut letter_by_id: std::collections::HashMap<i64, char> = std::collections::HashMap::new();
    for ids in ids_by_team.values_mut() {
        ids.sort_unstable();
        for (idx, prebattle_id) in ids.iter().enumerate() {
            letter_by_id.insert(*prebattle_id, char::from(b'A'.saturating_add(idx as u8)));
        }
    }

    for (entity, _, prebattle_id) in members {
        if let Some(&letter) = letter_by_id.get(&prebattle_id)
            && let Ok(mut e) = world.get_entity_mut(entity)
        {
            e.insert(Division { prebattle_id, letter });
        }
    }
}

/// Register players from mid-battle spawns (Operations reinforcement waves).
///
/// Mirrors NewPlayerSpawnedInBattle in BattleController: only inserts the player
/// into PlayerIndex. No Vehicle entity is created here; the subsequent EntityCreate
/// for that player's vehicle handles entity creation and attaches PlayerLink.
pub fn seed_spawned_players<'a, G: ResourceLoader>(
    players: impl Iterator<Item = &'a PlayerStateData>,
    world: &mut World,
    resources: &G,
    _constants: &GameConstants,
    _version: Version,
) {
    let players: Vec<&PlayerStateData> = players.collect();

    let self_team_id = world
        .resource::<PlayerIndex>()
        .0
        .values()
        .find(|p| p.relation().is_self())
        .map(|p| p.initial_state().team_id());

    for player in &players {
        let entity_id = player.entity_id();

        if world.resource::<PlayerIndex>().0.contains_key(&entity_id) {
            continue;
        }

        if let Some(self_team) = self_team_id {
            let relation = if player.team_id() == self_team { Relation::new(1) } else { Relation::new(2) };
            if let Some(battle_player) = Player::from_spawned_player(player, resources, relation) {
                let battle_player = Rc::new(battle_player);
                world.resource_mut::<PlayerIndex>().0.insert(entity_id, battle_player);
            }
        } else {
            warn!("NewPlayerSpawnedInBattle before self player resolved: skipping relation derivation");
        }
    }
}

/// Handle EntityLeave.
///
/// Policy (mirrors BattleController):
/// - SmokeScreen entity: despawn and remove from EntityIndex.
/// - BuffZone entity: despawn and remove from EntityIndex.
/// - Vehicle/Building: keep the ECS entity; only remove its Transform3d component
///   so stale world-position rendering stops. MinimapPlacement is kept. Any open
///   PresenceLog window for this entity id is closed at `clock`.
pub fn handle_entity_leave(entity_id: EntityId, clock: GameClock, world: &mut World) {
    let ecs_entity = world.resource::<EntityIndex>().get(entity_id);

    let is_smoke =
        ecs_entity.and_then(|e| world.get_entity(e).ok()).map(|er| er.contains::<SmokeScreen>()).unwrap_or(false);
    let is_buff =
        ecs_entity.and_then(|e| world.get_entity(e).ok()).map(|er| er.contains::<BuffZone>()).unwrap_or(false);

    if is_smoke || is_buff {
        if let Some(entity) = world.resource_mut::<EntityIndex>().remove(entity_id)
            && world.get_entity(entity).is_ok()
        {
            world.despawn(entity);
        }
        return;
    }

    // Vehicles and buildings: remove only Transform3d, keeping MinimapPlacement.
    // Presence is only ever opened for vehicles, so this is a no-op for buildings.
    close_presence(world, entity_id, clock);
    if let Some(ecs_entity) = world.resource::<EntityIndex>().get(entity_id)
        && let Ok(mut er) = world.get_entity_mut(ecs_entity)
    {
        er.remove::<Transform3d>();
    }
}

/// Open a presence window for `id` at `clock`. Idempotent: if `id` already
/// holds an open window (no EntityLeave has closed it), this does nothing.
fn open_presence(world: &mut World, id: EntityId, clock: GameClock) {
    let mut log = world.resource_mut::<PresenceLog>();
    // The create is itself a sighting, so a vehicle that never gets another
    // update still certifies the instant it was seen rather than nothing.
    log.note_seen(id, clock);
    let windows = log.windows.entry(id).or_default();
    if windows.last().is_some_and(|w| w.left.is_none()) {
        return;
    }
    windows.push(PresenceWindow { entered: clock, left: None });
}

/// Close `id`'s open presence window at `clock`, if it has one. An id with no
/// windows, or whose latest window is already closed, is left untouched.
pub(crate) fn close_presence(world: &mut World, id: EntityId, clock: GameClock) {
    world.resource_mut::<PresenceLog>().close_window(id, clock);
}

fn spawn_or_get(world: &mut World, id: EntityId) -> bevy_ecs::entity::Entity {
    if let Some(entity) = world.resource::<EntityIndex>().get(id) {
        return entity;
    }
    let entity = world.spawn((GameId(id),)).id();
    world.resource_mut::<EntityIndex>().insert(id, entity);
    entity
}

fn as_dict<'a, 'b>(v: &'a ArgValue<'b>) -> Option<&'a std::collections::HashMap<&'b str, ArgValue<'b>>> {
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

fn arena_state_to_args<'a>(
    player: &PlayerStateData,
    ship_config: Option<&'a [u8]>,
) -> std::collections::HashMap<&'static str, ArgValue<'a>> {
    let mut args = std::collections::HashMap::new();
    if player.max_health() > 0 {
        args.insert("maxHealth", ArgValue::Float32(player.max_health() as f32));
    }
    if let Some(blob) = ship_config {
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
    use wows_replays::analyzer::battle_controller::state::CapturePointState;
    use wows_replays::analyzer::battle_controller::state::ControlPointType;

    let dict = as_dict(entry)?;
    let position = match dict.get("position") {
        Some(ArgValue::Vector2((x, z))) => Some(WorldPos::new(*x, 0.0, *z)),
        Some(ArgValue::Array(p)) if p.len() >= 2 => {
            Some(WorldPos::new(p[0].as_f32().unwrap_or(0.0), 0.0, p[1].as_f32().unwrap_or(0.0)))
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

#[cfg(test)]
fn test_world() -> World {
    let mut world = World::new();
    world.insert_resource(EntityIndex::default());
    world.insert_resource(PresenceLog::default());
    world
}

/// Feed `id` updates from `from` to `to` at the ~1 Hz cadence an AOI entity
/// replicates at, which is what keeps a window continuously observed across
/// the span. A single update at `to` would instead read as a silence and
/// close the window; see `PresenceLog::note_seen`.
#[cfg(test)]
fn note_updates_through(world: &mut World, id: EntityId, from: GameClock, to: GameClock) {
    let mut clock = from;
    while clock < to {
        clock = GameClock((clock.seconds() + 1.0).min(to.seconds()));
        world.resource_mut::<PresenceLog>().note_seen(id, clock);
    }
}

/// Stands in for a real `ResourceLoader`: the create and seed paths only call
/// into it for a captain param id, which stays 0 (no lookup) for fixtures with
/// no `crewModifiersCompactParams`.
#[cfg(test)]
struct NoResources;

#[cfg(test)]
impl ResourceLoader for NoResources {
    fn localized_name_from_param(&self, _param: &wowsunpack::game_params::types::Param) -> Option<String> {
        None
    }
    fn localized_name_from_id(&self, _id: &wowsunpack::data::TranslationKey) -> Option<String> {
        None
    }
    fn game_param_by_id(&self, _id: GameParamId) -> Option<wowsunpack::Rc<wowsunpack::game_params::types::Param>> {
        None
    }
    fn entity_specs(&self) -> &[wowsunpack::rpc::entitydefs::EntitySpec] {
        &[]
    }
}

#[cfg(test)]
fn handle_entity_leave_at(id: EntityId, clock: GameClock, world: &mut World) {
    handle_entity_leave(id, clock, world);
}

#[cfg(test)]
mod presence_tests {
    use super::*;

    /// A re-entering vehicle opens a second window. The gap between them is a
    /// blind spot the analysis must not span.
    #[test]
    fn leave_and_re_enter_produce_two_windows() {
        let mut world = test_world();
        let id = EntityId::from(9u32);

        open_presence(&mut world, id, GameClock(10.0));
        handle_entity_leave_at(id, GameClock(50.0), &mut world);
        open_presence(&mut world, id, GameClock(90.0));
        // Updates keep arriving after the re-entry, which is what lets the
        // second (still open) window certify anything past its own instant.
        note_updates_through(&mut world, id, GameClock(90.0), GameClock(120.0));

        let log = world.resource::<PresenceLog>();
        let windows = &log.windows[&id];
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].left, Some(GameClock(50.0)));
        assert_eq!(windows[1].left, None);

        assert!(log.continuously_observed(id, GameClock(20.0), GameClock(40.0)));
        assert!(!log.continuously_observed(id, GameClock(40.0), GameClock(95.0)));
        assert!(log.continuously_observed(id, GameClock(95.0), GameClock(120.0)));
    }

    /// A vehicle whose updates stop is not observed afterwards, even though no
    /// `EntityLeave` ever closed its window. On real replays 13% of windows are
    /// still open at end of parse, so without this an entity that went dark
    /// early would answer every later query with a false "observed".
    #[test]
    fn a_vehicle_that_goes_dark_stops_being_observed() {
        let mut world = test_world();
        let id = EntityId::from(13u32);

        open_presence(&mut world, id, GameClock(10.0));
        note_updates_through(&mut world, id, GameClock(10.0), GameClock(60.0));

        let log = world.resource::<PresenceLog>();
        assert_eq!(log.windows[&id][0].left, None, "no EntityLeave arrived");
        assert!(log.continuously_observed(id, GameClock(20.0), GameClock(60.0)));
        assert!(!log.continuously_observed(id, GameClock(20.0), GameClock(160.0)));
    }

    /// Updates that stop and later resume leave a blackout in the middle of an
    /// open window. Nothing was received across it, so no burn transition
    /// inside it could have been logged, and a range spanning it must not be
    /// certified. `last_seen` alone cannot see this: it holds only the latest
    /// update, which the resumed traffic pushes past the whole gap.
    #[test]
    fn a_silence_inside_an_open_window_ends_it() {
        let mut world = test_world();
        let id = EntityId::from(14u32);

        open_presence(&mut world, id, GameClock(100.0));
        note_updates_through(&mut world, id, GameClock(100.0), GameClock(150.0));
        // AOI re-entry with no EntityLeave and no fresh EntityCreate.
        world.resource_mut::<PresenceLog>().note_seen(id, GameClock(900.0));

        let log = world.resource::<PresenceLog>();
        assert_eq!(log.windows[&id].len(), 1, "a silence closes the window, it does not open another");
        assert_eq!(log.windows[&id][0].left, Some(GameClock(150.0)));
        assert!(!log.continuously_observed(id, GameClock(100.0), GameClock(900.0)));
        assert!(!log.continuously_observed(id, GameClock(140.0), GameClock(160.0)));
        assert!(!log.continuously_observed(id, GameClock(900.0), GameClock(905.0)));
        assert!(log.continuously_observed(id, GameClock(100.0), GameClock(150.0)));
    }

    /// A gap within the update period is ordinary jitter, not a blackout: the
    /// worst gap measured on the fixture corpus is under 7s, so a window must
    /// survive one.
    #[test]
    fn an_ordinary_update_gap_keeps_the_window_open() {
        let mut world = test_world();
        let id = EntityId::from(15u32);

        open_presence(&mut world, id, GameClock(100.0));
        world.resource_mut::<PresenceLog>().note_seen(id, GameClock(107.0));

        let log = world.resource::<PresenceLog>();
        assert_eq!(log.windows[&id][0].left, None);
        assert!(log.continuously_observed(id, GameClock(100.0), GameClock(107.0)));
    }

    /// An inverted range satisfies both containment comparisons trivially, so
    /// it would answer "observed" for a window it has nothing to do with. It
    /// is rejected in release builds too, not just behind the `debug_assert`.
    #[test]
    fn an_inverted_range_is_never_observed() {
        let mut world = test_world();
        let id = EntityId::from(16u32);

        open_presence(&mut world, id, GameClock(100.0));
        handle_entity_leave_at(id, GameClock(200.0), &mut world);

        let log = world.resource::<PresenceLog>();
        assert!(!log.continuously_observed(id, GameClock(150.0), GameClock(120.0)));
        assert!(!log.continuously_observed(id, GameClock(900.0), GameClock(0.0)));
    }

    /// An entity never seen was never observed. Absence of windows must not
    /// read as "always present".
    #[test]
    fn an_unseen_entity_is_never_continuously_observed() {
        let world = test_world();
        assert!(!world.resource::<PresenceLog>().continuously_observed(
            EntityId::from(404u32),
            GameClock(0.0),
            GameClock(1.0)
        ));
    }

    /// Two opens with no leave in between must not stack a second window;
    /// this is the early return `open_presence` promises in its doc comment.
    #[test]
    fn opening_an_already_open_window_is_a_no_op() {
        let mut world = test_world();
        let id = EntityId::from(11u32);

        open_presence(&mut world, id, GameClock(5.0));
        open_presence(&mut world, id, GameClock(6.0));

        let windows = &world.resource::<PresenceLog>().windows[&id];
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].entered, GameClock(5.0));
        assert_eq!(windows[0].left, None);
    }

    /// `continuously_observed`'s boundary comparisons are inclusive on both
    /// ends: a query touching `entered` or `left` exactly is still inside the
    /// window, and a query entirely before the first window is not.
    #[test]
    fn continuously_observed_boundaries_are_inclusive() {
        let mut world = test_world();
        let id = EntityId::from(12u32);

        open_presence(&mut world, id, GameClock(10.0));
        handle_entity_leave_at(id, GameClock(50.0), &mut world);

        let log = world.resource::<PresenceLog>();
        assert!(log.continuously_observed(id, GameClock(10.0), GameClock(20.0)));
        assert!(log.continuously_observed(id, GameClock(20.0), GameClock(50.0)));
        assert!(!log.continuously_observed(id, GameClock(0.0), GameClock(5.0)));
    }

    /// Regression test for the false premise that arena-state seeding proves
    /// observation. `seed_vehicles_from_arena_state` pre-creates a Vehicle
    /// entity (with max HP etc.) for every roster entry so the ship is
    /// queryable, including ones the recording client's AOI never detects and
    /// for which no EntityCreate ever arrives (see `gather_replay_facts`'s doc
    /// comment in `wows_replays::analyzer::battle_controller::merged`). That
    /// pre-create must not open a presence window: a ship seeded but never
    /// created has no window at all, not an always-open one.
    #[test]
    fn arena_state_seed_alone_does_not_prove_observation() {
        let mut world = World::new();
        world.insert_resource(EntityIndex::default());
        world.insert_resource(PresenceLog::default());
        world.insert_resource(PlayerIndex::default());
        world.insert_resource(MetadataPlayers::default());

        // Minimal PlayerStateData built via its derived Deserialize impl:
        // its fields are private to wows_replays, so this is the only way to
        // construct one from outside that crate. `raw`/`raw_with_names` are
        // `#[serde(skip_deserializing)]` and default to empty.
        let json = r#"{
            "username": "bot9",
            "clan": "",
            "clan_id": 0,
            "clan_color": 0,
            "db_id": 0,
            "realm": null,
            "player_id": 500,
            "entity_id": 9,
            "team_id": 1,
            "max_health": 50000,
            "is_abuser": false,
            "is_hidden": false,
            "is_bot": true,
            "human_properties": null
        }"#;
        let player: PlayerStateData = serde_json::from_str(json).expect("fixture matches PlayerStateData's shape");
        let id = EntityId::from(9u32);
        assert_eq!(player.entity_id(), id);

        seed_vehicles_from_arena_state(
            std::iter::once(&player),
            GameClock(10.0),
            &mut world,
            &NoResources,
            &GameConstants::defaults(),
            Version::default(),
        );

        // The seed path still pre-creates the ECS entity (for HP tracking
        // etc.); the assertion that matters is that no presence window opened.
        assert!(world.resource::<EntityIndex>().get(id).is_some());
        assert!(world.resource::<PresenceLog>().windows.get(&id).is_none_or(|w| w.is_empty()));
        assert!(!world.resource::<PresenceLog>().continuously_observed(id, GameClock(10.0), GameClock(10.0)));
        assert!(!world.resource::<PresenceLog>().continuously_observed(id, GameClock(0.0), GameClock(1_000_000.0)));
    }
}

#[cfg(test)]
mod burn_baseline_tests {
    use std::collections::HashMap;

    use wows_replays::packet2::Rot3;
    use wows_replays::packet2::Vec3;

    use super::*;

    fn create_test_world() -> World {
        let mut world = World::new();
        world.insert_resource(EntityIndex::default());
        world.insert_resource(PresenceLog::default());
        world.insert_resource(BurnStateLog::default());
        world.insert_resource(BurnFlagsObserved::default());
        world.insert_resource(PlayerIndex::default());
        world
    }

    fn vehicle_create(id: EntityId, burning_flags: u16) -> EntityCreatePacket<'static> {
        let mut props: HashMap<&'static str, ArgValue<'static>> = HashMap::new();
        props.insert(BURNING_FLAGS_PROPERTY, ArgValue::Uint16(burning_flags));
        vehicle_create_with_props(id, props)
    }

    fn vehicle_create_with_props(
        id: EntityId,
        props: HashMap<&'static str, ArgValue<'static>>,
    ) -> EntityCreatePacket<'static> {
        EntityCreatePacket {
            entity_id: id,
            spec_idx: 0,
            entity_type: "Vehicle",
            space_id: 0,
            vehicle_id: GameParamId::default(),
            position: Vec3 { x: 0.0, y: 0.0, z: 0.0 },
            rotation: Rot3 { roll: 0.0, pitch: 0.0, yaw: 0.0 },
            state_length: 0,
            props,
        }
    }

    fn create_vehicle(world: &mut World, id: EntityId, burning_flags: u16, clock: GameClock) {
        handle_entity_create(
            clock,
            &vehicle_create(id, burning_flags),
            world,
            &NoResources,
            &GameConstants::defaults(),
            Version::default(),
        );
    }

    /// Reconstruct a victim's burn mask at `clock` the way the downstream
    /// analysis does: the `current` of the last transition at or before it,
    /// with no transitions meaning nothing burning.
    fn burn_mask_at(log: &[BurnStateChange], victim: EntityId, clock: GameClock) -> u16 {
        log.iter().rfind(|c| c.victim == victim && c.clock <= clock).map(|c| c.current).unwrap_or(0)
    }

    /// The create-time burn state is the baseline the presence window is sound
    /// against. A ship first detected while a teammate's fire is already
    /// burning must log that fact, or a mask reconstructed at a later clock
    /// reports the section free and the analysis counts an impossible fire
    /// trial.
    #[test]
    fn first_sighting_of_a_burning_vehicle_logs_a_baseline() {
        let mut world = create_test_world();
        let id = EntityId::from(21u32);

        create_vehicle(&mut world, id, 0b0001, GameClock(200.0));
        // The vehicle keeps sending state, so its open window still covers the
        // clock this case queries.
        note_updates_through(&mut world, id, GameClock(200.0), GameClock(250.0));

        let log = world.resource::<BurnStateLog>().0.clone();
        assert_eq!(log.len(), 1, "expected one baseline transition, got {log:?}");
        assert_eq!(log[0].victim, id);
        assert_eq!(log[0].clock, GameClock(200.0));
        assert_eq!(log[0].previous, 0);
        assert_eq!(log[0].current, 0b0001);

        assert!(world.resource::<PresenceLog>().continuously_observed(id, GameClock(250.0), GameClock(250.0)));
        assert_eq!(burn_mask_at(&log, id, GameClock(250.0)), 0b0001);
    }

    /// The baseline must precede the window it makes sound: a consumer that
    /// walks transitions up to a window's `entered` clock has to see the
    /// baseline, so the push happens before `open_presence`.
    #[test]
    fn the_baseline_is_logged_at_or_before_the_window_opens() {
        let mut world = create_test_world();
        let id = EntityId::from(22u32);

        create_vehicle(&mut world, id, 0b0010, GameClock(200.0));

        let entered = world.resource::<PresenceLog>().windows[&id][0].entered;
        let log = world.resource::<BurnStateLog>().0.clone();
        assert_eq!(log.len(), 1);
        assert!(log[0].clock <= entered);
        assert_eq!(burn_mask_at(&log, id, entered), 0b0010);
    }

    /// A vehicle created with nothing alight has nothing to report; a zero
    /// baseline against a zero-by-default state is not a transition.
    #[test]
    fn creating_an_unburnt_vehicle_logs_nothing() {
        let mut world = create_test_world();
        create_vehicle(&mut world, EntityId::from(23u32), 0, GameClock(200.0));
        assert!(world.resource::<BurnStateLog>().0.is_empty());
    }

    /// Only burn bits count. A vehicle first seen with a flood running and no
    /// fire is not burning, so the create must not log.
    #[test]
    fn a_create_carrying_only_flood_bits_logs_nothing() {
        let mut world = create_test_world();
        create_vehicle(&mut world, EntityId::from(24u32), 0b0001_0000, GameClock(200.0));
        assert!(world.resource::<BurnStateLog>().0.is_empty());
        assert_eq!(0b0001_0000 & BURN_MASK, 0);
    }

    /// A build that never replicates `burningFlags` produces the same empty
    /// `BurnStateLog` as a match where nothing burned. The observation signal
    /// is what tells them apart, and it keys off the property being present,
    /// not off the mask being non-zero.
    #[test]
    fn a_create_without_burning_flags_leaves_the_field_unobserved() {
        let mut world = create_test_world();
        handle_entity_create(
            GameClock(100.0),
            &vehicle_create_with_props(EntityId::from(26u32), HashMap::new()),
            &mut world,
            &NoResources,
            &GameConstants::defaults(),
            Version::default(),
        );

        assert!(world.resource::<BurnStateLog>().0.is_empty());
        assert!(!world.resource::<BurnFlagsObserved>().0);

        create_vehicle(&mut world, EntityId::from(27u32), 0, GameClock(200.0));

        assert!(world.resource::<BurnStateLog>().0.is_empty(), "a zero mask is still not a transition");
        assert!(world.resource::<BurnFlagsObserved>().0, "a zero mask is still an observation");
    }

    /// AOI re-entry: the second create replaces `VehicleState` wholesale, so
    /// the burn bits it carries are diffed against the ones the vehicle last
    /// held rather than against zero.
    #[test]
    fn a_second_create_diffs_against_the_previous_state() {
        let mut world = create_test_world();
        let id = EntityId::from(25u32);

        create_vehicle(&mut world, id, 0b0001, GameClock(100.0));
        handle_entity_leave(id, GameClock(150.0), &mut world);
        create_vehicle(&mut world, id, 0b0101, GameClock(300.0));

        let log = world.resource::<BurnStateLog>().0.clone();
        assert_eq!(log.len(), 2, "{log:?}");
        assert_eq!(log[1].previous, 0b0001);
        assert_eq!(log[1].current, 0b0101);
        assert_eq!(log[1].clock, GameClock(300.0));
    }
}

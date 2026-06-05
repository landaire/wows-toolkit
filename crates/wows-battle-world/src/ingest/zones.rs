//! Property-update and entity-property ingestion for zones, scores, and smoke.

use bevy_ecs::world::World;
use wows_replays::analyzer::battle_controller::state::{
    CapturedBuff, LocalWeatherZone, ScoringRules as ScoringRulesState, TeamScore,
};
use wows_replays::nested_property_path::{PropertyNestLevel, UpdateAction};
use wows_replays::packet2::PropertyUpdatePacket;
use wows_replays::types::{EntityId, GameClock, GameParamId};
use wowsunpack::rpc::typedefs::ArgValue;
use wowsunpack::game_types::WorldPos;

use crate::components::{
    BuffZoneData, CapturePointData, SmokeScreenState, WeatherZoneData,
};
use crate::resources::{
    CapturePointOrder, CapturedBuffs, EntityIndex, InteractiveZoneIndex, InteractiveZoneRef,
    PendingDropParams, PlayerIndex, ScoringRules, TeamScores, WeatherZoneOrder,
};

/// Extract a dict from FixedDict or NullableFixedDict(Some).
fn as_dict<'a, 'b>(
    v: &'a ArgValue<'b>,
) -> Option<&'a std::collections::HashMap<&'b str, ArgValue<'b>>> {
    match v {
        ArgValue::FixedDict(d) => Some(d),
        ArgValue::NullableFixedDict(Some(d)) => Some(d),
        _ => None,
    }
}

fn extract_weather_position(value: &ArgValue<'_>) -> Option<WorldPos> {
    match value {
        ArgValue::Vector2((x, z)) => Some(WorldPos { x: *x, y: 0.0, z: *z }),
        ArgValue::Array(arr) if arr.len() >= 2 => {
            let x = arr[0].float_32_ref().copied().unwrap_or(0.0);
            let z = arr[1].float_32_ref().copied().unwrap_or(0.0);
            Some(WorldPos { x, y: 0.0, z })
        }
        _ => None,
    }
}

fn parse_weather_zone(value: &ArgValue<'_>) -> Option<LocalWeatherZone> {
    let dict = as_dict(value)?;

    let name = match dict.get("name") {
        Some(ArgValue::Array(arr)) => {
            let bytes: Vec<u8> = arr.iter().filter_map(|v| v.as_i64().map(|i| i as u8)).collect();
            String::from_utf8(bytes).unwrap_or_default()
        }
        Some(ArgValue::String(s)) => String::from_utf8_lossy(s).into_owned(),
        _ => String::new(),
    };

    let position = match dict.get("position") {
        Some(v) => extract_weather_position(v)?,
        _ => return None,
    };

    let radius = dict.get("radius").and_then(|v| v.float_32_ref().copied()).unwrap_or(0.0);
    let params_id = dict.get("paramsId").and_then(|v| v.as_i64()).map(GameParamId::from).unwrap_or_default();

    Some(LocalWeatherZone { name, position, radius, params_id, entity_id: None })
}

/// Seed TeamScores, ScoringRules, and LocalWeatherZones from BattleLogic EntityCreate state.
///
/// Called from entity ingestion when the BattleLogic entity is created. Mirrors
/// the BattleLogic arm of handle_entity_create_with_clock in the reference controller.
pub fn seed_battle_logic_state(packet_props: &std::collections::HashMap<&str, ArgValue<'_>>, world: &mut World) {
    let Some(state) = packet_props.get("state") else { return };
    let Some(state_dict) = as_dict(state) else { return };

    if let Some(missions) = state_dict.get("missions")
        && let Some(missions_dict) = as_dict(missions)
    {
        if let Some(ArgValue::Array(teams)) = missions_dict.get("teamsScore") {
            for (idx, entry) in teams.iter().enumerate() {
                if let Some(entry_dict) = as_dict(entry) {
                    let score = entry_dict.get("score").and_then(|v| v.as_i64()).unwrap_or(0);
                    let mut scores = world.resource_mut::<TeamScores>();
                    while scores.0.len() <= idx {
                        let next_idx = scores.0.len();
                        scores.0.push(TeamScore { team_index: next_idx, ..Default::default() });
                    }
                    scores.0[idx].score = score;
                }
            }
        }

        let team_win_score = missions_dict.get("teamWinScore").and_then(|v| v.as_i64()).unwrap_or(1000);
        let mut hold_reward: i64 = 3;
        let mut hold_period: f32 = 5.0;
        let mut hold_cp_indices: Vec<usize> = Vec::new();

        if let Some(ArgValue::Array(holds)) = missions_dict.get("hold")
            && let Some(first_hold) = holds.first()
            && let Some(hold_dict) = as_dict(first_hold)
        {
            if let Some(v) = hold_dict.get("reward").and_then(|v| v.as_i64()) {
                hold_reward = v;
            }
            if let Some(v) = hold_dict.get("period") {
                hold_period = v.float_32_ref().copied().unwrap_or(5.0);
            }
            if let Some(ArgValue::Array(indices)) = hold_dict.get("cpIndices") {
                for idx in indices {
                    if let Some(i) = idx.as_i64() {
                        hold_cp_indices.push(i as usize);
                    }
                }
            }
        }

        world.resource_mut::<ScoringRules>().0 =
            Some(ScoringRulesState { team_win_score, hold_reward, hold_period, hold_cp_indices });
    }

    if let Some(weather) = state_dict.get("weather")
        && let Some(weather_dict) = as_dict(weather)
        && let Some(ArgValue::Array(local_weather)) = weather_dict.get("localWeather")
    {
        let new_zones: Vec<LocalWeatherZone> = local_weather.iter().filter_map(parse_weather_zone).collect();

        for zone in new_zones {
            // Collect names of already-known weather zones to avoid duplicates.
            let existing_names: Vec<String> = collect_weather_zone_names(world);
            if !existing_names.iter().any(|n| n == &zone.name) {
                let ecs_entity = world.spawn(()).id();
                if let Ok(mut e) = world.get_entity_mut(ecs_entity) {
                    e.insert(crate::components::WeatherZone);
                    e.insert(WeatherZoneData(zone));
                }
                world.resource_mut::<WeatherZoneOrder>().0.push(ecs_entity);
            }
        }
    }
}

fn collect_weather_zone_names(world: &mut World) -> Vec<String> {
    let mut q = world.query::<&WeatherZoneData>();
    q.iter(world).map(|d| d.0.name.clone()).collect()
}

/// Dispatch a PropertyUpdate packet into ECS zone state.
///
/// Mirrors the branches in BattleController::handle_property_update (state.missions.*,
/// state.drop.*, state.weather.*) and the smoke-screen point mutations that arrive
/// on entity-specific PropertyUpdates.
pub fn handle_property_update(update: &PropertyUpdatePacket<'_>, clock: GameClock, world: &mut World) {
    let levels = &update.update_cmd.levels;
    let action = &update.update_cmd.action;

    // state -> missions -> teamsScore -> [N] -> SetKey{score}
    if update.property == "state"
        && levels.len() == 3
        && let PropertyNestLevel::DictKey("missions") = &levels[0]
        && let PropertyNestLevel::DictKey("teamsScore") = &levels[1]
        && let PropertyNestLevel::ArrayIndex(team_idx) = &levels[2]
        && let UpdateAction::SetKey { key: "score", value } = action
        && let Some(score) = value.as_i64()
    {
        let team_idx = *team_idx;
        let mut scores = world.resource_mut::<TeamScores>();
        while scores.0.len() <= team_idx {
            let next_idx = scores.0.len();
            scores.0.push(TeamScore { team_index: next_idx, ..Default::default() });
        }
        scores.0[team_idx].score = score;
    }

    // state -> drop -> data -> SetRange{[{zoneId, paramsId, ...}]}
    if update.property == "state"
        && levels.len() == 2
        && let PropertyNestLevel::DictKey("drop") = &levels[0]
        && let PropertyNestLevel::DictKey("data") = &levels[1]
        && let UpdateAction::SetRange { values, .. } = action
    {
        for value in values {
            if let ArgValue::FixedDict(dict) = value {
                let zone_id =
                    dict.get("zoneId").and_then(|v| v.as_i64()).map(|v| EntityId::from(v as i32));
                let params_id =
                    dict.get("paramsId").and_then(|v| v.as_i64()).map(GameParamId::from);

                if let (Some(zone_id), Some(params_id)) = (zone_id, params_id) {
                    // Store for pre-arrival case (entity may not exist yet).
                    world.resource_mut::<PendingDropParams>().0.insert(zone_id, params_id);
                    // Also update the component directly if the entity already exists.
                    if let Some(ecs_entity) = world.resource::<EntityIndex>().get(zone_id)
                        && let Ok(mut er) = world.get_entity_mut(ecs_entity)
                        && let Some(mut bz) = er.get_mut::<BuffZoneData>()
                    {
                        bz.0.drop_params_id = Some(params_id);
                    }
                }
            }
        }
    }

    // state -> drop -> picked -> SetRange{[{paramsId, owners: [...]}]}
    if update.property == "state"
        && levels.len() == 2
        && let PropertyNestLevel::DictKey("drop") = &levels[0]
        && let PropertyNestLevel::DictKey("picked") = &levels[1]
        && let UpdateAction::SetRange { values, .. } = action
    {
        for value in values {
            if let ArgValue::FixedDict(dict) = value {
                let params_id =
                    dict.get("paramsId").and_then(|v| v.as_i64()).map(GameParamId::from);
                let owners: Option<Vec<EntityId>> = dict.get("owners").and_then(|v| {
                    if let ArgValue::Array(arr) = v {
                        Some(
                            arr.iter()
                                .filter_map(|o| o.as_i64().map(|id| EntityId::from(id as i32)))
                                .collect(),
                        )
                    } else {
                        None
                    }
                });

                if let (Some(params_id), Some(owners)) = (params_id, owners) {
                    let team_id = owners.first().and_then(|owner_id| {
                        world
                            .resource::<PlayerIndex>()
                            .0
                            .values()
                            .find(|p| p.initial_state().entity_id() == *owner_id)
                            .map(|p| p.initial_state().team_id() as i64)
                    });
                    if let Some(team_id) = team_id {
                        world.resource_mut::<CapturedBuffs>().0.push(CapturedBuff {
                            params_id,
                            team_id,
                            clock,
                        });
                    }
                }
            }
        }
    }

    // state -> weather -> localWeather -> SetRange/SetElement/RemoveRange
    if update.property == "state"
        && levels.len() == 2
        && let PropertyNestLevel::DictKey("weather") = &levels[0]
        && let PropertyNestLevel::DictKey("localWeather") = &levels[1]
    {
        apply_weather_zone_array_update(action, world);
    }

    // state -> weather -> localWeather -> [N] -> SetKey{position|radius|name|paramsId}
    if update.property == "state"
        && levels.len() == 3
        && let PropertyNestLevel::DictKey("weather") = &levels[0]
        && let PropertyNestLevel::DictKey("localWeather") = &levels[1]
        && let PropertyNestLevel::ArrayIndex(idx) = &levels[2]
        && let UpdateAction::SetKey { key, value } = action
    {
        apply_weather_zone_field_update(*idx, key, value, world);
    }

    // state -> controlPoints -> [N] -> SetKey{...} (legacy, pre-InteractiveZone)
    if update.property == "state"
        && let [PropertyNestLevel::DictKey("controlPoints"), PropertyNestLevel::ArrayIndex(cp_idx)] =
            levels.as_slice()
        && let UpdateAction::SetKey { key, value } = action
    {
        let cp_idx = *cp_idx;
        apply_legacy_cp_field_update(cp_idx, key, value, world);
    }

    // Smoke screen points (entity-specific PropertyUpdate, property == "points")
    if update.property == "points" {
        if let Some(ecs_entity) = world.resource::<EntityIndex>().get(update.entity_id)
            && world.get_entity(ecs_entity).ok().map(|e| e.contains::<crate::components::SmokeScreen>()).unwrap_or(false)
        {
            apply_smoke_points_update(ecs_entity, action, world);
        }
    }

    // InteractiveZone componentsState.captureLogic updates
    if update.property == "componentsState"
        && let Some(zone_ref) = world.resource::<InteractiveZoneIndex>().0.get(&update.entity_id).copied()
        && let InteractiveZoneRef::CapturePoint(cp_idx) = zone_ref
        && matches!(update.update_cmd.levels.first(), Some(PropertyNestLevel::DictKey("captureLogic")))
        && let UpdateAction::SetKey { key, value } = action
    {
        apply_cp_components_state_update(cp_idx, key, value, world);
    }
}

fn apply_weather_zone_array_update(action: &UpdateAction<'_>, world: &mut World) {
    match action {
        UpdateAction::SetRange { start, stop: _, values } => {
            let start = *start;
            let needed = start + values.len();
            let current_len = world.resource::<WeatherZoneOrder>().0.len();
            for _ in current_len..needed {
                let ecs_entity = world.spawn(()).id();
                if let Ok(mut e) = world.get_entity_mut(ecs_entity) {
                    e.insert(crate::components::WeatherZone);
                    e.insert(WeatherZoneData(LocalWeatherZone {
                        name: String::new(),
                        position: WorldPos::default(),
                        radius: 0.0,
                        params_id: GameParamId::default(),
                        entity_id: None,
                    }));
                }
                world.resource_mut::<WeatherZoneOrder>().0.push(ecs_entity);
            }
            for (i, value) in values.iter().enumerate() {
                if let Some(mut zone) = parse_weather_zone(value) {
                    let ecs_entity = world.resource::<WeatherZoneOrder>().0[start + i];
                    if let Ok(er) = world.get_entity(ecs_entity) {
                        if let Some(existing) = er.get::<WeatherZoneData>() {
                            zone.entity_id = existing.0.entity_id;
                        }
                    }
                    if let Ok(mut er) = world.get_entity_mut(ecs_entity) {
                        if let Some(mut data) = er.get_mut::<WeatherZoneData>() {
                            data.0 = zone;
                        }
                    }
                }
            }
        }
        UpdateAction::SetElement { index, value } => {
            let index = *index;
            let current_len = world.resource::<WeatherZoneOrder>().0.len();
            for _ in current_len..=index {
                let ecs_entity = world.spawn(()).id();
                if let Ok(mut e) = world.get_entity_mut(ecs_entity) {
                    e.insert(crate::components::WeatherZone);
                    e.insert(WeatherZoneData(LocalWeatherZone {
                        name: String::new(),
                        position: WorldPos::default(),
                        radius: 0.0,
                        params_id: GameParamId::default(),
                        entity_id: None,
                    }));
                }
                world.resource_mut::<WeatherZoneOrder>().0.push(ecs_entity);
            }
            if let Some(mut zone) = parse_weather_zone(value) {
                let ecs_entity = world.resource::<WeatherZoneOrder>().0[index];
                if let Ok(er) = world.get_entity(ecs_entity) {
                    if let Some(existing) = er.get::<WeatherZoneData>() {
                        zone.entity_id = existing.0.entity_id;
                    }
                }
                if let Ok(mut er) = world.get_entity_mut(ecs_entity) {
                    if let Some(mut data) = er.get_mut::<WeatherZoneData>() {
                        data.0 = zone;
                    }
                }
            }
        }
        UpdateAction::RemoveRange { start, stop } => {
            let start = *start;
            let order_len = world.resource::<WeatherZoneOrder>().0.len();
            let end = (*stop).min(order_len);
            if start < end {
                let to_despawn: Vec<_> =
                    world.resource::<WeatherZoneOrder>().0[start..end].to_vec();
                for ecs_entity in &to_despawn {
                    if world.get_entity(*ecs_entity).is_ok() {
                        world.despawn(*ecs_entity);
                    }
                }
                world.resource_mut::<WeatherZoneOrder>().0.drain(start..end);
            }
        }
        _ => {}
    }
}

fn apply_weather_zone_field_update(idx: usize, key: &str, value: &ArgValue<'_>, world: &mut World) {
    let Some(ecs_entity) = world.resource::<WeatherZoneOrder>().0.get(idx).copied() else { return };
    let Ok(mut er) = world.get_entity_mut(ecs_entity) else { return };
    let Some(mut data) = er.get_mut::<WeatherZoneData>() else { return };
    match key {
        "position" => {
            if let Some(pos) = extract_weather_position(value) {
                data.0.position = pos;
            }
        }
        "radius" => {
            if let Some(r) = value.float_32_ref().copied() {
                data.0.radius = r;
            }
        }
        "name" => {
            data.0.name = match value {
                ArgValue::Array(arr) => {
                    let bytes: Vec<u8> =
                        arr.iter().filter_map(|v| v.as_i64().map(|i| i as u8)).collect();
                    String::from_utf8(bytes).unwrap_or_default()
                }
                ArgValue::String(s) => String::from_utf8_lossy(s).into_owned(),
                _ => String::new(),
            };
        }
        "paramsId" => {
            if let Some(id) = value.as_i64() {
                data.0.params_id = GameParamId::from(id);
            }
        }
        _ => {}
    }
}

fn apply_legacy_cp_field_update(cp_idx: usize, key: &str, value: &ArgValue<'_>, world: &mut World) {
    let order = world.resource::<CapturePointOrder>().0.clone();
    let Some(&ecs_entity) = order.get(cp_idx) else { return };
    let Ok(mut er) = world.get_entity_mut(ecs_entity) else { return };
    let Some(mut data) = er.get_mut::<CapturePointData>() else { return };
    let cp = &mut data.0;
    match key {
        "teamId" => {
            if let Some(v) = value.as_i64() {
                cp.team_id = v;
            }
        }
        "invaderTeam" => {
            if let Some(v) = value.as_i64() {
                cp.invader_team = v;
            }
        }
        "hasInvaders" => {
            if let Some(v) = value.as_i64() {
                cp.has_invaders = v != 0;
            }
        }
        "bothInside" => {
            if let Some(v) = value.as_i64() {
                cp.both_inside = v != 0;
            }
        }
        "isEnabled" => {
            if let Some(v) = value.as_i64() {
                cp.is_enabled = v != 0;
            }
        }
        "progress" => match value {
            ArgValue::Array(p) if p.len() >= 2 => {
                cp.progress =
                    (p[0].as_f32().unwrap_or(0.0) as f64, p[1].as_f32().unwrap_or(0.0) as f64);
            }
            _ => {
                if let Some(f) = value.as_f32() {
                    cp.progress.0 = f as f64;
                }
            }
        },
        _ => {}
    }
}

fn apply_cp_components_state_update(cp_idx: usize, key: &str, value: &ArgValue<'_>, world: &mut World) {
    let order = world.resource::<CapturePointOrder>().0.clone();
    let Some(&ecs_entity) = order.get(cp_idx) else { return };
    let Ok(mut er) = world.get_entity_mut(ecs_entity) else { return };
    let Some(mut data) = er.get_mut::<CapturePointData>() else { return };
    let cp = &mut data.0;
    match key {
        "hasInvaders" => {
            if let Some(v) = value.as_i64() {
                cp.has_invaders = v != 0;
            }
        }
        "invaderTeam" => {
            if let Some(v) = value.as_i64() {
                cp.invader_team = v;
            }
        }
        "progress" => {
            if let Some(f) = value.float_32_ref() {
                cp.progress = (*f as f64, 0.0);
            }
        }
        "bothInside" => {
            if let Some(v) = value.as_i64() {
                cp.both_inside = v != 0;
            }
        }
        "teamId" | "invaderTeamId" => {
            if let Some(v) = value.as_i64() {
                cp.invader_team = v;
            }
        }
        "isEnabled" => {
            if let Some(v) = value.as_i64() {
                cp.is_enabled = v != 0;
            }
        }
        _ => {}
    }
}

fn apply_smoke_points_update(
    ecs_entity: bevy_ecs::entity::Entity,
    action: &UpdateAction<'_>,
    world: &mut World,
) {
    let Ok(mut er) = world.get_entity_mut(ecs_entity) else { return };
    let Some(mut state) = er.get_mut::<SmokeScreenState>() else { return };
    match action {
        UpdateAction::SetRange { start, values, .. } => {
            let start = *start;
            while state.points.len() < start + values.len() {
                state.points.push(WorldPos::default());
            }
            for (i, v) in values.iter().enumerate() {
                let pos = match v {
                    ArgValue::Vector3((x, y, z)) => Some(WorldPos { x: *x, y: *y, z: *z }),
                    ArgValue::Vector2((x, z)) => Some(WorldPos { x: *x, y: 0.0, z: *z }),
                    ArgValue::Array(arr) if arr.len() >= 2 => {
                        match (arr[0].float_32_ref(), arr[1].float_32_ref()) {
                            (Some(x), Some(z)) => Some(WorldPos { x: *x, y: 0.0, z: *z }),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(pos) = pos {
                    state.points[start + i] = pos;
                }
            }
        }
        UpdateAction::RemoveRange { start, stop } => {
            let end = (*stop).min(state.points.len());
            if *start < end {
                state.points.drain(*start..end);
            }
        }
        _ => {}
    }
}

/// Handle an EntityProperty for interactive zones: teamId on a CapturePoint entity.
///
/// Mirrors the EntityProperty teamId arm in BattleController's packet loop.
pub fn handle_entity_property_zone(entity_id: EntityId, property: &str, value: &ArgValue<'_>, world: &mut World) {
    if property == "teamId"
        && let Some(v) = value.as_i64()
        && let Some(zone_ref) = world.resource::<InteractiveZoneIndex>().0.get(&entity_id).copied()
        && let InteractiveZoneRef::CapturePoint(cp_idx) = zone_ref
    {
        let order = world.resource::<CapturePointOrder>().0.clone();
        if let Some(&ecs_entity) = order.get(cp_idx)
            && let Ok(mut er) = world.get_entity_mut(ecs_entity)
            && let Some(mut data) = er.get_mut::<CapturePointData>()
        {
            data.0.team_id = v;
        }
    }
}

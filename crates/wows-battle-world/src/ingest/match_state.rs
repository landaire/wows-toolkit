//! Ingestion handlers for match-level state.

use std::collections::HashMap;

use bevy_ecs::world::World;
use wows_replays::analyzer::battle_controller::ConnectionChangeInfo;
use wows_replays::analyzer::battle_controller::ConnectionChangeKind;
use wows_replays::analyzer::decoder::FinishType;
use wows_replays::analyzer::decoder::PlayerStateData;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::ArenaId;
use wows_replays::types::GameClock;
use wows_replays::types::TeamId;
use wowsunpack::data::Version;
use wowsunpack::game_types::BattleStage;
use wowsunpack::rpc::typedefs::ArgValue;

use crate::resources::EntityIndex;
use crate::resources::KillLog;
use crate::resources::MatchState;
use crate::resources::PlayerIndex;
use crate::units::MatchWinner;
use crate::units::SecondsRemaining;

pub fn handle_arena_id(arena_id: i64, world: &mut World) {
    let mut ms = world.resource_mut::<MatchState>();
    if ms.arena_id.is_none() {
        ms.arena_id = Some(ArenaId::from(arena_id));
    }
}

pub fn handle_entity_property_match(
    property: &str,
    value: &ArgValue<'_>,
    clock: GameClock,
    world: &mut World,
    constants: &GameConstants,
    version: Version,
) {
    match property {
        "timeLeft" => {
            if let Some(v) = value.as_i64() {
                world.resource_mut::<MatchState>().time_left = Some(SecondsRemaining(v));
            }
        }
        "battleStage" => {
            if let Some(v) = value.as_i64() {
                let resolved = constants.common().battle_stage(v as i32).cloned();
                let mut ms = world.resource_mut::<MatchState>();
                if ms.battle_start_clock.is_none() && matches!(resolved, Some(BattleStage::Waiting)) {
                    ms.battle_start_clock = Some(clock);
                }
                ms.battle_stage = resolved;
            }
        }
        "battleResult" => {
            if let Some(dict) = as_dict(value) {
                let winner = dict.get("winnerTeamId").and_then(|v: &ArgValue<'_>| v.as_i64());
                let reason = dict.get("finishReason").and_then(|v: &ArgValue<'_>| v.as_i64());
                {
                    let mut ms = world.resource_mut::<MatchState>();
                    if let Some(winner) = winner {
                        if winner >= -1 {
                            let mw = if winner >= 0 {
                                MatchWinner::Team(TeamId::from(winner))
                            } else {
                                MatchWinner::Draw
                            };
                            ms.winning_team = Some(mw);
                            if ms.battle_result_clock.is_none() {
                                ms.battle_result_clock = Some(clock);
                            }
                        }
                    }
                    if let Some(reason) = reason {
                        if reason > 0 {
                            ms.finish_type = FinishType::from_id(reason as i32, constants.battle(), version);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

pub fn handle_battle_end(
    winning_team: Option<i8>,
    finish_type: Option<wows_replays::analyzer::decoder::Recognized<FinishType>>,
    clock: GameClock,
    world: &mut World,
) {
    let mut ms = world.resource_mut::<MatchState>();
    ms.match_finished = true;
    ms.battle_end_clock = Some(clock);
    if winning_team.is_some() {
        ms.winning_team = winning_team.map(|t| {
            if t >= 0 {
                MatchWinner::Team(TeamId::from(t as i64))
            } else {
                MatchWinner::Draw
            }
        });
    }
    if finish_type.is_some() {
        ms.finish_type = finish_type;
    }
}

pub fn handle_battle_results(json: &str, world: &mut World) {
    world.resource_mut::<MatchState>().battle_results = Some(json.to_owned());
}

pub fn handle_game_room_state_changed(
    player_states: &[HashMap<&'static str, pickled::Value>],
    clock: GameClock,
    world: &mut World,
) {
    for player_state in player_states {
        let Some(meta_ship_id) = player_state.get(PlayerStateData::KEY_ID) else {
            continue;
        };
        let meta_ship_id = *meta_ship_id.i64_ref().expect("player_id is not an i64");

        let player = {
            let index = world.resource::<PlayerIndex>();
            index
                .0
                .values()
                .find(|p| {
                    p.initial_state().meta_ship_id()
                        == wows_replays::types::AccountId::from(meta_ship_id)
                })
                .cloned()
        };
        let Some(player) = player else {
            continue;
        };

        player.end_state_mut().update_from_dict(player_state);

        let player_entity_id = player.initial_state().entity_id();
        // Mirror controller.rs ~3760-3769: only report had_death_event when the
        // vehicle entity exists; unwrap_or(false) when it has never spawned.
        let player_has_died = world
            .resource::<EntityIndex>()
            .get(player_entity_id)
            .is_some_and(|_| {
                world
                    .resource::<KillLog>()
                    .0
                    .iter()
                    .any(|kill| kill.victim == player_entity_id)
            });

        let connection_event_kind = if player.end_state().is_connected() {
            ConnectionChangeKind::Connected
        } else {
            ConnectionChangeKind::Disconnected
        };

        let should_push = (player.connection_change_info().is_empty()
            && connection_event_kind != ConnectionChangeKind::Disconnected)
            || player
                .connection_change_info()
                .last()
                .map(|info| info.event_kind() != connection_event_kind)
                .unwrap_or_default();

        if should_push {
            player.connection_change_info_mut().push(ConnectionChangeInfo::new(
                clock.to_duration(),
                connection_event_kind,
                player_has_died,
            ));
        }
    }
}

fn as_dict<'a, 'b>(value: &'a ArgValue<'b>) -> Option<&'a HashMap<&'b str, ArgValue<'b>>> {
    match value {
        ArgValue::FixedDict(d) => Some(d),
        ArgValue::NullableFixedDict(Some(d)) => Some(d),
        _ => None,
    }
}

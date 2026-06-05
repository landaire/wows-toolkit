//! Per-vehicle fact gathering shared by replay renderers.
//!
//! These utilities walk a replay's raw packet stream to extract per-vehicle
//! initial-state facts (max HP, ship config, crew) and the match arena id,
//! without constructing any battle controller. Renderers use them to display
//! ship facts for every player from the first frame of a session.

use std::collections::HashMap;

use wowsunpack::data::Version;
use wowsunpack::data::ship_config::ShipConfig;
use wowsunpack::game_types::GameParamId;
use wowsunpack::rpc::entitydefs::EntitySpec;

use crate::ReplayFile;
use crate::analyzer::decoder::DecodedPacketPayload;
use crate::analyzer::decoder::PacketDecoder;
use crate::game_constants::GameConstants;
use crate::packet2::PacketType;
use crate::packet2::Parser;
use crate::types::ArenaId;
use crate::types::EntityId;

use super::controller::CrewModifiersCompactParams;
use super::controller::EntityType;
use super::controller::VehicleProps;

/// Walk a replay's stream to find the first `onArenaStateReceived` and
/// extract its arena id. Lets callers reject merge candidates that
/// don't belong to the same match *before* a full re-parse is kicked off.
pub fn scan_arena_id(specs: &[EntitySpec], version: Version, replay: &ReplayFile) -> Option<ArenaId> {
    let mut parser = Parser::with_version(specs, version);
    let mut remaining = &replay.packet_data[..];
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).ok()?;
        if let PacketType::EntityMethod(em) = &packet.payload
            && em.method == "onArenaStateReceived"
            && let Some(wowsunpack::rpc::typedefs::ArgValue::Int64(v)) = em.args.first()
        {
            return Some(ArenaId::from(*v));
        }
    }
    None
}

/// Per-vehicle facts collected from initial EntityCreate packets, regardless
/// of which perspective spotted the entity first. Lets renderers display max
/// HP and consumable inventories for every player from the first frame of a
/// merged session, even before the primary perspective has spotted them.
#[derive(Debug, Clone)]
pub struct VehicleFacts {
    pub vehicle_id: GameParamId,
    pub max_health: f32,
    pub ship_config: ShipConfig,
    pub crew: CrewModifiersCompactParams,
}

/// Gather per-vehicle facts directly from every replay's packet stream.
///
/// Walks each replay's packets raw and extracts initial-state facts from
/// every vehicle-create packet (EntityCreate, CellPlayerCreate,
/// BasePlayerCreate). Each packet's `props` map is parsed via
/// `VehicleProps::from_create_props`, so any of `shipConfig`, `maxHealth`,
/// and `crewModifiersCompactParams` lands regardless of which packet type
/// carried it for a given perspective.
///
/// Also folds in `maxHealth` from any later `EntityProperty(maxHealth)`
/// update, since some ships only broadcast it on first damage.
///
/// Also seeds `max_health` + `vehicle_id` from `onArenaStateReceived` for
/// ships the active perspective never detects (the corresponding
/// `EntityCreate` never arrives but `onArenaStateReceived` lists every
/// participant with their max HP and ship params id).
pub fn gather_replay_facts(
    constants: &GameConstants,
    version: Version,
    specs: &[EntitySpec],
    replays: &[&ReplayFile],
) -> HashMap<EntityId, VehicleFacts> {
    let mut combined: HashMap<EntityId, VehicleFacts> = HashMap::new();

    for (replay_idx, replay) in replays.iter().enumerate() {
        let before = combined.len();
        let mut parser = Parser::with_version(specs, version);
        let decoder = PacketDecoder::builder()
            .version(version)
            .battle_constants(constants.battle())
            .common_constants(constants.common())
            .ships_constants(constants.ships())
            .build();
        let mut remaining = &replay.packet_data[..];
        while !remaining.is_empty() {
            let Ok(packet) = parser.parse_packet(&mut remaining) else { break };
            match &packet.payload {
                PacketType::EntityCreate(ec) => {
                    if !matches!(ec.entity_type.parse::<EntityType>(), Ok(EntityType::Vehicle)) {
                        continue;
                    }
                    fold_props_into(&mut combined, ec.entity_id, &ec.props, version, constants);
                }
                PacketType::CellPlayerCreate(cell) => {
                    if !matches!(cell.entity_type.parse::<EntityType>(), Ok(EntityType::Vehicle)) {
                        continue;
                    }
                    fold_props_into(&mut combined, cell.entity_id, &cell.props, version, constants);
                }
                PacketType::BasePlayerCreate(base) => {
                    if !matches!(base.entity_type.parse::<EntityType>(), Ok(EntityType::Vehicle)) {
                        continue;
                    }
                    fold_props_into(&mut combined, base.entity_id, &base.props, version, constants);
                }
                PacketType::EntityMethod(em) if em.method == "onArenaStateReceived" => {
                    let decoded = decoder.decode(&packet);
                    if let DecodedPacketPayload::OnArenaStateReceived { player_states, bot_states, .. } =
                        decoded.payload
                    {
                        for player in player_states.iter().chain(bot_states.iter()) {
                            let entity_id = player.entity_id();
                            let entry = combined.entry(entity_id).or_insert_with(|| VehicleFacts {
                                vehicle_id: GameParamId::default(),
                                max_health: 0.0,
                                ship_config: ShipConfig::default(),
                                crew: CrewModifiersCompactParams::default(),
                            });
                            if entry.max_health == 0.0 && player.max_health() > 0 {
                                entry.max_health = player.max_health() as f32;
                            }
                            if entry.vehicle_id.raw() == 0
                                && let Some(spid) = player.ship_params_id()
                                && spid.raw() != 0
                            {
                                entry.vehicle_id = spid;
                            }
                        }
                    }
                }
                PacketType::EntityProperty(ep) => {
                    // Fold any single-property update that carries one of the
                    // fields we care about. shipConfig (and sometimes maxHealth)
                    // arrives this way after the initial create, especially for
                    // own-team ships once the player customizes their loadout.
                    let mut single = std::collections::HashMap::new();
                    single.insert(ep.property, ep.value.clone());
                    let parsed = VehicleProps::from_create_props(&single, version, constants);

                    let entry = combined.entry(ep.entity_id).or_insert_with(|| VehicleFacts {
                        vehicle_id: GameParamId::default(),
                        max_health: 0.0,
                        ship_config: ShipConfig::default(),
                        crew: CrewModifiersCompactParams::default(),
                    });
                    if entry.max_health == 0.0 && parsed.max_health() > 0.0 {
                        entry.max_health = parsed.max_health();
                    }
                    if entry.ship_config.abilities().is_empty() && !parsed.ship_config().abilities().is_empty() {
                        entry.ship_config = parsed.ship_config().clone();
                        if entry.vehicle_id.raw() == 0 && parsed.ship_config().ship_params_id().raw() != 0 {
                            entry.vehicle_id = parsed.ship_config().ship_params_id();
                        }
                    }
                    if entry.crew.params_id().raw() == 0
                        && parsed.crew_modifiers_compact_params().params_id().raw() != 0
                    {
                        entry.crew = parsed.crew_modifiers_compact_params().clone();
                    }
                }
                _ => {}
            }
        }
        let with_ship_config = combined.values().filter(|f| !f.ship_config.abilities().is_empty()).count();
        let with_max_health = combined.values().filter(|f| f.max_health > 0.0).count();
        tracing::info!(
            replay_idx,
            player = %replay.meta.playerName,
            total_facts = combined.len(),
            new_facts = combined.len() - before,
            with_ship_config,
            with_max_health,
            "gather_replay_facts processed replay"
        );
    }

    combined
}

pub fn fold_props_into(
    out: &mut HashMap<EntityId, VehicleFacts>,
    entity_id: EntityId,
    props: &std::collections::HashMap<&str, wowsunpack::rpc::typedefs::ArgValue<'_>>,
    version: Version,
    constants: &GameConstants,
) {
    // The packet-level `vehicle_id` field on EntityCreate / CellPlayerCreate
    // is misnamed in the wows_replays types: it's some BigWorld internal
    // (likely the avatar's entity_id), not a GameParams ID. Multiple ships
    // share the same value within a single replay. The only authoritative
    // source for the ship class param ID is the parsed shipConfig blob.
    let parsed = VehicleProps::from_create_props(props, version, constants);
    let parsed_vehicle_id = parsed.ship_config().ship_params_id();
    let parsed_max_health = parsed.max_health();
    let parsed_ship_config = parsed.ship_config().clone();
    let parsed_crew = parsed.crew_modifiers_compact_params().clone();

    let entry = out.entry(entity_id).or_insert_with(|| VehicleFacts {
        vehicle_id: GameParamId::default(),
        max_health: 0.0,
        ship_config: ShipConfig::default(),
        crew: CrewModifiersCompactParams::default(),
    });

    if entry.vehicle_id.raw() == 0 && parsed_vehicle_id.raw() != 0 {
        entry.vehicle_id = parsed_vehicle_id;
    }
    if entry.max_health == 0.0 && parsed_max_health > 0.0 {
        entry.max_health = parsed_max_health;
    }
    if entry.ship_config.abilities().is_empty() && !parsed_ship_config.abilities().is_empty() {
        entry.ship_config = parsed_ship_config;
    }
    if entry.crew.params_id().raw() == 0 && parsed_crew.params_id().raw() != 0 {
        entry.crew = parsed_crew;
    }
}

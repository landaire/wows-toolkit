//! Packet ingestion: translates decoded packet payloads into ECS state changes.

pub mod chat;
pub mod combat;
pub mod entities;
pub mod positions;
pub mod vehicles;

use bevy_ecs::world::World;
use wows_replays::analyzer::decoder::DecodedPacketPayload;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_types::WorldPos;

use crate::ids::IngestOptions;

/// Dispatch one decoded packet into the ECS world.
///
/// Every variant has an explicit arm so a future-added variant is a compile
/// error rather than silently dropped.
pub fn dispatch<G: ResourceLoader>(
    payload: DecodedPacketPayload<'_, '_, '_>,
    world: &mut World,
    resources: &G,
    constants: &GameConstants,
    version: Version,
    options: &IngestOptions,
    clock: GameClock,
) {
    match payload {
        DecodedPacketPayload::Chat { entity_id, sender_id, audience, message, extra_data } => {
            chat::handle_chat_message(
                entity_id,
                sender_id,
                audience,
                message,
                extra_data,
                clock,
                world,
                resources,
                version,
            );
        }
        DecodedPacketPayload::VoiceLine { .. } => {}
        DecodedPacketPayload::Ribbon(ribbon) => {
            combat::handle_ribbon(ribbon, world);
        }
        DecodedPacketPayload::Position(pos) => {
            positions::handle_position(&pos, world, clock);
        }
        DecodedPacketPayload::PlayerOrientation(orient) => {
            positions::handle_player_orientation(&orient, world, clock);
        }
        DecodedPacketPayload::DamageStat(ref entries) => {
            combat::handle_damage_stat(entries, world);
        }
        DecodedPacketPayload::ShipDestroyed { killer, victim, cause } => {
            combat::handle_ship_destroyed(killer, victim, cause, clock, world);
        }
        DecodedPacketPayload::EntityMethod(_) => {}
        DecodedPacketPayload::EntityProperty(prop) => {
            vehicles::handle_vehicle_property(
                prop.entity_id,
                prop.property,
                &prop.value,
                world,
                version,
                constants,
            );
        }
        DecodedPacketPayload::BasePlayerCreate(base) => {
            vehicles::apply_player_create_props(base.entity_id, &base.props, world, version, constants);
        }
        DecodedPacketPayload::CellPlayerCreate(cell) => {
            vehicles::apply_player_create_props(cell.entity_id, &cell.props, world, version, constants);
        }
        DecodedPacketPayload::EntityEnter(_) => {}
        DecodedPacketPayload::EntityLeave(leave) => {
            entities::handle_entity_leave(leave.entity_id, world);
        }
        DecodedPacketPayload::EntityCreate(entity_create) => {
            entities::handle_entity_create(clock, entity_create, world, resources, constants, version);
        }
        DecodedPacketPayload::OnArenaStateReceived {
            arena_id: _,
            team_build_type_id: _,
            pre_battles_info: _,
            player_states: players,
            bot_states: bots,
        } => {
            entities::seed_vehicles_from_arena_state(
                players.iter().chain(bots.iter()),
                world,
                resources,
                constants,
                version,
            );
        }
        DecodedPacketPayload::OnGameRoomStateChanged { .. } => {}
        DecodedPacketPayload::NewPlayerSpawnedInBattle {
            player_states: players,
            bot_states: bots,
        } => {
            entities::seed_spawned_players(
                players.iter().chain(bots.iter()),
                world,
                resources,
                constants,
                version,
            );
        }
        DecodedPacketPayload::CheckPing(_) => {}
        DecodedPacketPayload::DamageReceived { victim, ref aggressors } => {
            combat::handle_damage_received(victim, aggressors, clock, world);
        }
        DecodedPacketPayload::MinimapUpdate { updates, .. } => {
            positions::handle_minimap_updates(&updates, world, clock, options.source_team);
        }
        DecodedPacketPayload::PropertyUpdate(_) => {}
        DecodedPacketPayload::BattleEnd { .. } => {}
        DecodedPacketPayload::Consumable { .. } => {}
        DecodedPacketPayload::CruiseState { .. } => {}
        DecodedPacketPayload::Map(_) => {}
        DecodedPacketPayload::Version(_) => {}
        DecodedPacketPayload::Camera(_) => {}
        DecodedPacketPayload::CameraMode(_) => {}
        DecodedPacketPayload::CameraFreeLook(_) => {}
        DecodedPacketPayload::ArtilleryShots { .. } => {}
        DecodedPacketPayload::TorpedoesReceived { .. } => {}
        DecodedPacketPayload::TorpedoDirection { .. } => {}
        DecodedPacketPayload::ShotKills { .. } => {}
        DecodedPacketPayload::GunSync { entity_id, weapon_type, gun_id, yaw, .. } => {
            vehicles::handle_gun_sync(entity_id, weapon_type, gun_id, yaw, world);
        }
        DecodedPacketPayload::PlaneAdded { .. } => {}
        DecodedPacketPayload::WardAdded { .. } => {}
        DecodedPacketPayload::WardRemoved { .. } => {}
        DecodedPacketPayload::PlaneRemoved { .. } => {}
        DecodedPacketPayload::PlanePosition { .. } => {}
        DecodedPacketPayload::SetAmmoForWeapon { entity_id, weapon_type, ammo_param_id, .. } => {
            vehicles::handle_set_ammo_for_weapon(entity_id, weapon_type, ammo_param_id, world);
        }
        DecodedPacketPayload::EntityControl(_) => {}
        DecodedPacketPayload::NonVolatilePosition(sd) => {
            let pos = WorldPos { x: sd.position.x, y: sd.position.y, z: sd.position.z };
            positions::handle_non_volatile_position(sd.entity_id, pos, world);
        }
        DecodedPacketPayload::PlayerNetStats(_) => {}
        DecodedPacketPayload::ServerTimestamp(_) => {}
        DecodedPacketPayload::OwnShip(_) => {}
        DecodedPacketPayload::SetWeaponLock(_) => {}
        DecodedPacketPayload::ServerTick(_) => {}
        DecodedPacketPayload::SubController(_) => {}
        DecodedPacketPayload::ShotTracking(_) => {}
        DecodedPacketPayload::GunMarker(_) => {}
        DecodedPacketPayload::SyncShipCracks { .. } => {}
        DecodedPacketPayload::InitFlag(_) => {}
        DecodedPacketPayload::InitMarker => {}
        DecodedPacketPayload::Unknown(_) => {}
        DecodedPacketPayload::Invalid(_) => {}
        DecodedPacketPayload::Audit(_) => {}
        DecodedPacketPayload::BattleResults(_) => {}
    }
}

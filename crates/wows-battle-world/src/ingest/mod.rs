//! Packet ingestion: translates decoded packet payloads into ECS state changes.

use bevy_ecs::world::World;
use wows_replays::analyzer::decoder::DecodedPacketPayload;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::GameClock;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use crate::ids::IngestOptions;

/// Dispatch one decoded packet into the ECS world.
///
/// Every variant has an explicit arm so a future-added variant is a compile
/// error rather than silently dropped.
pub fn dispatch<G: ResourceLoader>(
    payload: DecodedPacketPayload<'_, '_, '_>,
    _world: &mut World,
    _resources: &G,
    _constants: &GameConstants,
    _version: Version,
    _options: &IngestOptions,
    _clock: GameClock,
) {
    match payload {
        DecodedPacketPayload::Chat { .. } => {}
        DecodedPacketPayload::VoiceLine { .. } => {}
        DecodedPacketPayload::Ribbon(_) => {}
        DecodedPacketPayload::Position(_) => {}
        DecodedPacketPayload::PlayerOrientation(_) => {}
        DecodedPacketPayload::DamageStat(_) => {}
        DecodedPacketPayload::ShipDestroyed { .. } => {}
        DecodedPacketPayload::EntityMethod(_) => {}
        DecodedPacketPayload::EntityProperty(_) => {}
        DecodedPacketPayload::BasePlayerCreate(_) => {}
        DecodedPacketPayload::CellPlayerCreate(_) => {}
        DecodedPacketPayload::EntityEnter(_) => {}
        DecodedPacketPayload::EntityLeave(_) => {}
        DecodedPacketPayload::EntityCreate(_) => {}
        DecodedPacketPayload::OnArenaStateReceived { .. } => {}
        DecodedPacketPayload::OnGameRoomStateChanged { .. } => {}
        DecodedPacketPayload::NewPlayerSpawnedInBattle { .. } => {}
        DecodedPacketPayload::CheckPing(_) => {}
        DecodedPacketPayload::DamageReceived { .. } => {}
        DecodedPacketPayload::MinimapUpdate { .. } => {}
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
        DecodedPacketPayload::GunSync { .. } => {}
        DecodedPacketPayload::PlaneAdded { .. } => {}
        DecodedPacketPayload::WardAdded { .. } => {}
        DecodedPacketPayload::WardRemoved { .. } => {}
        DecodedPacketPayload::PlaneRemoved { .. } => {}
        DecodedPacketPayload::PlanePosition { .. } => {}
        DecodedPacketPayload::SetAmmoForWeapon { .. } => {}
        DecodedPacketPayload::EntityControl(_) => {}
        DecodedPacketPayload::NonVolatilePosition(_) => {}
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

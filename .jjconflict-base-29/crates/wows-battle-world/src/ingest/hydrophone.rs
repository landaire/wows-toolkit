//! Submarine hydrophone ingestion.

use bevy_ecs::world::World;
use wows_replays::analyzer::decoder::HydrophoneZoneContact;
use wows_replays::analyzer::decoder::SubmarineHydrophoneContact;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;

use crate::resources::Hydrophone;
use crate::resources::HydrophoneContact;
use crate::resources::HydrophoneContactKey;
use crate::resources::HydrophoneContactPosition;
use crate::resources::HydrophoneDetectionChange;

/// The recording player's ship entered or left an enemy hydrophone's hold.
pub fn handle_detection(detected: bool, world: &mut World, clock: GameClock) {
    let mut state = world.resource_mut::<Hydrophone>();
    if state.detected == Some(detected) {
        return;
    }
    state.detected = Some(detected);
    state.detection_changes.push(HydrophoneDetectionChange { clock, detected });
}

/// The zone channel reports the full contact set each time, so an empty array
/// is the client's signal that nothing is held any more.
pub fn handle_zone_contacts(contacts: &[HydrophoneZoneContact], broadcast: bool, world: &mut World, clock: GameClock) {
    let mut state = world.resource_mut::<Hydrophone>();
    state.contacts.retain(|key, _| key.holder.is_some());
    for contact in contacts {
        state.contacts.insert(
            HydrophoneContactKey { holder: None, target: contact.entity_id },
            HydrophoneContact {
                position: HydrophoneContactPosition::Zone {
                    zone_id: contact.zone_id,
                    position: contact.position,
                    broadcast,
                },
                expires_at: None,
                last_updated: clock,
            },
        );
    }
}

pub fn handle_submarine_contacts(
    holder: EntityId,
    contacts: &[SubmarineHydrophoneContact],
    zone_life_time: Option<std::time::Duration>,
    world: &mut World,
    clock: GameClock,
) {
    let expires_at = zone_life_time.map(|ttl| GameClock(clock.seconds() + ttl.as_secs_f32()));
    let mut state = world.resource_mut::<Hydrophone>();
    state.expire(clock);
    for contact in contacts {
        state.contacts.insert(
            HydrophoneContactKey { holder: Some(holder), target: contact.entity_id },
            HydrophoneContact {
                position: HydrophoneContactPosition::Pose {
                    params_id: contact.params_id,
                    position: contact.position,
                    yaw: contact.yaw,
                    pitch: contact.pitch,
                },
                expires_at,
                last_updated: clock,
            },
        );
    }
}

pub fn handle_contact_lost(entity: EntityId, world: &mut World) {
    world.resource_mut::<Hydrophone>().contacts.retain(|key, _| key.target != entity);
}

pub fn handle_cleared(world: &mut World) {
    world.resource_mut::<Hydrophone>().contacts.clear();
}

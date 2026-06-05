//! Ingestion handlers for kill, damage, ribbon, and self-stat packets.

use bevy_ecs::world::World;
use wows_replays::analyzer::battle_controller::DamageEvent;
use wows_replays::analyzer::battle_controller::state::DeadShip;
use wows_replays::analyzer::battle_controller::state::KillRecord;
use wows_replays::analyzer::decoder::DamageReceived as AggressorDamage;
use wows_replays::analyzer::decoder::DamageStatEntry;
use wows_replays::analyzer::decoder::DeathCause;
use wows_replays::analyzer::decoder::Recognized;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::NormalizedPos;
use wows_replays::types::WorldPos;
use wowsunpack::game_types::Ribbon;

use crate::components::{MinimapPlacement, Transform3d};
use crate::resources::{DamageLedger, DeadShips, EntityIndex, KillLog, SelfStats};

/// Increment the ribbon count for the self player.
pub fn handle_ribbon(ribbon: Ribbon, world: &mut World) {
    let mut self_stats = world.resource_mut::<SelfStats>();
    *self_stats.ribbons.entry(ribbon).or_insert(0) += 1;
}

/// Replace (or insert) a damage-stat entry for the self player.
pub fn handle_damage_stat(
    entries: &[DamageStatEntry],
    world: &mut World,
) {
    let mut self_stats = world.resource_mut::<SelfStats>();
    for entry in entries {
        self_stats
            .damage_stats
            .insert((entry.weapon.clone(), entry.category.clone()), entry.clone());
    }
}

/// Record a ship kill and the dead ship's last known positions.
pub fn handle_ship_destroyed(
    killer: EntityId,
    victim: EntityId,
    cause: Recognized<DeathCause>,
    clock: GameClock,
    world: &mut World,
) {
    world.resource_mut::<KillLog>().0.push(KillRecord { clock, killer, victim, cause });

    let world_pos: Option<WorldPos> = victim_world_pos(victim, world);
    let minimap_pos: Option<NormalizedPos> = victim_minimap_pos(victim, world);

    world
        .resource_mut::<DeadShips>()
        .0
        .insert(victim, DeadShip { clock, position: world_pos, minimap_position: minimap_pos });
}

/// Append damage events from a DamageReceived packet, keyed by aggressor.
pub fn handle_damage_received(
    victim: EntityId,
    aggressors: &[AggressorDamage],
    clock: GameClock,
    world: &mut World,
) {
    let mut ledger = world.resource_mut::<DamageLedger>();
    for dmg in aggressors {
        ledger
            .0
            .entry(dmg.aggressor)
            .or_default()
            .push(DamageEvent { amount: dmg.damage, victim, clock });
    }
}

fn victim_world_pos(victim: EntityId, world: &mut World) -> Option<WorldPos> {
    let ecs_entity = world.resource::<EntityIndex>().get(victim)?;
    world.get_entity(ecs_entity).ok()?.get::<Transform3d>().map(|t| t.pos)
}

fn victim_minimap_pos(victim: EntityId, world: &mut World) -> Option<NormalizedPos> {
    let ecs_entity = world.resource::<EntityIndex>().get(victim)?;
    world.get_entity(ecs_entity).ok()?.get::<MinimapPlacement>().map(|m| m.pos)
}

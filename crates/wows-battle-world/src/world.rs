//! Top-level BattleWorld type and entry points.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use tracing::warn;
use wows_replays::ReplayMeta;
use wows_replays::analyzer::battle_controller::MetadataPlayer;
use wows_replays::analyzer::battle_controller::SharedPlayer;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::Relation;
use wows_replays::Rc;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use crate::components::Consumables;
use crate::components::GameId;
use crate::ids::IngestOptions;
use crate::ids::ShotTracking;
use crate::ids::SourceTeam;
use crate::resources::CapturePointOrder;
use crate::resources::CapturedBuffs;
use crate::resources::ChatLog;
use crate::resources::Clock;
use crate::resources::DamageLedger;
use crate::resources::EntityIndex;
use crate::resources::InteractiveZoneIndex;
use crate::resources::KillLog;
use crate::resources::MatchState;
use crate::resources::MetadataPlayers;
use crate::resources::ScoringRules;
use crate::resources::SelfStats;
use crate::resources::ShotHitLog;
use crate::resources::TeamScores;

pub struct BattleWorld<'res, 'replay, G: ResourceLoader> {
    world: World,
    meta: &'replay ReplayMeta,
    resources: &'res G,
    constants: Option<&'res GameConstants>,
    version: Version,
    options: IngestOptions,
}

impl<'res, 'replay, G: ResourceLoader> BattleWorld<'res, 'replay, G> {
    pub fn new(
        meta: &'replay ReplayMeta,
        resources: &'res G,
        constants: Option<&'res GameConstants>,
    ) -> Self {
        let version = Version::from_client_exe(&meta.clientVersionFromExe);
        let mut world = World::new();
        insert_empty_resources(&mut world);
        seed_metadata_players(&mut world, meta, resources);
        Self { world, meta, resources, constants, version, options: IngestOptions::default() }
    }

    /// Reset all mutable state for seeking (re-parse from start).
    ///
    /// Config fields (meta, resources, constants, version, options) are preserved.
    pub fn reset(&mut self) {
        self.world.clear_all();
        insert_empty_resources(&mut self.world);
        seed_metadata_players(&mut self.world, self.meta, self.resources);
    }

    pub fn set_shot_tracking(&mut self, tracking: ShotTracking) {
        self.options.shot_tracking = tracking;
    }

    pub fn set_source_team(&mut self, team: Option<wows_replays::types::TeamId>) {
        self.options.source_team = SourceTeam(team);
    }

    /// Replace the consumable inventory for one entity.
    ///
    /// If `inventory` is empty, any existing `Consumables` component is removed.
    /// If the entity does not yet have a `Consumables` component, one is created.
    pub fn set_consumable_inventory(
        &mut self,
        id: EntityId,
        slots: Vec<wows_replays::analyzer::battle_controller::state::ConsumableInventory>,
    ) {
        if slots.is_empty() {
            let entity = self.world.resource::<EntityIndex>().get(id);
            if let Some(entity) = entity {
                self.world.entity_mut(entity).remove::<Consumables>();
            }
            return;
        }
        let entity = self.spawn_or_get(id);
        let consumables = Consumables { active: Vec::new(), slots };
        if let Ok(mut e) = self.world.get_entity_mut(entity) {
            if let Some(mut c) = e.get_mut::<Consumables>() {
                c.slots = consumables.slots;
            } else {
                e.insert(consumables);
            }
        }
    }

    /// Drop all consumable inventories (e.g. when loading a new replay).
    pub fn clear_consumable_inventories(&mut self) {
        let entities_with_consumables: Vec<Entity> = self
            .world
            .query::<(Entity, &Consumables)>()
            .iter(&self.world)
            .map(|(e, _)| e)
            .collect();
        for entity in entities_with_consumables {
            self.world.entity_mut(entity).remove::<Consumables>();
        }
    }

    /// Get the ECS entity for a game entity id, creating it if absent.
    fn spawn_or_get(&mut self, id: EntityId) -> Entity {
        if let Some(entity) = self.world.resource::<EntityIndex>().get(id) {
            return entity;
        }
        let entity = self.world.spawn((GameId(id),)).id();
        self.world.resource_mut::<EntityIndex>().insert(id, entity);
        entity
    }
}

/// Insert all resources at their default state.
fn insert_empty_resources(world: &mut World) {
    world.insert_resource(Clock::default());
    world.insert_resource(MetadataPlayers::default());
    world.insert_resource(MatchState::default());
    world.insert_resource(TeamScores::default());
    world.insert_resource(ScoringRules::default());
    world.insert_resource(CapturedBuffs::default());
    world.insert_resource(ChatLog::default());
    world.insert_resource(KillLog::default());
    world.insert_resource(DamageLedger::default());
    world.insert_resource(ShotHitLog::default());
    world.insert_resource(SelfStats::default());
    world.insert_resource(CapturePointOrder::default());
    world.insert_resource(InteractiveZoneIndex::default());
    world.insert_resource(EntityIndex::default());
}

/// Build MetadataPlayers from the replay vehicles list.
///
/// Vehicles whose shipId cannot be resolved are skipped with a warning, matching
/// BattleController behavior.
fn seed_metadata_players<G: ResourceLoader>(world: &mut World, meta: &ReplayMeta, resources: &G) {
    let players: Vec<SharedPlayer> = meta
        .vehicles
        .iter()
        .filter_map(|vehicle| {
            let vehicle_param = resources.game_param_by_id(vehicle.shipId).or_else(|| {
                warn!(
                    "skipping unknown vehicle shipId={} for player {:?}",
                    vehicle.shipId, vehicle.name
                );
                None
            })?;
            Some(Rc::new(MetadataPlayer::new(
                vehicle.id,
                vehicle.name.clone(),
                Relation::new(vehicle.relation),
                vehicle_param,
            )))
        })
        .collect();
    world.resource_mut::<MetadataPlayers>().0 = players;
}

impl<'res, 'replay, G: ResourceLoader> wows_replays::analyzer::Analyzer
    for BattleWorld<'res, 'replay, G>
{
    fn process(&mut self, packet: &wows_replays::packet2::Packet<'_, '_>) {
        // Advance the clock unless the packet has no time and the clock has not
        // yet started (initial pre-battle packets carry clock=0).
        if packet.clock.seconds() > 0.0 || self.world.resource::<Clock>().0.seconds() == 0.0 {
            self.world.resource_mut::<Clock>().0 = packet.clock;
        }

        // In untracked mode the shot hit log accumulates; in tracked mode it is
        // cleared each packet so callers see only the current frame's hits.
        if self.options.shot_tracking == ShotTracking::Tracked {
            self.world.resource_mut::<ShotHitLog>().0.clear();
        }

        // DEFAULT_GAME_CONSTANTS is the correct fallback for replays that were
        // recorded without extracting constants from the game install.
        let default_constants = &*wows_replays::game_constants::DEFAULT_GAME_CONSTANTS;
        let constants = self.constants.unwrap_or(default_constants);

        let packet_decoder = wows_replays::analyzer::decoder::PacketDecoder::builder()
            .version(self.version)
            .battle_constants(constants.battle())
            .common_constants(constants.common())
            .ships_constants(constants.ships())
            .build();

        let decoded = packet_decoder.decode(packet);
        let clock: GameClock = packet.clock;

        crate::ingest::dispatch(
            decoded.payload,
            &mut self.world,
            self.resources,
            constants,
            self.version,
            &self.options,
            clock,
        );
    }

    fn finish(&mut self) {
        // Finalization (report assembly, derived state) lands in a later task.
    }
}

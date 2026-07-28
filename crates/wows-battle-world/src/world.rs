//! Top-level BattleWorld type and entry points.

use bevy_ecs::entity::Entity;
use bevy_ecs::world::World;
use tracing::warn;
use wows_replays::Rc;
use wows_replays::ReplayMeta;
use wows_replays::analyzer::battle_controller::MetadataPlayer;
use wows_replays::analyzer::battle_controller::SharedPlayer;
use wows_replays::game_constants::GameConstants;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::Relation;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;

use crate::components::Consumables;
use crate::components::GameId;
use crate::ids::IngestOptions;
use crate::ids::ShotTracking;
use crate::ids::SourceTeam;
use crate::resources::ActiveShotOrder;
use crate::resources::ActiveTorpedoOrder;
use crate::resources::BurnStateLog;
use crate::resources::CapturePointOrder;
use crate::resources::CapturedBuffs;
use crate::resources::ChatLog;
use crate::resources::Clock;
use crate::resources::DamageLedger;
use crate::resources::DeadShips;
use crate::resources::EntityIndex;
use crate::resources::HitHistoryLog;
use crate::resources::Hydrophone;
use crate::resources::InteractiveZoneIndex;
use crate::resources::KillLog;
use crate::resources::MatchState;
use crate::resources::MetadataPlayers;
use crate::resources::PendingDropParams;
use crate::resources::PlaneIndex;
use crate::resources::PlayerIndex;
use crate::resources::PresenceLog;
use crate::resources::ReplayVehicles;
use crate::resources::RibbonLog;
use crate::resources::ScoringRules;
use crate::resources::SelfStats;
use crate::resources::ShotHitLog;
use crate::resources::TeamScores;
use crate::resources::WardIndex;
use crate::resources::WeatherZoneOrder;

pub struct BattleWorld<'res, 'replay, G: ResourceLoader> {
    world: World,
    meta: &'replay ReplayMeta,
    resources: &'res G,
    constants: Option<&'res GameConstants>,
    version: Version,
    options: IngestOptions,
    /// Cached read-side query states, built on the first `view` call.
    query_cache: Option<crate::view::QueryCache>,
}

impl<'res, 'replay, G: ResourceLoader> BattleWorld<'res, 'replay, G> {
    pub fn new(meta: &'replay ReplayMeta, resources: &'res G, constants: Option<&'res GameConstants>) -> Self {
        let version = Version::from_client_exe(&meta.clientVersionFromExe);
        let mut world = World::new();
        insert_empty_resources(&mut world);
        seed_metadata_players(&mut world, meta, resources);
        world.resource_mut::<ReplayVehicles>().0 = meta.vehicles.clone();
        Self { world, meta, resources, constants, version, options: IngestOptions::default(), query_cache: None }
    }

    /// Reset all mutable state for seeking (re-parse from start).
    ///
    /// Config fields (meta, resources, constants, version, options) are preserved.
    /// Consumable inventories are preserved with dynamic state zeroed, mirroring
    /// BattleController::reset_consumable_inventory_state (charges_used=0, active_until=None).
    /// Call clear_consumable_inventories before reset to drop them entirely.
    pub fn reset(&mut self) {
        // Snapshot seeded slot definitions before wiping the world.
        let inventory_snapshot: Vec<(
            EntityId,
            Vec<wows_replays::analyzer::battle_controller::state::ConsumableInventory>,
        )> = self
            .world
            .query::<(&GameId, &Consumables)>()
            .iter(&self.world)
            .map(|(gid, cons)| (gid.0, cons.slots.clone()))
            .collect();

        self.world.clear_all();
        insert_empty_resources(&mut self.world);
        seed_metadata_players(&mut self.world, self.meta, self.resources);
        self.world.resource_mut::<ReplayVehicles>().0 = self.meta.vehicles.clone();

        // Re-attach consumable slot definitions with dynamic state zeroed.
        for (id, slots) in inventory_snapshot {
            let mut reset_slots = slots;
            for slot in reset_slots.iter_mut() {
                slot.charges_used = 0;
                slot.active_until = None;
            }
            let entity = self.spawn_or_get(id);
            if let Ok(mut e) = self.world.get_entity_mut(entity) {
                e.insert(Consumables { active: Vec::new(), slots: reset_slots });
            }
        }
    }

    pub fn set_shot_tracking(&mut self, tracking: ShotTracking) {
        self.options.shot_tracking = tracking;
    }

    pub fn set_source_team(&mut self, team: Option<wows_replays::types::TeamId>) {
        self.options.source_team = SourceTeam(team);
    }

    /// Accumulate every resolved hit in `HitHistoryLog` for the whole parse.
    ///
    /// Off by default because renderers only need the current frame's hits and
    /// should not pay the memory. Any consumer that reads
    /// `BattleReport::hit_history` must turn it on before feeding packets: the
    /// log is otherwise empty, which reads as "no hits" rather than "not
    /// recorded". `ShotTracking::Untracked` suppresses it regardless, since no
    /// `ResolvedShotHit` is constructed at all in that mode.
    pub fn set_record_hit_history(&mut self, record: bool) {
        self.options.record_hit_history = record;
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
        let entities_with_consumables: Vec<Entity> =
            self.world.query::<(Entity, &Consumables)>().iter(&self.world).map(|(e, _)| e).collect();
        for entity in entities_with_consumables {
            self.world.entity_mut(entity).remove::<Consumables>();
        }
    }

    /// Game type string from replay metadata, used to resolve `BattleType`.
    pub(crate) fn game_type(&self) -> Option<&str> {
        self.meta.gameType.as_deref()
    }

    /// Lazily build the read-side query cache, refresh its archetypes, and return
    /// shared borrows of the world and cache.
    ///
    /// Splitting the cache and world fields lets the caller hold both as shared
    /// borrows while the cache's `*_manual` reads run without further allocation.
    pub(crate) fn view_parts(&mut self) -> (&World, &crate::view::QueryCache) {
        if self.query_cache.is_none() {
            self.query_cache = Some(crate::view::QueryCache::new(&mut self.world));
        }
        let cache = self.query_cache.as_mut().expect("cache just initialized");
        cache.update_archetypes(&self.world);
        (&self.world, self.query_cache.as_ref().expect("cache present"))
    }

    pub(crate) fn world(&self) -> &World {
        &self.world
    }

    pub(crate) fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    pub(crate) fn meta(&self) -> &ReplayMeta {
        self.meta
    }

    pub(crate) fn resources(&self) -> &'res G {
        self.resources
    }

    pub(crate) fn version(&self) -> Version {
        self.version
    }

    /// Remove a game entity from EntityIndex and despawn its ECS entity.
    ///
    /// Entity lifetime policy: vehicles and buildings persist for the whole match
    /// (dead ships remain queryable and are tracked separately in DeadShips); only
    /// smoke screens and buff zones are despawned on EntityLeave. This helper is
    /// the single site that removes from EntityIndex; callers are responsible for
    /// applying the correct policy.
    ///
    /// Any open `PresenceLog` window for `id` is closed at the current `Clock`.
    /// A despawned entity can no longer be observed, and a window left open
    /// would keep answering `continuously_observed` with true for every range
    /// after the despawn. `Clock` rather than a raw `packet.clock` because
    /// `despawn` is not driven by a packet; the two agree once the match clock
    /// has advanced past zero.
    pub fn despawn(&mut self, id: EntityId) {
        let clock = self.world.resource::<Clock>().0;
        crate::ingest::entities::close_presence(&mut self.world, id, clock);
        if let Some(entity) = self.world.resource_mut::<EntityIndex>().remove(id)
            && self.world.get_entity(entity).is_ok()
        {
            self.world.despawn(entity);
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
    world.insert_resource(ActiveShotOrder::default());
    world.insert_resource(ActiveTorpedoOrder::default());
    world.insert_resource(SelfStats::default());
    world.insert_resource(Hydrophone::default());
    world.insert_resource(CapturePointOrder::default());
    world.insert_resource(WeatherZoneOrder::default());
    world.insert_resource(InteractiveZoneIndex::default());
    world.insert_resource(PendingDropParams::default());
    world.insert_resource(EntityIndex::default());
    world.insert_resource(PlaneIndex::default());
    world.insert_resource(WardIndex::default());
    world.insert_resource(DeadShips::default());
    world.insert_resource(PlayerIndex::default());
    world.insert_resource(ReplayVehicles::default());
    world.insert_resource(BurnStateLog::default());
    world.insert_resource(RibbonLog::default());
    world.insert_resource(PresenceLog::default());
    world.insert_resource(HitHistoryLog::default());
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
                warn!("skipping unknown vehicle shipId={} for player {:?}", vehicle.shipId, vehicle.name);
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

impl<'res, 'replay, G: ResourceLoader> wows_replays::analyzer::Analyzer for BattleWorld<'res, 'replay, G> {
    fn process(&mut self, packet: &wows_replays::packet2::Packet<'_, '_>) {
        // Advance the clock unless the packet has no time and the clock has not
        // yet started (initial pre-battle packets carry clock=0).
        if packet.clock.seconds() > 0.0 || self.world.resource::<Clock>().0.seconds() == 0.0 {
            self.world.resource_mut::<Clock>().0 = packet.clock;
        }

        // Tracked: clear each packet so callers see only the current frame's hits.
        // Untracked: log is never populated, so no clear needed.
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

#[cfg(test)]
mod tests {
    use wows_replays::Rc;
    use wows_replays::analyzer::decoder::HitType;
    use wows_replays::analyzer::decoder::Recognized;
    use wows_replays::analyzer::decoder::ShotHit;
    use wows_replays::types::AvatarId;
    use wows_replays::types::EntityId;
    use wows_replays::types::GameClock;
    use wows_replays::types::ShotId;
    use wows_replays::types::WorldPos;

    use crate::resources::HitHistoryLog;
    use crate::resources::PlayerIndex;
    use crate::resources::PresenceLog;
    use crate::resources::PresenceWindow;
    use crate::test_support::StubResources;
    use crate::test_support::fixture_param;
    use crate::test_support::minimal_meta;
    use crate::test_support::self_player;
    use crate::world::BattleWorld;

    /// A despawned entity is gone, so its presence window must not keep
    /// answering `continuously_observed` with true. A false "yes" there lets
    /// the fire analysis accept a sample for a vehicle that no longer exists.
    #[test]
    fn despawn_closes_an_open_presence_window() {
        let meta = minimal_meta();
        let resources = StubResources(fixture_param());
        let mut world = BattleWorld::new(&meta, &resources, None);
        let id = EntityId::from(31u32);

        let entity = world.spawn_or_get(id);
        assert!(world.world().get_entity(entity).is_ok());
        world
            .world_mut()
            .resource_mut::<PresenceLog>()
            .0
            .entry(id)
            .or_default()
            .push(PresenceWindow { entered: GameClock(10.0), left: None });
        world.world_mut().resource_mut::<crate::resources::Clock>().0 = GameClock(60.0);

        world.despawn(id);

        let log = world.world().resource::<PresenceLog>();
        assert_eq!(log.0[&id][0].left, Some(GameClock(60.0)));
        assert!(!log.continuously_observed(id, GameClock(20.0), GameClock(90.0)));
        assert!(log.continuously_observed(id, GameClock(20.0), GameClock(50.0)));
    }

    fn a_hit() -> ShotHit {
        ShotHit {
            owner_id: EntityId::from(3u32),
            hit_type: HitType {
                collision: Recognized::Unknown("0".to_string()),
                shell_hit: Recognized::Unknown("0".to_string()),
                raw: 0,
            },
            shot_id: ShotId::from(1u32),
            position: WorldPos::new(0.0, 0.0, 0.0),
            terminal_ballistics: None,
        }
    }

    /// The hit history is what every fire-chance measurement divides by, and it
    /// is off by default, so an unset flag reads as "no hits" rather than "not
    /// recorded". Drives the setter through the ingest handler that consults
    /// it, so a setter wired to the wrong field fails here.
    #[test]
    fn recording_the_hit_history_is_off_until_it_is_set() {
        for (record, expected) in [(false, 0usize), (true, 1usize)] {
            let meta = minimal_meta();
            let resources = StubResources(fixture_param());
            let mut world = BattleWorld::new(&meta, &resources, None);
            let (entity_id, player) = self_player(&resources);
            world.world_mut().resource_mut::<PlayerIndex>().0.insert(entity_id, Rc::new(player));

            world.set_record_hit_history(record);
            // Copied because `handle_shot_kills` takes the ECS world mutably;
            // `IngestOptions` is `Copy`, so this is the same value dispatch
            // would hand it.
            let options = world.options;
            crate::ingest::projectiles::handle_shot_kills(
                AvatarId::from(1u32),
                vec![a_hit()],
                GameClock(10.0),
                world.world_mut(),
                &options,
            );

            assert_eq!(world.world().resource::<HitHistoryLog>().0.len(), expected, "record_hit_history = {record}");
        }
    }
}

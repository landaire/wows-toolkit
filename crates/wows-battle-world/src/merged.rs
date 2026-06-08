//! Multi-replay merge driver.
//!
//! [`MergedReplays`] bundles a primary replay and any number of "alt"
//! perspectives of the same match into a single [`BattleWorld`]. The primary
//! owns broadcast-flavoured state (chat, kills, scores, battle stage, etc.);
//! the alt replays only contribute updates to other players' vehicles, so the
//! merged view sees through fog of war without altering match-wide outputs.
//!
//! Callers drive the merge with [`MergedReplays::step`]: process one packet
//! from whichever underlying replay is most behind in clock time, returning
//! the new "safe clock" (the latest moment that every active replay has
//! reached) so renderers know when it's safe to draw a frame.

use std::collections::HashMap;
use std::sync::Arc;

use wows_replays::ReplayFile;
use wows_replays::analyzer::Analyzer;
use wows_replays::game_constants::GameConstants;
use wows_replays::packet2::Packet;
use wows_replays::packet2::PacketType;
use wows_replays::packet2::Parser;
use wows_replays::types::ArenaId;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::TeamId;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::rpc::entitydefs::EntitySpec;

pub use wows_replays::analyzer::battle_controller::merged::VehicleFacts;
pub use wows_replays::analyzer::battle_controller::merged::gather_replay_facts;

use crate::scan::FactsCollector;
use crate::scan::MetadataCollector;
use crate::scan::PositionTimeline;
use crate::scan::PositionTimelineCollector;
use crate::scan::merge_position_timelines;
use crate::scan::scan_replay;

use crate::BattleWorld;
use crate::components::Vehicle;
use crate::resources::EntityIndex;

/// Errors raised while building or driving a [`MergedReplays`] session.
#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("merge replay version mismatch (primary={primary}, merge #{index}={merge}, player={player})")]
    VersionMismatch { primary: String, merge: String, index: usize, player: String },

    #[error("arena ID mismatch: replays are not from the same match (primary={primary}, merge #{index}={merge})")]
    ArenaIdMismatch { primary: ArenaId, merge: ArenaId, index: usize },

    #[error("packet parse error in replay #{index}: {message}")]
    PacketParse { index: usize, message: String },
}

/// Driver for a primary replay plus zero or more "alt" perspectives of the
/// same match. All underlying state is exposed through a single
/// [`BattleWorld`] whose view the caller reads.
pub struct MergedReplays<'specs, 'res, 'data, G: ResourceLoader> {
    world: BattleWorld<'res, 'data, G>,
    parsers: Vec<Parser<'specs>>,
    remainings: Vec<&'data [u8]>,
    replays: Vec<&'data ReplayFile>,
    self_teams: Vec<Option<TeamId>>,
    last_clocks: Vec<GameClock>,
    finished: Vec<bool>,
    arena_ids: Vec<Option<ArenaId>>,
    arena_validated: bool,
    total_duration: GameClock,
    battle_start_clock: Option<GameClock>,
    position_timeline: Arc<PositionTimeline>,
    vehicle_facts: HashMap<EntityId, VehicleFacts>,
}

impl<'specs, 'res, 'data, G: ResourceLoader> MergedReplays<'specs, 'res, 'data, G> {
    /// Build a session for `primary` plus `merges`. Validates that every
    /// merge has the same client version as the primary, pre-scans each
    /// replay for its recording player's team id (used to tag packets going
    /// into the world), and computes [`total_duration`] as the longest
    /// last-packet clock across all replays.
    ///
    /// [`total_duration`]: Self::total_duration
    pub fn new(
        specs: &'specs [EntitySpec],
        game_params: &'res G,
        game_constants: &'res GameConstants,
        version: Version,
        primary: &'data ReplayFile,
        merges: &'data [ReplayFile],
    ) -> Result<Self, MergeError> {
        for (i, m) in merges.iter().enumerate() {
            if m.meta.clientVersionFromExe != primary.meta.clientVersionFromExe {
                return Err(MergeError::VersionMismatch {
                    primary: primary.meta.clientVersionFromExe.clone(),
                    merge: m.meta.clientVersionFromExe.clone(),
                    index: i + 1,
                    player: m.meta.playerName.clone(),
                });
            }
        }

        let replay_count = 1 + merges.len();
        let mut replays: Vec<&ReplayFile> = Vec::with_capacity(replay_count);
        replays.push(primary);
        replays.extend(merges.iter());

        let parsers: Vec<Parser<'specs>> = (0..replay_count).map(|_| Parser::with_version(specs, version)).collect();
        let remainings: Vec<&[u8]> = replays.iter().map(|r| r.packet_data.as_slice()).collect();

        let mut self_teams = Vec::with_capacity(replay_count);
        let mut total_duration = GameClock(0.0);
        let mut timeline_parts: Vec<PositionTimeline> = Vec::with_capacity(replay_count);
        let mut battle_start_clock: Option<GameClock> = None;
        let mut combined_facts: HashMap<EntityId, VehicleFacts> = HashMap::new();
        for (i, r) in replays.iter().enumerate() {
            let mut meta = MetadataCollector::new(r.meta.playerName.as_str(), game_constants);
            let mut positions = PositionTimelineCollector::default();
            let mut facts = FactsCollector::new(version, game_constants);
            scan_replay(specs, game_constants, version, r, &mut [&mut meta, &mut positions, &mut facts]);
            self_teams.push(meta.self_team);
            total_duration = GameClock(total_duration.0.max(meta.last_clock.0));
            // battleStage is a primary-owned broadcast; trust only the primary.
            if i == 0 {
                battle_start_clock = meta.battle_start_clock;
            }
            timeline_parts.push(positions.timeline);
            for (entity_id, src) in facts.into_facts() {
                let dst = combined_facts.entry(entity_id).or_default();
                if dst.vehicle_id.raw() == 0 && src.vehicle_id.raw() != 0 {
                    dst.vehicle_id = src.vehicle_id;
                }
                if dst.max_health == 0.0 && src.max_health > 0.0 {
                    dst.max_health = src.max_health;
                }
                if dst.ship_config.abilities().is_empty() && !src.ship_config.abilities().is_empty() {
                    dst.ship_config = src.ship_config;
                }
                if dst.crew.params_id().raw() == 0 && src.crew.params_id().raw() != 0 {
                    dst.crew = src.crew;
                }
            }
        }
        let position_timeline = Arc::new(merge_position_timelines(timeline_parts));

        let world = BattleWorld::new(&primary.meta, game_params, Some(game_constants));

        Ok(Self {
            world,
            parsers,
            remainings,
            replays,
            self_teams,
            last_clocks: vec![GameClock(0.0); replay_count],
            finished: vec![false; replay_count],
            arena_ids: vec![None; replay_count],
            arena_validated: replay_count <= 1,
            total_duration,
            battle_start_clock,
            position_timeline,
            vehicle_facts: combined_facts,
        })
    }

    /// All replays, primary first.
    pub fn replays(&self) -> &[&'data ReplayFile] {
        &self.replays
    }

    /// Self-team for each replay (same index order as [`replays`]).
    ///
    /// [`replays`]: Self::replays
    pub fn self_teams(&self) -> &[Option<TeamId>] {
        &self.self_teams
    }

    /// Total replay duration, i.e. the latest last-packet clock across all
    /// merged replays. Useful for sizing the output video / progress bar.
    pub fn total_duration(&self) -> GameClock {
        self.total_duration
    }

    /// Clock at which the pre-battle phase ends and the battle becomes active,
    /// scanned from the primary replay. `None` if the replay never records the
    /// transition (e.g. very old replays).
    pub fn battle_start_clock(&self) -> Option<GameClock> {
        self.battle_start_clock
    }

    /// Per-entity position timeline for motion smoothing, built once at session
    /// construction and shared cheaply (`Arc`) with renderers.
    pub fn position_timeline(&self) -> Arc<PositionTimeline> {
        Arc::clone(&self.position_timeline)
    }

    /// Per-vehicle facts gathered during session construction. Contains max HP,
    /// ship config, and crew params for every vehicle seen across all replays.
    pub fn vehicle_facts(&self) -> &HashMap<EntityId, VehicleFacts> {
        &self.vehicle_facts
    }

    /// `true` once every replay's stream has been exhausted.
    pub fn is_done(&self) -> bool {
        self.finished.iter().all(|f| *f)
    }

    /// Read-only access to the merged world state. Renderers call this
    /// between [`step`](Self::step) calls to inspect the current merged view.
    pub fn world(&self) -> &BattleWorld<'res, 'data, G> {
        &self.world
    }

    /// Mutable access to the merged world, for operations that need `&mut`.
    pub fn world_mut(&mut self) -> &mut BattleWorld<'res, 'data, G> {
        &mut self.world
    }

    /// Consume the session and return the underlying merged world.
    pub fn into_world(self) -> BattleWorld<'res, 'data, G> {
        self.world
    }

    /// Latest clock for which every active replay has finished processing
    /// its stream. Renderers should only draw frames at clocks up to this
    /// value to ensure all perspectives are consistent.
    pub fn safe_clock(&self) -> Option<GameClock> {
        (0..self.replays.len())
            .filter(|i| !self.finished[*i])
            .map(|i| self.last_clocks[i].0)
            .fold(None, |acc, c| Some(acc.map(|a: f32| a.min(c)).unwrap_or(c)))
            .map(GameClock)
    }

    /// Drive one packet from the most-behind replay into the world.
    /// Returns:
    /// - `Ok(Some(safe_clock))` after a packet was processed.
    /// - `Ok(None)` once every replay is exhausted ([`is_done`] returns true).
    /// - `Err` on parse failure or arena-id mismatch.
    ///
    /// [`is_done`]: Self::is_done
    pub fn step(&mut self) -> Result<Option<GameClock>, MergeError> {
        let lag = (0..self.replays.len()).filter(|i| !self.finished[*i]).min_by(|a, b| {
            self.last_clocks[*a].0.partial_cmp(&self.last_clocks[*b].0).unwrap_or(std::cmp::Ordering::Equal)
        });
        let Some(idx) = lag else { return Ok(None) };

        if self.remainings[idx].is_empty() {
            self.finished[idx] = true;
            return self.step();
        }

        let packet = self.parsers[idx]
            .parse_packet(&mut self.remainings[idx])
            .map_err(|e| MergeError::PacketParse { index: idx, message: format!("{e:?}") })?;
        let packet_clock = packet.clock;

        let is_primary = idx == 0;

        // Only tag the world with a source team when we're actually merging
        // multiple replays; single-replay processing should behave exactly as
        // it does without any merger involved.
        if self.replays.len() > 1 {
            self.world.set_source_team(self.self_teams[idx]);
        }
        if is_primary || forward_secondary_packet(&self.world, &packet) {
            self.world.process(&packet);
        }

        // Cheap second pass to harvest the arena id without re-routing
        // through the world; the routing filter would drop the secondaries'
        // onArenaStateReceived calls.
        if self.arena_ids[idx].is_none()
            && let PacketType::EntityMethod(em) = &packet.payload
            && em.method == "onArenaStateReceived"
            && let Some(wowsunpack::rpc::typedefs::ArgValue::Int64(v)) = em.args.first()
        {
            self.arena_ids[idx] = Some(ArenaId::from(*v));
        }

        drop(packet);
        self.last_clocks[idx] = packet_clock;

        if !self.arena_validated
            && let Some(primary_arena) = self.arena_ids[0]
        {
            let mut all_set = true;
            for (i, id) in self.arena_ids.iter().enumerate().skip(1) {
                let Some(merge_arena) = id else {
                    all_set = false;
                    continue;
                };
                if *merge_arena != primary_arena {
                    return Err(MergeError::ArenaIdMismatch { primary: primary_arena, merge: *merge_arena, index: i });
                }
            }
            if all_set {
                self.arena_validated = true;
            }
        }

        Ok(self.safe_clock())
    }

    /// Finalize the merged world.
    pub fn finish(&mut self) {
        self.world.finish();
    }

    /// First-validated arena id, available once every replay has emitted its
    /// `onArenaStateReceived` packet.
    pub fn arena_id(&self) -> Option<ArenaId> {
        self.arena_ids[0]
    }
}

/// Walk a replay's stream to find the first `onArenaStateReceived` and
/// extract its arena id. Lets callers reject merge candidates that
/// don't belong to the same match before a full re-parse is kicked off.
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

/// Decide whether a packet from a non-primary replay should be fed to the
/// shared world. Forward iff the packet updates state on a Vehicle entity
/// that the world already tracks, plus a short allow-list of Avatar-method
/// calls that carry cross-perspective info about other ships (artillery in
/// flight, plane spawns, minimap vision, etc.).
fn forward_secondary_packet<G: ResourceLoader>(world: &BattleWorld<'_, '_, G>, packet: &Packet<'_, '_>) -> bool {
    match &packet.payload {
        PacketType::Position(p) => is_known_vehicle(world, p.pid),
        PacketType::EntityProperty(ep) => is_known_vehicle(world, ep.entity_id),
        PacketType::PropertyUpdate(pu) => is_known_vehicle(world, pu.entity_id),
        PacketType::EntityMethod(em) => is_cross_perspective_method(em.method),
        // Everything else (lifecycle, recording-player setup, primary's view,
        // match-wide one-shots, server timing) is owned by primary.
        _ => false,
    }
}

/// Return `true` if `id` is present in the EntityIndex and carries the
/// `Vehicle` marker component.
fn is_known_vehicle<G: ResourceLoader>(world: &BattleWorld<'_, '_, G>, id: EntityId) -> bool {
    let Some(entity) = world.world().resource::<EntityIndex>().get(id) else {
        return false;
    };
    world.world().get_entity(entity).ok().and_then(|e| e.get::<Vehicle>()).is_some()
}

fn is_cross_perspective_method(method: &str) -> bool {
    matches!(
        method,
        "receiveArtilleryShots"
            | "receiveTorpedoes"
            | "receiveTorpedoDirection"
            | "receive_addMinimapSquadron"
            | "receive_removeMinimapSquadron"
            | "receive_updateMinimapSquadron"
            | "receive_wardAdded"
            | "receive_wardRemoved"
            | "updateMinimapVisionInfo"
            | "consumableUsed"
            | "onConsumableUsed"
            | "syncGun"
            | "setAmmoForWeapon"
            | "syncShipCracks"
            | "shootOnClient"
            | "shootATBAGuns"
    )
}

/// Gather damage events from every replay's perspective and union them.
///
/// receiveDamagesOnShip events only fire on perspectives that can see the
/// victim. Damage dealt by a teammate to an enemy spotted by no one on the
/// recording player's perspective is invisible to the merged session, so we
/// run a per-replay BattleWorld for each alt perspective and combine
/// the results. Duplicate events (same aggressor + victim + clock) are
/// kept once.
pub fn gather_damage_events<G: ResourceLoader>(
    game_resources: &G,
    constants: &GameConstants,
    version: Version,
    specs: &[EntitySpec],
    replays: &[&ReplayFile],
) -> HashMap<EntityId, Vec<wows_replays::analyzer::battle_controller::DamageEvent>> {
    use wows_replays::analyzer::battle_controller::DamageEvent;

    let mut combined: HashMap<EntityId, Vec<DamageEvent>> = HashMap::new();
    let mut seen: std::collections::HashSet<(EntityId, EntityId, u64)> = std::collections::HashSet::new();

    for replay in replays {
        let mut world = BattleWorld::new(&replay.meta, game_resources, Some(constants));
        let mut parser = Parser::with_version(specs, version);
        let mut remaining = &replay.packet_data[..];
        while !remaining.is_empty() {
            let Ok(packet) = parser.parse_packet(&mut remaining) else { break };
            world.process(&packet);
        }
        world.finish();
        for (aggressor, events) in world.damage_ledger() {
            for event in events {
                // Same physical damage hit may be reported by multiple
                // perspectives; key by aggressor + victim + clock-bits.
                let key = (*aggressor, event.victim, event.clock.0.to_bits() as u64);
                if !seen.insert(key) {
                    continue;
                }
                combined.entry(*aggressor).or_default().push(*event);
            }
        }
    }

    for events in combined.values_mut() {
        events.sort_by(|a, b| a.clock.0.partial_cmp(&b.clock.0).unwrap_or(std::cmp::Ordering::Equal));
    }
    combined
}

/// Snapshot per-vehicle facts from a world that has finished processing
/// every packet in its packet stream. Each vehicle entity becomes one
/// `VehicleFacts` entry.
pub fn capture_vehicle_facts<G: ResourceLoader>(world: &mut BattleWorld<'_, '_, G>) -> HashMap<EntityId, VehicleFacts> {
    let mut out = HashMap::new();
    for (entity_id, props) in world.vehicle_props_all() {
        out.insert(
            entity_id,
            VehicleFacts {
                vehicle_id: props.ship_config().ship_params_id(),
                max_health: props.max_health(),
                ship_config: props.ship_config().clone(),
                crew: props.crew_modifiers_compact_params().clone(),
            },
        );
    }
    out
}

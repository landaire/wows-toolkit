//! Single-pass replay scanning: one packet walk feeds a set of collectors.

use std::collections::HashMap;

use wows_replays::ReplayFile;
use wows_replays::ReplayMeta;
use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::battle_controller::merged::VehicleFacts;
use wows_replays::analyzer::battle_controller::merged::VehicleFactsAccumulator;
use wows_replays::analyzer::decoder::DecodedPacketPayload;
use wows_replays::analyzer::decoder::PacketDecoder;
use wows_replays::game_constants::GameConstants;
use wows_replays::packet2::Packet;
use wows_replays::packet2::PacketType;
use wows_replays::packet2::Parser;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::NormalizedPos;
use wows_replays::types::TeamId;
use wows_replays::types::WorldPos;
use wowsunpack::data::ResourceLoader;
use wowsunpack::data::Version;
use wowsunpack::game_types::BattleStage;
use wowsunpack::rpc::entitydefs::EntitySpec;

use crate::view::BattleView;
use crate::world::BattleWorld;

/// A position sample tagged with the coordinate space it came from. Kept
/// distinct so the draw site converts each with the matching projection and so
/// world/minimap provenance is never silently mixed.
#[derive(Debug, Clone, Copy)]
pub enum SampledPos {
    /// Full-precision world position (Position / PlayerOrientation packets).
    World(WorldPos),
    /// Quantized normalized minimap position (updateMinimapVisionInfo).
    Minimap(NormalizedPos),
}

/// Per-entity samples split by source. World samples (Position /
/// PlayerOrientation) are dense; minimap samples (updateMinimapVisionInfo) are
/// sparse and quantized. They interleave while a ship is both in AOI and
/// spotted, so they are kept in separate tracks and interpolated independently.
/// Each vec is sorted ascending by clock.
#[derive(Debug, Default, Clone)]
pub struct EntityTrack {
    pub world: Vec<(GameClock, WorldPos)>,
    pub minimap: Vec<(GameClock, NormalizedPos)>,
}

/// Per-entity position tracks.
pub type PositionTimeline = HashMap<EntityId, EntityTrack>;

/// Merge per-replay timelines into one, concatenating each entity's world and
/// minimap tracks across perspectives, then sorting each track by clock and
/// dropping exact-duplicate clocks.
pub fn merge_position_timelines(parts: Vec<PositionTimeline>) -> PositionTimeline {
    let mut out: PositionTimeline = HashMap::new();
    for part in parts {
        for (eid, track) in part {
            let e = out.entry(eid).or_default();
            e.world.extend(track.world);
            e.minimap.extend(track.minimap);
        }
    }
    for track in out.values_mut() {
        track.world.sort_by(|a, b| a.0.0.total_cmp(&b.0.0));
        track.world.dedup_by(|a, b| a.0.0 == b.0.0);
        track.minimap.sort_by(|a, b| a.0.0.total_cmp(&b.0.0));
        track.minimap.dedup_by(|a, b| a.0.0 == b.0.0);
    }
    out
}

/// Observes each packet of a single replay during one [`scan_replay`] pass.
pub trait ScanCollector {
    /// Called once per successfully-parsed packet, with its decoded payload.
    fn observe(&mut self, packet: &Packet<'_, '_>, decoded: &DecodedPacketPayload<'_, '_, '_>);
}

/// Walk `replay`'s packet stream once, decoding each packet and feeding every
/// collector. Stops at the first parse error (the tail of some replays is
/// truncated); collectors keep whatever they gathered up to that point.
pub fn scan_replay(
    specs: &[EntitySpec],
    game_constants: &GameConstants,
    version: Version,
    replay: &ReplayFile,
    collectors: &mut [&mut dyn ScanCollector],
) {
    let mut parser = Parser::with_version(specs, version);
    let decoder = PacketDecoder::builder()
        .version(version)
        .battle_constants(game_constants.battle())
        .common_constants(game_constants.common())
        .ships_constants(game_constants.ships())
        .build();
    let mut remaining = &replay.packet_data[..];
    while !remaining.is_empty() {
        let Ok(packet) = parser.parse_packet(&mut remaining) else {
            break;
        };
        let decoded = decoder.decode(&packet);
        for c in collectors.iter_mut() {
            c.observe(&packet, &decoded.payload);
        }
    }
}

/// Builds a per-entity position timeline from world packets (Position,
/// PlayerOrientation for the self ship) and visible minimap-vision entries.
#[derive(Default)]
pub struct PositionTimelineCollector {
    pub timeline: PositionTimeline,
}

impl ScanCollector for PositionTimelineCollector {
    fn observe(&mut self, packet: &Packet<'_, '_>, decoded: &DecodedPacketPayload<'_, '_, '_>) {
        match &packet.payload {
            PacketType::Position(p) => {
                self.timeline
                    .entry(p.pid)
                    .or_default()
                    .world
                    .push((packet.clock, WorldPos::new(p.position.x, p.position.y, p.position.z)));
            }
            PacketType::PlayerOrientation(o) if o.parent_id == EntityId::from(0u32) => {
                self.timeline
                    .entry(o.pid)
                    .or_default()
                    .world
                    .push((packet.clock, WorldPos::new(o.position.x, o.position.y, o.position.z)));
            }
            _ => {}
        }
        if let DecodedPacketPayload::MinimapUpdate { updates, .. } = decoded {
            for u in updates {
                // Sentinels (not visible) and one-shot pings are not sustained tracking.
                if u.is_sentinel || u.is_minimap_ping() {
                    continue;
                }
                self.timeline.entry(u.entity_id).or_default().minimap.push((packet.clock, u.position));
            }
        }
    }
}

/// Recording-player team, replay duration, and battle-start clock, gathered in
/// one pass. `self_team` matches the recording player's name to the arena
/// roster; `battle_start_clock` is the first `battleStage == Waiting`.
pub struct MetadataCollector<'a> {
    player_name: &'a str,
    game_constants: &'a GameConstants,
    pub self_team: Option<TeamId>,
    pub last_clock: GameClock,
    pub battle_start_clock: Option<GameClock>,
}

impl<'a> MetadataCollector<'a> {
    pub fn new(player_name: &'a str, game_constants: &'a GameConstants) -> Self {
        Self { player_name, game_constants, self_team: None, last_clock: GameClock(0.0), battle_start_clock: None }
    }
}

impl ScanCollector for MetadataCollector<'_> {
    fn observe(&mut self, packet: &Packet<'_, '_>, decoded: &DecodedPacketPayload<'_, '_, '_>) {
        if packet.clock.0 > self.last_clock.0 {
            self.last_clock = packet.clock;
        }
        match decoded {
            DecodedPacketPayload::OnArenaStateReceived { player_states, bot_states, .. }
                if self.self_team.is_none() =>
            {
                self.self_team = player_states
                    .iter()
                    .chain(bot_states.iter())
                    .find(|p| p.username() == self.player_name)
                    .map(|p| TeamId::from(p.team_id()));
            }
            DecodedPacketPayload::EntityProperty(prop)
                if self.battle_start_clock.is_none() && prop.property == "battleStage" =>
            {
                if let Some(raw) = prop.value.as_i64()
                    && matches!(
                        self.game_constants.common().battle_stage(raw as i32).copied(),
                        Some(BattleStage::Waiting)
                    )
                {
                    self.battle_start_clock = Some(packet.clock);
                }
            }
            _ => {}
        }
    }
}

/// Per-shooter salvo flight times learned from actual impacts: owner entity ->
/// sorted-by-fire-time list of (fired_at, flight_seconds). Used to pace shell
/// tracers by the real time-to-impact instead of the unreliable serverTimeLeft.
pub type SalvoFlightTimes = HashMap<EntityId, Vec<(GameClock, f32)>>;

/// Accumulates per-salvo flight times from [`ResolvedShotHit`]s, averaging
/// across all shells in a salvo for robustness against missing hits.
///
/// [`ResolvedShotHit`]: wows_replays::analyzer::battle_controller::state::ResolvedShotHit
#[derive(Default)]
pub struct SalvoImpactCollector {
    acc: HashMap<(EntityId, u32), (f32, u32)>,
}

impl WorldScanCollector for SalvoImpactCollector {
    fn observe(&mut self, _packet: &Packet<'_, '_>, _prev_clock: GameClock, view: &BattleView<'_>) {
        for h in view.shot_hits() {
            let Some(fired_at) = h.fired_at else { continue };
            let flight = h.clock.0 - fired_at.0;
            if flight <= 0.0 {
                continue;
            }
            let owner = h.hit.owner_id;
            let key = (owner, fired_at.0.to_bits());
            let e = self.acc.entry(key).or_insert((0.0, 0));
            e.0 += flight;
            e.1 += 1;
        }
    }
}

impl SalvoImpactCollector {
    pub fn into_flight_times(self) -> SalvoFlightTimes {
        let mut out: SalvoFlightTimes = HashMap::new();
        for ((owner, fa_bits), (sum, n)) in self.acc {
            let fired_at = GameClock(f32::from_bits(fa_bits));
            let flight = sum / n as f32;
            out.entry(owner).or_default().push((fired_at, flight));
        }
        for v in out.values_mut() {
            v.sort_by(|a, b| a.0.0.total_cmp(&b.0.0));
        }
        out
    }
}

/// Pre-scan a replay and learn each shooter's salvo flight times from impacts.
pub fn scan_salvo_flight_times<G: ResourceLoader>(
    meta: &ReplayMeta,
    game_params: &G,
    game_constants: &GameConstants,
    version: Version,
    replay: &ReplayFile,
) -> SalvoFlightTimes {
    let mut col = SalvoImpactCollector::default();
    scan_replay_world(meta, game_params, game_constants, version, replay, &mut [&mut col]);
    col.into_flight_times()
}

/// Observes a `BattleWorld` stepped over one replay.
///
/// `observe_pre` runs before the world processes a packet (pre-process state).
/// `observe` runs after the world processes a packet (post-process state).
/// Both receive the previous packet's clock so collectors can detect boundaries.
pub trait WorldScanCollector {
    /// Called BEFORE the world processes `packet`, with the pre-process view.
    fn observe_pre(&mut self, _packet: &Packet<'_, '_>, _prev_clock: GameClock, _view: &BattleView<'_>) {}
    /// Called AFTER the world processes `packet`, with the post-process view.
    fn observe(&mut self, packet: &Packet<'_, '_>, prev_clock: GameClock, view: &BattleView<'_>);
    fn finish(&mut self, _view: &BattleView<'_>) {}
}

/// Collects per-vehicle facts during a [`scan_replay`] pass.
///
/// Wraps [`VehicleFactsAccumulator`] so facts gathering participates in the
/// same single packet walk as metadata and position collection. Call
/// [`into_facts`](Self::into_facts) after the scan to obtain the result.
pub struct FactsCollector<'c> {
    inner: VehicleFactsAccumulator<'c>,
}

impl<'c> FactsCollector<'c> {
    pub fn new(version: Version, constants: &'c GameConstants) -> Self {
        Self { inner: VehicleFactsAccumulator::new(version, constants) }
    }

    pub fn into_facts(self) -> HashMap<EntityId, VehicleFacts> {
        self.inner.into_facts()
    }
}

impl ScanCollector for FactsCollector<'_> {
    fn observe(&mut self, packet: &Packet<'_, '_>, decoded: &DecodedPacketPayload<'_, '_, '_>) {
        self.inner.observe(packet, decoded);
    }
}

/// Build one `BattleWorld`, step it over `replay`, and feed each collector the
/// post-process view per packet. Stops at the first parse error.
pub fn scan_replay_world<G: ResourceLoader>(
    meta: &ReplayMeta,
    game_params: &G,
    game_constants: &GameConstants,
    version: Version,
    replay: &ReplayFile,
    collectors: &mut [&mut dyn WorldScanCollector],
) {
    let mut world = BattleWorld::new(meta, game_params, Some(game_constants));
    let mut parser = Parser::with_version(game_params.entity_specs(), version);
    let mut remaining = &replay.packet_data[..];
    let mut prev_clock = GameClock(0.0);
    while let Ok(packet) = parser.parse_packet(&mut remaining) {
        {
            let view = world.view();
            for c in collectors.iter_mut() {
                c.observe_pre(&packet, prev_clock, &view);
            }
        }
        world.process(&packet);
        {
            let view = world.view();
            for c in collectors.iter_mut() {
                c.observe(&packet, prev_clock, &view);
            }
        }
        prev_clock = packet.clock;
    }
    world.finish();
    let view = world.view();
    for c in collectors.iter_mut() {
        c.finish(&view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Counter(usize);
    impl ScanCollector for Counter {
        fn observe(&mut self, _p: &Packet<'_, '_>, _d: &DecodedPacketPayload<'_, '_, '_>) {
            self.0 += 1;
        }
    }

    #[test]
    fn scan_replay_signature_compiles() {
        let _f: fn(&[EntitySpec], &GameConstants, Version, &ReplayFile, &mut [&mut dyn ScanCollector]) = scan_replay;
        let mut counter = Counter(0);
        counter.0 += 1;
        assert_eq!(counter.0, 1);
    }

    struct NoopWorldCollector;
    impl WorldScanCollector for NoopWorldCollector {
        fn observe(&mut self, _packet: &Packet<'_, '_>, _prev_clock: GameClock, _view: &BattleView<'_>) {}
    }

    #[test]
    fn scan_replay_world_trait_compiles() {
        // Verify WorldScanCollector is object-safe and finish has a default impl.
        let mut c = NoopWorldCollector;
        let _: &mut dyn WorldScanCollector = &mut c;
    }

    #[test]
    fn merge_sorts_and_dedups_by_clock() {
        let mut a: PositionTimeline = HashMap::new();
        a.entry(EntityId::from(1u32))
            .or_default()
            .world
            .extend([(GameClock(2.0), WorldPos::new(2.0, 0.0, 0.0)), (GameClock(0.0), WorldPos::new(0.0, 0.0, 0.0))]);
        let mut b: PositionTimeline = HashMap::new();
        b.entry(EntityId::from(1u32)).or_default().world.push((GameClock(2.0), WorldPos::new(9.0, 0.0, 0.0)));
        let merged = merge_position_timelines(vec![a, b]);
        let w = &merged[&EntityId::from(1u32)].world;
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].0.0, 0.0);
        assert_eq!(w[1].0.0, 2.0);
    }
}

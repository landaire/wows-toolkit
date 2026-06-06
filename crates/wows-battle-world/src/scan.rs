//! Single-pass replay scanning: one packet walk feeds a set of collectors.

use std::collections::HashMap;

use wows_replays::ReplayFile;
use wows_replays::analyzer::decoder::DecodedPacketPayload;
use wows_replays::analyzer::decoder::PacketDecoder;
use wows_replays::game_constants::GameConstants;
use wows_replays::packet2::Packet;
use wows_replays::packet2::Parser;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;
use wows_replays::types::NormalizedPos;
use wows_replays::types::WorldPos;
use wowsunpack::data::Version;
use wowsunpack::rpc::entitydefs::EntitySpec;

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

/// One timestamped position sample for an entity.
#[derive(Debug, Clone, Copy)]
pub struct PositionSample {
    pub clock: GameClock,
    pub pos: SampledPos,
}

/// Per-entity samples, sorted ascending by clock.
pub type PositionTimeline = HashMap<EntityId, Vec<PositionSample>>;

/// Merge per-replay timelines into one. Samples for the same entity from
/// multiple perspectives are concatenated, sorted by clock, and exact-duplicate
/// clocks dropped (keeping the first seen).
pub fn merge_position_timelines(parts: Vec<PositionTimeline>) -> PositionTimeline {
    let mut out: PositionTimeline = HashMap::new();
    for part in parts {
        for (eid, samples) in part {
            out.entry(eid).or_default().extend(samples);
        }
    }
    for samples in out.values_mut() {
        samples.sort_by(|a, b| a.clock.0.total_cmp(&b.clock.0));
        samples.dedup_by(|a, b| a.clock.0 == b.clock.0);
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

    #[test]
    fn merge_sorts_and_dedups_by_clock() {
        let mut a: PositionTimeline = HashMap::new();
        a.insert(
            EntityId::from(1u32),
            vec![
                PositionSample { clock: GameClock(2.0), pos: SampledPos::World(WorldPos::new(2.0, 0.0, 0.0)) },
                PositionSample { clock: GameClock(0.0), pos: SampledPos::World(WorldPos::new(0.0, 0.0, 0.0)) },
            ],
        );
        let mut b: PositionTimeline = HashMap::new();
        b.insert(
            EntityId::from(1u32),
            vec![PositionSample { clock: GameClock(2.0), pos: SampledPos::World(WorldPos::new(9.0, 0.0, 0.0)) }],
        );
        let merged = merge_position_timelines(vec![a, b]);
        let s = &merged[&EntityId::from(1u32)];
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].clock.0, 0.0);
        assert_eq!(s[1].clock.0, 2.0);
    }
}

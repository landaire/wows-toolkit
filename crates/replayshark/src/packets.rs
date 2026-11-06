use std::ops::ControlFlow;

use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use wows_replays::ReplayFile;
use wows_replays::analyzer::decoder::PacketDecoder;
use wows_replays::context::GameDataContext;
use wows_replays::packet2::Parser;
use wows_replays::types::EntityId;
use wows_replays::types::GameClock;

use crate::analysis::replay_version;
use crate::error::AnalysisError;

/// Which decoded packets to visit. Every condition given must hold.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PacketFilter {
    /// Payload kinds such as `Chat`, `DamageReceived` or `EntityProperty`,
    /// compared without case. Empty admits every kind.
    pub kinds: Vec<String>,

    /// Admits packets whose payload names this entity in a top-level id
    /// field such as `entity_id`, `vehicle_id`, `aggressor` or `victim`.
    pub entity: Option<EntityId>,

    pub from: Option<GameClock>,
    pub to: Option<GameClock>,
}

impl PacketFilter {
    fn admits_clock(&self, clock: GameClock) -> bool {
        self.from.is_none_or(|from| clock.seconds() >= from.seconds())
            && self.to.is_none_or(|to| clock.seconds() <= to.seconds())
    }

    fn admits_kind(&self, kind: &str) -> bool {
        self.kinds.is_empty() || self.kinds.iter().any(|wanted| wanted.eq_ignore_ascii_case(kind))
    }

    fn admits_entity(&self, payload: &Value) -> bool {
        let Some(entity) = self.entity else {
            return true;
        };

        entity_ids(payload).any(|named| named == entity)
    }
}

/// Entities a payload names in its top-level id fields.
pub fn entity_ids(payload: &Value) -> impl Iterator<Item = EntityId> + '_ {
    const ENTITY_KEYS: [&str; 6] = ["aggressor", "victim", "killer", "attacker", "target", "owner"];

    payload.as_object().into_iter().flatten().filter_map(|(key, value)| {
        let names_entity = key.ends_with("_id") || ENTITY_KEYS.contains(&key.as_str());
        if !names_entity {
            return None;
        }

        value.as_u64().and_then(|raw| u32::try_from(raw).ok()).map(EntityId::from)
    })
}

/// One decoded packet as JSON.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PacketRecord {
    /// Position in the packet stream.
    pub index: usize,
    pub clock: GameClock,

    /// The payload's variant name.
    pub kind: String,
    pub payload: Value,
}

/// What a scan covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacketScan {
    pub packets: usize,
    pub matched: usize,

    /// Set when a packet failed to parse; the scan ends there, as the rest
    /// of the stream cannot be framed.
    pub unparsed_from: Option<usize>,

    /// The visitor ended the scan.
    pub stopped: bool,
}

/// Decodes `replay`'s packets in order and passes those `filter` admits to
/// `visit` until it breaks.
pub fn scan_packets(
    replay: &ReplayFile,
    ctx: &dyn GameDataContext,
    filter: &PacketFilter,
    mut visit: impl FnMut(PacketRecord) -> ControlFlow<()>,
) -> Result<PacketScan, AnalysisError> {
    let version = replay_version(replay)?;
    let specs = ctx.entity_specs(&version)?;
    let constants = ctx.game_constants(&version)?;
    let decoder = PacketDecoder::builder()
        .version(version)
        .battle_constants(constants.battle())
        .common_constants(constants.common())
        .ships_constants(constants.ships())
        .build();

    let mut scan = PacketScan { packets: 0, matched: 0, unparsed_from: None, stopped: false };
    let mut parser = Parser::with_version(&specs, version);
    let mut remaining = replay.packet_data();
    while !remaining.is_empty() {
        let index = scan.packets;
        let Ok(packet) = parser.parse_packet(&mut remaining) else {
            scan.unparsed_from = Some(index);
            break;
        };
        scan.packets += 1;

        if !filter.admits_clock(packet.clock) {
            continue;
        }

        let decoded = decoder.decode(&packet);
        let encoded =
            serde_json::to_value(&decoded.payload).map_err(|source| AnalysisError::Encode { index, source })?;
        let (kind, payload) = split_variant(encoded);
        if !filter.admits_kind(&kind) || !filter.admits_entity(&payload) {
            continue;
        }

        scan.matched += 1;
        if visit(PacketRecord { index, clock: packet.clock, kind, payload }).is_break() {
            scan.stopped = true;
            break;
        }
    }

    Ok(scan)
}

/// Splits an externally tagged enum value into its variant name and body.
fn split_variant(value: Value) -> (String, Value) {
    match value {
        Value::String(kind) => (kind, Value::Null),
        Value::Object(fields) if fields.len() == 1 => match fields.into_iter().next() {
            Some((kind, body)) => (kind, body),
            None => (String::new(), Value::Null),
        },
        other => (String::new(), other),
    }
}

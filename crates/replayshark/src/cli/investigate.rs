//! `investigate`: prints decoded packets for reverse-engineering them.

use wows_replays::analyzer::Analyzer;
use wows_replays::analyzer::decoder::DecodedPacketPayload;
use wows_replays::analyzer::decoder::PacketDecoder;
use wows_replays::game_constants::GameConstants;
use wows_replays::packet2::Packet;
use wows_replays::packet2::PacketType;
use wows_replays::types::EntityId;
use wowsunpack::data::Version;

/// Seconds added to a packet's game clock before it is printed as
/// `hh:mm:ss`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClockOffset(pub f32);

/// Parses `hh:mm:ss+offset`: the wall time at which the game clock read
/// `offset` seconds.
pub fn parse_clock_offset(text: &str) -> Result<ClockOffset, String> {
    const EXPECTED: &str = "expected hh:mm:ss+offset";

    let (time, offset) = text.split_once('+').ok_or(EXPECTED)?;
    let offset: u32 = offset.parse().map_err(|_| EXPECTED)?;
    let parts: Vec<u32> = time.split(':').map(str::parse).collect::<Result<_, _>>().map_err(|_| EXPECTED)?;
    let [h, m, s] = parts[..] else { return Err(EXPECTED.to_owned()) };

    Ok(ClockOffset((h * 3600 + m * 60 + s) as f32 - offset as f32))
}

/// Which packets [`InvestigativePrinter`] prints.
pub struct InvestigateFilter {
    pub packet_type: Option<u32>,
    pub method: Option<String>,

    /// Applies to `method`.
    pub entity_id: Option<EntityId>,
    pub timestamp: Option<ClockOffset>,

    /// Also print each arena state's players and their ids.
    pub meta: bool,
}

/// Prints each packet `filter` keeps as JSON, one per line.
pub struct InvestigativePrinter<'a> {
    filter: InvestigateFilter,
    packet_decoder: PacketDecoder<'a>,
}

impl<'a> InvestigativePrinter<'a> {
    /// With `filter.meta`, prints the replay header first.
    pub fn new(meta: &wows_replays::ReplayMeta, filter: InvestigateFilter, game_constants: &'a GameConstants) -> Self {
        let version = Version::from_client_exe(&meta.clientVersionFromExe);
        let packet_decoder = PacketDecoder::builder()
            .version(version)
            .audit(true)
            .battle_constants(game_constants.battle())
            .common_constants(game_constants.common())
            .ships_constants(game_constants.ships())
            .build();

        if filter.meta {
            println!("{}", serde_json::to_string(&meta).expect("a replay header serializes"));
        }

        Self { filter, packet_decoder }
    }
}

impl Analyzer for InvestigativePrinter<'_> {
    fn finish(&mut self) {}

    fn process(&mut self, packet: &Packet<'_, '_>) {
        let decoded = self.packet_decoder.decode(packet);
        let filter = &self.filter;

        if filter.meta
            && let DecodedPacketPayload::OnArenaStateReceived { player_states: players, bot_states: bots, .. } =
                &decoded.payload
        {
            for player in players.iter().chain(bots.iter()) {
                match player.player_id() {
                    Some(player_id) => {
                        println!("{} {} ({:x?})", player.username(), player_id, (player_id.raw() as u32).to_le_bytes(),)
                    }
                    None => println!("{} (no player id)", player.username()),
                }
            }
        }

        if let Some(n) = filter.packet_type
            && n != decoded.packet_type.raw()
        {
            return;
        }
        if let Some(s) = filter.method.as_ref() {
            let PacketType::EntityMethod(method) = &packet.payload else { return };
            if method.method != s {
                return;
            }
            if let Some(eid) = filter.entity_id
                && method.entity_id != eid
            {
                return;
            }
        }

        let Some(ClockOffset(offset)) = filter.timestamp else {
            println!("{}", serde_json::to_string(&decoded).expect("a decoded packet serializes"));
            return;
        };

        let clock = (decoded.clock.seconds() + offset) as u32;
        let s = clock % 60;
        let clock = (clock - s) / 60;
        let m = clock % 60;
        let h = (clock - m) / 60;
        let encoded = match (&packet.payload, filter.method.is_some(), filter.packet_type.is_some()) {
            // The method filter only keeps entity method packets.
            (PacketType::EntityMethod(method), true, _) => {
                serde_json::to_string(&method).expect("a method packet serializes")
            }
            (PacketType::Unknown(x), false, true) => {
                let v: Vec<_> = x.iter().map(|n| format!("{n:02x}")).collect();
                format!("0x[{}]", v.join(","))
            }
            (_, false, true) => serde_json::to_string(&packet).expect("a packet serializes"),
            _ => serde_json::to_string(&decoded).expect("a decoded packet serializes"),
        };
        println!("{h:02}:{m:02}:{s:02}: {encoded}");
    }
}

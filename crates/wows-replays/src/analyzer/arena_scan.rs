//! Reading the match roster off a replay's stream without building a
//! controller.
//!
//! `onArenaStateReceived` carries the arena id and every participant's account
//! id and realm, which the replay metadata does not. Callers that only want
//! that (live match stats, rejecting merge candidates from another match) walk
//! to the first one and stop.

use wowsunpack::data::Version;
use wowsunpack::rpc::entitydefs::EntitySpec;

use crate::ReplayFile;
use crate::analyzer::decoder::PlayerStateData;
use crate::packet2::PacketType;
use crate::packet2::Parser;
use crate::types::ArenaId;

/// The roster an `onArenaStateReceived` packet carries, plus the arena it
/// belongs to.
#[derive(Debug, Clone)]
pub struct ArenaState {
    pub arena_id: ArenaId,
    pub players: Vec<PlayerStateData>,
}

/// Walk a replay's stream to the first `onArenaStateReceived` and read the
/// arena id and roster off it.
///
/// Only `args[0]` and `args[3]` are touched. The full decode of this method
/// also reads `args[2]`, whose parser panics on an unexpected shape, and this
/// runs against files the game is still writing.
pub fn scan_arena_state(specs: &[EntitySpec], version: Version, replay: &ReplayFile) -> Option<ArenaState> {
    let mut parser = Parser::with_version(specs, version);
    let mut remaining = replay.packet_data();
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).ok()?;
        let PacketType::EntityMethod(em) = &packet.payload else {
            continue;
        };
        if em.method != "onArenaStateReceived" {
            continue;
        }
        let Some(wowsunpack::rpc::typedefs::ArgValue::Int64(arena_id)) = em.args.first() else {
            continue;
        };
        let players = match em.args.get(3) {
            Some(wowsunpack::rpc::typedefs::ArgValue::Blob(blob)) => {
                crate::analyzer::decoder::player_states_from_blob(blob, &version)
            }
            _ => Vec::new(),
        };
        return Some(ArenaState { arena_id: ArenaId::from(*arena_id), players });
    }
    None
}

/// Walk a replay's stream to find the first `onArenaStateReceived` and
/// extract its arena id. Lets callers reject merge candidates that
/// don't belong to the same match *before* a full re-parse is kicked off.
pub fn scan_arena_id(specs: &[EntitySpec], version: Version, replay: &ReplayFile) -> Option<ArenaId> {
    scan_arena_state(specs, version, replay).map(|state| state.arena_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `scan_arena_state` is the one walk; `scan_arena_id` must be a
    /// projection of it, so a stream with no `onArenaStateReceived` has to
    /// give the same answer through both.
    #[test]
    fn an_empty_stream_yields_no_arena_state() {
        let replay = ReplayFile::from_parts(
            serde_json::from_str(EMPTY_META).expect("meta parses"),
            EMPTY_META.to_string(),
            Vec::new(),
        );

        assert!(scan_arena_state(&[], Version::default(), &replay).is_none());
        assert!(scan_arena_id(&[], Version::default(), &replay).is_none());
    }

    /// A battle in progress reaches the scan as the metadata and the packet
    /// stream read from two different files, assembled by
    /// `from_decrypted_parts`. The stream is the bare packets, so it needs no
    /// unwrapping on the way in.
    #[test]
    fn a_stream_read_apart_from_its_metadata_scans_the_same_way() {
        let replay = ReplayFile::from_decrypted_parts(EMPTY_META.as_bytes().to_vec(), Vec::new()).expect("meta parses");

        assert!(scan_arena_state(&[], Version::default(), &replay).is_none());
    }

    const EMPTY_META: &str = r#"{
        "gameMode": 7, "clientVersionFromExe": "13, 11, 0, 12668706",
        "mapDisplayName": "ocean", "mapId": 1, "clientVersionFromXml": "13, 11, 0, 12668706",
        "duration": 1200, "gameLogic": null, "name": "12x12", "scenario": "Domination",
        "playerID": 0, "vehicles": [], "playersPerTeam": 12,
        "dateTime": "28.12.2023 00:52:26", "mapName": "spaces/00_CO_ocean",
        "playerName": "Me", "scenarioConfigId": 1, "teamsCount": 2, "logic": null,
        "playerVehicle": "PFSD110-Kleber"
    }"#;
}

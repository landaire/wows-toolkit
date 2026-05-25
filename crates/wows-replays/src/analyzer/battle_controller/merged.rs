//! Multi-replay merge driver.
//!
//! [`MergedReplays`] bundles a primary replay and any number of "alt"
//! perspectives of the same match into a single [`BattleController`]. The
//! primary owns broadcast-flavoured state (chat, kills, scores, battle stage,
//! etc.); the alt replays only contribute updates to other players'
//! vehicles, so the merged view sees through fog of war without altering
//! match-wide outputs.
//!
//! Callers drive the merge with [`MergedReplays::step`]: process one packet
//! from whichever underlying replay is most behind in clock time, returning
//! the new "safe clock" (the latest moment that every active replay has
//! reached) so renderers know when it's safe to draw a frame.

use wowsunpack::data::ResourceLoader;

use crate::ReplayFile;
use crate::analyzer::Analyzer;
use crate::analyzer::decoder::DecodedPacketPayload;
use crate::analyzer::decoder::PacketDecoder;
use crate::game_constants::GameConstants;
use crate::packet2::Packet;
use crate::packet2::PacketType;
use crate::packet2::Parser;
use crate::types::ArenaId;
use crate::types::EntityId;
use crate::types::GameClock;
use crate::types::TeamId;

use super::controller::BattleController;
use super::listener::BattleControllerState;
use wowsunpack::data::Version;
use wowsunpack::rpc::entitydefs::EntitySpec;

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
/// [`BattleController`] whose [`BattleControllerState`] view the caller reads.
pub struct MergedReplays<'specs, 'res, 'data, G: ResourceLoader> {
    controller: BattleController<'res, 'data, G>,
    parsers: Vec<Parser<'specs>>,
    remainings: Vec<&'data [u8]>,
    replays: Vec<&'data ReplayFile>,
    self_teams: Vec<Option<TeamId>>,
    last_clocks: Vec<GameClock>,
    finished: Vec<bool>,
    arena_ids: Vec<Option<ArenaId>>,
    arena_validated: bool,
    total_duration: GameClock,
}

impl<'specs, 'res, 'data, G: ResourceLoader> MergedReplays<'specs, 'res, 'data, G> {
    /// Build a session for `primary` plus `merges`. Validates that every
    /// merge has the same client version as the primary, pre-scans each
    /// replay for its recording player's team id (used to tag packets going
    /// into the controller), and computes [`total_duration`] as the longest
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

        let parsers: Vec<Parser<'specs>> =
            (0..replay_count).map(|_| Parser::with_build(specs, version.build)).collect();
        let remainings: Vec<&[u8]> = replays.iter().map(|r| r.packet_data.as_slice()).collect();

        let mut self_teams = Vec::with_capacity(replay_count);
        let mut total_duration = GameClock(0.0);
        for r in &replays {
            self_teams.push(scan_self_team(specs, game_constants, version, r));
            total_duration = GameClock(total_duration.0.max(scan_last_clock(specs, version, r).0));
        }

        let controller = BattleController::new(&primary.meta, game_params, Some(game_constants));

        Ok(Self {
            controller,
            parsers,
            remainings,
            replays,
            self_teams,
            last_clocks: vec![GameClock(0.0); replay_count],
            finished: vec![false; replay_count],
            arena_ids: vec![None; replay_count],
            arena_validated: replay_count <= 1,
            total_duration,
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

    /// `true` once every replay's stream has been exhausted.
    pub fn is_done(&self) -> bool {
        self.finished.iter().all(|f| *f)
    }

    /// Read-only access to the merged controller state. Renderers call this
    /// between [`step`](Self::step) calls to inspect the current merged view.
    pub fn controller(&self) -> &BattleController<'res, 'data, G> {
        &self.controller
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

    /// Drive one packet from the most-behind replay into the controller.
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

        // Only tag the controller with a source team when we're actually
        // merging multiple replays; single-replay processing should behave
        // exactly as it does without any merger involved.
        if self.replays.len() > 1 {
            self.controller.set_source_team(self.self_teams[idx]);
        }
        if is_primary || forward_secondary_packet(&self.controller, &packet) {
            self.controller.process(&packet);
        }

        // Cheap second pass to harvest the arena id without re-routing
        // through the controller; the routing filter would drop the
        // secondaries' onArenaStateReceived calls.
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

    /// Finalize the merged controller (forwards to
    /// [`Analyzer::finish`](crate::analyzer::Analyzer::finish)).
    pub fn finish(&mut self) {
        self.controller.finish();
    }

    /// Mutable access to the merged controller, for `encoder.finish` and the
    /// like that need `&mut`.
    pub fn controller_mut(&mut self) -> &mut BattleController<'res, 'data, G> {
        &mut self.controller
    }

    /// Consume the session and return the underlying merged controller.
    /// Callers that want a [`crate::analyzer::battle_controller::BattleReport`]
    /// should call this then [`BattleController::build_report`].
    pub fn into_controller(self) -> BattleController<'res, 'data, G> {
        self.controller
    }

    /// First-validated arena id, available once every replay has emitted its
    /// `onArenaStateReceived` packet.
    pub fn arena_id(&self) -> Option<ArenaId> {
        self.arena_ids[0]
    }
}

/// Walk a replay's stream to find the first `onArenaStateReceived` and
/// return the team id whose username matches the replay meta's
/// `playerName`. Returns `None` if no match is found.
fn scan_self_team(
    specs: &[EntitySpec],
    game_constants: &GameConstants,
    version: Version,
    replay: &ReplayFile,
) -> Option<TeamId> {
    let mut parser = Parser::with_build(specs, version.build);
    let decoder = PacketDecoder::builder()
        .version(version)
        .battle_constants(game_constants.battle())
        .common_constants(game_constants.common())
        .ships_constants(game_constants.ships())
        .build();
    let mut remaining = &replay.packet_data[..];
    let player_name = replay.meta.playerName.as_str();
    while !remaining.is_empty() {
        let packet = parser.parse_packet(&mut remaining).ok()?;
        let decoded = decoder.decode(&packet);
        if let DecodedPacketPayload::OnArenaStateReceived { player_states, bot_states, .. } = decoded.payload {
            return player_states
                .iter()
                .chain(bot_states.iter())
                .find(|p| p.username() == player_name)
                .map(|p| TeamId::from(p.team_id()));
        }
    }
    None
}

/// Walk a replay's stream and return the largest packet clock observed.
fn scan_last_clock(specs: &[EntitySpec], version: Version, replay: &ReplayFile) -> GameClock {
    let mut parser = Parser::with_build(specs, version.build);
    let mut remaining = &replay.packet_data[..];
    let mut last = GameClock(0.0);
    while !remaining.is_empty() {
        match parser.parse_packet(&mut remaining) {
            Ok(p) => last = GameClock(p.clock.0.max(last.0)),
            Err(_) => break,
        }
    }
    last
}

/// Decide whether a packet from a non-primary replay should be fed to the
/// shared controller. Forward iff the packet updates state on a Vehicle
/// entity that the controller already tracks, plus a short allow-list of
/// Avatar-method calls that carry cross-perspective info about *other* ships
/// (artillery in flight, plane spawns, minimap vision, etc.).
fn forward_secondary_packet<G: ResourceLoader>(
    controller: &BattleController<'_, '_, G>,
    packet: &Packet<'_, '_>,
) -> bool {
    match &packet.payload {
        PacketType::Position(p) => is_known_vehicle(controller, p.pid),
        PacketType::EntityProperty(ep) => is_known_vehicle(controller, ep.entity_id),
        PacketType::PropertyUpdate(pu) => is_known_vehicle(controller, pu.entity_id),
        PacketType::EntityMethod(em) => is_cross_perspective_method(em.method),
        // Everything else (lifecycle, recording-player setup, primary's view,
        // match-wide one-shots, server timing) is owned by primary.
        _ => false,
    }
}

fn is_known_vehicle<G: ResourceLoader>(controller: &BattleController<'_, '_, G>, id: EntityId) -> bool {
    controller.entities_by_id().get(&id).and_then(|e| e.vehicle_ref().map(|_| ())).is_some()
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
    )
}

//! Turning a parsed battle into the rows the replay index stores.
//!
//! Reads [`NormalizedBattleReport`], which both front ends already build, so
//! the index holds the same figures the tables show and neither app can drift
//! from the other on what a replay meant. The damage fallbacks live on
//! [`NormalizedPlayer`] itself, so a row here and a cell on screen come from
//! one place.

use std::path::PathBuf;

use jiff::Timestamp;
use wows_replay_insights::battle_report::ConnectionNote;
use wows_replay_insights::battle_report::NormalizedBattleReport;
use wows_replay_insights::battle_report::NormalizedPlayer;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::types::ArenaId;
use wows_replays::types::GameParamId;
use wows_replays::types::Relation;
use wows_toolkit_config::index::rows::IndexedVehicleRow;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::ObjectiveMatch;
use wows_toolkit_config::index::rows::ReplayRecord;
use wows_toolkit_config::index::rows::SourceId;
use wows_toolkit_config::index::rows::VehicleRelation;

/// Whether the constants file matches the build the replay was recorded on.
///
/// Results resolved against the wrong constants are wrong rather than merely
/// stale, so a mismatch blanks them instead of storing them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstantsFit {
    /// The constants are for the build the replay was recorded on.
    Matched,
    /// They are not, so the keys the results are read through may have moved.
    Mismatched,
}

/// What a replay carries that its normalized report does not.
pub struct IndexContext {
    pub arena_id: ArenaId,
    /// The game mode's numeric id, when the table recognises it. `None` for
    /// one it does not, which reads the same as a row indexed before the
    /// column existed; the re-index hint counts exactly those.
    pub game_mode_id: Option<i32>,
    /// The build the replay was recorded on. `None` when the version carries
    /// no build number.
    pub version_build: Option<u32>,
    pub source_id: SourceId,
    pub replay_path: PathBuf,
    /// The file's modification time, for noticing a replay rewritten in
    /// place. `None` when it could not be read.
    pub file_mtime: Option<i64>,
    /// The ship the recording player was in.
    pub self_ship_id: Option<GameParamId>,
    /// Whether the server results are still to come, which is the ordinary
    /// state of a replay of a battle that has only just ended.
    pub results_pending: bool,
    pub indexed_at: Timestamp,
    pub fit: ConstantsFit,
}

/// The three row sets one replay produces.
pub struct MappedRows {
    pub objective: ObjectiveMatch,
    pub vehicles: Vec<IndexedVehicleRow>,
    pub record: ReplayRecord,
}

/// The verdict as the index stores it.
pub fn outcome_from(result: Option<&BattleResult>) -> MatchOutcome {
    match result {
        Some(BattleResult::Win(_)) => MatchOutcome::Win,
        Some(BattleResult::Loss(_)) => MatchOutcome::Loss,
        Some(BattleResult::Draw) => MatchOutcome::Draw,
        None => MatchOutcome::Unknown,
    }
}

/// Which side a player was on, as the index stores it.
pub fn relation_from(rel: Relation) -> VehicleRelation {
    if rel.is_self() {
        VehicleRelation::SelfPlayer
    } else if rel.is_enemy() {
        VehicleRelation::Enemy
    } else {
        VehicleRelation::Ally
    }
}

/// Whether a player dropped mid-battle while still alive.
///
/// A drop that came with a death is how a sinking ends, not a connection
/// fault, and a player who never joined is a no-show rather than a
/// disconnect; [`ConnectionNote`] already draws both distinctions.
pub fn disconnected(player: &NormalizedPlayer) -> bool {
    matches!(player.connection, Some(ConnectionNote::Interrupted { .. }))
}

/// Builds every row one replay contributes.
pub fn map_rows(report: &NormalizedBattleReport, context: &IndexContext) -> MappedRows {
    let arena_id = context.arena_id;
    let metadata = &report.metadata;

    let objective = ObjectiveMatch {
        arena_id,
        timestamp: metadata.timestamp,
        map: metadata.map.clone(),
        game_mode: metadata.game_mode.clone(),
        game_mode_id: context.game_mode_id,
        game_type: metadata.game_type.clone(),
        match_group: metadata.match_group.clone(),
        version_build: context.version_build,
    };

    // Bots are not people to meet again, so the index does not carry them.
    let humans = || report.players.iter().filter(|player| !player.is_bot);

    let vehicles: Vec<IndexedVehicleRow> = humans()
        .map(|player| IndexedVehicleRow {
            arena_id,
            account_id: player.db_id,
            // The raw username, not the display name: a bot's translated name
            // is not an identity, and a human's two are the same anyway.
            player_name: player.name.clone(),
            clan: player.clan.clone(),
            realm: player.realm.clone(),
            ship_id: player.ship_id,
            ship_index: player.ship_index.clone(),
            ship_name: player.ship_name.clone(),
            nation: player.ship_nation.clone(),
            species: format!("{:?}", player.ship_class),
            // A ship whose vehicle ref carries no tier stores zero, which is
            // what this column has always held for one.
            tier: player.ship_tier.unwrap_or(0),
            relation: relation_from(player.relation),
            division_id: player.division_id.map(i64::from),
            survived: player.survived,
            damage: player.server_results.as_ref().and_then(|results| results.damage),
            kills: player.server_results.as_ref().and_then(|results| results.kills),
            spotting: player.spotting_damage(),
            potential: player.potential_damage(),
            received: player.server_results.as_ref().map(|results| results.received_damage),
            pr: player.personal_rating.as_ref().map(|rating| rating.pr),
            is_test_ship: player.is_test_ship,
            disconnected: Some(disconnected(player)),
            is_stream_sniper: None,
            sniper_twitch_login: None,
        })
        .collect();

    let self_player = humans().find(|player| player.is_self);
    let record = ReplayRecord {
        arena_id,
        source_id: context.source_id,
        replay_path: context.replay_path.clone(),
        file_mtime: context.file_mtime,
        outcome: outcome_from(metadata.resolved_battle_result().as_ref()),
        self_account_id: self_player.map(|player| player.db_id),
        self_ship_id: context.self_ship_id,
        self_survived: self_player.and_then(|player| player.survived),
        self_damage: self_player.and_then(|player| player.server_results.as_ref().and_then(|r| r.damage)),
        self_kills: self_player.and_then(|player| player.server_results.as_ref().and_then(|r| r.kills)),
        self_pr: self_player.and_then(|player| player.personal_rating.as_ref().map(|rating| rating.pr)),
        results_available: !context.results_pending,
        indexed_at: context.indexed_at,
    };

    let mut rows = MappedRows { objective, vehicles, record };
    if context.fit == ConstantsFit::Mismatched {
        suppress_untrusted_results(&mut rows);
    }
    rows
}

/// Blanks every figure that came from the server results, and marks the
/// record as carrying none.
///
/// Used when the constants do not match the build: the keys the results are
/// read through moved, so the numbers would be wrong rather than missing.
pub fn suppress_untrusted_results(rows: &mut MappedRows) {
    for vehicle in &mut rows.vehicles {
        vehicle.damage = None;
        vehicle.kills = None;
        vehicle.spotting = None;
        vehicle.potential = None;
        vehicle.received = None;
        vehicle.pr = None;
    }
    rows.record.self_damage = None;
    rows.record.self_kills = None;
    rows.record.self_pr = None;
    rows.record.results_available = false;
}

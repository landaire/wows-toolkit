//! Map a parsed `Replay` into replay-index rows (objective match, roster, record).
//! Mirrors `PerGameStat::from_replay` and `Vehicle::new` for field extraction.

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use jiff::Timestamp;
use rootcause::Report;
use rootcause::prelude::*;
use sqlx::SqlitePool;
use tokio::runtime::Runtime;
use tracing::warn;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::analyzer::battle_controller::ConnectionChangeKind;
use wows_replays::types::Relation;

use crate::db::index::query;
use crate::db::index::rows::IndexError;
use crate::db::index::rows::IndexedVehicleRow;
use crate::db::index::rows::MatchOutcome;
use crate::db::index::rows::ObjectiveMatch;
use crate::db::index::rows::ReplayRecord;
use crate::db::index::rows::SourceId;
use crate::db::index::rows::VehicleRelation;
use crate::ui::replay_parser::PlayerReport;
use crate::ui::replay_parser::Replay;

pub fn outcome_from(result: Option<&BattleResult>) -> MatchOutcome {
    match result {
        Some(BattleResult::Win(_)) => MatchOutcome::Win,
        Some(BattleResult::Loss(_)) => MatchOutcome::Loss,
        Some(BattleResult::Draw) => MatchOutcome::Draw,
        None => MatchOutcome::Unknown,
    }
}

pub fn relation_from(rel: Relation) -> VehicleRelation {
    if rel.is_self() {
        VehicleRelation::SelfPlayer
    } else if rel.is_enemy() {
        VehicleRelation::Enemy
    } else {
        VehicleRelation::Ally
    }
}

/// Whether the player had a mid-match disconnect: a `Disconnected`
/// connection-change event whose `had_death_event` is false. An empty history
/// (or one with only reconnects, or a disconnect that coincided with death) is
/// not a disconnect. This intentionally does not cover the UI's separate
/// "never connected" / no-show case (a roster player who never spawned) --
/// that is out of scope for this boolean.
pub fn player_disconnected(player: &wows_replays::analyzer::battle_controller::Player) -> bool {
    player
        .connection_change_info()
        .iter()
        .any(|change| ConnectionChangeKind::Disconnected == change.event_kind() && !change.had_death_event())
}

pub struct MappedRows {
    pub objective: ObjectiveMatch,
    pub vehicles: Vec<IndexedVehicleRow>,
    pub record: ReplayRecord,
}

/// Build index rows from a parsed replay. Returns `None` if the reports needed
/// are not present (unparsed replay).
pub fn map_rows(replay: &Replay, source_id: SourceId, indexed_at: Timestamp) -> Option<MappedRows> {
    let ui_report = replay.ui_report.as_ref()?;
    let battle_report = replay.battle_report.as_ref()?;

    let arena_id = battle_report.arena_id();
    let version = battle_report.version();
    let objective = ObjectiveMatch {
        arena_id,
        timestamp: ui_report.match_timestamp(),
        map: battle_report.map_name().to_string(),
        game_mode: battle_report.game_mode().to_string(),
        game_type: battle_report.game_type().to_string(),
        match_group: battle_report.match_group().to_string(),
        version_build: version.build_number(),
    };

    let mut vehicles = Vec::new();
    let mut self_row: Option<&PlayerReport> = None;
    for report in ui_report.player_reports() {
        let player = report.player();
        let state = player.initial_state();
        if state.is_bot() {
            continue;
        }
        if report.relation().is_self() {
            self_row = Some(report);
        }
        let vehicle_param = player.vehicle();
        let species =
            vehicle_param.species().and_then(|s| s.known().cloned()).map(|s| format!("{s:?}")).unwrap_or_default();
        vehicles.push(IndexedVehicleRow {
            arena_id,
            account_id: state.db_id(),
            player_name: state.username().to_string(),
            clan: state.clan().to_string(),
            realm: state.realm().map(str::to_owned),
            ship_id: vehicle_param.id(),
            ship_index: vehicle_param.index().to_string(),
            ship_name: report.ship_name().to_string(),
            nation: vehicle_param.nation().to_string(),
            species,
            tier: vehicle_param.data().vehicle_ref().map(|v| v.level()).unwrap_or(0),
            relation: relation_from(report.relation()),
            division_id: (state.division_id() > 0).then_some(state.division_id()),
            survived: report.vehicle().map(|v| v.death_info().is_none()),
            damage: report.actual_damage(),
            kills: report.kills(),
            spotting: report.spotting_damage(),
            potential: report.potential_damage(),
            received: report.received_damage(),
            pr: report.personal_rating().map(|pr| pr.pr),
            is_test_ship: report.is_test_ship(),
            disconnected: Some(player_disconnected(player)),
            is_stream_sniper: None,
            sniper_twitch_login: None,
        });
    }

    let self_ship_id = replay.player_vehicle().map(|v| v.shipId);
    let record = ReplayRecord {
        arena_id,
        source_id,
        replay_path: replay.source_path.clone().unwrap_or_default(),
        file_mtime: replay
            .source_path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64),
        outcome: outcome_from(replay.battle_result().as_ref()),
        self_account_id: self_row.map(|r| r.player().initial_state().db_id()),
        self_ship_id,
        self_survived: self_row.and_then(|r| r.vehicle().map(|v| v.death_info().is_none())),
        self_damage: self_row.and_then(|r| r.actual_damage()),
        self_kills: self_row.and_then(|r| r.kills()),
        self_pr: self_row.and_then(|r| r.personal_rating().map(|pr| pr.pr)),
        results_available: !replay.battle_results_are_pending(),
        indexed_at,
    };

    Some(MappedRows { objective, vehicles, record })
}

/// Seconds before/after a match's start within which a Twitch chat
/// observation is considered relevant to that match, mirroring
/// `TwitchState::player_is_potential_stream_sniper`'s -2..+20 minute window.
const SNIPER_WINDOW_BEFORE_SECS: i64 = 2 * 60;
const SNIPER_WINDOW_AFTER_SECS: i64 = 20 * 60;

/// Applies stream-sniper flags to `vehicles` from a match's in-window Twitch
/// chat `observations` (login, seen_at unix seconds; already filtered to the
/// match's window by the caller).
///
/// If `observations` is empty, every row is left as-is (`None`, meaning
/// "unknown": no Twitch data was available for this match's window) -- this
/// is never turned into a `Some(false)` sentinel. Otherwise, each real player
/// (bots, which carry `AccountId(0)`, are always left `None` since they have
/// no Twitch-matchable account) gets `is_stream_sniper = Some(true)` plus the
/// matching login if any observation's login fuzzy-matches their player
/// name via `login_matches_ign`, or `Some(false)` if detection ran and found
/// no match.
pub fn apply_sniper_flags(vehicles: &mut [IndexedVehicleRow], observations: &[(String, i64)]) {
    if observations.is_empty() {
        return;
    }
    for vehicle in vehicles.iter_mut() {
        if vehicle.account_id.raw() == 0 {
            continue;
        }
        match observations.iter().find(|(login, _)| crate::twitch::login_matches_ign(login, &vehicle.player_name)) {
            Some((login, _)) => {
                vehicle.is_stream_sniper = Some(true);
                vehicle.sniper_twitch_login = Some(login.clone());
            }
            None => {
                vehicle.is_stream_sniper = Some(false);
                vehicle.sniper_twitch_login = None;
            }
        }
    }
}

/// Counts the index writes this process has made.
///
/// Every write goes through [`write_index`] on one of this process's own
/// background parser threads, so a process-local counter sees all of them and
/// costs a query nothing. A UI cache fed by index queries holds the value it
/// last read and rebuilds when it moves, which is what lets background indexing
/// invalidate a cache that no user action touched.
static INDEX_GENERATION: AtomicU64 = AtomicU64::new(0);

/// How many times index rows have been written since launch.
///
/// Relaxed: the rows behind a bump are committed to SQLite before it happens, so
/// a reader that sees the new value queries data that is already there, and one
/// that reads the old value picks the change up on a later frame.
pub fn index_generation() -> u64 {
    INDEX_GENERATION.load(Ordering::Relaxed)
}

pub async fn write_index(pool: &SqlitePool, rows: &MappedRows) -> Result<(), IndexError> {
    query::upsert_match(pool, &rows.objective).await?;
    query::upsert_vehicles(pool, &rows.vehicles).await?;
    query::upsert_record(pool, &rows.record).await?;
    INDEX_GENERATION.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Map, enrich with stream-sniper detection, and persist on the current
/// (background) thread. Shared by both the live indexing path and the
/// on-demand reindex/backfill path, so both benefit from persisted Twitch
/// observations. Best-effort: errors are logged and swallowed so indexing
/// never destabilizes the parser thread.
pub fn index_replay_blocking(rt: &Runtime, pool: &SqlitePool, replay: &Replay, source_id: SourceId, now: Timestamp) {
    if let Err(e) = index_replay_reporting(rt, pool, replay, source_id, now) {
        warn!("failed to index replay: {e}");
    }
}

/// [`index_replay_blocking`] with the reason nothing was written returned
/// rather than logged, for callers that report how many replays did not index.
pub fn index_replay_reporting(
    rt: &Runtime,
    pool: &SqlitePool,
    replay: &Replay,
    source_id: SourceId,
    now: Timestamp,
) -> Result<(), Report> {
    let Some(mut rows) = map_rows(replay, source_id, now) else {
        return Err(report!("replay carries no parsed report to index"));
    };

    let match_ts = rows.objective.timestamp.as_second();
    let window_start = match_ts - SNIPER_WINDOW_BEFORE_SECS;
    let window_end = match_ts + SNIPER_WINDOW_AFTER_SECS;

    rt.block_on(async {
        match query::observations_in_window(pool, window_start, window_end).await {
            Ok(observations) => apply_sniper_flags(&mut rows.vehicles, &observations),
            Err(e) => warn!("failed to fetch twitch observations for sniper detection: {e}"),
        }
        write_index(pool, &rows).await
    })
    .map_err(|e| report!("failed to write replay index rows: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::index::rows::MatchOutcome;
    use crate::db::index::rows::VehicleRelation;
    use wows_replays::analyzer::battle_controller::BattleResult;
    use wows_replays::types::AccountId;
    use wows_replays::types::ArenaId;
    use wows_replays::types::GameParamId;
    use wows_replays::types::Relation;

    fn vehicle_row(account_id: i64, player_name: &str) -> IndexedVehicleRow {
        IndexedVehicleRow {
            arena_id: ArenaId::new(1),
            account_id: AccountId(account_id),
            player_name: player_name.to_string(),
            clan: String::new(),
            realm: None,
            ship_id: GameParamId::from(1u64),
            ship_index: "PJSD018".into(),
            ship_name: "Harugumo".into(),
            nation: "japan".into(),
            species: "Destroyer".into(),
            tier: 10,
            relation: VehicleRelation::Ally,
            division_id: None,
            survived: Some(true),
            damage: Some(0),
            kills: Some(0),
            spotting: Some(0),
            potential: Some(0),
            received: Some(0),
            pr: None,
            is_test_ship: false,
            disconnected: Some(false),
            is_stream_sniper: None,
            sniper_twitch_login: None,
        }
    }

    #[test]
    fn apply_sniper_flags_empty_observations_leaves_all_rows_none() {
        let mut vehicles = vec![vehicle_row(7, "Player1"), vehicle_row(8, "Player2")];
        apply_sniper_flags(&mut vehicles, &[]);
        assert_eq!(vehicles[0].is_stream_sniper, None);
        assert_eq!(vehicles[0].sniper_twitch_login, None);
        assert_eq!(vehicles[1].is_stream_sniper, None);
        assert_eq!(vehicles[1].sniper_twitch_login, None);
    }

    #[test]
    fn apply_sniper_flags_matching_login_flags_true_with_login() {
        let mut vehicles = vec![vehicle_row(7, "Player1")];
        let observations = vec![("Player1".to_string(), 1000)];
        apply_sniper_flags(&mut vehicles, &observations);
        assert_eq!(vehicles[0].is_stream_sniper, Some(true));
        assert_eq!(vehicles[0].sniper_twitch_login, Some("Player1".to_string()));
    }

    #[test]
    fn apply_sniper_flags_non_matching_real_player_flags_false() {
        let mut vehicles = vec![vehicle_row(7, "Player1")];
        let observations = vec![("CompletelyDifferent".to_string(), 1000)];
        apply_sniper_flags(&mut vehicles, &observations);
        assert_eq!(vehicles[0].is_stream_sniper, Some(false));
        assert_eq!(vehicles[0].sniper_twitch_login, None);
    }

    #[test]
    fn apply_sniper_flags_bot_left_none_even_with_matching_observation() {
        // A bot carries AccountId(0) and has no real Twitch-matchable account.
        let mut vehicles = vec![vehicle_row(0, "Player1")];
        let observations = vec![("Player1".to_string(), 1000)];
        apply_sniper_flags(&mut vehicles, &observations);
        assert_eq!(vehicles[0].is_stream_sniper, None);
        assert_eq!(vehicles[0].sniper_twitch_login, None);
    }

    #[test]
    fn apply_sniper_flags_mixed_roster() {
        let mut vehicles = vec![
            vehicle_row(7, "Player1"),  // matches
            vehicle_row(8, "ZZZZZZZ"),  // no match
            vehicle_row(0, "AnyBot99"), // bot, skipped
        ];
        let observations = vec![("Player1".to_string(), 1000)];
        apply_sniper_flags(&mut vehicles, &observations);
        assert_eq!(vehicles[0].is_stream_sniper, Some(true));
        assert_eq!(vehicles[0].sniper_twitch_login, Some("Player1".to_string()));
        assert_eq!(vehicles[1].is_stream_sniper, Some(false));
        assert_eq!(vehicles[1].sniper_twitch_login, None);
        assert_eq!(vehicles[2].is_stream_sniper, None);
        assert_eq!(vehicles[2].sniper_twitch_login, None);
    }

    #[test]
    fn outcome_maps_all_variants() {
        assert_eq!(outcome_from(None), MatchOutcome::Unknown);
        assert_eq!(outcome_from(Some(&BattleResult::Draw)), MatchOutcome::Draw);
        // Win/Loss carry an i8 team_id payload, which has a Default impl.
        assert_eq!(outcome_from(Some(&BattleResult::Win(Default::default()))), MatchOutcome::Win);
        assert_eq!(outcome_from(Some(&BattleResult::Loss(Default::default()))), MatchOutcome::Loss);
    }

    #[test]
    fn relation_maps_self_ally_enemy() {
        // Relation has no self_player()/enemy() constructors (see
        // wows-core/src/game_types.rs); it is a raw PLAYER_RELATION value where
        // 0 = self, 1 = ally, 2 = enemy.
        assert_eq!(relation_from(Relation::new(0)), VehicleRelation::SelfPlayer);
        assert_eq!(relation_from(Relation::new(1)), VehicleRelation::Ally);
        assert_eq!(relation_from(Relation::new(2)), VehicleRelation::Enemy);
    }
}

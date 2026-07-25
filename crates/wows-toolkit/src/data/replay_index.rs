//! Map a parsed `Replay` into replay-index rows (objective match, roster, record).
//! Mirrors `PerGameStat::from_replay` and `Vehicle::new` for field extraction.

use jiff::Timestamp;
use wows_replays::analyzer::battle_controller::BattleResult;
use wows_replays::types::Relation;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::index::rows::MatchOutcome;
    use crate::db::index::rows::VehicleRelation;
    use wows_replays::analyzer::battle_controller::BattleResult;
    use wows_replays::types::Relation;

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

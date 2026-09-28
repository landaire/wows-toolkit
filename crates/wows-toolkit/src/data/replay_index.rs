//! Map a parsed `Replay` into replay-index rows (objective match, roster, record).
//! Mirrors `PerGameStat::from_replay` and `Vehicle::new` for field extraction.

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use jiff::Timestamp;
use parking_lot::RwLock;
use rootcause::Report;
use rootcause::prelude::*;
use sqlx::SqlitePool;
use tokio::runtime::Runtime;
use tracing::warn;
use wows_replays::analyzer::battle_controller::ConnectionChangeKind;
use wows_toolkit_viewmodel::index_rows::ConstantsFit as SharedFit;
use wows_toolkit_viewmodel::index_rows::IndexContext;

use crate::data::constants::ConstantsFit;
use crate::db::index::query;
use crate::db::index::rows::IndexError;
use crate::db::index::rows::IndexWriteMode;
use crate::db::index::rows::IndexedVehicleRow;
use crate::db::index::rows::MatchOutcome;
use crate::db::index::rows::PrInputs;
use crate::db::index::rows::PrRepair;
use crate::db::index::rows::ResultsWrite;
use crate::db::index::rows::SourceId;
use crate::ui::replay_parser::Replay;
use crate::util::personal_rating::PersonalRatingData;

pub use wows_toolkit_viewmodel::index_rows::outcome_from;

pub use wows_toolkit_viewmodel::index_rows::relation_from;

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

pub use wows_toolkit_viewmodel::index_rows::MappedRows;

/// Build index rows from a parsed replay. Returns `None` if the reports needed
/// are not present (unparsed replay).
/// Builds every row a replay contributes.
///
/// The mapping itself is `wows_toolkit_viewmodel::index_rows`, over the
/// normalized report both this app and the GPUI port build, so the two index
/// a replay identically. What is assembled here is the part that is this
/// app's own: where the file is and what the parse made of it.
///
/// `None` when the replay has not been parsed, which is not an indexable
/// state.
pub fn map_rows(replay: &Replay, source_id: SourceId, indexed_at: Timestamp, fit: ConstantsFit) -> Option<MappedRows> {
    let ui_report = replay.ui_report.as_ref()?;
    let battle_report = replay.battle_report.as_ref()?;

    let arena_id = battle_report.arena_id();

    // `game_mode_id()` widens to `Recognized<GameMode, u32>` specifically so an
    // id the table does not cover still carries its true value (see
    // `report_game_mode_id`'s doc comment). `.known()` below throws that value
    // away the moment it turns `Unknown(raw)` into `None`, and the resulting
    // NULL is then indistinguishable from "indexed before this column
    // existed" -- exactly what `matches_missing_game_mode_count` counts, and
    // exactly what the search UI's re-index hint tells the user to fix by
    // re-indexing. A raw id nothing here logs would keep the hint pointing at
    // a repair that can never move the count. This does not change the NULL
    // outcome itself: storing the raw id would defeat the typed column.
    let game_mode = battle_report.game_mode_id();
    if let Some(raw) = game_mode.unknown() {
        warn!("replay for arena {arena_id:?}: unrecognised game mode id {raw}; game_mode_id will stay unset");
    }

    if fit == ConstantsFit::Mismatched {
        // Expected for old or newly-released builds without a matching
        // constants file yet; logged so a stats-free listing has an
        // explanation to point to instead of looking like silent data loss.
        tracing::info!(
            build = ?battle_report.version().build_number(),
            ?fit,
            "suppressing results for arena {arena_id:?}: constants do not match this build"
        );
    }

    let context = IndexContext {
        arena_id,
        game_mode_id: game_mode.known().map(|mode| mode.id()),
        version_build: battle_report.version().build_number(),
        source_id,
        replay_path: replay.source_path.clone().unwrap_or_default(),
        file_mtime: replay
            .source_path
            .as_ref()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64),
        self_ship_id: replay.player_vehicle().map(|v| v.shipId),
        results_pending: replay.battle_results_are_pending(),
        indexed_at,
        fit: match fit {
            ConstantsFit::Matched => SharedFit::Matched,
            ConstantsFit::Mismatched => SharedFit::Mismatched,
        },
    };

    Some(wows_toolkit_viewmodel::index_rows::map_rows(ui_report.normalized(), &context))
}

/// Blank every value that came from the server results blob, and mark the
/// record as carrying no results.
///
/// Results are decoded through per-build lookup tables, so constants that do
/// not belong to the build read the wrong indices and produce numbers that look
/// ordinary. Everything left untouched here comes from the packet stream or
/// game params and is unaffected: roster, ship, map, mode, survival, outcome.
///
/// The blanked values are what these rows carry to the write path, but they are
/// not necessarily what ends up stored: `write_index` is called with
/// `ResultsWrite::Keep` for a mismatched pass, so on a brand-new row the blanks
/// here are what gets recorded (there is nothing earlier to preserve), while on
/// an existing row the upsert leaves what an earlier trusted pass stored.
pub use wows_toolkit_viewmodel::index_rows::suppress_untrusted_results;

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

pub async fn write_index(
    pool: &SqlitePool,
    rows: &MappedRows,
    mode: IndexWriteMode,
    results: ResultsWrite,
) -> Result<(), IndexError> {
    query::upsert_match_with_mode(pool, &rows.objective, mode).await?;
    query::upsert_vehicles_with_mode(pool, &rows.vehicles, mode, results).await?;
    query::upsert_record_with_mode(pool, &rows.record, mode, results).await?;
    INDEX_GENERATION.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// The single-battle rating for one stored row's worth of stats, or `None` when
/// the expected values carry nothing for that ship.
///
/// Mirrors `UiReport::populate_personal_ratings`: one battle, damage and frags
/// as recorded, and a win counted from the battle's outcome. Keeping the two in
/// one shape is what stops a repaired row from disagreeing with a freshly
/// indexed one.
pub fn single_battle_pr(pr_data: &PersonalRatingData, inputs: &PrInputs) -> Option<f64> {
    wows_replay_insights::personal_rating::rate_single_battle(
        pr_data,
        inputs.ship_id,
        inputs.damage,
        inputs.kills,
        inputs.is_win,
    )
    .map(|result| result.pr)
}

/// Fill the ratings a freshly mapped set of rows is missing, before it is
/// written. `map_rows` carries whatever the UI report already computed; a
/// report built while the expected values were still loading carries nothing,
/// and without this the row would be written NULL and wait for a later repair
/// against a later set of expected values.
///
/// A row that already has a rating is never touched.
fn fill_missing_pr(rows: &mut MappedRows, pr_data: &PersonalRatingData) {
    if !pr_data.is_loaded() {
        return;
    }
    let is_win = rows.record.outcome == MatchOutcome::Win;
    if rows.record.self_pr.is_none()
        && let Some(ship_id) = rows.record.self_ship_id
        && let Some(damage) = rows.record.self_damage
    {
        let inputs = PrInputs { ship_id, damage, kills: rows.record.self_kills.unwrap_or(0), is_win };
        rows.record.self_pr = single_battle_pr(pr_data, &inputs);
    }
    for vehicle in &mut rows.vehicles {
        if vehicle.pr.is_some() {
            continue;
        }
        let Some(damage) = vehicle.damage else {
            continue;
        };
        let inputs = PrInputs { ship_id: vehicle.ship_id, damage, kills: vehicle.kills.unwrap_or(0), is_win };
        vehicle.pr = single_battle_pr(pr_data, &inputs);
    }
}

/// Map, enrich with stream-sniper detection, and persist on the current
/// (background) thread. Shared by both the live indexing path and the
/// on-demand reindex/backfill path, so both benefit from persisted Twitch
/// observations. Best-effort: errors are logged and swallowed so indexing
/// never destabilizes the parser thread.
#[allow(clippy::too_many_arguments)]
pub fn index_replay_blocking(
    rt: &Runtime,
    pool: &SqlitePool,
    replay: &Replay,
    source_id: SourceId,
    now: Timestamp,
    pr_data: &RwLock<PersonalRatingData>,
    fit: ConstantsFit,
    mode: IndexWriteMode,
) {
    if let Err(e) = index_replay_reporting(rt, pool, replay, source_id, now, pr_data, fit, mode) {
        warn!("failed to index replay: {e}");
    }
}

/// [`index_replay_blocking`] with the reason nothing was written returned
/// rather than logged, for callers that report how many replays did not index.
#[allow(clippy::too_many_arguments)]
pub fn index_replay_reporting(
    rt: &Runtime,
    pool: &SqlitePool,
    replay: &Replay,
    source_id: SourceId,
    now: Timestamp,
    pr_data: &RwLock<PersonalRatingData>,
    fit: ConstantsFit,
    mode: IndexWriteMode,
) -> Result<(), Report> {
    let Some(mut rows) = map_rows(replay, source_id, now, fit) else {
        return Err(report!("replay carries no parsed report to index"));
    };
    fill_missing_pr(&mut rows, &pr_data.read());

    let match_ts = rows.objective.timestamp.as_second();
    let window_start = match_ts - SNIPER_WINDOW_BEFORE_SECS;
    let window_end = match_ts + SNIPER_WINDOW_AFTER_SECS;

    let results = match fit {
        ConstantsFit::Matched => ResultsWrite::Store,
        ConstantsFit::Mismatched => ResultsWrite::Keep,
    };

    rt.block_on(async {
        match query::observations_in_window(pool, window_start, window_end).await {
            Ok(observations) => apply_sniper_flags(&mut rows.vehicles, &observations),
            Err(e) => warn!("failed to fetch twitch observations for sniper detection: {e}"),
        }
        write_index(pool, &rows, mode, results).await
    })
    .map_err(|e| report!("failed to write replay index rows: {e}"))
}

/// Give a rating to every stored row that has none, and leave every row that
/// has one exactly as it is.
///
/// A rating is a point-in-time value: expected values drift, so recomputing a
/// stored one would make the same battle report a different number month to
/// month and quietly change what a saved `pr < 800` search matches. Only the
/// rows that never got a number are touched, and those are stamped with
/// today's expected values because no earlier set survives to stamp them with.
///
/// Returns how many rows were filled. The two reads scan `replay_record` and
/// `indexed_vehicle`, so this belongs on the runtime and not on a frame;
/// nothing to do costs those two scans and no writes at all.
pub async fn repair_missing_pr(
    pool: SqlitePool,
    pr_data: std::sync::Arc<RwLock<PersonalRatingData>>,
) -> Result<u64, IndexError> {
    // Checked before the scans: without expected values every gap would be
    // read only to compute nothing from it.
    if !pr_data.read().is_loaded() {
        return Ok(0);
    }

    let gaps = query::pr_gaps(&pool).await?;
    if gaps.is_empty() {
        return Ok(0);
    }

    // The guard is confined to this block: it is not a Send lock and must not
    // be held across the write below.
    let repairs: Vec<PrRepair> = {
        let pr = pr_data.read();
        gaps.iter()
            .filter_map(|gap| single_battle_pr(&pr, &gap.inputs).map(|pr| PrRepair { target: gap.target, pr }))
            .collect()
    };

    query::apply_pr_repairs(&pool, &repairs).await
}

#[cfg(test)]
mod tests {
    use super::*;
    // Built directly here rather than through `map_rows`, which needs a
    // parsed replay these tests do not have.
    use crate::db::index::rows::MatchOutcome;
    use crate::db::index::rows::ObjectiveMatch;
    use crate::db::index::rows::ReplayRecord;
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

    /// A ship the checked-in expected values carry a real entry for, so a
    /// rating computed against it is a number rather than `None`.
    const RATED_SHIP: u64 = 3_374_266_064;

    fn loaded_pr_data() -> PersonalRatingData {
        let mut pr = PersonalRatingData::new();
        pr.load_from_bytes(wows_toolkit_viewmodel::personal_rating::EXPECTED_VALUES_FIXTURE)
            .expect("fixture should parse");
        pr
    }

    fn mapped_rows(record_pr: Option<f64>, vehicle_pr: Option<f64>) -> MappedRows {
        let arena_id = ArenaId::new(1);
        let mut vehicle = vehicle_row(7, "Player1");
        vehicle.ship_id = GameParamId::from(RATED_SHIP);
        vehicle.damage = Some(80_000);
        vehicle.kills = Some(2);
        vehicle.pr = vehicle_pr;
        MappedRows {
            objective: ObjectiveMatch {
                arena_id,
                timestamp: Timestamp::from_second(1_700_000_000).unwrap(),
                map: "spaces/13_OC_new_dawn".into(),
                game_mode: "Domination".into(),
                game_mode_id: None,
                game_type: "pvp".into(),
                match_group: "pvp".into(),
                version_build: Some(1234),
            },
            vehicles: vec![vehicle],
            record: ReplayRecord {
                arena_id,
                source_id: SourceId(1),
                replay_path: std::path::PathBuf::from("1.wowsreplay"),
                file_mtime: Some(42),
                outcome: MatchOutcome::Win,
                self_account_id: Some(AccountId(7)),
                self_ship_id: Some(GameParamId::from(RATED_SHIP)),
                self_survived: Some(true),
                self_damage: Some(80_000),
                self_kills: Some(2),
                self_pr: record_pr,
                results_available: true,
                indexed_at: Timestamp::from_second(1_700_000_100).unwrap(),
            },
        }
    }

    #[test]
    fn missing_ratings_are_filled_before_the_rows_are_written() {
        let mut rows = mapped_rows(None, None);
        fill_missing_pr(&mut rows, &loaded_pr_data());
        assert!(rows.record.self_pr.is_some(), "the record's rating was computed from its own stored stats");
        assert!(rows.vehicles[0].pr.is_some(), "the roster row's rating was computed too");
    }

    #[test]
    fn a_rating_the_report_already_carried_is_not_recomputed() {
        let mut rows = mapped_rows(Some(1500.0), Some(1200.0));
        fill_missing_pr(&mut rows, &loaded_pr_data());
        assert_eq!(rows.record.self_pr, Some(1500.0));
        assert_eq!(rows.vehicles[0].pr, Some(1200.0));
    }

    #[test]
    fn nothing_is_invented_while_the_expected_values_are_unloaded() {
        let mut rows = mapped_rows(None, None);
        fill_missing_pr(&mut rows, &PersonalRatingData::new());
        assert_eq!(rows.record.self_pr, None);
        assert_eq!(rows.vehicles[0].pr, None);
    }

    /// The rating a filled row gets has to come from the battle's own outcome,
    /// which is the only place a single-battle win rate can come from.
    #[test]
    fn a_filled_rating_follows_the_records_outcome() {
        let pr_data = loaded_pr_data();

        let mut won = mapped_rows(None, None);
        fill_missing_pr(&mut won, &pr_data);

        let mut lost = mapped_rows(None, None);
        lost.record.outcome = MatchOutcome::Loss;
        fill_missing_pr(&mut lost, &pr_data);

        assert!(
            won.record.self_pr > lost.record.self_pr,
            "a won battle must rate above the same battle lost ({:?} vs {:?})",
            won.record.self_pr,
            lost.record.self_pr
        );
        assert!(won.vehicles[0].pr > lost.vehicles[0].pr, "the roster rows follow the same outcome");
    }

    #[test]
    fn a_win_and_a_loss_rate_differently() {
        let pr_data = loaded_pr_data();
        let inputs = PrInputs { ship_id: GameParamId::from(RATED_SHIP), damage: 80_000, kills: 2, is_win: true };
        let won = single_battle_pr(&pr_data, &inputs).expect("the fixture rates this ship");
        let lost = single_battle_pr(&pr_data, &PrInputs { is_win: false, ..inputs }).expect("same ship");
        assert!(won > lost, "a win must not rate the same as a loss ({won} vs {lost})");
    }

    fn sample_rows() -> MappedRows {
        use crate::db::index::rows::VehicleRelation;
        use wows_replays::types::AccountId;
        use wows_replays::types::ArenaId;
        use wows_replays::types::GameParamId;

        MappedRows {
            objective: ObjectiveMatch {
                arena_id: ArenaId::new(7),
                timestamp: Timestamp::from_second(9000).unwrap(),
                map: "Ocean".into(),
                game_mode: "Domination".into(),
                game_mode_id: Some(7),
                game_type: "pvp".into(),
                match_group: "pvp".into(),
                version_build: Some(12116141),
            },
            vehicles: vec![IndexedVehicleRow {
                arena_id: ArenaId::new(7),
                account_id: AccountId(42),
                player_name: "Me".into(),
                clan: "WT".into(),
                realm: Some("na".into()),
                ship_id: GameParamId::from(999u64),
                ship_index: "PJSD018".into(),
                ship_name: "Harugumo".into(),
                nation: "japan".into(),
                species: "Destroyer".into(),
                tier: 10,
                relation: VehicleRelation::SelfPlayer,
                division_id: Some(3),
                survived: Some(true),
                damage: Some(120_000),
                kills: Some(3),
                spotting: Some(40_000),
                potential: Some(900_000),
                received: Some(20_000),
                pr: Some(1800.0),
                is_test_ship: false,
                disconnected: Some(false),
                is_stream_sniper: None,
                sniper_twitch_login: None,
            }],
            record: ReplayRecord {
                arena_id: ArenaId::new(7),
                source_id: SourceId(1),
                replay_path: std::path::PathBuf::from("C:/wows/replays/a.wowsreplay"),
                file_mtime: Some(5),
                outcome: MatchOutcome::Win,
                self_account_id: Some(AccountId(42)),
                self_ship_id: Some(GameParamId::from(999u64)),
                self_survived: Some(true),
                self_damage: Some(120_000),
                self_kills: Some(3),
                self_pr: Some(1800.0),
                results_available: true,
                indexed_at: Timestamp::from_second(1).unwrap(),
            },
        }
    }

    #[test]
    fn suppression_blanks_every_results_derived_value() {
        let mut rows = sample_rows();

        suppress_untrusted_results(&mut rows);

        let vehicle = &rows.vehicles[0];
        assert_eq!(vehicle.damage, None);
        assert_eq!(vehicle.kills, None);
        assert_eq!(vehicle.spotting, None);
        assert_eq!(vehicle.potential, None);
        assert_eq!(vehicle.received, None);
        assert_eq!(vehicle.pr, None);
        assert_eq!(rows.record.self_damage, None);
        assert_eq!(rows.record.self_kills, None);
        assert_eq!(rows.record.self_pr, None);
        assert!(!rows.record.results_available);
    }

    #[test]
    fn suppression_keeps_everything_the_packet_stream_provided() {
        let before = sample_rows();
        let mut rows = sample_rows();

        suppress_untrusted_results(&mut rows);

        assert_eq!(rows.objective.map, before.objective.map);
        assert_eq!(rows.objective.game_mode_id, before.objective.game_mode_id);
        assert_eq!(rows.record.outcome, before.record.outcome);
        assert_eq!(rows.record.self_survived, before.record.self_survived);
        let (vehicle, was) = (&rows.vehicles[0], &before.vehicles[0]);
        assert_eq!(vehicle.player_name, was.player_name);
        assert_eq!(vehicle.clan, was.clan);
        assert_eq!(vehicle.ship_id, was.ship_id);
        assert_eq!(vehicle.tier, was.tier);
        assert_eq!(vehicle.division_id, was.division_id);
        assert_eq!(vehicle.survived, was.survived);
        assert_eq!(vehicle.disconnected, was.disconnected);
    }
}

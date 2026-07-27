use std::collections::BTreeSet;
use std::collections::HashSet;

use egui::Color32;
use jiff::Timestamp;
use jiff::ToSpan;
use jiff::Unit;
use jiff::ZonedDifference;
use jiff::tz::TimeZone;
use rust_i18n::t;
use serde::Deserialize;
use serde::Serialize;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;

use crate::icons;
use crate::ui::theme::semantic::SemanticExt;

use super::PlayerTracker;

/// Colour for the escalating encounter-count severity ramp, or `None` below
/// the threshold where it becomes notable.
pub(crate) fn encounter_severity_color(ui: &egui::Ui, times_encountered_in_range: usize) -> Option<Color32> {
    match times_encountered_in_range {
        0..=1 => None,
        2..=3 => Some(ui.sem().division),
        4..=5 => Some(ui.sem().warn),
        _ => Some(ui.sem().loss),
    }
}

/// Human-readable "how long ago" for a tracked player's most recent encounter.
pub(crate) fn last_seen_text(player: &TrackedPlayer, now: Timestamp) -> String {
    let Some(last) = player.timestamps.last() else {
        return String::new();
    };
    let timestamp = last.to_zoned(TimeZone::system());
    let now = now.to_zoned(TimeZone::system());
    let delta = now
        .since(
            ZonedDifference::new(&timestamp)
                .smallest(Unit::Minute)
                .largest(Unit::Year)
                .mode(jiff::RoundMode::HalfExpand),
        )
        .expect("failed to calculate player last seen delta");

    format!("{delta:#}")
}

/// Absolute local-time stamp of a tracked player's most recent encounter, for
/// the hover behind the relative "last seen" text. A tracked player always has
/// at least one timestamp; an empty hover is the right degradation if that
/// invariant ever breaks, rather than a panic.
pub(crate) fn last_seen_timestamp_text(player: &TrackedPlayer) -> String {
    player
        .timestamps
        .last()
        .map(|ts| ts.to_zoned(TimeZone::system()).strftime("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TrackedPlayer {
    pub(crate) last_name: String,
    pub(crate) db_id: AccountId,
    pub(crate) names: HashSet<String>,
    pub(crate) clan_id: i64,
    pub(crate) clan: String,
    pub(crate) timestamps: BTreeSet<Timestamp>,
    pub(crate) arena_ids: BTreeSet<ArenaId>,
    #[serde(default)]
    pub(crate) notes: String,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum TimePeriod {
    LastHour,
    LastSixHours,
    #[default]
    LastDay,
    LastWeek,
    LastMonth,
    AllTime,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    pub(crate) fn icon(&self) -> &'static str {
        match self {
            SortOrder::Asc => icons::SORT_ASCENDING,
            SortOrder::Desc => icons::SORT_DESCENDING,
        }
    }

    pub(crate) fn toggle(&mut self) {
        match self {
            SortOrder::Asc => *self = SortOrder::Desc,
            SortOrder::Desc => *self = SortOrder::Asc,
        }
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SortedBy {
    Name(SortOrder),
    Clan(SortOrder),
    LastEncountered(SortOrder),
    TimesEncountered(SortOrder),
    TimesEncounteredInTimeRange(SortOrder),
}

impl SortedBy {
    /// The direction this sort is currently running in, whichever column it is on.
    pub(crate) fn order(&self) -> SortOrder {
        match self {
            SortedBy::Name(order)
            | SortedBy::Clan(order)
            | SortedBy::LastEncountered(order)
            | SortedBy::TimesEncountered(order)
            | SortedBy::TimesEncounteredInTimeRange(order) => *order,
        }
    }

    pub(crate) fn transition_to(&mut self, new: SortedBy) {
        match (self, new) {
            (SortedBy::Name(sort_order), SortedBy::Name(_)) => sort_order.toggle(),
            (SortedBy::Clan(sort_order), SortedBy::Clan(_)) => {
                sort_order.toggle();
            }
            (SortedBy::LastEncountered(sort_order), SortedBy::LastEncountered(_)) => {
                sort_order.toggle();
            }
            (SortedBy::TimesEncountered(sort_order), SortedBy::TimesEncountered(_)) => {
                sort_order.toggle();
            }
            (SortedBy::TimesEncounteredInTimeRange(sort_order), SortedBy::TimesEncounteredInTimeRange(_)) => {
                sort_order.toggle();
            }
            (old, new) => {
                *old = new;
            }
        }
    }
}

impl Default for SortedBy {
    fn default() -> Self {
        SortedBy::TimesEncounteredInTimeRange(SortOrder::Desc)
    }
}

impl TimePeriod {
    pub(crate) fn description(&self) -> String {
        match self {
            TimePeriod::LastHour => t!("ui.player_tracker.period.past_hour").into(),
            TimePeriod::LastSixHours => t!("ui.player_tracker.period.past_six_hours").into(),
            TimePeriod::LastDay => t!("ui.player_tracker.period.past_day").into(),
            TimePeriod::LastWeek => t!("ui.player_tracker.period.past_week").into(),
            TimePeriod::LastMonth => t!("ui.player_tracker.period.past_month").into(),
            TimePeriod::AllTime => t!("ui.player_tracker.period.all_time").into(),
        }
    }

    pub(crate) fn to_date(self) -> Option<Timestamp> {
        let now = Timestamp::now();
        match self {
            TimePeriod::LastHour => Some(now - 1.hour()),
            TimePeriod::LastSixHours => Some(now - 6.hours()),
            TimePeriod::LastDay => Some(now - 24.hours()),
            TimePeriod::LastWeek => Some(now - (24 * 7).hours()),
            TimePeriod::LastMonth => Some(now - (24 * 30).hours()),
            TimePeriod::AllTime => None,
        }
    }
}

impl PlayerTracker {
    /// Rebuild tracked-player aggregates from the durable replay index. Cheaper
    /// than re-parsing every replay from disk. Live updates via
    /// `update_from_replay` are unchanged and continue to layer on top.
    ///
    /// Returns `true` when the index had at least one player to populate from,
    /// so callers can fall back to re-parsing replays when the index is empty
    /// (e.g. before the first reconciliation pass has run).
    pub fn populate_from_index(&mut self, pool: &sqlx::SqlitePool, rt: &tokio::runtime::Runtime) -> bool {
        use crate::db::index::query;
        use crate::db::index::rows::MatchFilter;

        let players = match rt.block_on(query::distinct_players(pool, &MatchFilter::default())) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("player tracker: index query failed: {e}");
                return false;
            }
        };
        if players.is_empty() {
            return false;
        }

        // Never track the replay-perspective (self) account: the live path
        // (`update_from_replay`) deliberately excludes it, and it would otherwise
        // dominate the default times-encountered sort.
        let self_accounts = rt.block_on(query::self_account_ids(pool, &MatchFilter::default())).unwrap_or_default();

        for facet in players {
            if self_accounts.contains(&facet.account_id) {
                continue;
            }

            let hits = rt
                .block_on(query::matches_with_player(pool, facet.account_id, &MatchFilter::default()))
                .unwrap_or_default();

            let entry = self.tracked_players.entry(facet.account_id).or_default();
            entry.db_id = facet.account_id;
            entry.last_name = facet.latest_name;
            entry.clan = facet.clan;
            for hit in hits {
                entry.timestamps.insert(hit.timestamp);
                entry.arena_ids.insert(hit.arena_id);
                self.tracked_players_by_time.entry(hit.timestamp).or_default().push(facet.account_id);
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::path::PathBuf;

    use wows_replays::types::GameParamId;

    use super::*;
    use crate::db::index::query;
    use crate::db::index::rows::IndexedVehicleRow;
    use crate::db::index::rows::MatchOutcome;
    use crate::db::index::rows::ObjectiveMatch;
    use crate::db::index::rows::ReplayRecord;
    use crate::db::index::rows::VehicleRelation;

    fn build_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("failed to build test runtime")
    }

    async fn mem_pool() -> sqlx::SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("../wows-toolkit-config/migrations").run(&pool).await.unwrap();
        pool
    }

    #[test]
    fn populate_from_index_fills_tracker_from_seeded_matches() {
        let rt = build_runtime();
        let pool = rt.block_on(async {
            let pool = mem_pool().await;
            let now = Timestamp::from_second(1_700_000_000).unwrap();
            let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

            let objective = ObjectiveMatch {
                arena_id: ArenaId::new(100),
                timestamp: Timestamp::from_second(1_700_000_100).unwrap(),
                map: "Ocean".into(),
                game_mode: "Domination".into(),
                game_type: "pvp".into(),
                match_group: "pvp".into(),
                version_build: Some(1234),
            };
            query::upsert_match(&pool, &objective).await.unwrap();

            let enemy = IndexedVehicleRow {
                arena_id: ArenaId::new(100),
                account_id: AccountId(501),
                player_name: "Enemy".into(),
                clan: "CLAN".into(),
                realm: Some("na".into()),
                ship_id: GameParamId::from(111u64),
                ship_index: "PJSB018".into(),
                ship_name: "Yamato".into(),
                nation: "japan".into(),
                species: "Battleship".into(),
                tier: 10,
                relation: VehicleRelation::Enemy,
                division_id: None,
                survived: Some(true),
                damage: Some(50_000),
                kills: Some(1),
                spotting: Some(0),
                potential: Some(0),
                received: Some(0),
                pr: Some(1200.0),
                is_test_ship: false,
                disconnected: None,
                is_stream_sniper: None,
                sniper_twitch_login: None,
            };
            query::upsert_vehicles(&pool, &[enemy]).await.unwrap();

            let record = ReplayRecord {
                arena_id: ArenaId::new(100),
                source_id: src,
                replay_path: PathBuf::from("a.wowsreplay"),
                file_mtime: Some(1),
                outcome: MatchOutcome::Win,
                self_account_id: Some(AccountId(7)),
                self_ship_id: Some(GameParamId::from(999u64)),
                self_survived: Some(true),
                self_damage: Some(120_000),
                self_kills: Some(2),
                self_pr: Some(1500.0),
                results_available: true,
                indexed_at: now,
            };
            query::upsert_record(&pool, &record).await.unwrap();

            pool
        });

        let mut tracker = PlayerTracker::default();
        let populated = tracker.populate_from_index(&pool, &rt);
        assert!(populated, "index had a match to populate from");

        let enemy = tracker.tracked_players.get(&AccountId(501)).expect("enemy account tracked from index");
        assert_eq!(enemy.last_name, "Enemy");
        assert_eq!(enemy.clan, "CLAN");
        assert!(enemy.arena_ids.contains(&ArenaId::new(100)));
        assert_eq!(enemy.timestamps.len(), 1);

        let timestamp = *enemy.timestamps.iter().next().unwrap();
        assert_eq!(tracker.tracked_players_by_time.get(&timestamp).map(|v| v.len()), Some(1));
    }

    #[test]
    fn populate_from_index_reports_false_when_index_is_empty() {
        let rt = build_runtime();
        let pool = rt.block_on(mem_pool());

        let mut tracker = PlayerTracker::default();
        assert!(!tracker.populate_from_index(&pool, &rt), "empty index must not report as populated");
        assert!(tracker.tracked_players.is_empty());
    }

    #[test]
    fn populate_from_index_excludes_self_account() {
        let rt = build_runtime();
        let pool = rt.block_on(async {
            let pool = mem_pool().await;
            let now = Timestamp::from_second(1_700_000_000).unwrap();
            let src = query::ensure_default_source(&pool, Path::new("C:/wows/replays"), now).await.unwrap();

            let objective = ObjectiveMatch {
                arena_id: ArenaId::new(100),
                timestamp: Timestamp::from_second(1_700_000_100).unwrap(),
                map: "Ocean".into(),
                game_mode: "Domination".into(),
                game_type: "pvp".into(),
                match_group: "pvp".into(),
                version_build: Some(1234),
            };
            query::upsert_match(&pool, &objective).await.unwrap();

            let self_vehicle = IndexedVehicleRow {
                arena_id: ArenaId::new(100),
                account_id: AccountId(7),
                player_name: "MyAccount".into(),
                clan: "SELF".into(),
                realm: Some("na".into()),
                ship_id: GameParamId::from(999u64),
                ship_index: "PJSD018".into(),
                ship_name: "Harugumo".into(),
                nation: "japan".into(),
                species: "Destroyer".into(),
                tier: 10,
                relation: VehicleRelation::SelfPlayer,
                division_id: None,
                survived: Some(true),
                damage: Some(120_000),
                kills: Some(2),
                spotting: Some(0),
                potential: Some(0),
                received: Some(0),
                pr: Some(1500.0),
                is_test_ship: false,
                disconnected: None,
                is_stream_sniper: None,
                sniper_twitch_login: None,
            };
            let enemy = IndexedVehicleRow {
                arena_id: ArenaId::new(100),
                account_id: AccountId(501),
                player_name: "Enemy".into(),
                clan: "CLAN".into(),
                realm: Some("na".into()),
                ship_id: GameParamId::from(111u64),
                ship_index: "PJSB018".into(),
                ship_name: "Yamato".into(),
                nation: "japan".into(),
                species: "Battleship".into(),
                tier: 10,
                relation: VehicleRelation::Enemy,
                division_id: None,
                survived: Some(true),
                damage: Some(50_000),
                kills: Some(1),
                spotting: Some(0),
                potential: Some(0),
                received: Some(0),
                pr: Some(1200.0),
                is_test_ship: false,
                disconnected: None,
                is_stream_sniper: None,
                sniper_twitch_login: None,
            };
            query::upsert_vehicles(&pool, &[self_vehicle, enemy]).await.unwrap();

            let record = ReplayRecord {
                arena_id: ArenaId::new(100),
                source_id: src,
                replay_path: PathBuf::from("a.wowsreplay"),
                file_mtime: Some(1),
                outcome: MatchOutcome::Win,
                self_account_id: Some(AccountId(7)),
                self_ship_id: Some(GameParamId::from(999u64)),
                self_survived: Some(true),
                self_damage: Some(120_000),
                self_kills: Some(2),
                self_pr: Some(1500.0),
                results_available: true,
                indexed_at: now,
            };
            query::upsert_record(&pool, &record).await.unwrap();

            pool
        });

        let mut tracker = PlayerTracker::default();
        assert!(tracker.populate_from_index(&pool, &rt), "index had a match to populate from");

        assert!(!tracker.tracked_players.contains_key(&AccountId(7)), "self-perspective account must not be tracked");
        assert!(tracker.tracked_players.contains_key(&AccountId(501)), "opponent account must still be tracked");
    }
}

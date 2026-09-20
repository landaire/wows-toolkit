//! Sort model for the player table. Mirrors `ui/replay_parser/sorting.rs`
//! (`SortColumn`, `SortOrder`, `SortKey`) and the sort-key tuple built by
//! `UiReport::sort_players` in `ui/replay_parser/mod.rs:899-975`
//! (allies-before-enemies, the per-column key, then a stable db_id
//! tiebreak).

use std::cmp::Reverse;

use wows_replays::types::TeamId;
use wowsunpack::game_params::types::Species;

use super::model::PlayerRow;

/// Internal sort key that allows comparison across different value types.
/// Mirrors `sorting.rs::SortKey`.
#[derive(Clone, PartialEq)]
enum SortKey {
    String(String),
    I64(Option<i64>),
    U64(Option<u64>),
    F64(Option<f64>),
    Species(Species),
}

impl Eq for SortKey {}

impl PartialOrd for SortKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SortKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (SortKey::String(a), SortKey::String(b)) => a.cmp(b),
            (SortKey::I64(a), SortKey::I64(b)) => a.cmp(b),
            (SortKey::U64(a), SortKey::U64(b)) => a.cmp(b),
            (SortKey::F64(a), SortKey::F64(b)) => a.partial_cmp(b).expect("could not compare f64 sort keys"),
            (SortKey::Species(a), SortKey::Species(b)) => a.cmp(b),
            _ => std::cmp::Ordering::Equal,
        }
    }
}

/// Columns the player table can be sorted by. Distinct from `ReplayColumn`:
/// `Fires`/`Floods`/`Citadels`/`Crits` have no dedicated table column but are
/// retained here so a persisted `SortOrder` from an older save still
/// deserializes, matching `sorting.rs::SortColumn`'s comment.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortColumn {
    Name,
    BaseXp,
    RawXp,
    ShipName,
    ShipClass,
    Kills,
    ObservedDamage,
    ActualDamage,
    SpottingDamage,
    PotentialDamage,
    Hits,
    Heals,
    TimeLived,
    Fires,
    Floods,
    Citadels,
    Crits,
    ReceivedDamage,
    DistanceTraveled,
    PersonalRating,
}

/// Sort order (ascending or descending) with the column being sorted.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SortOrder {
    Asc(SortColumn),
    Desc(SortColumn),
}

impl Default for SortOrder {
    fn default() -> Self {
        SortOrder::Asc(SortColumn::ShipClass)
    }
}

impl SortOrder {
    /// Desc -> Asc(same column) -> default (Asc(ShipClass)) -> Desc(ShipClass) -> ...
    pub fn toggle(&mut self) {
        match self {
            SortOrder::Asc(_) => *self = Default::default(),
            SortOrder::Desc(column) => *self = SortOrder::Asc(*column),
        }
    }

    /// Clicking a column header: switching to a new column starts it
    /// descending; clicking the active column toggles its order.
    pub fn update_column(&mut self, new_column: SortColumn) -> SortOrder {
        match self {
            SortOrder::Asc(sort_column) | SortOrder::Desc(sort_column) if *sort_column == new_column => {
                self.toggle();
            }
            _ => *self = SortOrder::Desc(new_column),
        }
        *self
    }

    pub fn column(&self) -> SortColumn {
        match self {
            SortOrder::Asc(sort_column) | SortOrder::Desc(sort_column) => *sort_column,
        }
    }
}

/// NDA-hidden stats (per `should_hide_stats() && !debug`) sort as `None`
/// (last in ascending order), matching `UiReport::sort_players`.
fn sort_key(row: &PlayerRow, column: SortColumn, debug: bool) -> SortKey {
    let hidden = row.should_hide_stats() && !debug;
    match column {
        SortColumn::Name => SortKey::String(row.display_name.clone()),
        SortColumn::BaseXp => SortKey::I64(row.base_xp),
        SortColumn::RawXp => SortKey::I64(row.raw_xp),
        SortColumn::ShipName => SortKey::String(row.ship_name.clone()),
        SortColumn::ShipClass => SortKey::Species(row.ship_class),
        SortColumn::ObservedDamage => SortKey::U64(Some(if hidden { 0 } else { row.observed_damage })),
        SortColumn::ActualDamage => SortKey::U64(if hidden { None } else { row.actual_damage }),
        SortColumn::SpottingDamage => SortKey::U64(row.spotting_damage),
        SortColumn::PotentialDamage => SortKey::U64(if hidden { None } else { row.potential_damage }),
        SortColumn::TimeLived => SortKey::U64(row.time_lived_secs),
        SortColumn::Fires => SortKey::U64(if hidden { None } else { row.fires }),
        SortColumn::Floods => SortKey::U64(if hidden { None } else { row.floods }),
        SortColumn::Citadels => SortKey::U64(if hidden { None } else { row.citadels }),
        SortColumn::Crits => SortKey::U64(if hidden { None } else { row.crits }),
        SortColumn::ReceivedDamage => SortKey::U64(if hidden { None } else { row.received_damage }),
        SortColumn::DistanceTraveled => SortKey::F64(row.distance_traveled),
        SortColumn::Kills => SortKey::I64(Some(row.kills.unwrap_or(row.observed_kills))),
        SortColumn::Hits => SortKey::U64(if hidden { None } else { row.hits }),
        SortColumn::Heals => SortKey::U64(row.heal_count.map(|c| c as u64)),
        SortColumn::PersonalRating => SortKey::F64(row.personal_rating.as_ref().map(|pr| pr.pr)),
    }
}

/// `(is_relative_to_self_an_enemy, column_key, db_id)`: allies (relative to
/// `self_team`) sort before enemies regardless of column/order, then the
/// column key, then a stable tiebreak by db_id. Team-grouping and the db_id
/// tiebreak are never reversed by `Desc`; only the column key is.
fn row_key(row: &PlayerRow, self_team: TeamId, column: SortColumn, debug: bool) -> (bool, SortKey, i64) {
    (row.team_id != self_team, sort_key(row, column, debug), row.db_id.raw())
}

/// Sorts `rows` in place. Mirrors `UiReport::sort_players`. `debug` lifts the
/// NDA-hidden-stat gate on the relevant columns, matching the app's
/// debug-mode override.
pub fn sort_rows(rows: &mut [PlayerRow], self_team: TeamId, order: SortOrder, debug: bool) {
    match order {
        SortOrder::Desc(column) => {
            rows.sort_unstable_by_key(|row| {
                let (team, key, id) = row_key(row, self_team, column, debug);
                (team, Reverse(key), id)
            });
        }
        SortOrder::Asc(column) => {
            rows.sort_unstable_by_key(|row| row_key(row, self_team, column, debug));
        }
    }
}

#[cfg(test)]
mod tests {
    use wows_replays::types::AccountId;
    use wows_replays::types::Relation;

    use super::*;
    use crate::replay_inspector::test_support::base_row;

    fn row_with(db_id: i64, team: i64, relation: Relation, is_self: bool, actual_damage: Option<u64>) -> PlayerRow {
        PlayerRow {
            db_id: AccountId(db_id),
            team_id: TeamId::from(team),
            actual_damage,
            actual_damage_text: actual_damage.map(|d| d.to_string()),
            ..base_row(db_id, relation, is_self)
        }
    }

    #[test]
    fn allies_sort_before_enemies_regardless_of_key() {
        let self_team = TeamId::from(0i64);
        let mut rows = vec![
            row_with(3, 1, Relation::new(2), false, Some(100_000)), // enemy, huge damage
            row_with(1, 0, Relation::new(0), true, Some(1)),        // self, tiny damage
            row_with(2, 0, Relation::new(1), false, Some(2)),       // ally
        ];

        sort_rows(&mut rows, self_team, SortOrder::Desc(SortColumn::ActualDamage), false);

        let ids: Vec<i64> = rows.iter().map(|r| r.db_id.raw()).collect();
        // Allies (db_id 1, 2) before the enemy (db_id 3), even though the
        // enemy has by far the largest ActualDamage.
        assert_eq!(ids, vec![2, 1, 3]);
    }

    #[test]
    fn desc_actual_damage_orders_descending_within_a_team() {
        let self_team = TeamId::from(0i64);
        let mut rows = vec![
            row_with(1, 0, Relation::new(0), true, Some(10_000)),
            row_with(2, 0, Relation::new(1), false, Some(50_000)),
            row_with(3, 0, Relation::new(1), false, Some(30_000)),
        ];

        sort_rows(&mut rows, self_team, SortOrder::Desc(SortColumn::ActualDamage), false);

        let ids: Vec<i64> = rows.iter().map(|r| r.db_id.raw()).collect();
        assert_eq!(ids, vec![2, 3, 1]);
    }

    #[test]
    fn ties_break_by_db_id_ascending_regardless_of_order() {
        let self_team = TeamId::from(0i64);
        let mut rows = vec![
            row_with(5, 0, Relation::new(1), false, Some(1_000)),
            row_with(2, 0, Relation::new(1), false, Some(1_000)),
            row_with(3, 0, Relation::new(1), false, Some(1_000)),
        ];

        sort_rows(&mut rows, self_team, SortOrder::Desc(SortColumn::ActualDamage), false);
        assert_eq!(rows.iter().map(|r| r.db_id.raw()).collect::<Vec<_>>(), vec![2, 3, 5]);

        sort_rows(&mut rows, self_team, SortOrder::Asc(SortColumn::ActualDamage), false);
        assert_eq!(rows.iter().map(|r| r.db_id.raw()).collect::<Vec<_>>(), vec![2, 3, 5]);
    }

    #[test]
    fn nda_hidden_stat_sorts_as_none_unless_debug() {
        let self_team = TeamId::from(0i64);
        let mut hidden = row_with(1, 0, Relation::new(1), false, Some(999_999));
        hidden.is_test_ship = true;
        let mut rows = vec![hidden, row_with(2, 0, Relation::new(1), false, Some(1))];

        // Not debug: the hidden row's ActualDamage reads as None, which sorts
        // before Some(1) in ascending order (and therefore last in Desc,
        // since None keys are the smallest / last-in-Desc even though the
        // real value is the largest).
        sort_rows(&mut rows, self_team, SortOrder::Desc(SortColumn::ActualDamage), false);
        assert_eq!(rows.iter().map(|r| r.db_id.raw()).collect::<Vec<_>>(), vec![2, 1]);

        // Debug mode: the real (huge) value participates, so the hidden row
        // sorts first under Desc.
        sort_rows(&mut rows, self_team, SortOrder::Desc(SortColumn::ActualDamage), true);
        assert_eq!(rows.iter().map(|r| r.db_id.raw()).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn default_sort_order_is_ascending_ship_class() {
        assert_eq!(SortOrder::default(), SortOrder::Asc(SortColumn::ShipClass));
    }

    #[test]
    fn update_column_toggles_same_column_and_resets_new_column_to_desc() {
        let mut order = SortOrder::default();
        assert_eq!(order.update_column(SortColumn::ActualDamage), SortOrder::Desc(SortColumn::ActualDamage));
        assert_eq!(order.update_column(SortColumn::ActualDamage), SortOrder::Asc(SortColumn::ActualDamage));
        // Toggling Asc(same column) again returns to the hardcoded default.
        assert_eq!(order.update_column(SortColumn::ActualDamage), SortOrder::default());
        // Switching to a different column starts it descending.
        assert_eq!(order.update_column(SortColumn::Kills), SortOrder::Desc(SortColumn::Kills));
    }
}

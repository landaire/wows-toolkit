//! The player table's sort order, kept under [`crate::settings::keys::REPLAY_SORT`] in the same
//! form the egui app writes, so a sort chosen in either front end holds in both.

use serde::Deserialize;
use serde::Serialize;

/// Columns the player table can be sorted by. `Fires`, `Floods`, `Citadels` and `Crits` have no
/// table column, but a stored order may still name them.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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
    MatchContribution,
}

/// Sort order (ascending or descending) with the column being sorted.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn the_stored_form_is_the_one_the_egui_app_writes() {
        assert_eq!(
            serde_json::from_str::<SortOrder>(r#"{"Asc":"ShipClass"}"#).expect("it reads"),
            SortOrder::Asc(SortColumn::ShipClass)
        );
        assert_eq!(
            serde_json::to_string(&SortOrder::Desc(SortColumn::ActualDamage)).expect("it writes"),
            r#"{"Desc":"ActualDamage"}"#
        );
    }
}

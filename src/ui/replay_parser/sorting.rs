//! Sorting infrastructure for replay player lists.

use serde::{Deserialize, Serialize};
use wowsunpack::game_params::types::Species;

use crate::icons;

/// Internal sort key that allows comparison across different types
#[derive(Clone)]
pub enum SortKey {
    String(String),
    I64(Option<i64>),
    U64(Option<u64>),
    F64(Option<f64>),
    Species(Species),
}

impl PartialEq for SortKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (SortKey::String(a), SortKey::String(b)) => a == b,
            (SortKey::I64(a), SortKey::I64(b)) => a == b,
            (SortKey::U64(a), SortKey::U64(b)) => a == b,
            (SortKey::F64(a), SortKey::F64(b)) => a == b,
            (SortKey::Species(a), SortKey::Species(b)) => a == b,
            _ => false,
        }
    }
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
            (SortKey::F64(a), SortKey::F64(b)) => a.partial_cmp(b).expect("could not compare f64 keys?"),
            (SortKey::Species(a), SortKey::Species(b)) => a.cmp(b),
            _ => std::cmp::Ordering::Equal,
        }
    }
}

/// Sort order (ascending or descending) with the column being sorted
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
    pub fn icon(&self) -> &'static str {
        match self {
            SortOrder::Asc(_) => icons::SORT_ASCENDING,
            SortOrder::Desc(_) => icons::SORT_DESCENDING,
        }
    }

    pub fn toggle(&mut self) {
        match self {
            // By default everything should be Descending. Descending transitions to ascending.
            // Ascending transitions back to default state.
            SortOrder::Asc(_) => *self = Default::default(),
            SortOrder::Desc(column) => *self = SortOrder::Asc(*column),
        }
    }

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

/// All displayable columns in the replay player list
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReplayColumn {
    Actions,
    Name,
    ShipName,
    Skills,
    PersonalRating,
    BaseXp,
    RawXp,
    Kills,
    ObservedDamage,
    ActualDamage,
    ReceivedDamage,
    SpottingDamage,
    PotentialDamage,
    Hits,
    Fires,
    Floods,
    Citadels,
    Crits,
    DistanceTraveled,
    TimeLived,
}

/// Columns which support sorting
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
    TimeLived,
    Fires,
    Floods,
    Citadels,
    Crits,
    ReceivedDamage,
    DistanceTraveled,
    PersonalRating,
}

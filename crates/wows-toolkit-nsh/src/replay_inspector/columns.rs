//! The player table's columns as the TSX names them.

use wows_toolkit_config::ReplaySettings;
use wows_toolkit_replay_data::columns::ReplayColumn;
use wows_toolkit_replay_data::columns::default_columns;

/// The name the TSX uses for `column`.
pub fn column_key(column: ReplayColumn) -> &'static str {
    match column {
        ReplayColumn::Name => "name",
        ReplayColumn::ShipName => "shipName",
        ReplayColumn::Skills => "skills",
        ReplayColumn::PersonalRating => "personalRating",
        ReplayColumn::BaseXp => "baseXp",
        ReplayColumn::RawXp => "rawXp",
        ReplayColumn::Kills => "kills",
        ReplayColumn::ObservedDamage => "observedDamage",
        ReplayColumn::ActualDamage => "actualDamage",
        ReplayColumn::ReceivedDamage => "receivedDamage",
        ReplayColumn::SpottingDamage => "spottingDamage",
        ReplayColumn::PotentialDamage => "potentialDamage",
        ReplayColumn::Hits => "hits",
        ReplayColumn::Heals => "heals",
        ReplayColumn::DistanceTraveled => "distanceTraveled",
        ReplayColumn::TimeLived => "timeLived",
    }
}

/// The column the TSX names `key`, if any.
pub fn column_from_key(key: &str) -> Option<ReplayColumn> {
    ReplayColumn::ALL.into_iter().find(|column| column_key(*column) == key)
}

/// The visible columns for `settings`, as the TSX names them.
pub fn visible_column_keys(settings: &ReplaySettings) -> Vec<String> {
    default_columns(settings).into_iter().map(|column| column_key(column).to_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_column_reads_back_from_its_key() {
        for column in ReplayColumn::ALL {
            assert_eq!(column_from_key(column_key(column)), Some(column));
        }
        assert_eq!(column_from_key("actions"), None);
    }
}

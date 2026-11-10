//! The replay player list's column set. Mirrors `ui/replay_parser/sorting.rs`
//! (`ReplayColumn`, declaration order).

use wows_toolkit_config::ReplaySettings;

/// All displayable columns in the replay player list, in the same
/// declaration order as the egui app's `ReplayColumn` (`sorting.rs:101`) minus
/// its `Actions` column: a row's actions are on the row itself, revealed on
/// hover and on a right-click, so they need no column of their own.
/// `default_columns` re-sorts to this order after filtering, matching
/// `update_visible_columns`.
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReplayColumn {
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
    Heals,
    DistanceTraveled,
    TimeLived,
}

impl ReplayColumn {
    pub const ALL: [ReplayColumn; 16] = [
        ReplayColumn::Name,
        ReplayColumn::ShipName,
        ReplayColumn::Skills,
        ReplayColumn::PersonalRating,
        ReplayColumn::BaseXp,
        ReplayColumn::RawXp,
        ReplayColumn::Kills,
        ReplayColumn::ObservedDamage,
        ReplayColumn::ActualDamage,
        ReplayColumn::ReceivedDamage,
        ReplayColumn::SpottingDamage,
        ReplayColumn::PotentialDamage,
        ReplayColumn::Hits,
        ReplayColumn::Heals,
        ReplayColumn::DistanceTraveled,
        ReplayColumn::TimeLived,
    ];

    /// The `ReplaySettings` flag that gates this column's visibility, for the
    /// five user-toggleable columns. `None` for always-on columns. Mirrors
    /// `update_visible_columns`'s `optional_columns` table.
    fn settings_gate(self) -> Option<fn(&ReplaySettings) -> bool> {
        match self {
            ReplayColumn::RawXp => Some(|s| s.show_raw_xp),
            ReplayColumn::ObservedDamage => Some(|s| s.show_observed_damage),
            ReplayColumn::Heals => Some(|s| s.show_heals),
            ReplayColumn::ReceivedDamage => Some(|s| s.show_received_damage),
            ReplayColumn::DistanceTraveled => Some(|s| s.show_distance_traveled),
            _ => None,
        }
    }
}

/// The column set for a fresh replay load, given the persisted display
/// settings: always-on columns unconditionally, the five optional columns
/// only when their `ReplaySettings` flag is set, in declaration order.
/// Mirrors `UiReport::update_visible_columns`.
pub fn default_columns(settings: &ReplaySettings) -> Vec<ReplayColumn> {
    ReplayColumn::ALL.into_iter().filter(|col| col.settings_gate().is_none_or(|gate| gate(settings))).collect()
}

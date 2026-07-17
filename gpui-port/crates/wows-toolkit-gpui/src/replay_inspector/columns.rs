//! Column identifiers for the replay player list. Mirrors
//! `ui/replay_parser/sorting.rs`'s `ReplayColumn` (declaration order).
//! Formatting, coloring, and the settings-driven default column set land in
//! a later commit (Task 2); `PlayerRow`'s parent (`ReplayReportModel`) only
//! needs the bare enum for its `columns` field.

/// All displayable columns in the replay player list, in the same
/// declaration order as the egui app's `ReplayColumn` (`sorting.rs:101`).
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    Heals,
    DistanceTraveled,
    TimeLived,
}

impl ReplayColumn {
    pub const ALL: [ReplayColumn; 17] = [
        ReplayColumn::Actions,
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
}

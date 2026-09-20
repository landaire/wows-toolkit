//! Players met across indexed battles: which window of time is in view, how
//! the table is sorted, and how a typed filter narrows it.
//!
//! The rows come from the shared replay index; this decides what the table
//! shows of them, the same way in both front ends.

use jiff::Timestamp;
use jiff::ToSpan;
use serde::Deserialize;
use serde::Serialize;
use wows_toolkit_config::index::rows::MatchFilter;
use wows_toolkit_config::index::rows::PlayerFacet;

/// How far back the tracker looks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimePeriod {
    LastHour,
    LastSixHours,
    #[default]
    LastDay,
    LastWeek,
    LastMonth,
    AllTime,
}

impl TimePeriod {
    pub const ALL: [TimePeriod; 6] = [
        Self::LastHour,
        Self::LastSixHours,
        Self::LastDay,
        Self::LastWeek,
        Self::LastMonth,
        Self::AllTime,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::LastHour => "Last hour",
            Self::LastSixHours => "Last 6 hours",
            Self::LastDay => "Last day",
            Self::LastWeek => "Last week",
            Self::LastMonth => "Last month",
            Self::AllTime => "All time",
        }
    }

    /// The earliest battle this period includes, measured from `now`.
    ///
    /// `None` for all-time, which is an absent bound rather than a very old
    /// one, so the query leaves the range open instead of guessing an epoch.
    pub fn earliest(self, now: Timestamp) -> Option<Timestamp> {
        match self {
            Self::LastHour => Some(now - 1.hour()),
            Self::LastSixHours => Some(now - 6.hours()),
            Self::LastDay => Some(now - 24.hours()),
            Self::LastWeek => Some(now - (24 * 7).hours()),
            Self::LastMonth => Some(now - (24 * 30).hours()),
            Self::AllTime => None,
        }
    }

    /// The index filter this period asks for.
    pub fn match_filter(self, now: Timestamp) -> MatchFilter {
        MatchFilter { date_from: self.earliest(now), ..MatchFilter::default() }
    }
}

/// Which column the table is ordered by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortColumn {
    Name,
    Clan,
    #[default]
    Encounters,
}

impl SortColumn {
    pub const ALL: [SortColumn; 3] = [Self::Name, Self::Clan, Self::Encounters];

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Player",
            Self::Clan => "Clan",
            Self::Encounters => "Encounters",
        }
    }

    /// The direction a fresh click on this column sorts.
    ///
    /// Counts open descending because the interesting end is the players met
    /// most; names open ascending because that is alphabetical order.
    pub fn default_order(self) -> SortOrder {
        match self {
            Self::Name | Self::Clan => SortOrder::Ascending,
            Self::Encounters => SortOrder::Descending,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    Ascending,
    Descending,
}

impl SortOrder {
    pub fn reversed(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }

    fn apply(self, ordering: std::cmp::Ordering) -> std::cmp::Ordering {
        match self {
            Self::Ascending => ordering,
            Self::Descending => ordering.reverse(),
        }
    }
}

/// How the table is currently ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sort {
    pub column: SortColumn,
    pub order: SortOrder,
}

impl Default for Sort {
    fn default() -> Self {
        Self { column: SortColumn::Encounters, order: SortColumn::Encounters.default_order() }
    }
}

impl Sort {
    /// Clicking a column: the same column flips direction, a new one starts at
    /// its own default.
    pub fn toggled(self, column: SortColumn) -> Self {
        if self.column == column {
            Self { column, order: self.order.reversed() }
        } else {
            Self { column, order: column.default_order() }
        }
    }
}

/// Keeps the players whose name or clan contains `needle`, case-insensitively.
/// An empty needle keeps everyone.
pub fn matches_filter(player: &PlayerFacet, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let needle = needle.to_lowercase();
    player.latest_name.to_lowercase().contains(&needle) || player.clan.to_lowercase().contains(&needle)
}

/// The rows the table draws: filtered, then sorted.
///
/// Ties break on the account id so the order is stable between refreshes
/// rather than reshuffling players with the same count.
pub fn visible_players(players: &[PlayerFacet], needle: &str, sort: Sort) -> Vec<PlayerFacet> {
    let mut rows: Vec<PlayerFacet> =
        players.iter().filter(|player| matches_filter(player, needle)).cloned().collect();

    rows.sort_by(|a, b| {
        let ordering = match sort.column {
            SortColumn::Name => a.latest_name.to_lowercase().cmp(&b.latest_name.to_lowercase()),
            SortColumn::Clan => a.clan.to_lowercase().cmp(&b.clan.to_lowercase()),
            SortColumn::Encounters => a.match_count.cmp(&b.match_count),
        };
        sort.order.apply(ordering).then_with(|| a.account_id.cmp(&b.account_id))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(account: i64, name: &str, clan: &str, count: i64) -> PlayerFacet {
        PlayerFacet {
            account_id: account.into(),
            latest_name: name.to_string(),
            clan: clan.to_string(),
            match_count: count,
        }
    }

    fn sample() -> Vec<PlayerFacet> {
        vec![
            player(1, "Zeta", "ALPHA", 2),
            player(2, "alpha", "ZULU", 9),
            player(3, "Mike", "", 9),
        ]
    }

    #[test]
    fn all_time_leaves_the_range_open_rather_than_guessing_a_start() {
        let now = Timestamp::now();
        assert_eq!(TimePeriod::AllTime.earliest(now), None);
        assert!(TimePeriod::AllTime.match_filter(now).date_from.is_none());
    }

    #[test]
    fn each_period_reaches_further_back_than_the_one_before_it() {
        let now = Timestamp::now();
        let bounded: Vec<Timestamp> =
            TimePeriod::ALL.iter().filter_map(|period| period.earliest(now)).collect();

        assert_eq!(bounded.len(), 5, "every period but all-time is bounded");
        for pair in bounded.windows(2) {
            assert!(pair[1] < pair[0], "a longer period starts earlier");
        }
        assert!(bounded.iter().all(|start| *start < now));
    }

    #[test]
    fn an_empty_filter_keeps_everyone_and_a_needle_matches_name_or_clan() {
        let players = sample();
        assert_eq!(visible_players(&players, "", Sort::default()).len(), 3);
        assert_eq!(visible_players(&players, "zulu", Sort::default()).len(), 1, "matched on clan");
        assert_eq!(visible_players(&players, "mike", Sort::default()).len(), 1, "matched on name");
        assert!(visible_players(&players, "nobody", Sort::default()).is_empty());
    }

    #[test]
    fn the_filter_ignores_case_on_both_sides() {
        let players = sample();
        assert_eq!(visible_players(&players, "ZETA", Sort::default()).len(), 1);
        assert_eq!(visible_players(&players, "alpha", Sort::default()).len(), 2, "one name, one clan");
    }

    #[test]
    fn encounters_sort_descending_by_default_so_the_most_met_lead() {
        let rows = visible_players(&sample(), "", Sort::default());
        assert_eq!(rows.iter().map(|row| row.match_count).collect::<Vec<_>>(), vec![9, 9, 2]);
    }

    #[test]
    fn ties_break_on_the_account_so_the_order_does_not_reshuffle() {
        let rows = visible_players(&sample(), "", Sort::default());
        let tied: Vec<i64> =
            rows.iter().filter(|row| row.match_count == 9).map(|row| row.account_id.raw()).collect();
        assert_eq!(tied, vec![2, 3], "the two nine-encounter players keep account order");
    }

    #[test]
    fn sorting_by_name_is_alphabetical_regardless_of_case() {
        let sort = Sort { column: SortColumn::Name, order: SortOrder::Ascending };
        let rows = visible_players(&sample(), "", sort);
        assert_eq!(rows.iter().map(|row| row.latest_name.as_str()).collect::<Vec<_>>(), vec!["alpha", "Mike", "Zeta"]);
    }

    #[test]
    fn clicking_the_same_column_flips_it_and_a_new_one_starts_at_its_own_default() {
        let sort = Sort::default();
        assert_eq!(sort.column, SortColumn::Encounters);
        assert_eq!(sort.order, SortOrder::Descending);

        let flipped = sort.toggled(SortColumn::Encounters);
        assert_eq!(flipped.order, SortOrder::Ascending, "the same column reverses");

        let by_name = flipped.toggled(SortColumn::Name);
        assert_eq!(by_name.column, SortColumn::Name);
        assert_eq!(by_name.order, SortOrder::Ascending, "names open alphabetically");

        let by_count = by_name.toggled(SortColumn::Encounters);
        assert_eq!(by_count.order, SortOrder::Descending, "counts open at the most met");
    }
}

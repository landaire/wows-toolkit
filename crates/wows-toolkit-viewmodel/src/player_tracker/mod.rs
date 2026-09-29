//! Players met across indexed battles: which window of time is in view, how
//! the table is sorted, and how a typed filter narrows it.
//!
//! The rows come from the shared replay index; this decides what the table
//! shows of them, the same way in both front ends.

pub mod clans;
pub mod history;
pub mod live;
pub mod store;

/// How loudly a number of encounters reads.
///
/// Meeting someone twice is worth a glance, four times worth a colour, six
/// times worth the colour a loss is drawn in. The bands are the egui
/// tracker's (`ui/player_tracker/model.rs`'s `encounter_severity_color`);
/// which tone each band resolves to is the front end's own, since the two
/// draw on different surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncounterSeverity {
    /// Once or never: nothing worth marking.
    None,
    Noted,
    Warned,
    Heavy,
}

pub fn encounter_severity(times_in_range: usize) -> EncounterSeverity {
    match times_in_range {
        0..=1 => EncounterSeverity::None,
        2..=3 => EncounterSeverity::Noted,
        4..=5 => EncounterSeverity::Warned,
        _ => EncounterSeverity::Heavy,
    }
}
pub mod tracked;

use crate::match_stats::Region;
use jiff::Timestamp;
use jiff::ToSpan;
use serde::Deserialize;
use serde::Serialize;
use std::collections::HashMap;
use wows_replays::types::AccountId;
use wows_toolkit_config::index::rows::MatchFilter;
use wows_toolkit_config::index::rows::PlayerFacet;

/// A player's wows-numbers page.
pub fn wows_numbers_player_url(region: Region, account_id: AccountId, name: &str) -> String {
    format!("https://{}.wows-numbers.com/player/{},{}", region.as_wire(), account_id.0, name)
}

/// A player's shipbuilds page.
///
/// Only the `EU` form of the region segment is confirmed against the live
/// site; the others follow the same shape.
pub fn shipbuilds_player_url(region: Region, account_id: AccountId, name: &str) -> String {
    format!("https://shipbuilds.com/player/{}/{}/{}", region.as_url_segment(), account_id.0, name)
}

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
    pub const ALL: [TimePeriod; 6] =
        [Self::LastHour, Self::LastSixHours, Self::LastDay, Self::LastWeek, Self::LastMonth, Self::AllTime];

    /// The translation key the period selector reads from.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::LastHour => "ui.player_tracker.period.past_hour",
            Self::LastSixHours => "ui.player_tracker.period.past_six_hours",
            Self::LastDay => "ui.player_tracker.period.past_day",
            Self::LastWeek => "ui.player_tracker.period.past_week",
            Self::LastMonth => "ui.player_tracker.period.past_month",
            Self::AllTime => "ui.player_tracker.period.all_time",
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
    /// Every battle the player was met in, whenever it was.
    TotalEncounters,
    /// Only the ones inside the period on screen.
    #[default]
    Encounters,
    LastEncountered,
}

impl SortColumn {
    pub const ALL: [SortColumn; 5] =
        [Self::Name, Self::Clan, Self::TotalEncounters, Self::Encounters, Self::LastEncountered];

    /// The translation key the column's heading reads from.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Name => "ui.player_tracker.column.player_name",
            Self::Clan => "ui.player_tracker.column.clan",
            Self::TotalEncounters => "ui.player_tracker.column.total_encounters",
            Self::Encounters => "ui.player_tracker.column.encounters_in_range",
            Self::LastEncountered => "ui.player_tracker.column.last_encountered",
        }
    }

    /// The direction a fresh click on this column sorts.
    ///
    /// Counts open descending because the interesting end is the players met
    /// most; names open ascending because that is alphabetical order.
    pub fn default_order(self) -> SortOrder {
        match self {
            Self::Name | Self::Clan => SortOrder::Ascending,
            Self::TotalEncounters | Self::Encounters | Self::LastEncountered => SortOrder::Descending,
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
    /// Clicking a column: a new one starts at its own default, the same one
    /// flips, and a third click puts the table back the way it opened.
    ///
    /// The third state is what lets a reader undo a sort without having to
    /// know which column the table sorts by to begin with.
    pub fn toggled(self, column: SortColumn) -> Self {
        if self.column != column {
            return Self { column, order: column.default_order() };
        }
        if self.order == column.default_order() {
            Self { column, order: self.order.reversed() }
        } else {
            Self::default()
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
    let mut rows: Vec<PlayerFacet> = players.iter().filter(|player| matches_filter(player, needle)).cloned().collect();

    rows.sort_by(|a, b| {
        let ordering = match sort.column {
            SortColumn::Name => a.latest_name.to_lowercase().cmp(&b.latest_name.to_lowercase()),
            SortColumn::Clan => a.clan.to_lowercase().cmp(&b.clan.to_lowercase()),
            SortColumn::TotalEncounters | SortColumn::Encounters | SortColumn::LastEncountered => {
                a.match_count.cmp(&b.match_count)
            }
        };
        sort.order.apply(ordering).then_with(|| a.account_id.cmp(&b.account_id))
    });
    rows
}

/// One historical row: an indexed player, joined to what the tracker recorded
/// about the same account.
#[derive(Debug, Clone)]
pub struct PlayerRow {
    pub facet: PlayerFacet,
    /// Every battle the player was met in, from the tracker. `None` for a
    /// player the index names but the tracker never recorded, where there is
    /// no all-time figure to give.
    pub total_encounters: Option<usize>,
    /// Battles inside the period on screen. Taken from the tracker, which
    /// knows which of them were division ones; a player the tracker never
    /// recorded falls back to the index's own count for the same period.
    pub encounters_in_range: usize,
    /// When the player was last met, from the tracker.
    pub last_seen: Option<Timestamp>,
}

/// The historical table's rows: filtered by `needle`, joined to `tracked`,
/// and sorted.
///
/// `since` is the period's resolved boundary and `show_division_mates` the
/// toggle above the table. While the toggle is off, a player met inside the
/// period only as a division mate leaves the table entirely, which is what
/// the egui tracker does.
///
/// Ties break on the account id so the order is stable between refreshes
/// rather than reshuffling players with the same count.
pub fn visible_player_rows(
    players: &[PlayerFacet],
    tracked: &HashMap<AccountId, tracked::TrackedPlayer>,
    needle: &str,
    since: Option<Timestamp>,
    show_division_mates: bool,
    sort: Sort,
) -> Vec<PlayerRow> {
    let mut rows: Vec<PlayerRow> = players
        .iter()
        .filter(|player| matches_filter(player, needle))
        .filter_map(|facet| {
            let Some(player) = tracked.get(&facet.account_id) else {
                return Some(PlayerRow {
                    facet: facet.clone(),
                    total_encounters: None,
                    encounters_in_range: facet.match_count.max(0) as usize,
                    last_seen: None,
                });
            };
            if !show_division_mates && history::met_only_in_division(player, since) {
                return None;
            }
            Some(PlayerRow {
                facet: facet.clone(),
                total_encounters: Some(player.visible_arena_ids(show_division_mates).count()),
                encounters_in_range: history::encounters_in_range(player, since, show_division_mates),
                last_seen: player.last_visible_timestamp(show_division_mates),
            })
        })
        .collect();

    rows.sort_by(|a, b| {
        let ordering = match sort.column {
            SortColumn::Name => a.facet.latest_name.to_lowercase().cmp(&b.facet.latest_name.to_lowercase()),
            SortColumn::Clan => a.facet.clan.to_lowercase().cmp(&b.facet.clan.to_lowercase()),
            SortColumn::TotalEncounters => a.total_encounters.cmp(&b.total_encounters),
            SortColumn::Encounters => a.encounters_in_range.cmp(&b.encounters_in_range),
            SortColumn::LastEncountered => a.last_seen.cmp(&b.last_seen),
        };
        sort.order.apply(ordering).then_with(|| a.facet.account_id.cmp(&b.facet.account_id))
    });
    rows
}

#[cfg(test)]
mod sort_cycle_tests {
    use super::ClanSort;
    use super::ClanSortColumn;
    use super::Sort;
    use super::SortColumn;

    #[test]
    fn a_third_click_on_one_column_returns_the_table_to_its_default() {
        let default = Sort::default();
        let first = default.toggled(SortColumn::Name);
        assert_eq!(first.order, SortColumn::Name.default_order(), "a new column opens at its own direction");
        let second = first.toggled(SortColumn::Name);
        assert_eq!(second.order, SortColumn::Name.default_order().reversed(), "the second click reverses it");
        let third = second.toggled(SortColumn::Name);
        assert_eq!(third, default, "the third click resets rather than flipping back");
    }

    #[test]
    fn moving_to_another_column_does_not_reset() {
        let sorted = Sort::default().toggled(SortColumn::Name).toggled(SortColumn::Name);
        let moved = sorted.toggled(SortColumn::Clan);
        assert_eq!(moved.column, SortColumn::Clan);
        assert_eq!(moved.order, SortColumn::Clan.default_order());
    }

    #[test]
    fn the_clans_table_cycles_the_same_way() {
        let default = ClanSort::default();
        let third = default.toggled(ClanSortColumn::Clan).toggled(ClanSortColumn::Clan).toggled(ClanSortColumn::Clan);
        assert_eq!(third, default);
    }
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
        vec![player(1, "Zeta", "ALPHA", 2), player(2, "alpha", "ZULU", 9), player(3, "Mike", "", 9)]
    }

    /// A tracked player met at `hours_ago`, each meeting flagged as a
    /// division one or not.
    fn met(account: i64, meetings: &[(i64, bool)]) -> (AccountId, tracked::TrackedPlayer) {
        let now = Timestamp::now();
        let mut player = tracked::TrackedPlayer::default();
        for (ix, (hours_ago, in_division)) in meetings.iter().enumerate() {
            let arena = wows_replays::types::ArenaId::from(ix as i64);
            let at = now - hours_ago.hours();
            player.timestamps.insert(at);
            player.arena_ids.insert(arena);
            if *in_division {
                player.division_encounters.mark(arena, at);
            }
        }
        (account.into(), player)
    }

    #[test]
    fn a_row_takes_its_counts_from_the_tracker_when_it_knows_the_player() {
        let now = Timestamp::now();
        let tracked: HashMap<AccountId, tracked::TrackedPlayer> =
            [met(1, &[(1, false), (24 * 40, false)])].into_iter().collect();
        let rows = visible_player_rows(
            &[player(1, "Zeta", "ALPHA", 99)],
            &tracked,
            "",
            Some(now - (24 * 7).hours()),
            false,
            Sort::default(),
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total_encounters, Some(2), "the all-time count reaches past the period");
        assert_eq!(rows[0].encounters_in_range, 1, "only the meeting inside the period counts");
        assert!(rows[0].last_seen.is_some());
    }

    #[test]
    fn a_player_the_tracker_never_recorded_falls_back_to_the_indexed_count() {
        let rows =
            visible_player_rows(&[player(1, "Zeta", "ALPHA", 4)], &HashMap::new(), "", None, false, Sort::default());

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total_encounters, None, "there is no all-time figure to give");
        assert_eq!(rows[0].encounters_in_range, 4);
        assert_eq!(rows[0].last_seen, None);
    }

    #[test]
    fn a_player_met_only_in_division_leaves_the_table_while_the_toggle_is_off() {
        let tracked: HashMap<AccountId, tracked::TrackedPlayer> = [met(1, &[(1, true)])].into_iter().collect();
        let players = [player(1, "Zeta", "ALPHA", 1)];

        assert!(visible_player_rows(&players, &tracked, "", None, false, Sort::default()).is_empty());
        let shown = visible_player_rows(&players, &tracked, "", None, true, Sort::default());
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].encounters_in_range, 1);
    }

    #[test]
    fn rows_sort_by_the_column_asked_for() {
        let tracked: HashMap<AccountId, tracked::TrackedPlayer> =
            [met(1, &[(1, false)]), met(2, &[(2, false), (3, false), (4, false)])].into_iter().collect();
        let players = [player(1, "Zeta", "ALPHA", 1), player(2, "Alpha", "ZULU", 3)];

        let by_count = visible_player_rows(&players, &tracked, "", None, false, Sort::default());
        assert_eq!(by_count[0].facet.latest_name, "Alpha", "the most-met player leads");

        let by_last_seen = visible_player_rows(
            &players,
            &tracked,
            "",
            None,
            false,
            Sort { column: SortColumn::LastEncountered, order: SortOrder::Descending },
        );
        assert_eq!(by_last_seen[0].facet.latest_name, "Zeta", "the most recently met player leads");
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
        let bounded: Vec<Timestamp> = TimePeriod::ALL.iter().filter_map(|period| period.earliest(now)).collect();

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
        let tied: Vec<i64> = rows.iter().filter(|row| row.match_count == 9).map(|row| row.account_id.raw()).collect();
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

/// Which column the clans table is ordered by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClanSortColumn {
    Clan,
    Members,
    /// Battles against the clan, whenever they were.
    #[default]
    Encounters,
    /// Only the ones inside the period on screen.
    EncountersInRange,
    /// Players of the clan met, counted per battle rather than per account.
    Sightings,
    LastEncountered,
}

impl ClanSortColumn {
    pub const ALL: [ClanSortColumn; 6] =
        [Self::Clan, Self::Members, Self::Encounters, Self::EncountersInRange, Self::Sightings, Self::LastEncountered];

    /// The translation key the column's heading reads from.
    pub fn label_key(self) -> &'static str {
        match self {
            Self::Clan => "ui.player_tracker.column.clan",
            Self::Members => "ui.player_tracker.column.members",
            Self::Encounters => "ui.player_tracker.column.matches",
            Self::EncountersInRange => "ui.player_tracker.column.encounters_in_range",
            Self::Sightings => "ui.player_tracker.column.sightings",
            Self::LastEncountered => "ui.player_tracker.column.last_encountered",
        }
    }

    pub fn default_order(self) -> SortOrder {
        match self {
            Self::Clan => SortOrder::Ascending,
            Self::Members | Self::Encounters | Self::EncountersInRange | Self::Sightings | Self::LastEncountered => {
                SortOrder::Descending
            }
        }
    }
}

/// How the clans table is ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClanSort {
    pub column: ClanSortColumn,
    pub order: SortOrder,
}

impl Default for ClanSort {
    fn default() -> Self {
        Self { column: ClanSortColumn::Encounters, order: ClanSortColumn::Encounters.default_order() }
    }
}

impl ClanSort {
    /// The same three states [`Sort::toggled`] cycles through.
    pub fn toggled(self, column: ClanSortColumn) -> Self {
        if self.column != column {
            return Self { column, order: column.default_order() };
        }
        if self.order == column.default_order() {
            Self { column, order: self.order.reversed() }
        } else {
            Self::default()
        }
    }
}

#[cfg(test)]
mod clan_tests {
    use super::ClanSort;
    use super::ClanSortColumn;
    use super::SortOrder;

    #[test]
    fn clicking_the_same_column_flips_it_and_a_new_one_starts_at_its_own_default() {
        let sort = ClanSort::default();
        assert_eq!(sort.order, SortOrder::Descending);
        assert_eq!(sort.toggled(ClanSortColumn::Encounters).order, SortOrder::Ascending);
        assert_eq!(sort.toggled(ClanSortColumn::Clan).order, SortOrder::Ascending, "tags read A to Z");
    }
}

#[cfg(test)]
mod player_link_tests {
    use super::shipbuilds_player_url;
    use super::wows_numbers_player_url;
    use crate::match_stats::Region;
    use wows_replays::types::AccountId;

    /// Both links carry the region, the account and the name, because the
    /// sites key on the id and show the name.
    #[test]
    fn a_player_links_to_both_sites_by_region_and_id() {
        let id = AccountId(123456789);
        assert_eq!(
            wows_numbers_player_url(Region::Eu, id, "gapedd"),
            "https://eu.wows-numbers.com/player/123456789,gapedd"
        );
        assert_eq!(
            shipbuilds_player_url(Region::Eu, id, "gapedd"),
            "https://shipbuilds.com/player/EU/123456789/gapedd"
        );
    }
}

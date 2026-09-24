//! Players met across indexed battles: which window of time is in view, how
//! the table is sorted, and how a typed filter narrows it.
//!
//! The rows come from the shared replay index; this decides what the table
//! shows of them, the same way in both front ends.

pub mod clans;
pub mod history;
pub mod live;

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

/// One clan's aggregate across the players met from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClanRow {
    pub clan: String,
    /// Distinct accounts met wearing this clan's tag.
    pub members_met: usize,
    /// Encounters with those accounts, summed.
    pub encounters: i64,
}

/// Which column the clans table is ordered by.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClanSortColumn {
    Clan,
    Members,
    #[default]
    Encounters,
}

impl ClanSortColumn {
    pub const ALL: [ClanSortColumn; 3] = [Self::Clan, Self::Members, Self::Encounters];

    pub fn label(self) -> &'static str {
        match self {
            Self::Clan => "Clan",
            Self::Members => "Members met",
            Self::Encounters => "Encounters",
        }
    }

    pub fn default_order(self) -> SortOrder {
        match self {
            Self::Clan => SortOrder::Ascending,
            Self::Members | Self::Encounters => SortOrder::Descending,
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
    pub fn toggled(self, column: ClanSortColumn) -> Self {
        if self.column == column {
            Self { column, order: self.order.reversed() }
        } else {
            Self { column, order: column.default_order() }
        }
    }
}

/// Groups the players met into their clans.
///
/// Players with no clan tag are left out rather than gathered under an empty
/// name: "no clan" is not a clan, and a row for it would top the table on
/// every account met solo.
pub fn clan_rows(players: &[PlayerFacet], needle: &str, sort: ClanSort) -> Vec<ClanRow> {
    let mut by_clan: std::collections::BTreeMap<&str, ClanRow> = std::collections::BTreeMap::new();

    for player in players.iter().filter(|player| !player.clan.is_empty()) {
        let row = by_clan.entry(player.clan.as_str()).or_insert_with(|| ClanRow {
            clan: player.clan.clone(),
            members_met: 0,
            encounters: 0,
        });
        row.members_met += 1;
        row.encounters += player.match_count;
    }

    let needle = needle.to_lowercase();
    let mut rows: Vec<ClanRow> =
        by_clan.into_values().filter(|row| needle.is_empty() || row.clan.to_lowercase().contains(&needle)).collect();

    rows.sort_by(|a, b| {
        let ordering = match sort.column {
            ClanSortColumn::Clan => a.clan.to_lowercase().cmp(&b.clan.to_lowercase()),
            ClanSortColumn::Members => a.members_met.cmp(&b.members_met),
            ClanSortColumn::Encounters => a.encounters.cmp(&b.encounters),
        };
        // Ties break on the tag so the order is stable between refreshes.
        sort.order.apply(ordering).then_with(|| a.clan.cmp(&b.clan))
    });
    rows
}

#[cfg(test)]
mod clan_tests {
    use super::ClanSort;
    use super::ClanSortColumn;
    use super::SortOrder;
    use super::clan_rows;
    use wows_toolkit_config::index::rows::PlayerFacet;

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
            player(1, "a", "ALPHA", 3),
            player(2, "b", "ALPHA", 4),
            player(3, "c", "ZULU", 10),
            player(4, "solo", "", 99),
        ]
    }

    #[test]
    fn players_are_gathered_into_their_clans() {
        let rows = clan_rows(&sample(), "", ClanSort::default());
        assert_eq!(rows.len(), 2, "two clans, and the clanless player is not one");

        let alpha = rows.iter().find(|row| row.clan == "ALPHA").unwrap();
        assert_eq!(alpha.members_met, 2);
        assert_eq!(alpha.encounters, 7, "three plus four");
    }

    #[test]
    fn a_player_with_no_tag_is_left_out_rather_than_gathered_under_an_empty_name() {
        let rows = clan_rows(&sample(), "", ClanSort::default());
        assert!(rows.iter().all(|row| !row.clan.is_empty()));
        assert!(rows.iter().all(|row| row.encounters != 99), "the solo player's count is nowhere");
    }

    #[test]
    fn encounters_lead_by_default_so_the_most_met_clan_is_first() {
        let rows = clan_rows(&sample(), "", ClanSort::default());
        assert_eq!(rows[0].clan, "ZULU", "ten beats seven");
    }

    #[test]
    fn sorting_by_members_counts_accounts_rather_than_encounters() {
        let sort = ClanSort { column: ClanSortColumn::Members, order: SortOrder::Descending };
        let rows = clan_rows(&sample(), "", sort);
        assert_eq!(rows[0].clan, "ALPHA", "two members beats one, despite fewer encounters");
    }

    #[test]
    fn the_filter_narrows_by_tag() {
        assert_eq!(clan_rows(&sample(), "zul", ClanSort::default()).len(), 1);
        assert!(clan_rows(&sample(), "nothing", ClanSort::default()).is_empty());
    }

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

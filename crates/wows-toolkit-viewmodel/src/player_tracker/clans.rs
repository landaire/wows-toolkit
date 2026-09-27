//! Aggregating tracked encounters by clan.
//!
//! Both front ends show a clans table over the same tracked history, so the
//! counting rule -- which encounters a clan gets credit for, and which the
//! division-mate toggle hides -- lives here rather than in either of them.

use std::collections::HashMap;
use std::collections::HashSet;

use jiff::Timestamp;
use wows_replays::types::AccountId;
use wows_replays::types::ArenaId;
use wows_toolkit_config::index::rows::ClanCorrection;

use super::tracked::TrackedPlayer;

/// One clan's encounter aggregates.
#[derive(Debug, Clone)]
pub struct ClanRow {
    pub clan: String,
    /// Accounts with at least one encounter attributed here, with the match
    /// count each contributed. Drives the expanded member list.
    pub members: Vec<(AccountId, usize)>,
    pub matches: usize,
    pub matches_in_range: usize,
    pub sightings: usize,
    pub sightings_in_range: usize,
    pub last_seen: Timestamp,
}

#[derive(Default)]
struct ClanAccumulator {
    members: HashMap<AccountId, usize>,
    arenas: HashSet<ArenaId>,
    range_timestamps: HashSet<Timestamp>,
    sightings: usize,
    sightings_in_range: usize,
    last_seen: Option<Timestamp>,
}

/// Aggregate tracked encounters by clan.
///
/// `index_latest_clan` is the index's latest clan per account, which wins over
/// the tracker's when the index knows the account. `corrections` are the roster
/// rows whose clan at the time differed from that latest clan.
///
/// Matches are counted by distinct arena and in-range matches by distinct
/// timestamp. Both keys are exact on their own, which is why the tracker's
/// unpaired `arena_ids` and `timestamps` sets need no reconciliation: every
/// player in a battle shares that battle's timestamp.
///
/// While `show_division_mates` is off, the encounters marked as division ones
/// contribute nothing at all, so a clan's matches, sightings and member count
/// are not inflated by the battles you arranged with them. Filtering happens
/// per encounter under each of the two keys, so a player met both in and out of
/// your division still contributes the meetings outside it.
///
pub fn build_clan_breakdown(
    tracked: &HashMap<AccountId, TrackedPlayer>,
    index_latest_clan: &HashMap<AccountId, String>,
    corrections: &[ClanCorrection],
    since: Option<Timestamp>,
    show_division_mates: bool,
) -> Vec<ClanRow> {
    let mut by_arena: HashMap<(AccountId, ArenaId), &str> = HashMap::new();
    let mut by_timestamp: HashMap<(AccountId, Timestamp), &str> = HashMap::new();
    for correction in corrections {
        by_arena.insert((correction.account_id, correction.arena_id), &correction.clan);
        by_timestamp.insert((correction.account_id, correction.timestamp), &correction.clan);
    }

    let mut clans: HashMap<String, ClanAccumulator> = HashMap::new();

    for (account_id, player) in tracked {
        // The tracker holds one clan per player, the latest it saw. The index's
        // latest is fresher wherever the index knows the account at all.
        let baseline = index_latest_clan.get(account_id).map(String::as_str).unwrap_or(player.clan.as_str());

        for arena_id in player.visible_arena_ids(show_division_mates) {
            let clan = by_arena.get(&(*account_id, arena_id)).copied().unwrap_or(baseline);
            if clan.is_empty() {
                continue;
            }
            let entry = clans.entry(clan.to_string()).or_default();
            entry.arenas.insert(arena_id);
            entry.sightings += 1;
            *entry.members.entry(*account_id).or_default() += 1;
        }

        for timestamp in player.visible_timestamps(show_division_mates) {
            let clan = by_timestamp.get(&(*account_id, timestamp)).copied().unwrap_or(baseline);
            if clan.is_empty() {
                continue;
            }
            let entry = clans.entry(clan.to_string()).or_default();
            entry.last_seen = Some(entry.last_seen.map_or(timestamp, |seen| seen.max(timestamp)));

            if since.is_none_or(|since| timestamp > since) {
                entry.range_timestamps.insert(timestamp);
                entry.sightings_in_range += 1;
            }
        }
    }

    let mut rows: Vec<ClanRow> = clans
        .into_iter()
        .filter_map(|(clan, acc)| {
            // A clan only reached here through an encounter, so `last_seen` is
            // set unless the arena and timestamp passes disagreed on the label,
            // which a correction to a different clan can cause. Drop those
            // rather than invent a timestamp, and say so: the row would
            // otherwise vanish with a non-zero match count and no signal.
            let Some(last_seen) = acc.last_seen else {
                tracing::warn!(
                    clan = clan.as_str(),
                    matches = acc.arenas.len(),
                    "clan breakdown: dropping a clan whose encounters carry no timestamp"
                );
                return None;
            };
            let mut members: Vec<(AccountId, usize)> = acc.members.into_iter().collect();
            members.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.raw().cmp(&b.0.raw())));

            Some(ClanRow {
                clan,
                members,
                matches: acc.arenas.len(),
                matches_in_range: acc.range_timestamps.len(),
                sightings: acc.sightings,
                sightings_in_range: acc.sightings_in_range,
                last_seen,
            })
        })
        .collect();

    rows.sort_by(|a, b| b.matches.cmp(&a.matches).then_with(|| a.clan.cmp(&b.clan)));

    rows
}

/// The rows a clans table shows: those whose tag matches `needle`, in the
/// order `sort` asks for.
///
/// The tag breaks every tie so the order is total, and does not shuffle
/// between refreshes when two clans carry the same counts.
pub fn visible_clans(rows: Vec<ClanRow>, needle: &str, sort: super::ClanSort) -> Vec<ClanRow> {
    use super::ClanSortColumn;

    let needle = needle.to_lowercase();
    let mut rows: Vec<ClanRow> =
        rows.into_iter().filter(|row| needle.is_empty() || row.clan.to_lowercase().contains(&needle)).collect();

    rows.sort_by(|a, b| {
        let ordering = match sort.column {
            ClanSortColumn::Clan => a.clan.to_lowercase().cmp(&b.clan.to_lowercase()),
            ClanSortColumn::Members => a.members.len().cmp(&b.members.len()),
            ClanSortColumn::Encounters => a.matches.cmp(&b.matches),
            ClanSortColumn::EncountersInRange => a.matches_in_range.cmp(&b.matches_in_range),
            ClanSortColumn::Sightings => a.sightings.cmp(&b.sightings),
            ClanSortColumn::LastEncountered => a.last_seen.cmp(&b.last_seen),
        };
        sort.order.apply(ordering).then_with(|| a.clan.cmp(&b.clan))
    });
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player_tracker::ClanSort;
    use crate::player_tracker::ClanSortColumn;
    use crate::player_tracker::SortOrder;

    fn at(minute: i64) -> Timestamp {
        Timestamp::from_second(1_700_000_000 + minute * 60).expect("a valid timestamp")
    }

    fn player(clan: &str, encounters: &[(i64, i64)]) -> TrackedPlayer {
        TrackedPlayer {
            clan: clan.to_string(),
            arena_ids: encounters.iter().map(|(arena, _)| ArenaId::from(*arena)).collect(),
            timestamps: encounters.iter().map(|(_, minute)| at(*minute)).collect(),
            ..TrackedPlayer::default()
        }
    }

    /// A battle you arranged says less about meeting someone than one you did
    /// not, so the toggle takes those encounters out of the counts entirely.
    #[test]
    fn a_division_encounter_is_left_out_until_the_toggle_asks_for_it() {
        let mut tracked = HashMap::new();
        let mut me = player("WTK", &[(1, 10), (2, 20)]);
        me.division_encounters.mark(ArenaId::from(2i64), at(20));
        tracked.insert(AccountId(7), me);

        let hidden = build_clan_breakdown(&tracked, &HashMap::new(), &[], None, false);
        assert_eq!(hidden.len(), 1);
        assert_eq!(hidden[0].matches, 1, "only the battle you did not arrange counts");

        let shown = build_clan_breakdown(&tracked, &HashMap::new(), &[], None, true);
        assert_eq!(shown[0].matches, 2, "the toggle puts it back");
    }

    /// The index's clan is fresher than the tracker's wherever it knows the
    /// account, which is what keeps a renamed clan from splitting in two.
    #[test]
    fn the_index_clan_wins_over_the_tracked_one() {
        let mut tracked = HashMap::new();
        tracked.insert(AccountId(7), player("OLD", &[(1, 10)]));

        let latest = HashMap::from([(AccountId(7), "NEW".to_string())]);
        let rows = build_clan_breakdown(&tracked, &latest, &[], None, false);

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].clan, "NEW");
    }

    #[test]
    fn a_clanless_player_is_no_clans_encounter() {
        let mut tracked = HashMap::new();
        tracked.insert(AccountId(7), player("", &[(1, 10)]));
        assert!(build_clan_breakdown(&tracked, &HashMap::new(), &[], None, false).is_empty());
    }

    #[test]
    fn the_table_filters_on_the_tag_and_orders_by_the_chosen_column() {
        let mut tracked = HashMap::new();
        tracked.insert(AccountId(1), player("AAA", &[(1, 10)]));
        tracked.insert(AccountId(2), player("BBB", &[(2, 20), (3, 30)]));

        let rows = build_clan_breakdown(&tracked, &HashMap::new(), &[], None, false);

        let by_encounters = visible_clans(
            rows.clone(),
            "",
            ClanSort { column: ClanSortColumn::Encounters, order: SortOrder::Descending },
        );
        let tags: Vec<&str> = by_encounters.iter().map(|row| row.clan.as_str()).collect();
        assert_eq!(tags, vec!["BBB", "AAA"], "the busiest clan leads");

        let filtered =
            visible_clans(rows, "aa", ClanSort { column: ClanSortColumn::Clan, order: SortOrder::Ascending });
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].clan, "AAA");
    }
}

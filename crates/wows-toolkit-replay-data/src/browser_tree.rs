//! File-browser grouping model: `ReplayLite` (a replay's minimal, cheaply
//! read summary) and `build_browser_tree`, which groups replay files by
//! Date/Ship/None and builds the group/leaf display labels. Pure: no
//! UI toolkit types. Mirrors `ui/replay_parser/mod.rs`'s
//! `build_file_listing_grouped`/`build_file_listing_ungrouped`
//! (`mod.rs:3267-3448`), `win_rate_label` (`mod.rs:150`), and `colorize_label`
//! (`mod.rs:136`, whose win/loss/draw color is resolved by the render layer
//! from `battle_result`, not stored as text here).
//!
//! A row's own two lines and its outcome come from the shared assembly
//! (`wows_toolkit_viewmodel::listing_row`), so the listing reads the same in
//! both apps; this module only decides which rows sit under which group.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::hash::Hash;
use std::path::PathBuf;

use jiff::civil::DateTime;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::RowSummary;
use wows_toolkit_viewmodel::listing_row::LinePart;
use wows_toolkit_viewmodel::listing_row::RowIdentity;
use wows_toolkit_viewmodel::listing_row::RowStats;
use wows_toolkit_viewmodel::listing_row::identity_line;
use wows_toolkit_viewmodel::listing_row::listed_row_identity;
use wows_toolkit_viewmodel::listing_row::resolve_row_stats;
use wows_toolkit_viewmodel::listing_row::stats_line_parts;
use wowsunpack::game_params::provider::GameMetadataProvider;

use crate::force_reindex::HEADER_DATE_FORMAT;
use crate::load::GameDataCache;
use crate::replay_dir::ScannedReplay;

/// A replay file's minimal summary for the file browser: enough to sort,
/// group, and label it without a full packet parse.
#[derive(Debug, Clone)]
pub struct ReplayLite {
    pub path: PathBuf,
    /// The map as the replay names it (`spaces/ocean`), which is what the
    /// hover preview loads its art by. The translated name the row draws is
    /// in `identity`.
    pub map_name: String,
    /// What the row names: ship, map, scenario, mode and timestamp, already
    /// translated against the loaded build.
    pub identity: RowIdentity,
    /// What the row reports: outcome, damage, kills, division. From the
    /// replay index, so it is unknown until the index has seen the file.
    pub stats: RowStats,
}

impl ReplayLite {
    /// `scanned` named against its own build when `cache` has it loaded, else against
    /// `fallback`, and reported from the index's `summary`. With no game data at all the row
    /// shows the raw ship id and map name.
    pub fn from_scanned(
        scanned: &ScannedReplay,
        cache: Option<&GameDataCache>,
        fallback: Option<&GameMetadataProvider>,
        summary: Option<&RowSummary>,
    ) -> Self {
        let listed = &scanned.listed;
        let loaded = listed.build.and_then(|build| cache?.loaded_build(build));
        let metadata = loaded.as_ref().map(|data| data.provider().as_ref()).or(fallback);
        let identity = match metadata {
            Some(metadata) => listed_row_identity(listed, metadata),
            None => RowIdentity {
                ship: listed.ship_id.map(|id| id.to_string()).unwrap_or_else(|| "Spectator".to_owned()),
                map: listed.map_name.clone(),
                scenario: listed.scenario.clone(),
                mode: listed.game_type.clone(),
                date_time: listed.date_time.clone(),
            },
        };

        Self {
            path: scanned.path.clone(),
            map_name: listed.map_name.clone(),
            identity,
            stats: resolve_row_stats(None, summary),
        }
    }
}

/// What a group gathers its replays by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupKind {
    Date,
    Ship,
}

impl GroupKind {
    /// The grouping that makes groups of this kind, which decides what its rows leave out.
    fn grouping(self) -> ReplayGrouping {
        match self {
            Self::Date => ReplayGrouping::Date,
            Self::Ship => ReplayGrouping::Ship,
        }
    }
}

/// One row of the file-browser tree: a Date/Ship group with its children, or
/// a single replay leaf. A leaf carries both of its lines plus the outcome
/// its first line is tinted by; a group's label already encodes its win rate.
#[derive(Debug, Clone)]
pub enum BrowserNode {
    Group {
        kind: GroupKind,
        /// The date or ship the group gathers, which stays while its count and results change.
        name: String,
        label: String,
        children: Vec<BrowserNode>,
    },
    Leaf {
        label: String,
        /// "Ship - Map", whatever the grouping drops from `label`.
        battle: String,
        /// The second line: damage, kills and the timestamp. In pieces, since
        /// this front end draws its glyphs as icons.
        stats: Vec<LinePart>,
        path: PathBuf,
        /// The map as the replay names it, for the hover preview's art.
        map_name: String,
        outcome: MatchOutcome,
        in_division: bool,
        /// Whether a player other than the self player was seen in Twitch chat.
        in_twitch_chat: bool,
    },
}

/// Builds the file-browser tree for `grouping`, newest battle first; the
/// caller does not need to pre-sort.
///
/// Replays are ordered by the battle time their headers record, not by path:
/// one folder's replays can lie in several directories (an overlaid install's
/// layers, versioned subfolders), whose names would otherwise order them. A
/// header time that does not parse sorts after every dated replay, and the
/// file name, which the game leads with the time, orders the rest.
pub fn build_browser_tree(files: &[ReplayLite], grouping: ReplayGrouping, locale: Option<&str>) -> Vec<BrowserNode> {
    let mut sorted: Vec<&ReplayLite> = files.iter().collect();
    sorted.sort_by_cached_key(|&r| {
        let played_at = DateTime::strptime(HEADER_DATE_FORMAT, &r.identity.date_time).ok();
        Reverse((played_at, r.path.file_name(), r.path.as_path()))
    });

    match grouping {
        ReplayGrouping::None => sorted.into_iter().map(|r| leaf(r, ReplayGrouping::None, locale)).collect(),
        ReplayGrouping::Date => {
            let groups = first_seen_groups(&sorted, |r| {
                r.identity.date_time.split(' ').next().unwrap_or(&r.identity.date_time).to_string()
            });
            groups.into_iter().map(|(date, replays)| group_node(GroupKind::Date, date, replays, locale)).collect()
        }
        ReplayGrouping::Ship => {
            let groups = first_seen_groups(&sorted, |r| r.identity.ship.clone());
            groups.into_iter().map(|(ship, replays)| group_node(GroupKind::Ship, ship, replays, locale)).collect()
        }
    }
}

/// One replay's leaf, both lines assembled the way the egui listing assembles
/// them (`ui/replay_parser/listing_row.rs`): the grouping decides which field
/// the identity line drops, since the group above already states it.
fn leaf(r: &ReplayLite, grouping: ReplayGrouping, locale: Option<&str>) -> BrowserNode {
    BrowserNode::Leaf {
        label: identity_line(&r.identity, grouping),
        battle: super::model::battle_title(&r.identity.ship, &r.identity.map),
        stats: stats_line_parts(&r.identity, &r.stats, grouping, locale),
        path: r.path.clone(),
        map_name: r.map_name.clone(),
        outcome: r.stats.outcome,
        in_division: r.stats.in_division,
        in_twitch_chat: !r.stats.twitch_chatters.is_empty(),
    }
}

/// `" - {W}W/{L}L ({pct}%)"` computed from the known battle results only
/// (entries with `battle_result: None` are skipped, matching the egui `_ =>`
/// arm); empty when no result in the group is known. Mirrors
/// `win_rate_label` (`mod.rs:150-162`).
fn win_rate_label(replays: &[&ReplayLite]) -> String {
    let (wins, losses) = replays.iter().fold((0u32, 0u32), |(w, l), r| match r.stats.outcome {
        MatchOutcome::Win => (w + 1, l),
        MatchOutcome::Loss => (w, l + 1),
        MatchOutcome::Draw | MatchOutcome::Unknown => (w, l),
    });
    let total = wins + losses;
    if total > 0 {
        format!(" - {wins}W/{losses}L ({:.0}%)", (wins as f64 / total as f64) * 100.0)
    } else {
        String::new()
    }
}

/// Builds one `BrowserNode::Group` from a named bucket of replays: label
/// `"{name} ({count})" + win_rate_label`, children labeled by `leaf_label`.
fn group_node(kind: GroupKind, name: String, replays: Vec<&ReplayLite>, locale: Option<&str>) -> BrowserNode {
    let label = format!("{} ({}){}", name, replays.len(), win_rate_label(&replays));
    let children = replays.into_iter().map(|r| leaf(r, kind.grouping(), locale)).collect();
    BrowserNode::Group { kind, name, label, children }
}

/// Buckets newest-first `files` by `key`, one group per key in first-seen
/// order. That orders the groups by their newest replay, and a replay joins
/// the group its key names wherever it falls in the order.
fn first_seen_groups<'a, K: Clone + Eq + Hash>(
    files: &[&'a ReplayLite],
    key: impl Fn(&ReplayLite) -> K,
) -> Vec<(K, Vec<&'a ReplayLite>)> {
    let mut groups: Vec<(K, Vec<&ReplayLite>)> = Vec::new();
    let mut by_key: HashMap<K, usize> = HashMap::new();
    for &r in files {
        let name = key(r);
        let index = *by_key.entry(name.clone()).or_insert_with(|| {
            groups.push((name, Vec::new()));
            groups.len() - 1
        });
        groups[index].1.push(r);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay(path: &str, ship: &str, map: &str, game_time: &str, outcome: MatchOutcome) -> ReplayLite {
        ReplayLite {
            map_name: map.to_string(),
            path: PathBuf::from(path),
            identity: RowIdentity {
                ship: ship.to_string(),
                map: map.to_string(),
                scenario: "Domination".to_string(),
                mode: "Randoms".to_string(),
                date_time: game_time.to_string(),
            },
            stats: RowStats {
                outcome,
                damage: None,
                kills: None,
                survived: None,
                in_division: false,
                division_mates: Vec::new(),
                twitch_chatters: Vec::new(),
                // Known, so the row reads as an indexed one; an unindexed row
                // is the `stats_line` fallback and has its own tests in the
                // shared assembly.
                known: true,
            },
        }
    }

    /// Flattens a tree to `(depth, label)` pairs for asserting shape.
    /// A leaf contributes its identity line; [`stats_lines`] covers the
    /// second one.
    fn flatten(nodes: &[BrowserNode], depth: usize, out: &mut Vec<(usize, String)>) {
        for node in nodes {
            match node {
                BrowserNode::Group { label, children, .. } => {
                    out.push((depth, label.clone()));
                    flatten(children, depth + 1, out);
                }
                BrowserNode::Leaf { label, .. } => out.push((depth, label.clone())),
            }
        }
    }

    /// Every leaf's second line, joined back into the text it draws.
    fn stats_lines(nodes: &[BrowserNode]) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(nodes: &[BrowserNode], out: &mut Vec<String>) {
            for node in nodes {
                match node {
                    BrowserNode::Group { children, .. } => walk(children, out),
                    BrowserNode::Leaf { stats, .. } => out.push(
                        stats
                            .iter()
                            .map(|part| match part {
                                LinePart::Text(text) => text.as_str(),
                                LinePart::Glyph(glyph) => glyph.codepoint(),
                            })
                            .collect(),
                    ),
                }
            }
        }
        walk(nodes, &mut out);
        out
    }

    fn labels(nodes: &[BrowserNode]) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        flatten(nodes, 0, &mut out);
        out
    }

    #[test]
    fn win_rate_label_counts_only_known_results_and_skips_none() {
        let replays = [
            replay("z", "Kleber", "Ocean", "01.01.2026 00:00:00", MatchOutcome::Win),
            replay("y", "Kleber", "Ocean", "01.01.2026 00:01:00", MatchOutcome::Win),
            replay("x", "Kleber", "Ocean", "01.01.2026 00:02:00", MatchOutcome::Loss),
            replay("w", "Kleber", "Ocean", "01.01.2026 00:03:00", MatchOutcome::Unknown),
            replay("v", "Kleber", "Ocean", "01.01.2026 00:04:00", MatchOutcome::Draw),
        ];
        let refs: Vec<&ReplayLite> = replays.iter().collect();
        // 2 wins, 1 loss out of the 3 known-outcome entries; the None entry
        // and the Draw entry are both excluded from the W/L tally.
        assert_eq!(win_rate_label(&refs), " - 2W/1L (67%)");
    }

    #[test]
    fn win_rate_label_is_empty_when_no_result_is_known() {
        let replays = [
            replay("a", "Kleber", "Ocean", "01.01.2026 00:00:00", MatchOutcome::Unknown),
            replay("b", "Kleber", "Ocean", "01.01.2026 00:01:00", MatchOutcome::Draw),
        ];
        let refs: Vec<&ReplayLite> = replays.iter().collect();
        assert_eq!(win_rate_label(&refs), "");
    }

    #[test]
    fn date_grouping_buckets_same_date_entries_newest_first() {
        // Battle dates are "02" for the b-paths and "01" for the a-paths, so
        // this is a clean two-group case.
        let files = vec![
            replay("replays/a1.wowsreplay", "Kleber", "Ocean", "01.01.2026 10:00:00", MatchOutcome::Unknown),
            replay("replays/a2.wowsreplay", "Kleber", "Volcano", "01.01.2026 11:30:00", MatchOutcome::Win),
            replay("replays/b1.wowsreplay", "Yamato", "Ocean", "02.01.2026 09:15:00", MatchOutcome::Loss),
            replay("replays/b2.wowsreplay", "Yamato", "Volcano", "02.01.2026 12:00:00", MatchOutcome::Unknown),
            replay("replays/b3.wowsreplay", "Yamato", "Ocean", "02.01.2026 14:45:00", MatchOutcome::Win),
        ];

        let tree = build_browser_tree(&files, ReplayGrouping::Date, None);

        assert_eq!(
            labels(&tree),
            vec![
                (0, "02.01.2026 (3) - 1W/1L (50%)".to_string()),
                (1, "Yamato - Ocean".to_string()),
                (1, "Yamato - Volcano".to_string()),
                (1, "Yamato - Ocean".to_string()),
                (0, "01.01.2026 (2) - 1W/0L (100%)".to_string()),
                (1, "Kleber - Volcano".to_string()),
                (1, "Kleber - Ocean".to_string()),
            ]
        );
    }

    /// An overlaid install's replays lie in two directories, the older ones
    /// in a layer whose path sorts after the newer ones'. The listing still
    /// leads with the newest battle, and orders days across a year end,
    /// which the header's day-first text does not.
    #[test]
    fn replays_in_several_directories_list_newest_battle_first() {
        let files = vec![
            replay("/mnt/r/20251231_a.wowsreplay", "Kleber", "Ocean", "31.12.2025 20:00:00", MatchOutcome::Unknown),
            replay("/mnt/r/20261116_b.wowsreplay", "Kleber", "Ocean", "16.11.2026 12:00:00", MatchOutcome::Unknown),
            replay("/home/r/20261119_c.wowsreplay", "Yamato", "Volcano", "19.11.2026 15:02:19", MatchOutcome::Unknown),
            replay("/home/r/20260101_d.wowsreplay", "Yamato", "Ocean", "01.01.2026 09:00:00", MatchOutcome::Unknown),
        ];

        let tree = build_browser_tree(&files, ReplayGrouping::Date, None);

        assert_eq!(
            labels(&tree),
            vec![
                (0, "19.11.2026 (1)".to_string()),
                (1, "Yamato - Volcano".to_string()),
                (0, "16.11.2026 (1)".to_string()),
                (1, "Kleber - Ocean".to_string()),
                (0, "01.01.2026 (1)".to_string()),
                (1, "Yamato - Ocean".to_string()),
                (0, "31.12.2025 (1)".to_string()),
                (1, "Kleber - Ocean".to_string()),
            ]
        );
    }

    /// A header time that does not parse places its replay after every dated one.
    #[test]
    fn a_replay_without_a_battle_time_lists_last() {
        let files = vec![
            replay("replays/b.wowsreplay", "Yamato", "Volcano", "", MatchOutcome::Unknown),
            replay("replays/a.wowsreplay", "Kleber", "Ocean", "01.01.2026 09:00:00", MatchOutcome::Unknown),
        ];

        let tree = build_browser_tree(&files, ReplayGrouping::None, None);

        assert_eq!(labels(&tree), vec![(0, "Kleber - Ocean".to_string()), (0, "Yamato - Volcano".to_string())]);
    }

    #[test]
    fn ship_grouping_orders_groups_by_each_ships_most_recent_replay() {
        // Kleber's most recent battle is "c", Yamato's is "b", so Kleber's
        // group comes first even though it has fewer replays and its file
        // lies in a directory whose path sorts last.
        let files = vec![
            replay("z/a.wowsreplay", "Yamato", "Ocean", "01.01.2026 08:00:00", MatchOutcome::Loss),
            replay("z/b.wowsreplay", "Yamato", "Volcano", "01.01.2026 09:00:00", MatchOutcome::Win),
            replay("a/c.wowsreplay", "Kleber", "Ocean", "01.01.2026 10:00:00", MatchOutcome::Win),
        ];

        let tree = build_browser_tree(&files, ReplayGrouping::Ship, None);

        assert_eq!(
            labels(&tree),
            vec![
                (0, "Kleber (1) - 1W/0L (100%)".to_string()),
                (1, "Ocean".to_string()),
                (0, "Yamato (2) - 1W/1L (50%)".to_string()),
                (1, "Volcano".to_string()),
                (1, "Ocean".to_string()),
            ]
        );
    }

    #[test]
    fn none_grouping_is_a_flat_newest_first_leaf_list() {
        let files = vec![
            replay("replays/20260101_a.wowsreplay", "Kleber", "Ocean", "01.01.2026 09:00:00", MatchOutcome::Win),
            replay("replays/20260102_b.wowsreplay", "Yamato", "Volcano", "02.01.2026 09:00:00", MatchOutcome::Unknown),
        ];

        let tree = build_browser_tree(&files, ReplayGrouping::None, None);

        assert_eq!(labels(&tree), vec![(0, "Yamato - Volcano".to_string()), (0, "Kleber - Ocean".to_string())]);
        // Flat: no group wrapping, so every entry is a Leaf at depth 0.
        assert!(tree.iter().all(|n| matches!(n, BrowserNode::Leaf { .. })));
    }

    /// The row's second line is what the index knows, and it carries the
    /// timestamp the identity line drops -- only the time of day under a
    /// date group, the whole stamp otherwise, matching the egui listing.
    #[test]
    fn the_stats_line_carries_the_timestamp_the_identity_line_drops() {
        let files = vec![replay("a", "Yamato", "Ocean", "01.01.2026 14:45:00", MatchOutcome::Win)];

        let dated = stats_lines(&build_browser_tree(&files, ReplayGrouping::Date, None));
        assert_eq!(dated.len(), 1);
        assert!(dated[0].ends_with("14:45"), "a date group states the date, so the row shows the time: {dated:?}");
        assert!(!dated[0].contains("01.01.2026"), "and not the date again: {dated:?}");

        let ungrouped = stats_lines(&build_browser_tree(&files, ReplayGrouping::None, None));
        assert!(
            ungrouped[0].contains("01.01.2026 14:45"),
            "with no group to state it, the row carries the whole stamp: {ungrouped:?}"
        );
    }

    /// A file the index has never seen says so rather than showing blank
    /// figures.
    #[test]
    fn an_unindexed_row_says_it_is_not_indexed() {
        let mut file = replay("a", "Yamato", "Ocean", "01.01.2026 14:45:00", MatchOutcome::Unknown);
        file.stats.known = false;

        let lines = stats_lines(&build_browser_tree(&[file], ReplayGrouping::Date, None));
        assert!(!lines[0].is_empty());
        assert!(lines[0].ends_with("14:45"), "the timestamp is still there: {lines:?}");
    }

    #[test]
    fn leaf_nodes_carry_the_source_path_and_outcome() {
        let files =
            vec![replay("replays/only.wowsreplay", "Kleber", "Ocean", "01.01.2026 09:00:00", MatchOutcome::Draw)];

        let tree = build_browser_tree(&files, ReplayGrouping::Date, None);
        let BrowserNode::Group { children, .. } = &tree[0] else { panic!("expected a Date group") };
        let BrowserNode::Leaf { path, outcome, .. } = &children[0] else { panic!("expected a leaf") };

        assert_eq!(path, &PathBuf::from("replays/only.wowsreplay"));
        assert!(matches!(outcome, MatchOutcome::Draw));
    }
}

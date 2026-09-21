//! File-browser grouping model: `ReplayLite` (a replay's minimal, cheaply
//! read summary) and `build_browser_tree`, which groups replay files by
//! Date/Ship/None and builds the group/leaf display labels. Pure: no
//! `gpui_kit`/`egui` types. Mirrors `ui/replay_parser/mod.rs`'s
//! `build_file_listing_grouped`/`build_file_listing_ungrouped`
//! (`mod.rs:3267-3448`), `win_rate_label` (`mod.rs:150`), and `colorize_label`
//! (`mod.rs:136`, whose win/loss/draw color is resolved by the render layer
//! from `battle_result`, not stored as text here).
//!
//! A row's own two lines and its outcome come from the shared assembly
//! (`wows_toolkit_viewmodel::listing_row`), so the listing reads the same in
//! both apps; this module only decides which rows sit under which group.

use std::collections::HashMap;
use std::path::PathBuf;

use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_viewmodel::listing_row::LinePart;
use wows_toolkit_viewmodel::listing_row::RowIdentity;
use wows_toolkit_viewmodel::listing_row::RowStats;
use wows_toolkit_viewmodel::listing_row::identity_line;
use wows_toolkit_viewmodel::listing_row::stats_line_parts;

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

/// One row of the file-browser tree: a Date/Ship group with its children, or
/// a single replay leaf. A leaf carries both of its lines plus the outcome
/// its first line is tinted by; a group's label already encodes its win rate.
#[derive(Debug, Clone)]
pub enum BrowserNode {
    Group {
        label: String,
        children: Vec<BrowserNode>,
    },
    Leaf {
        label: String,
        /// The second line: damage, kills and the timestamp. In pieces, since
        /// this front end keeps its icons in a font of their own.
        stats: Vec<LinePart>,
        path: PathBuf,
        /// The map as the replay names it, for the hover preview's art.
        map_name: String,
        outcome: MatchOutcome,
        in_division: bool,
    },
}

/// Builds the file-browser tree for `grouping`. Sorts `files` by path
/// descending first (mirroring both egui listing functions' own
/// `files.sort_by(|a, b| b.0.cmp(&a.0))`), so replay filenames' leading
/// timestamps put the newest replay first; the caller does not need to
/// pre-sort.
pub fn build_browser_tree(files: &[ReplayLite], grouping: ReplayGrouping, locale: Option<&str>) -> Vec<BrowserNode> {
    let mut sorted: Vec<&ReplayLite> = files.iter().collect();
    sorted.sort_by(|a, b| b.path.cmp(&a.path));

    match grouping {
        ReplayGrouping::None => sorted.into_iter().map(|r| leaf(r, ReplayGrouping::None, locale)).collect(),
        ReplayGrouping::Date => build_date_groups(&sorted, locale),
        ReplayGrouping::Ship => build_ship_groups(&sorted, locale),
    }
}

/// One replay's leaf, both lines assembled the way the egui listing assembles
/// them (`ui/replay_parser/listing_row.rs`): the grouping decides which field
/// the identity line drops, since the group above already states it.
fn leaf(r: &ReplayLite, grouping: ReplayGrouping, locale: Option<&str>) -> BrowserNode {
    BrowserNode::Leaf {
        label: identity_line(&r.identity, grouping),
        stats: stats_line_parts(&r.identity, &r.stats, grouping, locale),
        path: r.path.clone(),
        map_name: r.map_name.clone(),
        outcome: r.stats.outcome,
        in_division: r.stats.in_division,
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
fn group_node(name: String, replays: Vec<&ReplayLite>, grouping: ReplayGrouping, locale: Option<&str>) -> BrowserNode {
    let label = format!("{} ({}){}", name, replays.len(), win_rate_label(&replays));
    let children = replays.into_iter().map(|r| leaf(r, grouping, locale)).collect();
    BrowserNode::Group { label, children }
}

/// Groups path-descending-sorted `files` by the date part of `game_time`
/// (`game_time.split(' ').next()`), as a run of consecutive same-date entries
/// rather than a global grouping by date key: if two runs of the same date
/// are separated by a different date (an out-of-order file timestamp), they
/// become two separate groups. Mirrors `mod.rs:3330-3344` exactly, including
/// this quirk.
fn build_date_groups(files: &[&ReplayLite], locale: Option<&str>) -> Vec<BrowserNode> {
    let mut groups: Vec<(String, Vec<&ReplayLite>)> = Vec::new();
    for &r in files {
        let date = r.identity.date_time.split(' ').next().unwrap_or(&r.identity.date_time).to_string();
        if let Some((last_date, last_group)) = groups.last_mut()
            && *last_date == date
        {
            last_group.push(r);
            continue;
        }
        groups.push((date, vec![r]));
    }
    groups.into_iter().map(|(date, replays)| group_node(date, replays, ReplayGrouping::Date, locale)).collect()
}

/// Groups path-descending-sorted `files` by ship name. Groups are ordered by
/// each ship's most recent replay: since `files` is already newest-first, a
/// ship's first occurrence during the scan is its most recent path. Mirrors
/// `mod.rs:3345-3359`.
fn build_ship_groups(files: &[&ReplayLite], locale: Option<&str>) -> Vec<BrowserNode> {
    let mut ship_groups: HashMap<&str, Vec<&ReplayLite>> = HashMap::new();
    let mut ship_most_recent: HashMap<&str, &PathBuf> = HashMap::new();
    for &r in files {
        ship_groups.entry(r.identity.ship.as_str()).or_default().push(r);
        ship_most_recent.entry(r.identity.ship.as_str()).or_insert(&r.path);
    }

    let mut groups: Vec<(&str, Vec<&ReplayLite>)> = ship_groups.into_iter().collect();
    groups.sort_by(|a, b| {
        let a_recent = ship_most_recent[a.0];
        let b_recent = ship_most_recent[b.0];
        b_recent.cmp(a_recent)
    });

    groups
        .into_iter()
        .map(|(ship, replays)| group_node(ship.to_string(), replays, ReplayGrouping::Ship, locale))
        .collect()
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
                // Known, so the row reads as an indexed one; an unindexed row
                // is the `stats_line` fallback and has its own tests in the
                // shared assembly.
                known: true,
            },
        }
    }

    /// Flattens a tree to `(depth, label)` pairs, matching the test-helper
    /// style `gpui_kit::component::tree`'s own tests use for asserting shape.
    /// A leaf contributes its identity line; [`stats_lines`] covers the
    /// second one.
    fn flatten(nodes: &[BrowserNode], depth: usize, out: &mut Vec<(usize, String)>) {
        for node in nodes {
            match node {
                BrowserNode::Group { label, children } => {
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
                                LinePart::Glyph(glyph) => glyph,
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
    fn date_grouping_buckets_consecutive_same_date_entries_newest_first() {
        // Paths sort descending to "b3, b2, b1, a2, a1"; game_time dates are
        // "02" for the b-paths and "01" for the a-paths, so this is a clean
        // two-group case that also proves the path-descending sort feeds the
        // grouping loop (b-dated entries never touch the a group).
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

    #[test]
    fn date_grouping_splits_out_of_order_runs_of_the_same_date() {
        // Paths sort descending to "c, b, a"; "c" and "a" share a date but
        // "b" sits between them with a different date, so the same-date run
        // is not merged across "b" -- two "01.01.2026" groups result.
        let files = vec![
            replay("replays/a.wowsreplay", "Kleber", "Ocean", "01.01.2026 09:00:00", MatchOutcome::Unknown),
            replay("replays/b.wowsreplay", "Kleber", "Ocean", "02.01.2026 09:00:00", MatchOutcome::Unknown),
            replay("replays/c.wowsreplay", "Kleber", "Ocean", "01.01.2026 20:00:00", MatchOutcome::Unknown),
        ];

        let tree = build_browser_tree(&files, ReplayGrouping::Date, None);

        assert_eq!(
            labels(&tree),
            vec![
                (0, "01.01.2026 (1)".to_string()),
                (1, "Kleber - Ocean".to_string()),
                (0, "02.01.2026 (1)".to_string()),
                (1, "Kleber - Ocean".to_string()),
                (0, "01.01.2026 (1)".to_string()),
                (1, "Kleber - Ocean".to_string()),
            ]
        );
    }

    #[test]
    fn ship_grouping_orders_groups_by_each_ships_most_recent_replay() {
        // Paths sort descending to "c, b, a". Kleber's most recent (first
        // seen) path is "c"; Yamato's is "b". So Kleber's group comes first
        // even though it has fewer replays.
        let files = vec![
            replay("replays/a.wowsreplay", "Yamato", "Ocean", "01.01.2026 08:00:00", MatchOutcome::Loss),
            replay("replays/b.wowsreplay", "Yamato", "Volcano", "01.01.2026 09:00:00", MatchOutcome::Win),
            replay("replays/c.wowsreplay", "Kleber", "Ocean", "01.01.2026 10:00:00", MatchOutcome::Win),
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

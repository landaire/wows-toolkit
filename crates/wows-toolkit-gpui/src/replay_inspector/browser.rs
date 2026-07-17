//! File-browser grouping model: `ReplayLite` (a replay's minimal, cheaply
//! read summary) and `build_browser_tree`, which groups replay files by
//! Date/Ship/None and builds the group/leaf display labels. Pure: no
//! `gpui`/`egui` types. Mirrors `ui/replay_parser/mod.rs`'s
//! `build_file_listing_grouped`/`build_file_listing_ungrouped`
//! (`mod.rs:3267-3448`), `win_rate_label` (`mod.rs:150`), and `colorize_label`
//! (`mod.rs:136`, whose win/loss/draw color is resolved by the render layer
//! from `battle_result`, not stored as text here).
//!
//! `ReplayLite`'s `ship`/`map` fields carry whatever `browser_view.rs`'s
//! background scan could read cheaply; see that module's doc comment for why
//! they may be untranslated identifiers rather than the egui app's localized
//! ship/map names, and why `battle_result` is usually `None`.

use std::collections::HashMap;
use std::path::PathBuf;

use wows_replays::analyzer::battle_controller::BattleResult;
use wows_toolkit_config::ReplayGrouping;

/// A replay file's minimal summary for the file browser: enough to sort,
/// group, and label it without a full packet parse.
#[derive(Debug, Clone)]
pub struct ReplayLite {
    pub path: PathBuf,
    pub ship: String,
    pub map: String,
    pub game_time: String,
    pub battle_result: Option<BattleResult>,
}

/// One row of the file-browser tree: a Date/Ship group with its children, or
/// a single replay leaf. `Leaf::battle_result` drives the win/loss/draw label
/// color; `Group` carries none (its label already encodes the win rate).
#[derive(Debug, Clone)]
pub enum BrowserNode {
    Group { label: String, children: Vec<BrowserNode> },
    Leaf { label: String, path: PathBuf, battle_result: Option<BattleResult> },
}

/// Builds the file-browser tree for `grouping`. Sorts `files` by path
/// descending first (mirroring both egui listing functions' own
/// `files.sort_by(|a, b| b.0.cmp(&a.0))`), so replay filenames' leading
/// timestamps put the newest replay first; the caller does not need to
/// pre-sort.
pub fn build_browser_tree(files: &[ReplayLite], grouping: ReplayGrouping) -> Vec<BrowserNode> {
    let mut sorted: Vec<&ReplayLite> = files.iter().collect();
    sorted.sort_by(|a, b| b.path.cmp(&a.path));

    match grouping {
        ReplayGrouping::None => sorted.into_iter().map(none_leaf).collect(),
        ReplayGrouping::Date => build_date_groups(&sorted),
        ReplayGrouping::Ship => build_ship_groups(&sorted),
    }
}

/// Ungrouped leaf label. The egui app's ungrouped listing shows a five-field
/// label (ship, map, scenario, game mode, time) via `Replay::label`, which
/// needs data `ReplayLite` does not carry (scenario/game mode). This reuses
/// the Date-grouping leaf format instead, so the ship/map/time content that
/// IS available still renders in a single readable line.
fn none_leaf(r: &ReplayLite) -> BrowserNode {
    BrowserNode::Leaf { label: date_leaf_label(r), path: r.path.clone(), battle_result: r.battle_result }
}

/// Date-mode leaf label: `"{ship} - {map} ({time})"`, where `{time}` is only
/// the time-of-day part of `game_time` (the date already labels the group).
/// Mirrors `mod.rs:3423-3429`.
fn date_leaf_label(r: &ReplayLite) -> String {
    let time_part = r.game_time.split(' ').nth(1).unwrap_or(&r.game_time);
    format!("{} - {} ({})", r.ship, r.map, time_part)
}

/// Ship-mode leaf label: `"{map} - {game_time}"` (the full timestamp, since
/// the group no longer isolates a single date). Mirrors `mod.rs:3430-3434`.
fn ship_leaf_label(r: &ReplayLite) -> String {
    format!("{} - {}", r.map, r.game_time)
}

/// `" - {W}W/{L}L ({pct}%)"` computed from the known battle results only
/// (entries with `battle_result: None` are skipped, matching the egui `_ =>`
/// arm); empty when no result in the group is known. Mirrors
/// `win_rate_label` (`mod.rs:150-162`).
fn win_rate_label(replays: &[&ReplayLite]) -> String {
    let (wins, losses) = replays.iter().fold((0u32, 0u32), |(w, l), r| match r.battle_result {
        Some(BattleResult::Win(_)) => (w + 1, l),
        Some(BattleResult::Loss(_)) => (w, l + 1),
        _ => (w, l),
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
fn group_node(name: String, replays: Vec<&ReplayLite>, leaf_label: impl Fn(&ReplayLite) -> String) -> BrowserNode {
    let label = format!("{} ({}){}", name, replays.len(), win_rate_label(&replays));
    let children = replays
        .into_iter()
        .map(|r| BrowserNode::Leaf { label: leaf_label(r), path: r.path.clone(), battle_result: r.battle_result })
        .collect();
    BrowserNode::Group { label, children }
}

/// Groups path-descending-sorted `files` by the date part of `game_time`
/// (`game_time.split(' ').next()`), as a run of consecutive same-date entries
/// rather than a global grouping by date key: if two runs of the same date
/// are separated by a different date (an out-of-order file timestamp), they
/// become two separate groups. Mirrors `mod.rs:3330-3344` exactly, including
/// this quirk.
fn build_date_groups(files: &[&ReplayLite]) -> Vec<BrowserNode> {
    let mut groups: Vec<(String, Vec<&ReplayLite>)> = Vec::new();
    for &r in files {
        let date = r.game_time.split(' ').next().unwrap_or(&r.game_time).to_string();
        if let Some((last_date, last_group)) = groups.last_mut()
            && *last_date == date
        {
            last_group.push(r);
            continue;
        }
        groups.push((date, vec![r]));
    }
    groups.into_iter().map(|(date, replays)| group_node(date, replays, date_leaf_label)).collect()
}

/// Groups path-descending-sorted `files` by ship name. Groups are ordered by
/// each ship's most recent replay: since `files` is already newest-first, a
/// ship's first occurrence during the scan is its most recent path. Mirrors
/// `mod.rs:3345-3359`.
fn build_ship_groups(files: &[&ReplayLite]) -> Vec<BrowserNode> {
    let mut ship_groups: HashMap<&str, Vec<&ReplayLite>> = HashMap::new();
    let mut ship_most_recent: HashMap<&str, &PathBuf> = HashMap::new();
    for &r in files {
        ship_groups.entry(r.ship.as_str()).or_default().push(r);
        ship_most_recent.entry(r.ship.as_str()).or_insert(&r.path);
    }

    let mut groups: Vec<(&str, Vec<&ReplayLite>)> = ship_groups.into_iter().collect();
    groups.sort_by(|a, b| {
        let a_recent = ship_most_recent[a.0];
        let b_recent = ship_most_recent[b.0];
        b_recent.cmp(a_recent)
    });

    groups.into_iter().map(|(ship, replays)| group_node(ship.to_string(), replays, ship_leaf_label)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn replay(path: &str, ship: &str, map: &str, game_time: &str, battle_result: Option<BattleResult>) -> ReplayLite {
        ReplayLite {
            path: PathBuf::from(path),
            ship: ship.to_string(),
            map: map.to_string(),
            game_time: game_time.to_string(),
            battle_result,
        }
    }

    /// Flattens a tree to `(depth, label)` pairs, matching the test-helper
    /// style `gpui_component::tree`'s own tests use for asserting shape.
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

    fn labels(nodes: &[BrowserNode]) -> Vec<(usize, String)> {
        let mut out = Vec::new();
        flatten(nodes, 0, &mut out);
        out
    }

    #[test]
    fn win_rate_label_counts_only_known_results_and_skips_none() {
        let replays = [
            replay("z", "Kleber", "Ocean", "01.01.2026 00:00:00", Some(BattleResult::Win(0))),
            replay("y", "Kleber", "Ocean", "01.01.2026 00:01:00", Some(BattleResult::Win(0))),
            replay("x", "Kleber", "Ocean", "01.01.2026 00:02:00", Some(BattleResult::Loss(1))),
            replay("w", "Kleber", "Ocean", "01.01.2026 00:03:00", None),
            replay("v", "Kleber", "Ocean", "01.01.2026 00:04:00", Some(BattleResult::Draw)),
        ];
        let refs: Vec<&ReplayLite> = replays.iter().collect();
        // 2 wins, 1 loss out of the 3 known-outcome entries; the None entry
        // and the Draw entry are both excluded from the W/L tally.
        assert_eq!(win_rate_label(&refs), " - 2W/1L (67%)");
    }

    #[test]
    fn win_rate_label_is_empty_when_no_result_is_known() {
        let replays = [
            replay("a", "Kleber", "Ocean", "01.01.2026 00:00:00", None),
            replay("b", "Kleber", "Ocean", "01.01.2026 00:01:00", Some(BattleResult::Draw)),
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
            replay("replays/a1.wowsreplay", "Kleber", "Ocean", "01.01.2026 10:00:00", None),
            replay("replays/a2.wowsreplay", "Kleber", "Volcano", "01.01.2026 11:30:00", Some(BattleResult::Win(0))),
            replay("replays/b1.wowsreplay", "Yamato", "Ocean", "02.01.2026 09:15:00", Some(BattleResult::Loss(1))),
            replay("replays/b2.wowsreplay", "Yamato", "Volcano", "02.01.2026 12:00:00", None),
            replay("replays/b3.wowsreplay", "Yamato", "Ocean", "02.01.2026 14:45:00", Some(BattleResult::Win(0))),
        ];

        let tree = build_browser_tree(&files, ReplayGrouping::Date);

        assert_eq!(
            labels(&tree),
            vec![
                (0, "02.01.2026 (3) - 1W/1L (50%)".to_string()),
                (1, "Yamato - Ocean (14:45:00)".to_string()),
                (1, "Yamato - Volcano (12:00:00)".to_string()),
                (1, "Yamato - Ocean (09:15:00)".to_string()),
                (0, "01.01.2026 (2) - 1W/0L (100%)".to_string()),
                (1, "Kleber - Volcano (11:30:00)".to_string()),
                (1, "Kleber - Ocean (10:00:00)".to_string()),
            ]
        );
    }

    #[test]
    fn date_grouping_splits_out_of_order_runs_of_the_same_date() {
        // Paths sort descending to "c, b, a"; "c" and "a" share a date but
        // "b" sits between them with a different date, so the same-date run
        // is not merged across "b" -- two "01.01.2026" groups result.
        let files = vec![
            replay("replays/a.wowsreplay", "Kleber", "Ocean", "01.01.2026 09:00:00", None),
            replay("replays/b.wowsreplay", "Kleber", "Ocean", "02.01.2026 09:00:00", None),
            replay("replays/c.wowsreplay", "Kleber", "Ocean", "01.01.2026 20:00:00", None),
        ];

        let tree = build_browser_tree(&files, ReplayGrouping::Date);

        assert_eq!(
            labels(&tree),
            vec![
                (0, "01.01.2026 (1)".to_string()),
                (1, "Kleber - Ocean (20:00:00)".to_string()),
                (0, "02.01.2026 (1)".to_string()),
                (1, "Kleber - Ocean (09:00:00)".to_string()),
                (0, "01.01.2026 (1)".to_string()),
                (1, "Kleber - Ocean (09:00:00)".to_string()),
            ]
        );
    }

    #[test]
    fn ship_grouping_orders_groups_by_each_ships_most_recent_replay() {
        // Paths sort descending to "c, b, a". Kleber's most recent (first
        // seen) path is "c"; Yamato's is "b". So Kleber's group comes first
        // even though it has fewer replays.
        let files = vec![
            replay("replays/a.wowsreplay", "Yamato", "Ocean", "01.01.2026 08:00:00", Some(BattleResult::Loss(1))),
            replay("replays/b.wowsreplay", "Yamato", "Volcano", "01.01.2026 09:00:00", Some(BattleResult::Win(0))),
            replay("replays/c.wowsreplay", "Kleber", "Ocean", "01.01.2026 10:00:00", Some(BattleResult::Win(0))),
        ];

        let tree = build_browser_tree(&files, ReplayGrouping::Ship);

        assert_eq!(
            labels(&tree),
            vec![
                (0, "Kleber (1) - 1W/0L (100%)".to_string()),
                (1, "Ocean - 01.01.2026 10:00:00".to_string()),
                (0, "Yamato (2) - 1W/1L (50%)".to_string()),
                (1, "Volcano - 01.01.2026 09:00:00".to_string()),
                (1, "Ocean - 01.01.2026 08:00:00".to_string()),
            ]
        );
    }

    #[test]
    fn none_grouping_is_a_flat_newest_first_leaf_list() {
        let files = vec![
            replay(
                "replays/20260101_a.wowsreplay",
                "Kleber",
                "Ocean",
                "01.01.2026 09:00:00",
                Some(BattleResult::Win(0)),
            ),
            replay("replays/20260102_b.wowsreplay", "Yamato", "Volcano", "02.01.2026 09:00:00", None),
        ];

        let tree = build_browser_tree(&files, ReplayGrouping::None);

        assert_eq!(
            labels(&tree),
            vec![(0, "Yamato - Volcano (09:00:00)".to_string()), (0, "Kleber - Ocean (09:00:00)".to_string())]
        );
        // Flat: no group wrapping, so every entry is a Leaf at depth 0.
        assert!(tree.iter().all(|n| matches!(n, BrowserNode::Leaf { .. })));
    }

    #[test]
    fn leaf_nodes_carry_the_source_path_and_battle_result() {
        let files =
            vec![replay("replays/only.wowsreplay", "Kleber", "Ocean", "01.01.2026 09:00:00", Some(BattleResult::Draw))];

        let tree = build_browser_tree(&files, ReplayGrouping::Date);
        let BrowserNode::Group { children, .. } = &tree[0] else { panic!("expected a Date group") };
        let BrowserNode::Leaf { path, battle_result, .. } = &children[0] else { panic!("expected a leaf") };

        assert_eq!(path, &PathBuf::from("replays/only.wowsreplay"));
        assert!(matches!(battle_result, Some(BattleResult::Draw)));
    }
}

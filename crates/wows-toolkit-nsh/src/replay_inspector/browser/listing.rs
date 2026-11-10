//! The listing's state, apart from the view: the scanned replays, the tree
//! they are grouped into, which groups are collapsed, and the selection.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use rust_i18n::t;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::RowSummary;
use wows_toolkit_replay_data::browser_tree::BrowserNode;
use wows_toolkit_replay_data::browser_tree::ReplayLite;
use wows_toolkit_replay_data::browser_tree::build_browser_tree;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::replay_scan::ScannedReplay;
use wows_toolkit_viewmodel::listing_row::HoverFact;
use wows_toolkit_viewmodel::listing_row::LinePart;
use wows_toolkit_viewmodel::listing_row::ListedReplay;
use wows_toolkit_viewmodel::listing_row::RowIdentity;
use wows_toolkit_viewmodel::listing_row::hover_facts;
use wows_toolkit_viewmodel::listing_row::listed_row_identity;
use wows_toolkit_viewmodel::listing_row::resolve_row_stats;

/// What the rows are named against.
#[derive(Clone, Default)]
pub(super) struct Naming {
    /// The installed build's game data, once loaded.
    pub(super) current: Option<Arc<LoadedGameData>>,
    /// The builds loaded so far, so a replay is named against its own.
    pub(super) cache: Option<GameDataCache>,
    /// The locale the stats line's figures are grouped in.
    pub(super) locale: Option<String>,
}

impl Naming {
    /// The replay's own build when it is loaded, else the installed one's;
    /// with neither, the raw ids the header carries.
    fn identity(&self, listed: &ListedReplay) -> RowIdentity {
        let own = listed.build.and_then(|build| self.cache.as_ref()?.loaded_build(build));
        let Some(data) = own.as_ref().or(self.current.as_ref()) else {
            return RowIdentity {
                ship: listed.ship_id.map(|id| id.to_string()).unwrap_or_else(|| t!("ui.replay.spectator").into_owned()),
                map: listed.map_name.clone(),
                scenario: listed.scenario.clone(),
                mode: listed.game_type.clone(),
                date_time: listed.date_time.clone(),
            };
        };

        listed_row_identity(listed, data.provider())
    }
}

/// A row across rebuilds: a group by its label, since its position moves as
/// replays come and go, a replay by its path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum RowKey {
    Group(Rc<str>),
    Leaf(PathBuf),
}

/// A date or ship group.
#[derive(Debug)]
pub(super) struct Group {
    /// The name, count and win rate.
    pub(super) label: Rc<str>,
    /// Every replay under it, which its menu acts on.
    pub(super) paths: Vec<PathBuf>,
}

/// One replay's row.
#[derive(Debug)]
pub(super) struct Leaf {
    pub(super) path: PathBuf,
    pub(super) file_name: String,
    /// The identity line, without what the group already says.
    pub(super) label: String,
    /// The stats line, in text and glyph pieces.
    pub(super) stats: Vec<LinePart>,
    pub(super) outcome: MatchOutcome,
    pub(super) in_division: bool,
    /// The map as the replay names it, for the hover preview's art.
    pub(super) map_name: String,
    pub(super) hover: Vec<HoverFact>,
}

#[derive(Debug, Clone)]
pub(super) enum Node {
    Group(Rc<Group>),
    Leaf(Rc<Leaf>),
}

impl Node {
    fn key(&self) -> RowKey {
        match self {
            Self::Group(group) => RowKey::Group(Rc::clone(&group.label)),
            Self::Leaf(leaf) => RowKey::Leaf(leaf.path.clone()),
        }
    }
}

/// A row's position among the shown rows: a collapsed group's replays are
/// left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ShownIndex(pub(super) usize);

/// A row's position in the flattened tree, every replay included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TreeIndex(usize);

/// A node of the flattened tree.
#[derive(Debug, Clone)]
struct TreeRow {
    node: Node,
    depth: u32,
    /// The index past its subtree.
    end: TreeIndex,
}

/// What one shown row draws.
#[derive(Debug, Clone)]
pub(super) struct ShownRow {
    pub(super) node: Node,
    pub(super) depth: u32,
    pub(super) selected: bool,
    /// Groups only.
    pub(super) expanded: bool,
    /// Every other replay row is tinted, so a long group stays scannable.
    pub(super) stripe: bool,
}

impl PartialEq for ShownRow {
    fn eq(&self, other: &Self) -> bool {
        let same_node = match (&self.node, &other.node) {
            (Node::Group(a), Node::Group(b)) => Rc::ptr_eq(a, b),
            (Node::Leaf(a), Node::Leaf(b)) => Rc::ptr_eq(a, b),
            _ => false,
        };

        same_node
            && self.depth == other.depth
            && self.selected == other.selected
            && self.expanded == other.expanded
            && self.stripe == other.stripe
    }
}

/// What a press asks of the listing's owner.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Pressed {
    Nothing,
    Open(PathBuf),
}

/// Whether a replay the watcher read was new to the listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Upserted {
    Added,
    Replaced,
}

/// A keyboard move of the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Previous,
    Next,
    First,
    Last,
    /// Collapses a group, or goes to the group a replay is in.
    Out,
    /// Expands a group, or goes to its first replay once expanded.
    In,
}

#[derive(Clone, Default)]
pub(super) struct Listing {
    files: Vec<ScannedReplay>,
    summaries: HashMap<PathBuf, RowSummary>,
    grouping: ReplayGrouping,
    naming: Naming,

    tree: Vec<TreeRow>,
    /// The rows shown, in order.
    shown: Vec<TreeIndex>,

    /// Groups the reader closed, by label.
    collapsed: HashSet<Rc<str>>,
    selected: Option<RowKey>,
    /// The replays the reader ctrl-clicked, which batch actions work over.
    marked: HashSet<PathBuf>,
}

impl Listing {
    pub(super) fn files(&self) -> &[ScannedReplay] {
        &self.files
    }

    /// The newest replay: the game names each one by when it was recorded,
    /// which is also the order the listing shows them in.
    pub(super) fn newest(&self) -> Option<&Path> {
        self.files.iter().map(|file| file.path.as_path()).max()
    }

    pub(super) fn set_files(&mut self, files: Vec<ScannedReplay>) {
        self.files = files;
        self.rebuild();
    }

    /// Adds a replay the watcher read, or replaces the one at its path.
    pub(super) fn upsert(&mut self, scanned: ScannedReplay) -> Upserted {
        let held = self.files.iter().position(|existing| existing.path == scanned.path);
        let upserted = match held {
            Some(at) => {
                self.files[at] = scanned;
                Upserted::Replaced
            }
            None => {
                self.files.push(scanned);
                Upserted::Added
            }
        };
        self.rebuild();

        upserted
    }

    /// Drops a replay the directory no longer holds, with its mark and its
    /// selection.
    pub(super) fn forget(&mut self, path: &Path) -> bool {
        let Some(at) = self.files.iter().position(|existing| existing.path == path) else {
            return false;
        };

        self.files.remove(at);
        self.marked.remove(path);
        if self.selected.as_ref() == Some(&RowKey::Leaf(path.to_path_buf())) {
            self.selected = None;
        }
        self.rebuild();

        true
    }

    pub(super) fn set_summaries(&mut self, summaries: HashMap<PathBuf, RowSummary>) {
        self.summaries = summaries;
        self.rebuild();
    }

    pub(super) fn set_grouping(&mut self, grouping: ReplayGrouping) {
        if self.grouping == grouping {
            return;
        }

        self.grouping = grouping;
        self.rebuild();
    }

    pub(super) fn set_naming(&mut self, naming: Naming) {
        self.naming = naming;
        self.rebuild();
    }

    pub(super) fn shown_len(&self) -> usize {
        self.shown.len()
    }

    pub(super) fn shown_row(&self, at: ShownIndex) -> Option<ShownRow> {
        let index = *self.shown.get(at.0)?;
        let row = &self.tree[index.0];
        let (selected, expanded) = match &row.node {
            Node::Group(group) => (
                self.selected.as_ref() == Some(&RowKey::Group(Rc::clone(&group.label))),
                !self.collapsed.contains(&group.label),
            ),
            Node::Leaf(leaf) => (
                self.marked.contains(&leaf.path) || self.selected.as_ref() == Some(&RowKey::Leaf(leaf.path.clone())),
                false,
            ),
        };

        Some(ShownRow {
            node: row.node.clone(),
            depth: row.depth,
            selected,
            expanded,
            stripe: matches!(row.node, Node::Leaf(_)) && at.0 % 2 == 1,
        })
    }

    /// The shown index of the selected row.
    pub(super) fn selected_at(&self) -> Option<ShownIndex> {
        let selected = self.selected.as_ref()?;
        self.shown.iter().position(|index| self.tree[index.0].node.key() == *selected).map(ShownIndex)
    }

    /// A pointer press on the shown row `at`. A plain press selects and
    /// replaces the marked set, a second press opens; the secondary
    /// modifier marks or unmarks a replay instead. A press on a group
    /// selects it and opens or closes it.
    pub(super) fn press(&mut self, at: ShownIndex, secondary: bool, clicks: u32) -> Pressed {
        let Some(&index) = self.shown.get(at.0) else {
            return Pressed::Nothing;
        };

        let path = match &self.tree[index.0].node {
            Node::Group(group) => {
                let label = Rc::clone(&group.label);
                self.selected = Some(RowKey::Group(Rc::clone(&label)));
                self.toggle(label);
                return Pressed::Nothing;
            }
            Node::Leaf(leaf) => leaf.path.clone(),
        };

        if secondary {
            self.toggle_marked(path);
            return Pressed::Nothing;
        }

        self.marked.clear();
        self.selected = Some(RowKey::Leaf(path.clone()));
        if clicks >= 2 {
            return Pressed::Open(path);
        }

        Pressed::Nothing
    }

    /// Return on the selected row: a replay opens, a group opens or closes.
    pub(super) fn activate(&mut self) -> Pressed {
        match self.selected.clone() {
            Some(RowKey::Leaf(path)) => Pressed::Open(path),
            Some(RowKey::Group(label)) => {
                self.toggle(label);
                Pressed::Nothing
            }
            None => Pressed::Nothing,
        }
    }

    /// Moves the selection between the shown rows, and returns where it
    /// went.
    pub(super) fn step(&mut self, step: Step) -> Option<ShownIndex> {
        if self.shown.is_empty() {
            return None;
        }

        let last = self.shown.len() - 1;
        let current = self.selected_at();
        let target = match (step, current) {
            (Step::First, _) | (Step::Next, None) => 0,
            (Step::Last, _) | (Step::Previous, None) => last,
            (Step::Next, Some(at)) => (at.0 + 1).min(last),
            (Step::Previous, Some(at)) => at.0.saturating_sub(1),
            (Step::Out | Step::In, None) => return None,
            (Step::Out, Some(at)) => {
                let index = self.shown[at.0];
                match &self.tree[index.0].node {
                    Node::Group(group) if !self.collapsed.contains(&group.label) => {
                        let label = Rc::clone(&group.label);
                        self.collapsed.insert(label);
                        self.reshow();
                        return self.selected_at();
                    }
                    _ => self.parent_shown(at)?.0,
                }
            }
            (Step::In, Some(at)) => {
                let index = self.shown[at.0];
                match &self.tree[index.0].node {
                    Node::Group(group) if self.collapsed.contains(&group.label) => {
                        let label = Rc::clone(&group.label);
                        self.collapsed.remove(&label);
                        self.reshow();
                        return self.selected_at();
                    }
                    Node::Group(_) if self.tree[index.0].end.0 > index.0 + 1 => at.0 + 1,
                    _ => at.0,
                }
            }
        };

        let index = self.shown[target];
        self.selected = Some(self.tree[index.0].node.key());

        Some(ShownIndex(target))
    }

    /// Every replay the reader has marked, in path order. A single selected
    /// replay counts as a selection of one, so an action offered on the set
    /// is offered on it too.
    pub(super) fn selection(&self) -> Vec<PathBuf> {
        if self.marked.is_empty() {
            return match &self.selected {
                Some(RowKey::Leaf(path)) => vec![path.clone()],
                _ => Vec::new(),
            };
        }

        let mut paths: Vec<PathBuf> = self.marked.iter().cloned().collect();
        paths.sort();

        paths
    }

    fn toggle(&mut self, label: Rc<str>) {
        if !self.collapsed.remove(&label) {
            self.collapsed.insert(label);
        }
        self.reshow();
    }

    fn toggle_marked(&mut self, path: PathBuf) {
        // The row a plain press left selected joins the set, so marking a
        // second row marks both rather than only the second.
        if self.marked.is_empty()
            && let Some(RowKey::Leaf(current)) = &self.selected
            && *current != path
        {
            self.marked.insert(current.clone());
        }

        if !self.marked.remove(&path) {
            self.marked.insert(path.clone());
            self.selected = Some(RowKey::Leaf(path));
        }
    }

    /// The shown index of the group the shown row `at` is in.
    fn parent_shown(&self, at: ShownIndex) -> Option<ShownIndex> {
        let index = self.shown[at.0];
        let depth = self.tree[index.0].depth;
        if depth == 0 {
            return None;
        }

        self.shown[..at.0].iter().rposition(|before| self.tree[before.0].depth < depth).map(ShownIndex)
    }

    fn rebuild(&mut self) {
        let locale = self.naming.locale.as_deref();
        let translated: Vec<ReplayLite> = self
            .files
            .iter()
            .map(|file| ReplayLite {
                path: file.path.clone(),
                map_name: file.listed.map_name.clone(),
                identity: self.naming.identity(&file.listed),
                stats: resolve_row_stats(None, self.summaries.get(&file.path)),
            })
            .collect();
        let mut hover: HashMap<&Path, Vec<HoverFact>> = translated
            .iter()
            .map(|replay| (replay.path.as_path(), hover_facts(&replay.identity, &replay.stats, locale)))
            .collect();

        let nodes = build_browser_tree(&translated, self.grouping, locale);

        let mut tree = Vec::with_capacity(translated.len());
        let mut pending: Vec<(BrowserNode, u32)> = nodes.into_iter().rev().map(|node| (node, 0)).collect();
        while let Some((node, depth)) = pending.pop() {
            let node = match node {
                BrowserNode::Group { label, children } => {
                    pending.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
                    Node::Group(Rc::new(Group { label: label.into(), paths: Vec::new() }))
                }
                BrowserNode::Leaf { label, stats, path, map_name, outcome, in_division } => {
                    // Every leaf comes from `translated`, which `hover` was built from.
                    let hover = hover.remove(path.as_path()).unwrap_or_default();
                    // A scanned replay is a file, so its path always ends in a name.
                    let file_name =
                        path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                    Node::Leaf(Rc::new(Leaf { path, file_name, label, stats, outcome, in_division, map_name, hover }))
                }
            };
            // `close_subtrees` sets the end once every row is placed.
            tree.push(TreeRow { node, depth, end: TreeIndex(0) });
        }

        close_subtrees(&mut tree);
        self.tree = tree;

        let still_there = match &self.selected {
            Some(key) => self.tree.iter().any(|row| row.node.key() == *key),
            None => true,
        };
        if !still_there {
            self.selected = None;
        }

        self.reshow();
    }

    fn reshow(&mut self) {
        let mut shown = Vec::with_capacity(self.tree.len());
        let mut index = TreeIndex(0);
        while index.0 < self.tree.len() {
            shown.push(index);
            index = match &self.tree[index.0].node {
                Node::Group(group) if self.collapsed.contains(&group.label) => self.tree[index.0].end,
                _ => TreeIndex(index.0 + 1),
            };
        }

        self.shown = shown;
    }
}

/// Sets each row's subtree end and gives each group the replays under it.
fn close_subtrees(tree: &mut [TreeRow]) {
    let mut open: Vec<usize> = Vec::new();
    for index in 0..tree.len() {
        let depth = tree[index].depth;
        while open.last().is_some_and(|&top| tree[top].depth >= depth) {
            let Some(top) = open.pop() else { break };
            tree[top].end = TreeIndex(index);
        }
        open.push(index);
    }
    for top in open {
        tree[top].end = TreeIndex(tree.len());
    }

    for index in 0..tree.len() {
        if !matches!(tree[index].node, Node::Group(_)) {
            continue;
        }

        let paths: Vec<PathBuf> = tree[index + 1..tree[index].end.0]
            .iter()
            .filter_map(|row| match &row.node {
                Node::Leaf(leaf) => Some(leaf.path.clone()),
                Node::Group(_) => None,
            })
            .collect();
        if let Node::Group(group) = &tree[index].node {
            tree[index].node = Node::Group(Rc::new(Group { label: Rc::clone(&group.label), paths }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scanned(name: &str, date_time: &str) -> ScannedReplay {
        ScannedReplay {
            path: PathBuf::from(name),
            listed: ListedReplay {
                ship_id: None,
                map_name: "spaces/00_CO_ocean".to_owned(),
                game_type: String::new(),
                scenario: String::new(),
                date_time: date_time.to_owned(),
                build: None,
            },
            version: None,
        }
    }

    fn listing(grouping: ReplayGrouping, files: &[(&str, &str)]) -> Listing {
        let mut listing = Listing::default();
        listing.set_grouping(grouping);
        listing.set_files(files.iter().map(|(name, date)| scanned(name, date)).collect());
        listing
    }

    fn shown_at(listing: &Listing, path: &str) -> ShownIndex {
        (0..listing.shown_len())
            .map(ShownIndex)
            .find(|&at| {
                matches!(listing.shown_row(at).map(|row| row.node), Some(Node::Leaf(leaf)) if leaf.path == Path::new(path))
            })
            .expect("the replay is shown")
    }

    /// Ctrl-clicking builds a set: the row already selected joins it, so two
    /// ctrl-clicks mark two replays rather than only the second. Clicking
    /// again takes one out, and a plain click replaces the whole set.
    #[test]
    fn ctrl_clicking_builds_a_selection_and_a_plain_click_replaces_it() {
        let mut listing = listing(
            ReplayGrouping::None,
            &[
                ("c.wowsreplay", "01.01.2026 00:00:00"),
                ("b.wowsreplay", "01.01.2026 00:00:00"),
                ("a.wowsreplay", "01.01.2026 00:00:00"),
            ],
        );
        let a = PathBuf::from("a.wowsreplay");
        let b = PathBuf::from("b.wowsreplay");
        let c = PathBuf::from("c.wowsreplay");

        listing.press(shown_at(&listing, "a.wowsreplay"), false, 1);
        assert_eq!(listing.selection(), vec![a.clone()], "one click is a selection of one");

        listing.press(shown_at(&listing, "b.wowsreplay"), true, 1);
        assert_eq!(listing.selection(), vec![a.clone(), b.clone()], "the selected row joins the set");

        listing.press(shown_at(&listing, "a.wowsreplay"), true, 1);
        assert_eq!(listing.selection(), vec![b], "ctrl-clicking a marked row takes it out");

        listing.press(shown_at(&listing, "c.wowsreplay"), false, 1);
        assert_eq!(listing.selection(), vec![c], "a plain click replaces the set");
    }

    #[test]
    fn a_second_press_opens_and_a_ctrl_double_press_does_not() {
        let mut listing = listing(ReplayGrouping::None, &[("a.wowsreplay", "01.01.2026 00:00:00")]);

        assert_eq!(listing.press(ShownIndex(0), false, 1), Pressed::Nothing);
        assert_eq!(listing.press(ShownIndex(0), false, 2), Pressed::Open(PathBuf::from("a.wowsreplay")));
        assert_eq!(listing.press(ShownIndex(0), true, 2), Pressed::Nothing);
    }

    /// A replay the directory no longer holds leaves the listing, and its
    /// marking and selection go with it.
    #[test]
    fn a_removed_replay_leaves_the_listing() {
        let mut listing = listing(
            ReplayGrouping::None,
            &[("gone.wowsreplay", "01.01.2026 00:00:00"), ("kept.wowsreplay", "01.01.2026 00:00:00")],
        );
        listing.press(shown_at(&listing, "gone.wowsreplay"), false, 1);
        assert_eq!(listing.selection(), vec![PathBuf::from("gone.wowsreplay")]);

        assert!(listing.forget(Path::new("gone.wowsreplay")));

        let listed: Vec<&Path> = listing.files().iter().map(|file| file.path.as_path()).collect();
        assert_eq!(listed, vec![Path::new("kept.wowsreplay")], "only the replay still there is listed");
        assert!(listing.selection().is_empty(), "and nothing is selected, since the selection is gone");
        assert_eq!(listing.shown_len(), 1);
        assert!(!listing.forget(Path::new("gone.wowsreplay")), "a replay forgotten once is not there to forget");
    }

    /// The newest replay is the one the listing shows first, whatever order
    /// the scan found them in.
    #[test]
    fn the_newest_replay_is_the_one_shown_first() {
        assert_eq!(Listing::default().newest(), None);

        let listing = listing(
            ReplayGrouping::None,
            &[
                ("20260101_100000_A_Map.wowsreplay", "01.01.2026 10:00:00"),
                ("20260301_100000_C_Map.wowsreplay", "01.03.2026 10:00:00"),
                ("20260201_100000_B_Map.wowsreplay", "01.02.2026 10:00:00"),
            ],
        );

        let newest = Path::new("20260301_100000_C_Map.wowsreplay");
        assert_eq!(listing.newest(), Some(newest));
        assert_eq!(shown_at(&listing, "20260301_100000_C_Map.wowsreplay"), ShownIndex(0));
    }

    /// Pressing a group closes it and hides its replays; a rebuild (a new
    /// replay, a change of names) keeps it closed and keeps the selection.
    #[test]
    fn a_closed_group_and_the_selection_survive_a_rebuild() {
        let mut listing = listing(
            ReplayGrouping::Date,
            &[("b.wowsreplay", "02.01.2026 10:00:00"), ("a.wowsreplay", "01.01.2026 10:00:00")],
        );
        assert_eq!(listing.shown_len(), 4, "two groups of one replay each");

        listing.press(shown_at(&listing, "a.wowsreplay"), false, 1);
        let newest_group = ShownIndex(0);
        listing.press(newest_group, false, 1);
        assert_eq!(listing.shown_len(), 3, "the closed group's replay is hidden");
        assert!(!listing.shown_row(newest_group).expect("the group is shown").expanded);

        listing.press(shown_at(&listing, "a.wowsreplay"), false, 1);
        listing.set_summaries(HashMap::new());

        assert_eq!(listing.shown_len(), 3, "the group stays closed");
        assert_eq!(listing.selection(), vec![PathBuf::from("a.wowsreplay")], "and the replay stays selected");
    }

    #[test]
    fn a_group_menu_acts_on_every_replay_under_it() {
        let listing = listing(
            ReplayGrouping::Date,
            &[("b.wowsreplay", "01.01.2026 11:00:00"), ("a.wowsreplay", "01.01.2026 10:00:00")],
        );

        let Some(Node::Group(group)) = listing.shown_row(ShownIndex(0)).map(|row| row.node) else {
            panic!("the first row is the date group");
        };
        assert_eq!(group.paths, vec![PathBuf::from("b.wowsreplay"), PathBuf::from("a.wowsreplay")]);
    }

    #[test]
    fn the_keys_move_over_shown_rows_and_open_and_close_groups() {
        let mut listing = listing(
            ReplayGrouping::Date,
            &[("b.wowsreplay", "02.01.2026 10:00:00"), ("a.wowsreplay", "01.01.2026 10:00:00")],
        );

        assert_eq!(listing.step(Step::Next), Some(ShownIndex(0)), "nothing selected: Down starts at the top");
        assert_eq!(listing.step(Step::Out), Some(ShownIndex(0)), "Left closes the group");
        assert_eq!(listing.shown_len(), 3);
        assert_eq!(listing.step(Step::Next), Some(ShownIndex(1)), "Down passes over the closed group's replay");
        assert_eq!(listing.step(Step::In), Some(ShownIndex(2)), "Right on an open group goes to its first replay");
        assert_eq!(listing.activate(), Pressed::Open(PathBuf::from("a.wowsreplay")), "Return opens it");
        assert_eq!(listing.step(Step::Out), Some(ShownIndex(1)), "Left on a replay goes to its group");
        assert_eq!(listing.step(Step::First), Some(ShownIndex(0)));
        assert_eq!(listing.activate(), Pressed::Nothing, "Return on a group opens it");
        assert_eq!(listing.shown_len(), 4);
    }

    #[test]
    fn rows_without_game_data_carry_the_raw_names() {
        let listing = listing(ReplayGrouping::None, &[("a.wowsreplay", "01.01.2026 10:00:00")]);

        let Some(Node::Leaf(leaf)) = listing.shown_row(ShownIndex(0)).map(|row| row.node) else {
            panic!("the row is the replay");
        };
        assert_eq!(leaf.file_name, "a.wowsreplay");
        assert!(leaf.label.contains("spaces/00_CO_ocean"), "got {:?}", leaf.label);
        assert!(leaf.label.contains(t!("ui.replay.spectator").as_ref()), "got {:?}", leaf.label);
    }
}

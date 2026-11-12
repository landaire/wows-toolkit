//! The listing's rows as a [`TreeModel`]: the shared Date/Ship grouping with what each row draws.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use landaire_ui::TreeModel;
use wows_replay_insights::personal_rating::PersonalRatingResult;
use wows_toolkit_config::ReplayGrouping;
use wows_toolkit_config::index::rows::MatchOutcome;
use wows_toolkit_config::index::rows::RowSummary;
use wows_toolkit_replay_data::browser_tree::BrowserNode;
use wows_toolkit_replay_data::browser_tree::ReplayLite;
use wows_toolkit_replay_data::browser_tree::build_browser_tree;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::replay_dir::ScannedReplay;
use wows_toolkit_viewmodel::listing_row::HoverFact;
use wows_toolkit_viewmodel::listing_row::LinePart;
use wows_toolkit_viewmodel::listing_row::hover_facts;
use wowsunpack::game_params::provider::GameMetadataProvider;

/// A group row, by its place under its parent and its label. A rebuild that keeps a group's label
/// keeps its key, so it stays collapsed or selected.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GroupId(String);

/// One row of the listing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NodeKey {
    Group(GroupId),
    Leaf(PathBuf),
}

impl NodeKey {
    pub fn leaf_path(&self) -> Option<&Path> {
        match self {
            Self::Leaf(path) => Some(path),
            Self::Group(_) => None,
        }
    }
}

/// What a group row draws, and the replays under it for its menu.
#[derive(Clone, Debug)]
pub struct GroupRow {
    pub label: String,
    pub paths: Vec<PathBuf>,
}

/// What a replay row draws.
#[derive(Clone, Debug)]
pub struct LeafRow {
    pub label: String,
    /// "Ship - Map", for messages about the battle.
    pub battle: String,
    pub stats: Vec<LinePart>,
    /// The map as the replay names it, for the thumbnail and the hover preview.
    pub map_name: String,
    pub outcome: MatchOutcome,
    pub in_division: bool,
    pub hover: Vec<HoverFact>,
    /// The reader's rating in the battle, once the index has scored it.
    pub personal_rating: Option<PersonalRatingResult>,
}

/// The listing's rows, grouped as the reader chose.
#[derive(Debug, Default)]
pub struct BrowserTree {
    roots: Vec<NodeKey>,
    children: HashMap<NodeKey, Vec<NodeKey>>,
    parents: HashMap<NodeKey, NodeKey>,
    groups: HashMap<GroupId, GroupRow>,
    leaves: HashMap<PathBuf, LeafRow>,
    /// The visible order of the replays, for a shift-click run.
    order: Vec<PathBuf>,
}

/// What the rows are named and reported from.
pub struct TreeInputs<'a> {
    pub files: &'a [ScannedReplay],
    pub summaries: &'a HashMap<PathBuf, RowSummary>,
    pub cache: Option<&'a GameDataCache>,
    pub fallback: Option<&'a GameMetadataProvider>,
    pub grouping: ReplayGrouping,
    pub locale: Option<&'a str>,
}

impl BrowserTree {
    pub fn build(inputs: &TreeInputs<'_>) -> Self {
        let lites: Vec<ReplayLite> = inputs
            .files
            .iter()
            .map(|file| ReplayLite::from_scanned(file, inputs.cache, inputs.fallback, inputs.summaries.get(&file.path)))
            .collect();
        let mut hover: HashMap<PathBuf, Vec<HoverFact>> = lites
            .iter()
            .map(|lite| (lite.path.clone(), hover_facts(&lite.identity, &lite.stats, inputs.locale)))
            .collect();
        let nodes = build_browser_tree(&lites, inputs.grouping, inputs.locale);

        let mut tree = Self::default();
        let mut used: HashSet<String> = HashSet::new();
        // A worklist of (node, parent key, parent id prefix), walked in order so the visible order
        // of the replays is the order they are pushed in.
        let mut work: Vec<(BrowserNode, Option<NodeKey>, String)> =
            nodes.into_iter().rev().map(|node| (node, None, "replay-browser-group".to_owned())).collect();
        while let Some((node, parent, prefix)) = work.pop() {
            let (key, below) = match node {
                BrowserNode::Group { label, children } => {
                    let mut id = format!("{prefix}/{label}");
                    let mut repeat = 1;
                    while !used.insert(id.clone()) {
                        id = format!("{prefix}/{label}#{repeat}");
                        repeat += 1;
                    }
                    let paths = descendant_paths(&children);
                    let group = GroupId(id.clone());
                    tree.groups.insert(group.clone(), GroupRow { label, paths });
                    (NodeKey::Group(group), Some((children, id)))
                }
                BrowserNode::Leaf { label, battle, stats, path, map_name, outcome, in_division } => {
                    let personal_rating =
                        inputs.summaries.get(&path).and_then(|summary| summary.self_pr).map(PersonalRatingResult::new);
                    let leaf = LeafRow {
                        label,
                        battle,
                        stats,
                        map_name,
                        outcome,
                        in_division,
                        hover: hover.remove(&path).unwrap_or_default(),
                        personal_rating,
                    };
                    tree.order.push(path.clone());
                    tree.leaves.insert(path.clone(), leaf);
                    (NodeKey::Leaf(path), None)
                }
            };

            match &parent {
                Some(parent) => {
                    tree.children.entry(parent.clone()).or_default().push(key.clone());
                    tree.parents.insert(key.clone(), parent.clone());
                }
                None => tree.roots.push(key.clone()),
            }
            if let Some((children, id)) = below {
                work.extend(children.into_iter().rev().map(|child| (child, Some(key.clone()), id.clone())));
            }
        }

        tree
    }

    pub fn group(&self, id: &GroupId) -> Option<&GroupRow> {
        self.groups.get(id)
    }

    pub fn leaf(&self, path: &Path) -> Option<&LeafRow> {
        self.leaves.get(path)
    }

    pub fn groups(&self) -> impl Iterator<Item = &GroupId> {
        self.groups.keys()
    }

    /// Every replay in listing order, collapsed groups included.
    pub fn order(&self) -> &[PathBuf] {
        &self.order
    }

    pub fn holds(&self, path: &Path) -> bool {
        self.leaves.contains_key(path)
    }
}

impl TreeModel for BrowserTree {
    type Key = NodeKey;

    fn roots(&self) -> &[NodeKey] {
        &self.roots
    }

    fn children(&self, key: &NodeKey) -> &[NodeKey] {
        self.children.get(key).map_or(&[], Vec::as_slice)
    }

    fn parent(&self, key: &NodeKey) -> Option<NodeKey> {
        self.parents.get(key).cloned()
    }

    fn is_branch(&self, key: &NodeKey) -> bool {
        matches!(key, NodeKey::Group(_))
    }

    fn text(&self, key: &NodeKey) -> Option<&str> {
        match key {
            NodeKey::Group(id) => self.groups.get(id).map(|group| group.label.as_str()),
            NodeKey::Leaf(path) => self.leaves.get(path).map(|leaf| leaf.label.as_str()),
        }
    }
}

/// Every replay under `nodes`, in order.
fn descendant_paths(nodes: &[BrowserNode]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut work: Vec<&BrowserNode> = nodes.iter().rev().collect();
    while let Some(node) = work.pop() {
        match node {
            BrowserNode::Group { children, .. } => work.extend(children.iter().rev()),
            BrowserNode::Leaf { path, .. } => paths.push(path.clone()),
        }
    }

    paths
}

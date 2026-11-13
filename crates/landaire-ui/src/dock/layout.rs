//! A dock's arrangement as plain data, and every change it takes.
//!
//! Each region of the dock holds a tree: splits whose leaves are tab groups. Panels are addressed
//! by the id the application gave them and groups by an id the layout hands out, so a panel can be
//! taken out of one group and put into another without either side knowing the tree's shape.

use std::cell::Cell;
use std::collections::HashSet;

use serde::Deserialize;
use serde::Serialize;

use crate::dock::saved::SavedLayout;
use crate::dock::saved::SavedNode;
use crate::dock::saved::SavedPart;
use crate::dock::saved::SavedRegion;
use crate::error::DockError;
use crate::error::LengthError;
use crate::error::ShareError;

/// Names one panel in a dock. The application chooses it, so it can address the panel later.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PanelId(String);

impl PanelId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What sort of panel one is: the name its factory is registered under.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PanelKind(String);

impl PanelKind {
    pub fn new(kind: impl Into<String>) -> Self {
        Self(kind.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One tab group for the life of the layout it is in. A restored layout numbers its groups anew.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GroupId(u64);

/// One split for the life of the layout it is in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct SplitId(u64);

/// A part of a split, or of the dock, as a percentage from 0 to 100.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct Share(f64);

impl Share {
    pub fn new(percent: f64) -> Result<Self, ShareError> {
        if (0.0..=100.0).contains(&percent) { Ok(Self(percent)) } else { Err(ShareError { percent }) }
    }

    pub fn percent(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for Share {
    type Error = ShareError;

    fn try_from(percent: f64) -> Result<Self, ShareError> {
        Self::new(percent)
    }
}

impl From<Share> for f64 {
    fn from(share: Share) -> Self {
        share.0
    }
}

/// Which way a split lays its parts out.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitAxis {
    /// Side by side.
    Horizontal,
    /// One above another.
    Vertical,
}

/// One of the four areas of a dock. The sides and the bottom fold away; the center takes the room
/// they leave.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockRegion {
    Center,
    Left,
    Right,
    Bottom,
}

impl DockRegion {
    pub const ALL: [Self; 4] = [Self::Center, Self::Left, Self::Right, Self::Bottom];
}

/// One of the regions of a dock that fold away and take a set amount of room while they show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FoldingRegion {
    Left,
    Right,
    Bottom,
}

impl FoldingRegion {
    /// The share the region takes in a new layout.
    pub(crate) const fn default_share(self) -> Share {
        match self {
            Self::Left => Share(20.0),
            Self::Right => Share(25.0),
            Self::Bottom => Share(30.0),
        }
    }
}

/// A length in CSS pixels, finite and not below zero.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(try_from = "f64", into = "f64")]
pub struct CssLength(f64);

impl CssLength {
    pub fn new(pixels: f64) -> Result<Self, LengthError> {
        if pixels.is_finite() && pixels >= 0.0 { Ok(Self(pixels)) } else { Err(LengthError { pixels }) }
    }

    pub fn pixels(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for CssLength {
    type Error = LengthError;

    fn try_from(pixels: f64) -> Result<Self, LengthError> {
        Self::new(pixels)
    }
}

impl From<CssLength> for f64 {
    fn from(length: CssLength) -> Self {
        length.0
    }
}

/// The edge of a tab group a new group is split off at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Where [`add_panel`](crate::DockState::add_panel) puts a panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockTarget {
    /// The region's first tab group, made when the region has none.
    Region(DockRegion),
    Group(GroupId),
    /// A new tab group split off `group` at `edge`, halving the room `group` had.
    Beside {
        group: GroupId,
        edge: Edge,
    },
}

/// A panel as a layout holds it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PanelEntry {
    pub id: PanelId,
    pub kind: PanelKind,
    /// Whether the user may close it from its tab.
    pub closable: bool,
}

impl PanelEntry {
    /// A closable panel.
    pub fn new(id: PanelId, kind: PanelKind) -> Self {
        Self { id, kind, closable: true }
    }
}

/// How much room one folding region takes while it shows. The region keeps its share of the dock
/// as the dock is resized, and a drag sets that share.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionExtent {
    /// A share of the dock's width for a side, or of the height under the center for the bottom.
    Share(Share),
    /// A length to start at. Once the dock is laid out it becomes the share of the dock that it
    /// took then.
    Length(CssLength),
}

/// How much room each folding region takes while it shows.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RegionExtents {
    pub left: RegionExtent,
    pub right: RegionExtent,
    pub bottom: RegionExtent,
}

impl RegionExtents {
    pub const DEFAULT: Self = Self {
        left: RegionExtent::Share(FoldingRegion::Left.default_share()),
        right: RegionExtent::Share(FoldingRegion::Right.default_share()),
        bottom: RegionExtent::Share(FoldingRegion::Bottom.default_share()),
    };

    pub fn get(&self, region: FoldingRegion) -> RegionExtent {
        match region {
            FoldingRegion::Left => self.left,
            FoldingRegion::Right => self.right,
            FoldingRegion::Bottom => self.bottom,
        }
    }

    pub fn set(&mut self, region: FoldingRegion, extent: RegionExtent) {
        match region {
            FoldingRegion::Left => self.left = extent,
            FoldingRegion::Right => self.right = extent,
            FoldingRegion::Bottom => self.bottom = extent,
        }
    }
}

/// The least and most room a folding region may be dragged to, beside the floor every region
/// has. A least above the most wins.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RegionLimits {
    pub min: Option<CssLength>,
    pub max: Option<CssLength>,
}

/// The limits of each folding region. Set by the application rather than saved with a layout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct FoldingLimits {
    left: RegionLimits,
    right: RegionLimits,
    bottom: RegionLimits,
}

impl FoldingLimits {
    pub(crate) fn get(&self, region: FoldingRegion) -> RegionLimits {
        match region {
            FoldingRegion::Left => self.left,
            FoldingRegion::Right => self.right,
            FoldingRegion::Bottom => self.bottom,
        }
    }

    pub(crate) fn set(&mut self, region: FoldingRegion, limits: RegionLimits) {
        match region {
            FoldingRegion::Left => self.left = limits,
            FoldingRegion::Right => self.right = limits,
            FoldingRegion::Bottom => self.bottom = limits,
        }
    }
}

/// Panels shown one at a time behind a strip of tabs. Never empty: a group whose last panel goes
/// is taken out of the tree.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TabGroup {
    pub(crate) id: GroupId,
    pub(crate) panels: Vec<PanelEntry>,
    pub(crate) active: PanelId,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Split {
    pub(crate) id: SplitId,
    pub(crate) axis: SplitAxis,
    /// Two or more.
    pub(crate) parts: Vec<Part>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Part {
    pub(crate) share: Share,
    pub(crate) node: Node,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Node {
    Group(TabGroup),
    Split(Split),
}

impl Node {
    /// Builds a value for the tree from its leaves up.
    pub(crate) fn fold<T>(
        &self,
        mut group: impl FnMut(&TabGroup) -> T,
        mut split: impl FnMut(&Split, Vec<T>) -> T,
    ) -> T {
        enum Visit<'a> {
            Enter(&'a Node),
            Leave(&'a Split),
        }

        let mut pending = vec![Visit::Enter(self)];
        let mut built: Vec<T> = Vec::new();
        while let Some(visit) = pending.pop() {
            match visit {
                Visit::Enter(Node::Group(tabs)) => built.push(group(tabs)),
                Visit::Enter(Node::Split(node)) => {
                    pending.push(Visit::Leave(node));
                    pending.extend(node.parts.iter().rev().map(|part| Visit::Enter(&part.node)));
                }
                Visit::Leave(node) => {
                    let children = built.split_off(built.len() - node.parts.len());
                    built.push(split(node, children));
                }
            }
        }

        built.pop().expect("a tree folds to one value")
    }
}

/// Where a node sits: its region, and the part index at each split on the way down.
#[derive(Clone, Debug, PartialEq)]
struct NodePath {
    region: DockRegion,
    steps: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Region {
    pub(crate) root: Option<Node>,
    pub(crate) open: bool,
}

impl Region {
    const EMPTY: Self = Self { root: None, open: true };
}

/// The tree a region shows, with the ids a view keys its parts by and nothing that changes while
/// the tree keeps its shape.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NodeShape {
    Group(GroupId),
    Split { id: SplitId, axis: SplitAxis, parts: Vec<NodeShape> },
}

/// What a dock holds: each region's tree, whether it shows, and how much room the folding ones
/// take.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Layout {
    center: Region,
    left: Region,
    right: Region,
    bottom: Region,
    pub(crate) extents: RegionExtents,
    pub(crate) limits: FoldingLimits,
    next_id: u64,
}

impl Layout {
    pub(crate) fn new() -> Self {
        Self {
            center: Region::EMPTY,
            left: Region::EMPTY,
            right: Region::EMPTY,
            bottom: Region::EMPTY,
            extents: RegionExtents::DEFAULT,
            limits: FoldingLimits::default(),
            next_id: 0,
        }
    }

    pub(crate) fn region(&self, region: DockRegion) -> &Region {
        match region {
            DockRegion::Center => &self.center,
            DockRegion::Left => &self.left,
            DockRegion::Right => &self.right,
            DockRegion::Bottom => &self.bottom,
        }
    }

    fn region_mut(&mut self, region: DockRegion) -> &mut Region {
        match region {
            DockRegion::Center => &mut self.center,
            DockRegion::Left => &mut self.left,
            DockRegion::Right => &mut self.right,
            DockRegion::Bottom => &mut self.bottom,
        }
    }

    /// Whether `region` takes room: it is open and has a panel in it.
    pub(crate) fn shows(&self, region: DockRegion) -> bool {
        let region = self.region(region);

        region.open && region.root.is_some()
    }

    pub(crate) fn set_open(&mut self, region: DockRegion, open: bool) {
        self.region_mut(region).open = open;
    }

    pub(crate) fn shape(&self, region: DockRegion) -> Option<NodeShape> {
        let root = self.region(region).root.as_ref()?;

        Some(root.fold(
            |group| NodeShape::Group(group.id),
            |split, parts| NodeShape::Split { id: split.id, axis: split.axis, parts },
        ))
    }

    pub(crate) fn group(&self, id: GroupId) -> Option<&TabGroup> {
        let path = self.locate(|node| matches!(node, Node::Group(group) if group.id == id))?;
        match self.node(&path) {
            Node::Group(group) => Some(group),
            Node::Split(_) => None,
        }
    }

    pub(crate) fn region_of(&self, id: GroupId) -> Option<DockRegion> {
        self.locate(|node| matches!(node, Node::Group(group) if group.id == id)).map(|path| path.region)
    }

    /// Whether group `id` is in a region that takes room.
    pub(crate) fn shows_group(&self, id: GroupId) -> bool {
        self.region_of(id).is_some_and(|region| self.shows(region))
    }

    pub(crate) fn split_shares(&self, id: SplitId) -> Vec<Share> {
        let Some(path) = self.locate(|node| matches!(node, Node::Split(split) if split.id == id)) else {
            return Vec::new();
        };

        match self.node(&path) {
            Node::Split(split) => split.parts.iter().map(|part| part.share).collect(),
            Node::Group(_) => Vec::new(),
        }
    }

    /// Writes back the shares a split's parts were resized to. A split that has gone, or a count
    /// that does not match its parts, is a resize of a tree that has since changed and is dropped.
    pub(crate) fn set_split_shares(&mut self, id: SplitId, shares: &[Share]) {
        let Some(path) = self.locate(|node| matches!(node, Node::Split(split) if split.id == id)) else {
            return;
        };
        let Node::Split(split) = self.node_mut(&path) else {
            return;
        };
        if split.parts.len() != shares.len() {
            return;
        }

        for (part, share) in split.parts.iter_mut().zip(shares) {
            part.share = *share;
        }
    }

    pub(crate) fn group_of(&self, panel: &PanelId) -> Option<GroupId> {
        let path = self.locate_panel(panel)?;
        match self.node(&path) {
            Node::Group(group) => Some(group.id),
            Node::Split(_) => None,
        }
    }

    /// Every panel, region by region in [`DockRegion::ALL`] order and in tab order within a group.
    pub(crate) fn panels(&self) -> Vec<PanelEntry> {
        DockRegion::ALL
            .into_iter()
            .filter_map(|region| self.region(region).root.as_ref())
            .flat_map(|root| {
                root.fold(|group| group.panels.clone(), |_, children| children.into_iter().flatten().collect())
            })
            .collect()
    }

    /// The panel each group shows, in the open regions, in [`DockRegion::ALL`] order.
    pub(crate) fn shown_panels(&self) -> Vec<PanelId> {
        DockRegion::ALL
            .into_iter()
            .map(|region| self.region(region))
            .filter(|region| region.open)
            .filter_map(|region| region.root.as_ref())
            .flat_map(|root| {
                root.fold(|group| vec![group.active.clone()], |_, children| children.into_iter().flatten().collect())
            })
            .collect()
    }

    pub(crate) fn add_panel(&mut self, target: DockTarget, entry: PanelEntry) -> Result<GroupId, DockError> {
        if self.locate_panel(&entry.id).is_some() {
            return Err(DockError::DuplicatePanel { id: entry.id });
        }

        let path = match target {
            DockTarget::Region(region) => match self.first_group(region) {
                Some(path) => path,
                None => {
                    let id = self.next_group();
                    let root = Node::Group(TabGroup { id, panels: Vec::new(), active: entry.id.clone() });
                    self.region_mut(region).root = Some(root);
                    NodePath { region, steps: Vec::new() }
                }
            },
            DockTarget::Group(group) => self.locate_group(group)?,
            DockTarget::Beside { group, edge } => self.split_off(group, edge, entry.id.clone())?,
        };

        self.region_mut(path.region).open = true;
        let Node::Group(group) = self.node_mut(&path) else {
            unreachable!("a located group is a group");
        };
        group.active = entry.id.clone();
        group.panels.push(entry);

        Ok(group.id)
    }

    /// Shows `panel` in its group and opens the region it is in.
    pub(crate) fn activate(&mut self, panel: &PanelId) -> Result<(), DockError> {
        let path = self.locate_panel(panel).ok_or_else(|| DockError::UnknownPanel { id: panel.clone() })?;

        self.region_mut(path.region).open = true;
        if let Node::Group(group) = self.node_mut(&path) {
            group.active = panel.clone();
        }

        Ok(())
    }

    pub(crate) fn close(&mut self, panel: &PanelId) -> Result<PanelEntry, DockError> {
        let path = self.locate_panel(panel).ok_or_else(|| DockError::UnknownPanel { id: panel.clone() })?;
        let Node::Group(group) = self.node(&path) else {
            unreachable!("a located panel is in a group");
        };
        if group.panels.iter().any(|entry| &entry.id == panel && !entry.closable) {
            return Err(DockError::NotClosable { id: panel.clone() });
        }

        self.remove(panel)
    }

    /// Takes `panel` out whether or not it is closable. The tab beside it shows in its place; a
    /// group left empty is taken out, and a split left with one part gives its room to that part.
    pub(crate) fn remove(&mut self, panel: &PanelId) -> Result<PanelEntry, DockError> {
        let path = self.locate_panel(panel).ok_or_else(|| DockError::UnknownPanel { id: panel.clone() })?;
        let Node::Group(group) = self.node_mut(&path) else {
            unreachable!("a located panel is in a group");
        };
        let Some(index) = group.panels.iter().position(|entry| &entry.id == panel) else {
            unreachable!("a located panel is in its group");
        };

        let entry = group.panels.remove(index);
        let neighbour = group.panels.get(index).or_else(|| group.panels.last());
        match neighbour {
            Some(next) if group.active == entry.id => group.active = next.id.clone(),
            Some(_) => {}
            None => self.prune(path),
        }

        Ok(entry)
    }

    pub(crate) fn dump(&self) -> SavedLayout {
        let region = |region: &Region| SavedRegion {
            open: region.open,
            root: region.root.as_ref().map(|root| {
                root.fold(
                    |group| SavedNode::Tabs { panels: group.panels.clone(), active: group.active.clone() },
                    |split, children| SavedNode::Split {
                        axis: split.axis,
                        parts: split
                            .parts
                            .iter()
                            .zip(children)
                            .map(|(part, node)| SavedPart { share: part.share, node })
                            .collect(),
                    },
                )
            }),
        };

        SavedLayout {
            center: region(&self.center),
            left: region(&self.left),
            right: region(&self.right),
            bottom: region(&self.bottom),
            extents: self.extents,
        }
    }

    /// A layout from `saved`, keeping only panels whose kind `known` accepts and the first panel
    /// of each id. What was left out is returned beside it.
    pub(crate) fn restore(saved: &SavedLayout, known: impl Fn(&PanelKind) -> bool) -> (Self, Vec<SkippedPanel>) {
        let minted = Cell::new(0);
        let mint = || {
            minted.set(minted.get() + 1);
            minted.get()
        };
        let mut seen = HashSet::new();
        let mut skipped = Vec::new();

        let mut region = |saved: &SavedRegion| {
            let root = saved.root.as_ref().and_then(|root| {
                root.fold(
                    |panels, active| {
                        let mut kept = Vec::new();
                        for entry in panels {
                            let reason = if !known(&entry.kind) {
                                Some(SkipReason::UnknownKind)
                            } else if !seen.insert(entry.id.clone()) {
                                Some(SkipReason::DuplicateId)
                            } else {
                                None
                            };
                            match reason {
                                Some(reason) => skipped.push(SkippedPanel { entry: entry.clone(), reason }),
                                None => kept.push(entry.clone()),
                            }
                        }

                        let first = kept.first()?.id.clone();
                        let active = if kept.iter().any(|entry| &entry.id == active) { active.clone() } else { first };
                        Some(Node::Group(TabGroup { id: GroupId(mint()), panels: kept, active }))
                    },
                    |axis, children| {
                        let parts: Vec<(Share, Node)> =
                            children.into_iter().filter_map(|(share, node)| Some((share, node?))).collect();
                        match parts.len() {
                            0 => None,
                            1 => parts.into_iter().next().map(|(_, node)| node),
                            _ => Some(Node::Split(Split { id: SplitId(mint()), axis, parts: rescaled(parts) })),
                        }
                    },
                )
            });

            Region { root, open: saved.open }
        };

        let layout = Self {
            center: region(&saved.center),
            left: region(&saved.left),
            right: region(&saved.right),
            bottom: region(&saved.bottom),
            extents: saved.extents,
            limits: FoldingLimits::default(),
            next_id: 0,
        };

        (Self { next_id: minted.get(), ..layout }, skipped)
    }

    fn next_group(&mut self) -> GroupId {
        self.next_id += 1;

        GroupId(self.next_id)
    }

    fn next_split(&mut self) -> SplitId {
        self.next_id += 1;

        SplitId(self.next_id)
    }

    /// The first node, depth first across every region, that `matches`.
    fn locate(&self, matches: impl Fn(&Node) -> bool) -> Option<NodePath> {
        for region in DockRegion::ALL {
            let Some(root) = self.region(region).root.as_ref() else {
                continue;
            };

            let mut pending = vec![(root, Vec::new())];
            while let Some((node, steps)) = pending.pop() {
                if matches(node) {
                    return Some(NodePath { region, steps });
                }
                if let Node::Split(split) = node {
                    for (index, part) in split.parts.iter().enumerate().rev() {
                        let mut deeper = steps.clone();
                        deeper.push(index);
                        pending.push((&part.node, deeper));
                    }
                }
            }
        }

        None
    }

    fn locate_panel(&self, panel: &PanelId) -> Option<NodePath> {
        self.locate(|node| matches!(node, Node::Group(group) if group.panels.iter().any(|entry| &entry.id == panel)))
    }

    fn locate_group(&self, id: GroupId) -> Result<NodePath, DockError> {
        self.locate(|node| matches!(node, Node::Group(group) if group.id == id))
            .ok_or(DockError::UnknownGroup { group: id })
    }

    /// The leftmost, topmost group in `region`.
    fn first_group(&self, region: DockRegion) -> Option<NodePath> {
        let mut node = self.region(region).root.as_ref()?;
        let mut steps = Vec::new();
        while let Node::Split(split) = node {
            steps.push(0);
            node = &split.parts[0].node;
        }

        Some(NodePath { region, steps })
    }

    fn node(&self, path: &NodePath) -> &Node {
        let mut node = self.region(path.region).root.as_ref().expect("a path leads into a region's tree");
        for &index in &path.steps {
            let Node::Split(split) = node else {
                unreachable!("a path steps only through splits");
            };
            node = &split.parts[index].node;
        }

        node
    }

    fn node_mut(&mut self, path: &NodePath) -> &mut Node {
        let mut node = self.region_mut(path.region).root.as_mut().expect("a path leads into a region's tree");
        for &index in &path.steps {
            let Node::Split(split) = node else {
                unreachable!("a path steps only through splits");
            };
            node = &mut split.parts[index].node;
        }

        node
    }

    /// Replaces the group `id` with a split of it and a new empty group at `edge`, and returns the
    /// path to the new group, which takes `first` as its active panel.
    fn split_off(&mut self, id: GroupId, edge: Edge, first: PanelId) -> Result<NodePath, DockError> {
        let path = self.locate_group(id)?;
        let new_group = self.next_group();
        let split = self.next_split();

        let (axis, new_first) = match edge {
            Edge::Left => (SplitAxis::Horizontal, true),
            Edge::Right => (SplitAxis::Horizontal, false),
            Edge::Top => (SplitAxis::Vertical, true),
            Edge::Bottom => (SplitAxis::Vertical, false),
        };
        let fresh = Part {
            share: Share(50.0),
            node: Node::Group(TabGroup { id: new_group, panels: Vec::new(), active: first }),
        };
        let slot = self.node_mut(&path);
        let old = Part {
            share: Share(50.0),
            node: std::mem::replace(slot, Node::Split(Split { id: split, axis, parts: Vec::new() })),
        };
        let Node::Split(split) = slot else {
            unreachable!("the slot was just made a split");
        };
        split.parts = if new_first { vec![fresh, old] } else { vec![old, fresh] };

        let mut steps = path.steps;
        steps.push(if new_first { 0 } else { 1 });

        Ok(NodePath { region: path.region, steps })
    }

    /// Takes the node at `path` out of its tree and gives its room to its siblings.
    fn prune(&mut self, path: NodePath) {
        let Some((&last, parent_steps)) = path.steps.split_last() else {
            self.region_mut(path.region).root = None;
            return;
        };

        let parent = NodePath { region: path.region, steps: parent_steps.to_vec() };
        let slot = self.node_mut(&parent);
        let Node::Split(split) = slot else {
            unreachable!("a path steps only through splits");
        };
        split.parts.remove(last);
        if split.parts.len() == 1 {
            let only = split.parts.remove(0).node;
            *slot = only;
            return;
        }

        let parts = std::mem::take(&mut split.parts);
        split.parts = rescaled(parts.into_iter().map(|part| (part.share, part.node)).collect())
    }
}

/// A panel a restore left out, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedPanel {
    pub entry: PanelEntry,
    pub reason: SkipReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SkipReason {
    /// No factory is registered for the panel's kind.
    UnknownKind,
    /// An earlier panel in the saved layout has the same id.
    DuplicateId,
}

/// `parts` with their shares scaled to fill 100 between them, as they were in proportion.
fn rescaled(parts: Vec<(Share, Node)>) -> Vec<Part> {
    let total: f64 = parts.iter().map(|(share, _)| share.0).sum();
    let count = parts.len() as f64;

    parts
        .into_iter()
        .map(|(share, node)| {
            // Parts saved at no size at all share the room evenly rather than dividing by zero.
            let percent = if total > 0.0 { share.0 * 100.0 / total } else { 100.0 / count };
            Part { share: Share(percent.clamp(0.0, 100.0)), node }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> PanelEntry {
        PanelEntry::new(PanelId::new(id), PanelKind::new("view"))
    }

    fn ids(layout: &Layout) -> Vec<String> {
        layout.panels().into_iter().map(|entry| entry.id.0).collect()
    }

    fn active_of(layout: &Layout, group: GroupId) -> String {
        layout.group(group).expect("the group is in the layout").active.0.clone()
    }

    #[test]
    fn a_new_panel_shows_in_its_group_and_opens_its_region() {
        let mut layout = Layout::new();
        layout.set_open(DockRegion::Right, false);

        let group = layout.add_panel(DockTarget::Region(DockRegion::Right), entry("a")).expect("adds");
        layout.add_panel(DockTarget::Group(group), entry("b")).expect("adds");

        assert_eq!(active_of(&layout, group), "b");
        assert!(layout.shows(DockRegion::Right));
        assert_eq!(
            layout.add_panel(DockTarget::Region(DockRegion::Left), entry("a")),
            Err(DockError::DuplicatePanel { id: PanelId::new("a") })
        );
    }

    #[test]
    fn removing_the_shown_panel_shows_the_next_one_then_the_previous() {
        let mut layout = Layout::new();
        let group = layout.add_panel(DockTarget::Region(DockRegion::Center), entry("a")).expect("adds");
        for id in ["b", "c"] {
            layout.add_panel(DockTarget::Group(group), entry(id)).expect("adds");
        }

        layout.activate(&PanelId::new("b")).expect("b is in the dock");
        layout.remove(&PanelId::new("b")).expect("removes");
        assert_eq!(active_of(&layout, group), "c");

        layout.remove(&PanelId::new("c")).expect("removes");
        assert_eq!(active_of(&layout, group), "a");
        assert_eq!(ids(&layout), ["a"]);
    }

    #[test]
    fn closing_a_fixed_panel_is_refused_and_removing_it_is_not() {
        let mut layout = Layout::new();
        let fixed = PanelEntry { closable: false, ..entry("fixed") };
        layout.add_panel(DockTarget::Region(DockRegion::Center), fixed).expect("adds");

        assert_eq!(layout.close(&PanelId::new("fixed")), Err(DockError::NotClosable { id: PanelId::new("fixed") }));
        assert!(layout.remove(&PanelId::new("fixed")).is_ok());
        assert_eq!(layout.region(DockRegion::Center).root, None);
        assert_eq!(layout.remove(&PanelId::new("fixed")), Err(DockError::UnknownPanel { id: PanelId::new("fixed") }));
    }

    #[test]
    fn emptying_one_side_of_a_split_gives_the_whole_room_to_the_other() {
        let mut layout = Layout::new();
        let left = layout.add_panel(DockTarget::Region(DockRegion::Center), entry("a")).expect("adds");
        let right =
            layout.add_panel(DockTarget::Beside { group: left, edge: Edge::Right }, entry("b")).expect("splits");
        layout.add_panel(DockTarget::Beside { group: right, edge: Edge::Bottom }, entry("c")).expect("splits");

        let Some(NodeShape::Split { axis: SplitAxis::Horizontal, parts, .. }) = layout.shape(DockRegion::Center) else {
            panic!("the center is split side by side");
        };
        assert!(matches!(parts.as_slice(), [NodeShape::Group(_), NodeShape::Split { axis: SplitAxis::Vertical, .. }]));

        layout.remove(&PanelId::new("a")).expect("removes");

        let Some(NodeShape::Split { axis: SplitAxis::Vertical, id, .. }) = layout.shape(DockRegion::Center) else {
            panic!("the stacked split is all that is left");
        };
        assert_eq!(layout.split_shares(id), [Share(50.0), Share(50.0)]);
    }

    #[test]
    fn a_closed_region_keeps_its_panels_and_shows_none_of_them() {
        let mut layout = Layout::new();
        layout.add_panel(DockTarget::Region(DockRegion::Bottom), entry("log")).expect("adds");

        layout.set_open(DockRegion::Bottom, false);
        assert!(!layout.shows(DockRegion::Bottom));
        assert_eq!(ids(&layout), ["log"]);

        layout.activate(&PanelId::new("log")).expect("log is in the dock");
        assert!(layout.shows(DockRegion::Bottom));
    }

    #[test]
    fn each_group_shows_its_active_panel_and_a_closed_region_shows_none() {
        let mut layout = Layout::new();
        let left = layout.add_panel(DockTarget::Region(DockRegion::Center), entry("a")).expect("adds");
        layout.add_panel(DockTarget::Group(left), entry("b")).expect("adds");
        layout.add_panel(DockTarget::Beside { group: left, edge: Edge::Right }, entry("c")).expect("splits");
        layout.add_panel(DockTarget::Region(DockRegion::Bottom), entry("log")).expect("adds");
        let shown = |layout: &Layout| layout.shown_panels().into_iter().map(|id| id.0).collect::<Vec<_>>();

        assert_eq!(shown(&layout), ["b", "c", "log"]);

        layout.activate(&PanelId::new("a")).expect("a is in the dock");
        layout.set_open(DockRegion::Bottom, false);
        assert_eq!(shown(&layout), ["a", "c"]);
    }
}

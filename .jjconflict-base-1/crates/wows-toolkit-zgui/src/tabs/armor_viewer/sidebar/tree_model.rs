//! The sidebar's rows as a [`TreeModel`]: one built listing, with what each row refers to.

use std::collections::HashMap;

use landaire_ui::TreeModel;
use wows_toolkit_armor::sidebar_tree::BuiltTree;
use wows_toolkit_armor::sidebar_tree::ItemChildren;
use wows_toolkit_armor::sidebar_tree::RowId;
use wows_toolkit_armor::sidebar_tree::RowKind;
use wows_toolkit_armor::sidebar_tree::RowNote;
use wows_toolkit_armor::sidebar_tree::SidebarItem;
use wowsunpack::export::model_catalog::GeometryAvailability;

/// One built listing. A collapsed folder is a branch with no rows under it; the sidebar builds
/// them when it is expanded.
#[derive(Default)]
pub struct SidebarTree {
    roots: Vec<RowId>,
    nodes: HashMap<RowId, Node>,
    rows: HashMap<RowId, RowKind>,
    notes: HashMap<RowId, RowNote>,
    unavailable: HashMap<RowId, GeometryAvailability>,
}

struct Node {
    label: String,
    parent: Option<RowId>,
    children: Vec<RowId>,
    branch: bool,
}

impl SidebarTree {
    pub fn from_built(built: BuiltTree) -> Self {
        let BuiltTree { items, rows, notes, unavailable } = built;
        let mut nodes = HashMap::new();
        let roots = items.iter().map(|item| item.id.clone()).collect();

        let mut pending: Vec<(SidebarItem, Option<RowId>)> = items.into_iter().map(|item| (item, None)).collect();
        while let Some((item, parent)) = pending.pop() {
            let SidebarItem { id, label, disabled: _, children } = item;
            let (branch, children) = match children {
                ItemChildren::Leaf => (false, Vec::new()),
                ItemChildren::Collapsed => (true, Vec::new()),
                ItemChildren::Expanded(children) => (true, children),
            };
            let child_ids = children.iter().map(|child| child.id.clone()).collect();
            pending.extend(children.into_iter().map(|child| (child, Some(id.clone()))));
            nodes.insert(id, Node { label, parent, children: child_ids, branch });
        }

        Self { roots, nodes, rows, notes, unavailable }
    }

    pub fn kind(&self, id: &RowId) -> Option<&RowKind> {
        self.rows.get(id)
    }

    pub fn label(&self, id: &RowId) -> Option<&str> {
        self.nodes.get(id).map(|node| node.label.as_str())
    }

    pub fn note(&self, id: &RowId) -> Option<&RowNote> {
        self.notes.get(id)
    }

    /// Why the install cannot draw the row's model, for a row drawn dimmed.
    pub fn missing(&self, id: &RowId) -> Option<GeometryAvailability> {
        self.unavailable.get(id).copied()
    }

    pub fn contains(&self, id: &RowId) -> bool {
        self.nodes.contains_key(id)
    }

    /// The rows in the order they are drawn.
    pub fn listed(&self) -> Vec<&RowId> {
        let mut listed = Vec::with_capacity(self.nodes.len());
        let mut pending: Vec<&RowId> = self.roots.iter().rev().collect();
        while let Some(id) = pending.pop() {
            listed.push(id);
            if let Some(node) = self.nodes.get(id) {
                pending.extend(node.children.iter().rev());
            }
        }

        listed
    }

    /// The first listed row that opens a ship, building or model.
    pub fn first_openable(&self) -> Option<RowId> {
        self.listed().into_iter().find(|id| self.kind(id).is_some_and(RowKind::opens_on_its_own)).cloned()
    }

    #[cfg(test)]
    pub fn notes(&self) -> &HashMap<RowId, RowNote> {
        &self.notes
    }
}

impl TreeModel for SidebarTree {
    type Key = RowId;

    fn roots(&self) -> &[RowId] {
        &self.roots
    }

    fn children(&self, key: &RowId) -> &[RowId] {
        self.nodes.get(key).map_or(&[], |node| node.children.as_slice())
    }

    fn parent(&self, key: &RowId) -> Option<RowId> {
        self.nodes.get(key).and_then(|node| node.parent.clone())
    }

    fn is_branch(&self, key: &RowId) -> bool {
        self.nodes.get(key).is_some_and(|node| node.branch)
    }

    fn text(&self, key: &RowId) -> Option<&str> {
        self.label(key)
    }
}

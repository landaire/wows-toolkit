//! The sidebar's rows ([`wows_toolkit_armor::sidebar_tree`]) as
//! gpui-component tree items. A collapsed folder carries one placeholder
//! child so it still draws as a folder.

use gpui_kit::SharedString;
use gpui_kit::component::tree::TreeItem;

use wows_toolkit_armor::sidebar_tree::ItemChildren;
use wows_toolkit_armor::sidebar_tree::RowId;
use wows_toolkit_armor::sidebar_tree::SidebarItem;

/// Folders nest a handful of levels deep, so this recursion stays shallow.
pub(super) fn tree_items(items: Vec<SidebarItem>) -> Vec<TreeItem> {
    items.into_iter().map(tree_item).collect()
}

fn tree_item(item: SidebarItem) -> TreeItem {
    let SidebarItem { id, label, disabled, children } = item;
    let tree_item = TreeItem::new(shared_id(&id), label).disabled(disabled);
    match children {
        ItemChildren::Leaf => tree_item,
        ItemChildren::Collapsed => tree_item.child(TreeItem::new(format!("{id}/placeholder"), "")),
        ItemChildren::Expanded(children) => tree_item.children(tree_items(children)).expanded(true),
    }
}

/// The tree's id for row `id`.
pub(super) fn shared_id(id: &RowId) -> SharedString {
    SharedString::from(id.as_str().to_string())
}

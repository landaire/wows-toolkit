//! The Armor Viewer tab.

pub mod panels;
pub mod sidebar;
pub mod viewport;

use zgui::component;
use zgui::view;
use zgui::view::IntoView;

#[component]
pub fn ArmorViewerTab() -> impl IntoView {
    view! { box(class = "wt-tab-empty", attr:data-testid = "armor-viewer-tab") }
}

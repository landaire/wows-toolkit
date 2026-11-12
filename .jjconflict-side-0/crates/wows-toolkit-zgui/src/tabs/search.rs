//! The Search tab.

use zgui::component;
use zgui::view;
use zgui::view::IntoView;

#[component]
pub fn SearchTab() -> impl IntoView {
    view! { box(class = "wt-tab-empty", attr:data-testid = "search-tab") }
}

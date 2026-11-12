//! The session Stats tab.

use zgui::component;
use zgui::view;
use zgui::view::IntoView;

#[component]
pub fn StatsTab() -> impl IntoView {
    view! { box(class = "wt-tab-empty", attr:data-testid = "stats-tab") }
}

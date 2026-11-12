//! The Player Tracker tab.

use zgui::component;
use zgui::view;
use zgui::view::IntoView;

#[component]
pub fn PlayerTrackerTab() -> impl IntoView {
    view! { box(class = "wt-tab-empty", attr:data-testid = "player-tracker-tab") }
}

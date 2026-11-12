//! The Replays tab.

use zgui::component;
use zgui::view;
use zgui::view::IntoView;

#[component]
pub fn ReplaysTab() -> impl IntoView {
    view! { box(class = "wt-tab-empty", attr:data-testid = "replays-tab") }
}

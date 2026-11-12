//! The Launcher tab.

use zgui::component;
use zgui::view;
use zgui::view::IntoView;

#[component]
pub fn LauncherTab() -> impl IntoView {
    view! { box(class = "wt-tab-empty", attr:data-testid = "launcher-tab") }
}

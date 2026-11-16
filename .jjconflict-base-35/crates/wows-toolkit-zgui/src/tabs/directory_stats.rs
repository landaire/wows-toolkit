//! A Stats tab over every replay in one directory.

use zgui::component;
use zgui::view;
use zgui::view::IntoView;

use crate::shell::tabs::DirectoryStatsId;

#[component]
pub fn DirectoryStatsTab(id: DirectoryStatsId) -> impl IntoView {
    view! { box(class = "wt-tab-empty", attr:data-testid = format!("directory-stats-tab-{}", id.0)) }
}

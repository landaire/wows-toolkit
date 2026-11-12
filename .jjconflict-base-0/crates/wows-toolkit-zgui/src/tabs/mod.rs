//! The tabs' content. [`TabBody`] mounts the component for a [`TabKey`]; the shell mounts it the
//! first time the tab is shown, inside the tab's [`TabChrome`](crate::shell::tabs::TabChrome).
//!
//! Not yet ported, each an empty content area: `replays`, `launcher`, `stats`, `player_tracker`,
//! `search`, `armor_viewer`, `unpacker`, `directory_stats`.

mod armor_viewer;
mod directory_stats;
mod launcher;
mod player_tracker;
mod replays;
mod search;
mod stats;
mod unpacker;

use zgui::component;
use zgui::css;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;

use crate::shell::tabs::AppTab;
use crate::shell::tabs::TabKey;
use crate::tabs::armor_viewer::ArmorViewerTabProps;
use crate::tabs::directory_stats::DirectoryStatsTabProps;
use crate::tabs::launcher::LauncherTabProps;
use crate::tabs::player_tracker::PlayerTrackerTabProps;
use crate::tabs::replays::ReplaysTabProps;
use crate::tabs::search::SearchTabProps;
use crate::tabs::stats::StatsTabProps;
use crate::tabs::unpacker::UnpackerTabProps;

const SHEET: &str = "wt-tabs";

const CSS: &str = css!(
    ".wt-tab-empty {
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
    }"
);

/// The content of `tab`.
#[component]
pub fn TabBody(tab: TabKey) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    match tab {
        TabKey::App(AppTab::ReplayInspector) => AnyView::new(view! { ReplaysTab() }),
        TabKey::App(AppTab::Launcher) => AnyView::new(view! { LauncherTab() }),
        TabKey::App(AppTab::Stats) => AnyView::new(view! { StatsTab() }),
        TabKey::App(AppTab::PlayerTracker) => AnyView::new(view! { PlayerTrackerTab() }),
        TabKey::App(AppTab::Search) => AnyView::new(view! { SearchTab() }),
        TabKey::App(AppTab::ArmorViewer) => AnyView::new(view! { ArmorViewerTab() }),
        TabKey::App(AppTab::Unpacker) => AnyView::new(view! { UnpackerTab() }),
        TabKey::DirectoryStats(id) => AnyView::new(view! { DirectoryStatsTab(id = id) }),
    }
}

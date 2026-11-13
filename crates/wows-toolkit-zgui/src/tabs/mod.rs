//! The tabs' content. [`TabBody`] mounts the component for a [`TabKey`]; the shell mounts it the
//! first time the tab is shown, inside the tab's [`TabChrome`](crate::shell::tabs::TabChrome).

pub mod armor_viewer;
mod directory_stats;
mod launcher;
mod player_tracker;
mod replays;
mod search;
mod stats;
mod unpacker;

use landaire_ui::DockMenuStrings;
use landaire_ui::TabStripStrings;
use zgui::component;
use zgui::css;
use zgui::reactive::Signal;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;

use crate::i18n::Locale;
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

/// The words of a dock's group menus in the reader's language.
fn dock_menu_strings(locale: Locale) -> DockMenuStrings {
    DockMenuStrings {
        menu: Signal::derive_local(locale.text("ui.buttons.more_options")),
        zoom_in: Signal::derive_local(locale.text("ui.tabs.zoom_in_panel")),
        zoom_out: Signal::derive_local(locale.text("ui.tabs.zoom_out_panel")),
        close: Signal::derive_local(locale.text("ui.tabs.close_panel")),
    }
}

/// The words of a tab's right-click menu in the reader's language.
pub(crate) fn tab_menu_strings(locale: Locale) -> TabStripStrings {
    TabStripStrings {
        close: Signal::derive_local(locale.text("ui.tabs.close_tab")),
        close_others: Signal::derive_local(locale.text("ui.tabs.close_other_tabs")),
        close_to_the_right: Signal::derive_local(locale.text("ui.tabs.close_tabs_to_the_right")),
    }
}

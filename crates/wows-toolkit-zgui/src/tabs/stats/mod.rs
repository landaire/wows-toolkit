//! The session Stats tab, and the body a directory stats tab shares with it.

mod art;
mod chart;
mod overview;
#[cfg(test)]
mod panel_tests;
mod rating;
mod ships;

use std::sync::Arc;

use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::PerGameStat;
use zgui::component;
use zgui::reactive::Signal;
use zgui::view;
use zgui::view::IntoView;

/// Which games a [`StatsBody`] shows, and whether its filters, charts and layout are saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsScope {
    /// The recorded session: saved, and its games can be cleared.
    Session,
    /// Every replay in one directory: read only, nothing saved.
    Directory,
}

#[component]
pub fn StatsTab() -> impl IntoView {
    view! { box(class = "wt-tab-empty", attr:data-testid = "stats-tab") }
}

/// The filter bar over the dock of Overview, Ships and charts, for `games` oldest first.
#[component]
pub fn StatsBody(
    scope: StatsScope,
    #[prop(into)] games: Signal<Arc<[PerGameStat]>>,
    #[prop(into)] personal_rating: Signal<Option<Arc<PersonalRatingData>>>,
) -> impl IntoView {
    let _ = (scope, games, personal_rating);
    view! { box(class = "wt-tab-empty") }
}

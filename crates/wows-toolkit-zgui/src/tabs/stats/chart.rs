//! One chart panel: its settings menu over a plot of the games.

use std::sync::Arc;

use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::StatsFilters;
use wows_toolkit_viewmodel::stats::chart::ChartSettings;
use zgui::component;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::view;
use zgui::view::IntoView;

/// `games` is the whole scope, oldest first; `filters` is the tab's filter bar, which the chart
/// follows unless its settings carry their own. The chart writes its changes to `settings`.
#[component]
pub fn StatsChart(
    settings: RwSignal<ChartSettings>,
    #[prop(into)] games: Signal<Arc<[PerGameStat]>>,
    #[prop(into)] filters: Signal<StatsFilters>,
    #[prop(into)] personal_rating: Signal<Option<Arc<PersonalRatingData>>>,
) -> impl IntoView {
    let _ = (settings, games, filters, personal_rating);
    view! { box(attr:data-testid = "stats-chart") }
}

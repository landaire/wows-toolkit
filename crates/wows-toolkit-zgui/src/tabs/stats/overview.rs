//! The Overview panel: the filtered games' summary, rating and achievements.

use std::sync::Arc;

use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::PerGameStat;
use zgui::component;
use zgui::reactive::Signal;
use zgui::view;
use zgui::view::IntoView;

/// `games` are the filtered games, oldest first.
#[component]
pub fn StatsOverview(
    #[prop(into)] games: Signal<Arc<[PerGameStat]>>,
    #[prop(into)] personal_rating: Signal<Option<Arc<PersonalRatingData>>>,
) -> impl IntoView {
    let _ = (games, personal_rating);
    view! { box(attr:data-testid = "stats-overview") }
}

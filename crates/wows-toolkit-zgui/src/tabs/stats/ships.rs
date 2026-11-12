//! The Ships panel: one row per ship played, with its detail table.

use std::sync::Arc;

use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_replays::types::GameParamId;
use zgui::component;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::view;
use zgui::view::IntoView;

/// `games` are the games the filters leave per ship, oldest first. `on_clear_ship` is offered
/// when `clearable`.
#[component]
pub fn StatsShips(
    #[prop(into)] games: Signal<Arc<[PerGameStat]>>,
    #[prop(into)] personal_rating: Signal<Option<Arc<PersonalRatingData>>>,
    clearable: bool,
    on_clear_ship: UnsyncCallback<GameParamId>,
) -> impl IntoView {
    let _ = (games, personal_rating, clearable, on_clear_ship);
    view! { box(attr:data-testid = "stats-ships") }
}

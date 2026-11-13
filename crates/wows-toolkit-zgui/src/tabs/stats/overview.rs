//! The Overview panel: the filtered games' summary, rating and achievements.

use std::collections::HashMap;
use std::sync::Arc;

use landaire_ui::ColumnSizing;
use landaire_ui::ColumnWidth;
use landaire_ui::GridColumn;
use landaire_ui::GridProps;
use landaire_ui::GridSelectionMode;
use landaire_ui::TABULAR_CLASS;
use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use wows_replays::types::GameParamId;
use wows_toolkit_icons::Glyph;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingResult;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::PerformanceInfo;
use wows_toolkit_viewmodel::stats::SerializableAchievement;
use wows_toolkit_viewmodel::stats::SessionSummary;
use wows_toolkit_viewmodel::stats::aggregate_achievements;
use wows_toolkit_viewmodel::stats::per_ship_performance_by_id;
use wows_toolkit_viewmodel::stats::session_personal_rating;
use wowsunpack::data::ResourceLoader;
use wowsunpack::game_params::translations::translate_achievement_description;
use wowsunpack::game_params::translations::translate_achievement_name;
use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::With;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::chevron::CHEVRON_RIGHT;

use crate::icons::AppIconProps;
use crate::services::GameDataState;
use crate::services::Services;
use crate::tabs::stats::art::Art;
use crate::tabs::stats::art::ArtKey;
use crate::tabs::stats::art::GameArt;
use crate::tabs::stats::rating::RatingChipProps;

const SHEET: &str = "wt-stats-overview";

const CSS: &str = css!(
    ".wt-stats-overview {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
    }
    .wt-stats-empty {
        display: flex;
        flex: 1 1 auto;
        align-items: center;
        justify-content: center;
        color: var(--wt-color-text-dim);
    }
    .wt-stats-summary {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
        flex: none;
        align-items: center;
        gap: 12px;
        padding: 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-stats-summary__record { font-weight: 700; }
    .wt-stats-summary__fact {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
        color: var(--wt-color-text-dim);
    }
    .wt-stats-summary__fact .wt-rating-chip { font-weight: 700; }
    .wt-stats-rule {
        flex: none;
        width: 1px;
        height: 1.1em;
        background-color: var(--lui-color-border);
    }
    .wt-stats-achievements {
        display: flex;
        flex-direction: column;
        flex: none;
        gap: 4px;
        padding: 4px 8px;
        border-bottom: 1px solid var(--lui-color-border);
    }
    .wt-stats-achievements[data-open] { max-height: 190px; }
    .wt-stats-achievements__head {
        display: flex;
        flex-direction: row;
        align-items: center;
        gap: 8px;
    }
    .wt-stats-achievements__head .wt-stats-achievements__toggle {
        flex: 1 1 auto;
        justify-content: flex-start;
        gap: 4px;
        font-size: 0.85em;
        font-weight: 700;
    }
    .wt-stats-chevron {
        display: flex;
        flex: none;
        transform: rotate(0deg);
        transition: transform var(--lui-motion-turn) var(--lui-motion-ease);
    }
    .wt-stats-chevron[data-open] { transform: rotate(90deg); }
    .wt-stats-dim-small {
        font-size: 0.85em;
        color: var(--wt-color-text-dim);
    }
    .wt-stats-achievements__list {
        display: block;
        flex: 1 1 auto;
        min-height: 0;
        overflow-y: auto;
    }
    .wt-stats-achievements__wrap {
        display: flex;
        flex-direction: row;
        flex-wrap: wrap;
    }
    .wt-stats-achievement {
        display: flex;
        flex-direction: column;
        align-items: center;
        width: 84px;
        font-size: 0.85em;
    }
    .wt-stats-achievement__art {
        width: 48px;
        height: 48px;
    }
    .wt-stats-achievement__glyph {
        display: flex;
        width: 48px;
        height: 48px;
        align-items: center;
        justify-content: center;
        font-size: 24px;
    }
    .wt-stats-achievement__count { font-weight: 700; }
    .wt-stats-achievement__name {
        text-align: center;
        color: var(--wt-color-text-dim);
    }
    .wt-stats-overview__table {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-height: 0;
    }"
);

/// What the panel shows, recomputed when the games or the rating table change.
#[derive(Clone, Debug, PartialEq)]
struct Computed {
    summary: SessionSummary,
    ships: Vec<OverviewShip>,
    achievements: Vec<SerializableAchievement>,
    /// Absent without an expected-values table, or when no game rated.
    rating: Option<PersonalRatingResult>,
    /// The name each ship goes by, for the records that name only an id.
    names: HashMap<GameParamId, String>,
}

/// One ship's row of the table.
#[derive(Clone, Debug, PartialEq)]
struct OverviewShip {
    name: String,
    info: PerformanceInfo,
}

impl Computed {
    fn from_games(games: &[PerGameStat], table: Option<&PersonalRatingData>) -> Self {
        let games: Vec<&PerGameStat> = games.iter().collect();

        Self {
            summary: SessionSummary::from_games(&games),
            ships: per_ship_performance_by_id(&games)
                .into_iter()
                .map(|(name, info)| OverviewShip { name, info })
                .collect(),
            achievements: aggregate_achievements(&games),
            rating: session_personal_rating(&games, table),
            names: games.iter().map(|game| (game.ship_id, game.ship_name.clone())).collect(),
        }
    }

    fn is_empty(&self) -> bool {
        self.summary.games_played() == 0 && self.ships.is_empty()
    }

    /// The name of the ship `ship` names. A record is set in a game of the session, so its ship
    /// is always among the games; the id stands in only if that ever stops holding.
    fn ship_name(&self, ship: GameParamId) -> String {
        self.names.get(&ship).cloned().unwrap_or_else(|| ship.raw().to_string())
    }
}

/// `games` are the filtered games, oldest first.
#[component]
pub fn StatsOverview(
    #[prop(into)] games: Signal<Arc<[PerGameStat]>>,
    #[prop(into)] personal_rating: Signal<Option<Arc<PersonalRatingData>>>,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let services = Services::current();
    let locale = services.locale;
    let computed = Memo::new(move |_| Computed::from_games(&games.get(), personal_rating.get().as_deref()));

    let art = GameArt::new();
    let wanting = RenderEffect::new(move |_| {
        let keys: Vec<ArtKey> = computed.with(|computed| {
            computed.achievements.iter().map(|earned| ArtKey::Achievement(earned.icon_key.clone())).collect()
        });
        art.want(loaded_build(services), keys);
    });
    on_cleanup_local(move || drop(wanting));

    view! {
        column(class = "wt-stats-overview", attr:data-testid = "stats-overview") {
            if move || computed.with(Computed::is_empty) {
                box(class = "wt-stats-empty") {
                    text {{move || {
                        locale.track();
                        t!("ui.stats.no_games").into_owned()
                    }}}
                }
            } else {
                {move || {
                    locale.track();
                    computed.with(summary_line)
                }}
                Achievements(computed = computed, art = art)
                column(class = "wt-stats-overview__table") {
                    {move || {
                        locale.track();
                        ship_table(computed)
                    }}
                }
            }
        }
    }
}

/// The build the art and the achievement names are read from, once one has loaded.
pub(super) fn loaded_build(services: Services) -> Option<Arc<wows_toolkit_replay_data::load::LoadedGameData>> {
    match services.game_data.state() {
        GameDataState::Ready(data) => Some(data),
        GameDataState::NoInstall | GameDataState::Loading { .. } | GameDataState::Failed(_) => None,
    }
}

/// The record, the rating and the session's best games on one wrapping line.
fn summary_line(computed: &Computed) -> AnyView {
    let summary = &computed.summary;
    let locale = wows_toolkit_viewmodel::locale();
    let record = match summary.win_rate() {
        Some(rate) => format!("{} ({rate:.1}%)", summary.record_label()),
        None => summary.record_label(),
    };
    let rating = computed.rating.as_ref().map(|rating| {
        let chip = format!("{:.0} ({})", rating.pr, rating.category.name());
        let category = rating.category;
        AnyView::new(view! {
            row(class = "wt-stats-summary__fact", attr:data-testid = "stats-overview-rating") {
                box(class = "wt-stats-rule") {}
                text {{t!("ui.stats.pr").into_owned()}}
                RatingChip(text = chip, category = category)
            }
        })
    });
    let frags = t!("ui.stats.frags", count = summary.total_frags).into_owned();
    let best_frags = summary
        .best_frags
        .map(|(ship, count)| t!("ui.stats.best_frags", ship = computed.ship_name(ship), count = count).into_owned());
    let best_damage = summary.best_damage.map(|(ship, damage)| {
        let damage = wows_toolkit_viewmodel::formatting::separate_number(damage, Some(&locale));
        t!("ui.stats.max_damage", ship = computed.ship_name(ship), damage = damage).into_owned()
    });
    let facts: Vec<AnyView> = [Some(frags), best_frags, best_damage]
        .into_iter()
        .flatten()
        .map(|fact| {
            AnyView::new(view! {
                row(class = "wt-stats-summary__fact") {
                    box(class = "wt-stats-rule") {}
                    text {{fact}}
                }
            })
        })
        .collect();

    AnyView::new(view! {
        row(class = "wt-stats-summary", attr:data-testid = "stats-overview-summary") {
            text(class = "wt-stats-summary__record", class = TABULAR_CLASS) {{record}}
            {rating}
            {facts}
        }
    })
}

/// One achievement as the panel lists it, named in the build's language when a build is loaded.
#[derive(Clone, Debug, PartialEq)]
struct ShownAchievement {
    icon_key: String,
    count: usize,
    name: String,
    description: String,
}

fn shown_achievements(achievements: &[SerializableAchievement], services: Services) -> Vec<ShownAchievement> {
    let data = loaded_build(services);
    let loader = data.as_ref().map(|data| data.provider().as_ref() as &dyn ResourceLoader);
    let mut shown: Vec<ShownAchievement> = achievements
        .iter()
        .map(|earned| ShownAchievement {
            icon_key: earned.icon_key.clone(),
            count: earned.count,
            name: loader
                .and_then(|loader| translate_achievement_name(&earned.icon_key, loader))
                .unwrap_or_else(|| earned.display_name.clone()),
            description: loader
                .and_then(|loader| translate_achievement_description(&earned.icon_key, loader))
                .unwrap_or_else(|| earned.description.clone()),
        })
        .collect();
    shown.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));

    shown
}

/// The achievements earned, behind a heading that folds them away.
#[component]
fn Achievements(computed: Memo<Computed>, art: GameArt) -> impl IntoView {
    let services = Services::current();
    let locale = services.locale;
    let open = RwSignal::new_local(true);
    let shown = Memo::new(move |_| {
        locale.track();
        computed.with(|computed| shown_achievements(&computed.achievements, services))
    });
    let open_attr = move || open.get().then(String::new);

    view! {
        if move || shown.with(|shown| !shown.is_empty()) {
            column(class = "wt-stats-achievements", attr:data-open = open_attr, attr:data-testid = "stats-achievements") {
                row(class = "wt-stats-achievements__head") {
                    Button(
                        variant = ButtonVariant::Ghost,
                        size = ButtonSize::Xs,
                        class = "wt-stats-achievements__toggle",
                        attr:data-testid = "stats-achievements-toggle",
                        a11y:expanded = move || open.get(),
                        on:click = move |_| open.update(|open| *open = !*open)
                    ) {
                        box(class = "wt-stats-chevron", attr:data-open = open_attr) {
                            Icon(icon = CHEVRON_RIGHT, size = IconSize::Sm)
                        }
                        text {{locale.text("ui.replay.sections.achievements")}}
                    }
                    text(class = "wt-stats-dim-small", class = TABULAR_CLASS) {{move || shown.with(Vec::len).to_string()}}
                }
                if move || open.get() {
                    scroll(class = "wt-stats-achievements__list") {
                        row(class = "wt-stats-achievements__wrap") {
                            for earned in move || shown.get(), key = |earned: &ShownAchievement| earned.icon_key.clone() {
                                AchievementTile(earned = earned, art = art)
                            }
                        }
                    }
                } else {}
            }
        } else {}
    }
}

/// One achievement: its art, how often it was earned and its name, described on hover.
#[component]
fn AchievementTile(earned: ShownAchievement, art: GameArt) -> impl IntoView {
    let ShownAchievement { icon_key, count, name, description } = earned;
    let hover = if description.is_empty() { name.clone() } else { format!("{name}: {description}") };
    let key = ArtKey::Achievement(icon_key);
    let picture = move || match art.get(&key) {
        Some(Art::Raster(bytes)) => {
            AnyView::new(view! { image(class = "wt-stats-achievement__art", src = Some(bytes.url())) })
        }
        Some(Art::Vector(_)) | None => AnyView::new(view! {
            box(class = "wt-stats-achievement__glyph") {
                AppIcon(glyph = Glyph::Trophy)
            }
        }),
    };

    let label = hover.clone();

    view! {
        Tooltip {
            TooltipTrigger {
                column(class = "wt-stats-achievement", a11y:label = label) {
                    {picture}
                    text(class = "wt-stats-achievement__count", class = TABULAR_CLASS) {{format!("x{count}")}}
                    // Wrapped, not truncated: the name is what tells two achievements apart.
                    text(class = "wt-stats-achievement__name") {{name}}
                }
            }
            TooltipContent {{hover.clone()}}
        }
    }
}

/// The per-ship table, newest played first.
fn ship_table(computed: Memo<Computed>) -> AnyView {
    let locale = wows_toolkit_viewmodel::locale();
    let rows = Signal::derive_local(move || computed.with(|computed| computed.ships.clone()));
    let ship_width = ColumnSizing::Fixed(ColumnWidth(200.0));
    let number_width = ColumnSizing::Fixed(ColumnWidth(96.0));
    let damage_locale = locale.clone();
    let columns = vec![
        GridColumn::new("ship", t!("ui.stats.column_ship").into_owned(), |ship: &OverviewShip| ship.name.clone())
            .text(|ship| ship.name.clone())
            .sizing(ship_width),
        GridColumn::new("battles", t!("ui.stats.column_battles").into_owned(), |ship: &OverviewShip| {
            ship.info.total_games().to_string()
        })
        .text(|ship| ship.info.total_games().to_string())
        .sizing(number_width),
        GridColumn::new("win_rate", t!("ui.stats.column_win_rate").into_owned(), |ship: &OverviewShip| {
            optional_percent(ship.info.win_rate())
        })
        .text(|ship| optional_percent(ship.info.win_rate()))
        .sizing(number_width),
        GridColumn::new("avg_damage", t!("ui.stats.column_avg_damage").into_owned(), move |ship: &OverviewShip| {
            optional_number(ship.info.avg_damage(), &damage_locale)
        })
        .text(move |ship| optional_number(ship.info.avg_damage(), &locale))
        .sizing(number_width),
        GridColumn::new("avg_frags", t!("ui.stats.column_avg_frags").into_owned(), |ship: &OverviewShip| {
            optional_decimal(ship.info.avg_frags())
        })
        .text(|ship| optional_decimal(ship.info.avg_frags()))
        .sizing(number_width),
    ];
    let rows: Signal<Vec<OverviewShip>, LocalStorage> = rows;

    AnyView::new(view! {
        Grid(
            rows = rows,
            columns = columns,
            row_key = |ship: &OverviewShip| ship.info.ship_id(),
            selection_mode = GridSelectionMode::None,
            label = t!("ui.stats.ships").into_owned(),
            class = TABULAR_CLASS,
            attr:data-testid = "stats-overview-ships"
        )
    })
}

/// A dash for an absent value, so an empty cell is never read as a zero.
pub(super) fn optional_percent(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.1}%")).unwrap_or_else(|| "-".to_string())
}

pub(super) fn optional_number(value: Option<f64>, locale: &str) -> String {
    value
        .map(|value| wows_toolkit_viewmodel::formatting::separate_number(value.round() as u64, Some(locale)))
        .unwrap_or_else(|| "-".to_string())
}

pub(super) fn optional_decimal(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.2}")).unwrap_or_else(|| "-".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_value_reads_as_a_dash_rather_than_a_zero() {
        assert_eq!(optional_percent(None), "-");
        assert_eq!(optional_number(None, "en"), "-");
        assert_eq!(optional_decimal(None), "-");
        assert_eq!(optional_percent(Some(52.25)), "52.2%");
        assert_eq!(optional_number(Some(75_000.4), "en"), "75,000");
        assert_eq!(optional_decimal(Some(1.005)), "1.00");
    }
}

//! What one chart shows for its settings, and how each control changes those settings.
//!
//! GPUI's chart panel keeps these as fields of its own; here the settings row is the state, so
//! every change is a whole [`ChartSettings`] the tab can save as it stands.

use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::stats::DivisionFilter;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::StatsFilters;
use wows_toolkit_viewmodel::stats::all_match_groups;
use wows_toolkit_viewmodel::stats::chart::ChartBar;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart::ChartSeries;
use wows_toolkit_viewmodel::stats::chart::ChartSettings;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::chart::bar_chart_series;
use wows_toolkit_viewmodel::stats::chart::line_chart_series;
use wows_toolkit_viewmodel::stats::chart::selectable_stats;
use wows_toolkit_viewmodel::stats::chart::ships_played;
use wows_toolkit_viewmodel::stats::filter_games;
use wows_toolkit_viewmodel::stats::filter_games_per_ship;
use wows_toolkit_viewmodel::stats::per_ship_performance_by_id;

/// Everything a chart draws and offers, derived from its settings and the tab's games.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ChartModel {
    /// The statistic drawn: the saved one, or damage when the saved one cannot be plotted.
    pub stat: ChartableStat,
    /// The filters the chart is drawn under: its own, or the tab's, with the tab's game limit.
    pub filters: StatsFilters,
    /// The ships the filtered games were played in, by name: what the ship picker lists.
    pub played: Vec<(GameParamId, String)>,
    /// The ships plotted.
    pub selected: Vec<GameParamId>,
    /// Every match group the scope holds, which an override can narrow to.
    pub modes: Vec<String>,
    /// The lines, empty in bar mode.
    pub series: Vec<ChartSeries>,
    /// The bars, empty in line mode.
    pub bars: Vec<ChartBar>,
}

impl ChartModel {
    /// What `settings` draws of `games` under the tab's `filters`.
    pub fn new(
        settings: &ChartSettings,
        games: &[PerGameStat],
        filters: &StatsFilters,
        rating: Option<&PersonalRatingData>,
    ) -> Self {
        let stat = shown_stat(settings, rating.is_some());
        let effective = effective_filters(settings, filters);

        // Per ship, as the egui charts count it: a chart of ten ships under "last 25" plots 25
        // battles in each. The picker follows the tab's last N games for the session, so old
        // ships are not offered once the limit is on.
        let per_ship = filter_games_per_ship(games, &effective);
        let mut played = ships_played(&filter_games(games, &effective));
        played.sort_by(|left, right| left.1.cmp(&right.1));
        let selected = selection(settings, &played);

        let series = match settings.mode {
            ChartMode::Line => line_chart_series(
                &per_ship,
                stat,
                &selected,
                rating,
                settings.running || settings.combined,
                settings.combined,
            ),
            ChartMode::Bar => Vec::new(),
        };
        let bars = match settings.mode {
            ChartMode::Line => Vec::new(),
            ChartMode::Bar => {
                let mut ships = per_ship_performance_by_id(&per_ship);
                ships.sort_by(|left, right| left.0.cmp(&right.0));
                bar_chart_series(&ships, stat, &selected, rating)
            }
        };
        let modes = all_match_groups(games).into_iter().collect();

        Self { stat, filters: effective, played, selected, modes, series, bars }
    }

    /// Whether there is nothing to draw.
    pub fn is_empty(&self) -> bool {
        self.series.is_empty() && self.bars.is_empty()
    }
}

/// Whether the reader has picked the ships, rather than the chart following the games.
///
/// An older settings row has no flag but may hold a list, which was a pick.
pub(super) fn selection_touched(settings: &ChartSettings) -> bool {
    settings.selection_touched || !settings.selected_ships.is_empty()
}

pub(super) fn set_stat(settings: &mut ChartSettings, stat: ChartableStat) {
    settings.stat = stat;
}

pub(super) fn set_mode(settings: &mut ChartSettings, mode: ChartMode, rated: bool) {
    settings.mode = mode;
    settle_stat(settings, rated);
}

pub(super) fn set_combined(settings: &mut ChartSettings, combined: bool, rated: bool) {
    settings.combined = combined;
    // One line over every ship is a line, whatever the mode said.
    if combined {
        settings.mode = ChartMode::Line;
    }
    settle_stat(settings, rated);
}

pub(super) fn set_running(settings: &mut ChartSettings, running: bool, rated: bool) {
    settings.running = running;
    settle_stat(settings, rated);
}

/// Takes the chart off the tab's filters, starting from what the tab says so the plot does not
/// jump, or puts it back on them.
pub(super) fn set_own_filters(settings: &mut ChartSettings, own: bool, tab: &StatsFilters) {
    settings.own_filters = own.then(|| tab.clone());
}

pub(super) fn set_own_division(settings: &mut ChartSettings, division: DivisionFilter) {
    if let Some(filters) = settings.own_filters.as_mut() {
        filters.division = division;
    }
}

/// Ticks or unticks one match group of the chart's own filters; `None` is every group.
pub(super) fn toggle_own_mode(settings: &mut ChartSettings, mode: Option<&str>) {
    let Some(filters) = settings.own_filters.as_mut() else { return };
    let Some(mode) = mode else {
        filters.game_modes.clear();
        return;
    };

    if !filters.game_modes.remove(mode) {
        filters.game_modes.insert(mode.to_owned());
    }
}

/// Ticks or unticks `ship`, starting from the ships drawn when the reader has not picked any.
pub(super) fn toggle_ship(settings: &mut ChartSettings, played: &[(GameParamId, String)], ship: GameParamId) {
    let mut selected = selection(settings, played);
    match selected.iter().position(|selected| *selected == ship) {
        Some(at) => {
            selected.remove(at);
        }
        None => selected.push(ship),
    }
    pick_ships(settings, &selected);
}

/// Adds or removes every ship in `ships` at once.
pub(super) fn set_ships_selected(
    settings: &mut ChartSettings,
    played: &[(GameParamId, String)],
    ships: &[GameParamId],
    chosen: bool,
) {
    let mut selected = selection(settings, played);
    if chosen {
        for ship in ships {
            if !selected.contains(ship) {
                selected.push(*ship);
            }
        }
    } else {
        selected.retain(|ship| !ships.contains(ship));
    }
    pick_ships(settings, &selected);
}

/// The settings with the statistic changed to damage when this mode cannot plot it.
pub(super) fn settle_stat(settings: &mut ChartSettings, rated: bool) {
    if !selectable_stats(settings.mode, settings.running, settings.combined, rated).contains(&settings.stat) {
        settings.stat = ChartableStat::Damage;
    }
}

/// The statistic actually drawn. A saved personal-rating chart opened without the rating table
/// draws damage rather than a run of zeroes, and keeps its setting for when the table returns.
fn shown_stat(settings: &ChartSettings, rated: bool) -> ChartableStat {
    if settings.stat.requires_personal_rating() && !rated {
        return ChartableStat::Damage;
    }

    settings.stat
}

fn effective_filters(settings: &ChartSettings, tab: &StatsFilters) -> StatsFilters {
    let mut filters = settings.own_filters.clone().unwrap_or_else(|| tab.clone());
    filters.limit = tab.limit;

    filters
}

/// The ships plotted: the reader's pick, or every ship played until there is one.
fn selection(settings: &ChartSettings, played: &[(GameParamId, String)]) -> Vec<GameParamId> {
    if selection_touched(settings) {
        return settings.selected_ships.iter().copied().map(GameParamId::from).collect();
    }

    played.iter().map(|(id, _)| *id).collect()
}

fn pick_ships(settings: &mut ChartSettings, ships: &[GameParamId]) {
    settings.selected_ships = ships.iter().map(|ship| ship.raw()).collect();
    settings.selection_touched = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use wows_toolkit_viewmodel::stats::GameLimit;

    fn game(ship: &str, id: u64, time: &str, damage: u64, win: bool, mode: &str) -> PerGameStat {
        PerGameStat {
            ship_name: ship.to_string(),
            ship_id: id.into(),
            game_time: time.to_string(),
            sort_key: time.to_string(),
            player_id: 1,
            damage,
            spotting_damage: damage / 10,
            frags: 1,
            raw_xp: damage as i64 / 100,
            base_xp: damage as i64 / 200,
            is_win: win,
            is_loss: !win,
            is_draw: false,
            is_div: false,
            match_group: mode.to_string(),
            achievements: Vec::new(),
        }
    }

    fn session() -> Vec<PerGameStat> {
        vec![
            game("Yamato", 1, "t1", 50_000, true, "pvp"),
            game("Shimakaze", 2, "t2", 30_000, false, "pvp"),
            game("Yamato", 1, "t3", 70_000, true, "ranked"),
            game("Shimakaze", 2, "t4", 20_000, true, "pvp"),
        ]
    }

    #[test]
    fn a_new_chart_draws_a_line_per_ship_played() {
        let model = ChartModel::new(&ChartSettings::default(), &session(), &StatsFilters::default(), None);

        assert_eq!(model.played.iter().map(|(_, name)| name.as_str()).collect::<Vec<_>>(), ["Shimakaze", "Yamato"]);
        assert_eq!(model.selected.len(), 2, "an untouched selection follows the games");
        assert_eq!(model.series.len(), 2);
        assert!(model.bars.is_empty());
        assert_eq!(model.modes, ["pvp", "ranked"]);
    }

    #[test]
    fn bar_mode_draws_a_bar_per_ship_and_no_line() {
        let settings = ChartSettings { mode: ChartMode::Bar, ..ChartSettings::default() };

        let model = ChartModel::new(&settings, &session(), &StatsFilters::default(), None);

        assert!(model.series.is_empty());
        assert_eq!(model.bars.iter().map(|bar| bar.label.as_str()).collect::<Vec<_>>(), ["Shimakaze", "Yamato"]);
        assert_eq!(model.bars[1].value, 60_000.0, "the average over the ship's games");
    }

    #[test]
    fn a_chart_follows_the_tab_until_it_has_filters_of_its_own() {
        let tab = StatsFilters { game_modes: ["ranked".to_string()].into_iter().collect(), ..StatsFilters::default() };
        let mut settings = ChartSettings::default();

        let following = ChartModel::new(&settings, &session(), &tab, None);
        assert_eq!(following.played.len(), 1, "only the ranked game's ship");

        set_own_filters(&mut settings, true, &tab);
        assert_eq!(settings.own_filters.as_ref(), Some(&tab), "an override starts from the tab");
        toggle_own_mode(&mut settings, None);
        let own = ChartModel::new(&settings, &session(), &tab, None);
        assert_eq!(own.played.len(), 2, "every mode once the override clears them");

        set_own_filters(&mut settings, false, &tab);
        assert_eq!(settings.own_filters, None);
    }

    #[test]
    fn the_tab_limit_applies_per_ship_even_under_an_override() {
        let tab = StatsFilters { limit: GameLimit::Recent(1), ..StatsFilters::default() };
        let settings = ChartSettings { own_filters: Some(StatsFilters::default()), ..ChartSettings::default() };

        let model = ChartModel::new(&settings, &session(), &tab, None);

        assert_eq!(model.filters.limit, GameLimit::Recent(1));
        assert!(model.series.iter().all(|series| series.points.len() == 1), "one game per ship");
    }

    #[test]
    fn an_own_mode_toggles_and_all_clears() {
        let mut settings = ChartSettings::default();
        toggle_own_mode(&mut settings, Some("pvp"));
        assert_eq!(settings.own_filters, None, "no override, nothing to narrow");

        set_own_filters(&mut settings, true, &StatsFilters::default());
        toggle_own_mode(&mut settings, Some("pvp"));
        toggle_own_mode(&mut settings, Some("ranked"));
        toggle_own_mode(&mut settings, Some("pvp"));
        assert_eq!(settings.own_filters.as_ref().map(|filters| filters.game_modes.len()), Some(1));
        set_own_division(&mut settings, DivisionFilter::DivOnly);
        assert_eq!(settings.own_filters.as_ref().map(|filters| filters.division), Some(DivisionFilter::DivOnly));
        toggle_own_mode(&mut settings, None);
        assert!(settings.own_filters.as_ref().is_some_and(|filters| filters.game_modes.is_empty()));
    }

    #[test]
    fn unticking_a_ship_starts_from_every_ship_and_is_remembered() {
        let model = ChartModel::new(&ChartSettings::default(), &session(), &StatsFilters::default(), None);
        let mut settings = ChartSettings::default();

        toggle_ship(&mut settings, &model.played, 1u64.into());

        assert!(settings.selection_touched);
        assert_eq!(settings.selected_ships, [2]);
        let model = ChartModel::new(&settings, &session(), &StatsFilters::default(), None);
        assert_eq!(model.series.len(), 1);
        assert_eq!(model.series[0].name, "Shimakaze");
    }

    #[test]
    fn none_selects_no_ship_and_draws_nothing() {
        let played = ChartModel::new(&ChartSettings::default(), &session(), &StatsFilters::default(), None).played;
        let ids: Vec<GameParamId> = played.iter().map(|(id, _)| *id).collect();
        let mut settings = ChartSettings::default();

        set_ships_selected(&mut settings, &played, &ids, false);
        let model = ChartModel::new(&settings, &session(), &StatsFilters::default(), None);
        assert!(settings.selection_touched && settings.selected_ships.is_empty());
        assert!(model.is_empty(), "an explicit empty pick stays empty");

        set_ships_selected(&mut settings, &played, &ids, true);
        assert_eq!(settings.selected_ships.len(), 2);
    }

    #[test]
    fn a_running_only_statistic_falls_back_when_the_line_stops_running() {
        let mut settings = ChartSettings::default();
        set_running(&mut settings, true, false);
        set_stat(&mut settings, ChartableStat::WinRate);

        set_running(&mut settings, false, false);

        assert_eq!(settings.stat, ChartableStat::Damage);
    }

    #[test]
    fn combining_ships_forces_a_running_line() {
        let mut settings =
            ChartSettings { mode: ChartMode::Bar, stat: ChartableStat::WinRate, ..ChartSettings::default() };

        set_combined(&mut settings, true, false);

        assert_eq!(settings.mode, ChartMode::Line);
        assert_eq!(settings.stat, ChartableStat::WinRate, "a combined line is running, so win rate stays");
        let model = ChartModel::new(&settings, &session(), &StatsFilters::default(), None);
        assert_eq!(model.series.len(), 1, "one line over every ship");
    }

    #[test]
    fn a_rating_chart_without_the_table_draws_damage_and_keeps_its_setting() {
        let settings = ChartSettings { stat: ChartableStat::PersonalRating, running: true, ..ChartSettings::default() };

        let model = ChartModel::new(&settings, &session(), &StatsFilters::default(), None);

        assert_eq!(model.stat, ChartableStat::Damage);
        assert_eq!(settings.stat, ChartableStat::PersonalRating);
    }
}

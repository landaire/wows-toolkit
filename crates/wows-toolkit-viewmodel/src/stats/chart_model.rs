//! One chart of the Stats tab: what it plots, how, over which ships, and
//! under which filters.
//!
//! Mirrors the egui Stats tab's chart panes, which are added and closed
//! individually and each hold their own statistic, mode, ship selection and
//! display options (`ui/stats_tab.rs`'s chart toolbar and
//! `ui/session_stats_chart.rs`). Each front end draws the controls and the
//! plot; the state and every rule between the controls live here.

use std::sync::Arc;

use rust_i18n::t;
use serde::Deserialize;
use serde::Serialize;
use wows_replays::types::GameParamId;

use super::DivisionFilter;
use super::PerGameStat;
use super::PerformanceInfo;
use super::StatsFilters;
use super::all_match_groups;
use super::chart::ChartBar;
use super::chart::ChartMode;
use super::chart::ChartSeries;
use super::chart::ChartableStat;
use super::chart::bar_chart_series;
use super::chart::line_chart_series;
use super::chart::ships_played;
use super::filter_games;
use super::filter_games_per_ship;
use super::per_ship_performance_by_id;
use super::plot::PlotView;
use crate::personal_rating::PersonalRatingData;

/// Distinguishes one chart from another in a tab. Never reused within a tab,
/// so a closed chart's state cannot be mistaken for a later one's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChartId(pub usize);

impl ChartId {
    /// The id after this one.
    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

/// One chart as a settings row remembers it.
///
/// The games themselves are not here: they come from the session, and a
/// chart is a way of looking at them rather than a copy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChartSettings {
    #[serde(default)]
    pub id: Option<ChartId>,
    #[serde(default)]
    pub stat: ChartableStat,
    #[serde(default)]
    pub mode: ChartMode,
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub combined: bool,
    #[serde(default)]
    pub show_values: bool,
    /// The chart's own narrowing, when it was taken off the tab's.
    #[serde(default)]
    pub own_filters: Option<StatsFilters>,
    /// The ships plotted, when the reader picked them. Empty follows the
    /// session in older settings rows.
    #[serde(default)]
    pub selected_ships: Vec<GameParamId>,
    /// Distinguishes an explicit empty selection from an older empty list.
    #[serde(default)]
    pub selection_touched: bool,
}

/// A chart's state and the session it plots.
#[derive(Debug, Clone)]
pub struct ChartModel {
    id: ChartId,
    stat: ChartableStat,
    mode: ChartMode,
    /// Whether each point is the average of everything up to it, which is the
    /// egui pane's "rolling average".
    running: bool,
    /// Whether every selected ship is drawn as one line instead of one each.
    combined: bool,
    /// Whether each point carries its own value.
    show_values: bool,
    games: Vec<PerGameStat>,
    /// Every recorded game, before any narrowing, so an override can select
    /// from the whole session rather than from the tab's selection.
    all_games: Vec<PerGameStat>,
    /// What the tab's filter bar currently says, followed unless this chart
    /// has been taken off it.
    tab_filters: StatsFilters,
    /// This chart's own narrowing. `None` follows the tab, which is what a
    /// new chart does; `Some` is an override the reader set here.
    own_filters: Option<StatsFilters>,
    ships: Vec<(String, PerformanceInfo)>,
    /// The ships the games were played in, by name: what the ship filter
    /// lists.
    played: Vec<(GameParamId, String)>,
    selected_ships: Vec<GameParamId>,
    /// Until the selection is touched, it follows the games: a session that
    /// gains a ship plots it rather than leaving it out of a selection made
    /// before it was played.
    selection_touched: bool,
    /// Absent until the expected values are cached, which is what decides
    /// whether personal rating is offered at all.
    personal_rating: Option<Arc<PersonalRatingData>>,
    view: PlotView,
}

impl ChartModel {
    pub fn new(id: ChartId) -> Self {
        Self {
            id,
            stat: ChartableStat::default(),
            mode: ChartMode::default(),
            running: false,
            combined: false,
            show_values: false,
            games: Vec::new(),
            all_games: Vec::new(),
            tab_filters: StatsFilters::default(),
            own_filters: None,
            ships: Vec::new(),
            played: Vec::new(),
            selected_ships: Vec::new(),
            selection_touched: false,
            personal_rating: None,
            view: PlotView::default(),
        }
    }

    pub fn id(&self) -> ChartId {
        self.id
    }

    pub fn stat(&self) -> ChartableStat {
        self.stat
    }

    pub fn mode(&self) -> ChartMode {
        self.mode
    }

    pub fn running(&self) -> bool {
        self.running
    }

    pub fn combined(&self) -> bool {
        self.combined
    }

    pub fn show_values(&self) -> bool {
        self.show_values
    }

    /// The ships the filter lists, by name.
    pub fn played(&self) -> &[(GameParamId, String)] {
        &self.played
    }

    pub fn selected_ships(&self) -> &[GameParamId] {
        &self.selected_ships
    }

    pub fn is_selected(&self, ship: GameParamId) -> bool {
        self.selected_ships.contains(&ship)
    }

    pub fn view(&self) -> PlotView {
        self.view
    }

    pub fn view_mut(&mut self) -> &mut PlotView {
        &mut self.view
    }

    /// Adopts the expected-values table. A chart plotting personal rating
    /// when the table turns out to be unavailable falls back rather than
    /// plotting a run of zeroes.
    pub fn set_personal_rating(&mut self, table: Option<Arc<PersonalRatingData>>) {
        self.personal_rating = table;
        if self.stat.requires_personal_rating() && self.personal_rating.is_none() {
            self.stat = ChartableStat::Damage;
        }
    }

    /// How this chart is currently set up.
    pub fn settings(&self) -> ChartSettings {
        ChartSettings {
            id: Some(self.id),
            stat: self.stat,
            mode: self.mode,
            running: self.running,
            combined: self.combined,
            show_values: self.show_values,
            own_filters: self.own_filters.clone(),
            selected_ships: if self.selection_touched { self.selected_ships.clone() } else { Vec::new() },
            selection_touched: self.selection_touched,
        }
    }

    /// Puts `settings` back on this chart.
    ///
    /// Called before the session is handed over, so the narrowing is in place
    /// the first time the games are filtered.
    pub fn apply_settings(&mut self, settings: ChartSettings) {
        if let Some(id) = settings.id {
            self.id = id;
        }
        self.stat = settings.stat;
        self.mode = settings.mode;
        self.running = settings.running;
        self.combined = settings.combined;
        self.show_values = settings.show_values;
        self.own_filters = settings.own_filters;
        if settings.selection_touched || !settings.selected_ships.is_empty() {
            self.selected_ships = settings.selected_ships;
            self.selection_touched = true;
        }
        self.rebuild();
    }

    /// Adopts the whole session and the tab's filters.
    ///
    /// Both, not just the selection the tab made: a chart the reader has
    /// taken off the tab's filters narrows the session itself, and one that
    /// has not needs the tab's answer.
    pub fn set_games(&mut self, all_games: &[PerGameStat], tab_filters: &StatsFilters) {
        self.all_games = all_games.to_vec();
        self.tab_filters = tab_filters.clone();
        self.rebuild();
    }

    /// Every match group the session holds, which is what an override can
    /// narrow to.
    pub fn offered_modes(&self) -> Vec<String> {
        all_match_groups(&self.all_games).into_iter().collect()
    }

    /// The filters this chart is drawn under: its own division and modes if
    /// it has any, the tab's otherwise, and always the tab's recency limit.
    pub fn effective_filters(&self) -> StatsFilters {
        let mut filters = self.own_filters.as_ref().unwrap_or(&self.tab_filters).clone();
        filters.limit = self.tab_filters.limit;
        filters
    }

    /// Whether this chart narrows the session itself rather than following
    /// the tab's filter bar.
    pub fn overrides_tab(&self) -> bool {
        self.own_filters.is_some()
    }

    /// Takes this chart off the tab's filters, or puts it back on them.
    ///
    /// Taking it off starts from what the tab currently says, so the plot
    /// does not jump the moment the override is turned on.
    pub fn set_overrides_tab(&mut self, overrides: bool) {
        self.own_filters = overrides.then(|| self.tab_filters.clone());
        self.rebuild();
    }

    /// Returns whether the chart has its own filters to change.
    pub fn set_own_division(&mut self, division: DivisionFilter) -> bool {
        let Some(filters) = self.own_filters.as_mut() else { return false };

        filters.division = division;
        self.rebuild();
        true
    }

    /// Returns whether the chart has its own filters to change.
    pub fn toggle_own_mode(&mut self, mode: &str) -> bool {
        let Some(filters) = self.own_filters.as_mut() else { return false };

        if !filters.game_modes.remove(mode) {
            filters.game_modes.insert(mode.to_string());
        }
        self.rebuild();
        true
    }

    /// Returns whether anything was cleared.
    pub fn clear_own_modes(&mut self) -> bool {
        let Some(filters) = self.own_filters.as_mut() else { return false };
        if filters.game_modes.is_empty() {
            return false;
        }

        filters.game_modes.clear();
        self.rebuild();
        true
    }

    /// Re-narrows the session and rebuilds everything drawn from it.
    fn rebuild(&mut self) {
        let filters = self.effective_filters();
        // Per ship, as the egui charts count it (`ui/stats_tab.rs:541`): a chart
        // of ten ships under "last 25" plots 25 battles in each.
        let games = filter_games_per_ship(&self.all_games, &filters);
        self.ships = per_ship_performance_by_id(&games);
        self.games = games.iter().map(|game| (*game).clone()).collect();
        // The chart keeps up to N games per ship, but its picker follows the
        // tab's last N games for the session. Otherwise old ships remain
        // offered after the recent-games limit is enabled.
        let recent_games = filter_games(&self.all_games, &filters);
        self.played = ships_played(&recent_games);
        // Listed by name: a long session's picker has no other order anyone
        // could look a ship up in.
        self.played.sort_by(|left, right| left.1.cmp(&right.1));
        if !self.selection_touched {
            self.selected_ships = self.played.iter().map(|(id, _)| *id).collect();
        }
    }

    /// Statistics this chart can plot.
    ///
    /// Personal rating is offered only once the expected-values table is
    /// cached; without it there is nothing to rate against. Win rate and
    /// personal rating carry no meaning for a single game, so a line plots
    /// them only as a running figure -- the same rule the egui pane applies.
    pub fn selectable_stats(&self) -> Vec<ChartableStat> {
        ChartableStat::ALL
            .into_iter()
            .filter(|stat| !stat.requires_personal_rating() || self.personal_rating.is_some())
            .filter(|stat| self.mode == ChartMode::Bar || stat.is_per_game() || self.running || self.combined)
            .collect()
    }

    /// The lines to draw, empty in bar mode.
    pub fn series(&self) -> Vec<ChartSeries> {
        if self.mode == ChartMode::Bar {
            return Vec::new();
        }

        let games: Vec<&PerGameStat> = self.games.iter().collect();
        line_chart_series(
            &games,
            self.stat,
            &self.selected_ships,
            self.personal_rating.as_deref(),
            self.running || self.combined,
            self.combined,
        )
    }

    /// The bars to draw, empty in line mode.
    pub fn bars(&self) -> Vec<ChartBar> {
        if self.mode == ChartMode::Line {
            return Vec::new();
        }

        let mut ships = self.ships.clone();
        ships.sort_by(|left, right| left.0.cmp(&right.0));
        bar_chart_series(&ships, self.stat, &self.selected_ships, self.personal_rating.as_deref())
    }

    /// What the category axis is called.
    pub fn x_label(&self) -> String {
        match self.mode {
            ChartMode::Bar => t!("ui.stats.column_ship").into_owned(),
            ChartMode::Line => t!("ui.stats.axis_game").into_owned(),
        }
    }

    /// What the value axis is called: the statistic, plus how it is being
    /// read when that is not the raw per-game figure.
    pub fn value_label(&self) -> String {
        if self.mode == ChartMode::Bar {
            return t!("ui.stats.series_average", stat = self.stat.label()).into_owned();
        }
        if self.running || self.combined {
            return t!("ui.stats.series_running", stat = self.stat.label()).into_owned();
        }

        self.stat.label()
    }

    /// The chart's tab title.
    pub fn tab_title(&self) -> String {
        let stat = self.stat.label();
        match self.mode {
            ChartMode::Bar if self.stat != ChartableStat::WinRate => t!("stat.avg_prefix", name = stat).into_owned(),
            ChartMode::Bar => stat,
            ChartMode::Line if self.combined => format!("{stat} {}", t!("chart.combined_suffix")),
            ChartMode::Line if self.running => format!("{stat} {}", t!("chart.rolling_average_suffix")),
            ChartMode::Line => stat,
        }
    }

    /// Returns whether the mode changed.
    pub fn set_mode(&mut self, mode: ChartMode) -> bool {
        if self.mode == mode {
            return false;
        }

        self.mode = mode;
        self.settle_stat();
        self.view.reset();
        true
    }

    /// Returns whether the statistic changed.
    pub fn set_stat(&mut self, stat: ChartableStat) -> bool {
        if self.stat == stat {
            return false;
        }

        self.stat = stat;
        self.view.reset();
        true
    }

    pub fn set_combined(&mut self, combined: bool) {
        self.combined = combined;
        // One line over every ship is a line, whatever the mode said.
        if combined {
            self.mode = ChartMode::Line;
        }
        self.settle_stat();
        self.view.reset();
    }

    pub fn set_running(&mut self, running: bool) {
        self.running = running;
        self.settle_stat();
        self.view.reset();
    }

    pub fn set_show_values(&mut self, show: bool) {
        self.show_values = show;
    }

    /// Falls back to a statistic the current mode can actually plot.
    fn settle_stat(&mut self) {
        if !self.selectable_stats().contains(&self.stat) {
            self.stat = ChartableStat::Damage;
        }
    }

    pub fn toggle_ship(&mut self, ship: GameParamId) {
        self.selection_touched = true;
        if let Some(at) = self.selected_ships.iter().position(|selected| *selected == ship) {
            self.selected_ships.remove(at);
        } else {
            self.selected_ships.push(ship);
        }
    }

    pub fn set_ships_selected(&mut self, ships: &[GameParamId], selected: bool) {
        self.selection_touched = true;
        if selected {
            for ship in ships {
                if !self.selected_ships.contains(ship) {
                    self.selected_ships.push(*ship);
                }
            }
        } else {
            self.selected_ships.retain(|ship| !ships.contains(ship));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::GameLimit;

    fn game(ship: u64, name: &str, sort_key: &str, damage: u64, match_group: &str) -> PerGameStat {
        PerGameStat {
            ship_name: name.to_string(),
            ship_id: GameParamId::from(ship),
            game_time: sort_key.to_string(),
            sort_key: sort_key.to_string(),
            player_id: 1,
            damage,
            spotting_damage: 0,
            frags: 1,
            raw_xp: 1000,
            base_xp: 800,
            is_win: true,
            is_loss: false,
            is_draw: false,
            is_div: false,
            match_group: match_group.to_string(),
            achievements: Vec::new(),
        }
    }

    fn session() -> Vec<PerGameStat> {
        vec![
            game(1, "Yamato", "2026-01-01", 100_000, "pvp"),
            game(2, "Montana", "2026-01-02", 80_000, "ranked"),
            game(1, "Yamato", "2026-01-03", 120_000, "pvp"),
        ]
    }

    /// A chart comes back set up the way it was left, override and ship
    /// selection included.
    #[test]
    fn a_chart_round_trips_through_the_settings_row() {
        let saved = ChartSettings {
            id: Some(ChartId(7)),
            stat: ChartableStat::Frags,
            mode: ChartMode::Bar,
            running: true,
            combined: true,
            show_values: true,
            own_filters: Some(StatsFilters {
                limit: GameLimit::Recent(25),
                division: DivisionFilter::DivOnly,
                game_modes: ["RandomBattle".to_string()].into_iter().collect(),
            }),
            selected_ships: vec![GameParamId::from(4288575440u64), GameParamId::from(3541279184u64)],
            selection_touched: true,
        };

        let json = serde_json::to_string(&saved).expect("a chart serializes");
        let read: ChartSettings = serde_json::from_str(&json).expect("and reads back");
        assert_eq!(read, saved);

        // A row written before a field existed still reads, on the defaults.
        let older: ChartSettings = serde_json::from_str("{}").expect("an empty row reads");
        assert_eq!(older, ChartSettings::default());
    }

    /// The shape the GPUI app has always written: plain numbers for the id
    /// and the ships.
    #[test]
    fn the_settings_row_keeps_its_stored_shape() {
        let row = r#"{"id":3,"stat":"Damage","mode":"Line","running":false,"combined":false,"show_values":false,"own_filters":null,"selected_ships":[42],"selection_touched":true}"#;

        let read: ChartSettings = serde_json::from_str(row).expect("a stored row reads");

        assert_eq!(read.id, Some(ChartId(3)));
        assert_eq!(read.selected_ships, vec![GameParamId::from(42u64)]);
        assert_eq!(serde_json::to_string(&read).expect("it writes back"), row);
    }

    #[test]
    fn an_untouched_selection_follows_the_ships_played() {
        let mut chart = ChartModel::new(ChartId(0));

        chart.set_games(&session(), &StatsFilters::default());

        assert_eq!(chart.played().iter().map(|(_, name)| name.as_str()).collect::<Vec<_>>(), ["Montana", "Yamato"]);
        assert_eq!(chart.selected_ships().len(), 2);
        assert_eq!(chart.settings().selected_ships, Vec::<GameParamId>::new(), "an untouched selection is not saved");
    }

    #[test]
    fn a_touched_selection_narrows_the_lines() {
        let mut chart = ChartModel::new(ChartId(0));
        chart.set_games(&session(), &StatsFilters::default());

        chart.toggle_ship(GameParamId::from(2u64));

        let series = chart.series();
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].name, "Yamato");
        assert_eq!(series[0].points.len(), 2);
        assert!(chart.settings().selection_touched);
    }

    #[test]
    fn combining_forces_a_line_and_offers_running_only_stats() {
        let mut chart = ChartModel::new(ChartId(0));
        chart.set_games(&session(), &StatsFilters::default());
        chart.set_mode(ChartMode::Bar);
        assert!(chart.selectable_stats().contains(&ChartableStat::WinRate));

        chart.set_combined(true);

        assert_eq!(chart.mode(), ChartMode::Line);
        assert!(chart.selectable_stats().contains(&ChartableStat::WinRate), "a combined line is a running one");
        assert!(!chart.selectable_stats().contains(&ChartableStat::PersonalRating), "no table, no rating");
    }

    #[test]
    fn a_line_without_a_running_average_falls_back_from_win_rate() {
        let mut chart = ChartModel::new(ChartId(0));
        chart.set_mode(ChartMode::Bar);
        chart.set_stat(ChartableStat::WinRate);

        chart.set_mode(ChartMode::Line);

        assert_eq!(chart.stat(), ChartableStat::Damage);
    }

    #[test]
    fn its_own_filters_narrow_the_session_but_keep_the_tab_limit() {
        let mut chart = ChartModel::new(ChartId(0));
        let tab = StatsFilters { limit: GameLimit::Recent(1), ..StatsFilters::default() };
        chart.set_games(&session(), &tab);

        chart.set_overrides_tab(true);
        assert!(chart.toggle_own_mode("ranked"));

        assert_eq!(chart.effective_filters().limit, GameLimit::Recent(1));
        assert_eq!(chart.played().iter().map(|(_, name)| name.as_str()).collect::<Vec<_>>(), ["Montana"]);
        assert_eq!(chart.offered_modes(), ["pvp", "ranked"]);
        assert!(chart.clear_own_modes());
        assert!(!chart.clear_own_modes(), "nothing left to clear");
    }

    #[test]
    fn a_bar_chart_draws_one_bar_per_selected_ship() {
        let mut chart = ChartModel::new(ChartId(0));
        chart.set_games(&session(), &StatsFilters::default());
        chart.set_mode(ChartMode::Bar);

        let bars = chart.bars();

        assert_eq!(bars.iter().map(|bar| bar.label.as_str()).collect::<Vec<_>>(), ["Montana", "Yamato"]);
        assert_eq!(bars[1].value, 110_000.0);
    }
}

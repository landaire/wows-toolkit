use egui::Image;
use egui::ImageSource;
use egui::RichText;
use egui::ScrollArea;
use egui_dock::DockArea;
use egui_dock::TabViewer;
use egui_dock::tab_viewer::OnCloseResponse;

use crate::app::ToolkitTabViewer;
use crate::data::directory_stats::DirectoryStatsId;
use crate::data::directory_stats::ReadState;
use crate::data::session_stats::DivisionFilter;
use crate::data::session_stats::PerGameStat;
use crate::data::session_stats::SerializableAchievement;
use crate::data::session_stats::SessionStats;
use crate::data::session_stats::resolve_ship_name;
use crate::data::settings::StatsFilterSettings;
use crate::data::wows_data::GameAsset;
use crate::icons;
use crate::tab_state::ChartMode;
use crate::tab_state::ChartableStat;
use crate::tab_state::SessionStatsChartConfig;
use crate::tab_state::StatsSubTab;
use crate::tab_state::TabState;
use crate::ui::session_stats_chart::render_bar_chart;
use crate::ui::session_stats_chart::render_line_chart;
use crate::util::personal_rating::PersonalRatingData;
use crate::util::personal_rating::PersonalRatingResult;
use crate::util::separate_number;
use rust_i18n::t;
use std::cmp::Reverse;
use std::sync::Arc;
use wows_replays::types::GameParamId;
use wows_toolkit_viewmodel::stats::PerformanceInfo;
use wows_toolkit_viewmodel::stats::PrStats;
use wows_toolkit_viewmodel::stats::table as stats_table;
use wowsunpack::game_params::provider::GameMetadataProvider;

/// A column heading in the active locale.
fn translated_column(column: stats_table::Column) -> String {
    t!(column.translation_key()).into_owned()
}

/// A row label in the active locale.
fn translated_label(label: stats_table::StatLabel) -> String {
    t!(label.translation_key()).into_owned()
}

/// Which games a stats pane shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum StatsScope {
    /// The games the session recorded as they were played.
    Session,
    /// The games in the replays under one directory.
    Directory(DirectoryStatsId),
}

/// The state each stats pane keeps. The session pane's lives in the persisted
/// settings, a directory pane's in its `DirectoryStatsPane`. A write to a
/// directory pane that has closed has nothing to land on and is dropped.
impl TabState {
    fn scoped_stats<R>(&self, scope: StatsScope, read: impl FnOnce(&SessionStats) -> R) -> Option<R> {
        match scope {
            StatsScope::Session => Some(read(&self.persisted.read().session_stats)),
            StatsScope::Directory(id) => self.directory_stats.get(&id).map(|pane| read(&pane.session)),
        }
    }

    fn scoped_filters(&self, scope: StatsScope) -> Option<StatsFilterSettings> {
        match scope {
            StatsScope::Session => Some(self.persisted.read().settings.stats_filters.clone()),
            StatsScope::Directory(id) => self.directory_stats.get(&id).map(|pane| pane.filters.clone()),
        }
    }

    fn set_scoped_filters(&mut self, scope: StatsScope, filters: StatsFilterSettings) {
        match scope {
            StatsScope::Session => self.persisted.write().settings.stats_filters = filters,
            StatsScope::Directory(id) => {
                if let Some(pane) = self.directory_stats.get_mut(&id) {
                    pane.filters = filters;
                }
            }
        }
    }

    /// Mirror the pane's filters onto the games it shows.
    fn sync_scoped_filters(&mut self, scope: StatsScope) {
        fn mirror(filters: &StatsFilterSettings, stats: &mut SessionStats) {
            stats.game_count_limit = if filters.limit_enabled { Some(filters.game_count) } else { None };
            stats.division_filter = filters.division_filter;
            stats.game_mode_filter = filters.game_mode_filter.iter().cloned().collect();
        }

        match scope {
            // Untracked: these three fields are `serde(skip)`, so a write that
            // mirrors them changes nothing the save task would write out. The
            // save task re-serializes the whole persisted state on a five-second
            // timer regardless; a tracked guard here would add a debounced full
            // save roughly every second the tab is open on top of it.
            StatsScope::Session => {
                let mut persisted = self.persisted.write_untracked();
                let persisted = &mut *persisted;
                mirror(&persisted.settings.stats_filters, &mut persisted.session_stats);
            }
            StatsScope::Directory(id) => {
                if let Some(pane) = self.directory_stats.get_mut(&id) {
                    mirror(&pane.filters, &mut pane.session);
                }
            }
        }
    }

    /// Runs `edit` over chart `chart_id`'s settings, creating them on the
    /// chart's first frame.
    fn edit_scoped_chart_config<R>(
        &mut self,
        scope: StatsScope,
        chart_id: u64,
        edit: impl FnOnce(&mut SessionStatsChartConfig) -> R,
    ) -> Option<R> {
        match scope {
            StatsScope::Session => Some(edit(self.persisted.write().chart_configs.entry(chart_id).or_default())),
            StatsScope::Directory(id) => {
                self.directory_stats.get_mut(&id).map(|pane| edit(pane.chart_configs.entry(chart_id).or_default()))
            }
        }
    }

    fn read_scoped_chart_config<R>(
        &self,
        scope: StatsScope,
        chart_id: u64,
        read: impl FnOnce(Option<&SessionStatsChartConfig>) -> R,
    ) -> Option<R> {
        match scope {
            StatsScope::Session => Some(read(self.persisted.read().chart_configs.get(&chart_id))),
            StatsScope::Directory(id) => {
                self.directory_stats.get(&id).map(|pane| read(pane.chart_configs.get(&chart_id)))
            }
        }
    }

    fn remove_scoped_chart_config(&mut self, scope: StatsScope, chart_id: u64) {
        match scope {
            StatsScope::Session => self.remove_chart_config(chart_id),
            StatsScope::Directory(id) => {
                if let Some(pane) = self.directory_stats.get_mut(&id) {
                    pane.chart_configs.remove(&chart_id);
                }
            }
        }
    }

    fn next_scoped_chart_id(&mut self, scope: StatsScope) -> Option<u64> {
        fn take_next(next: &mut u64) -> u64 {
            let id = *next;
            *next += 1;
            id
        }

        match scope {
            StatsScope::Session => Some(take_next(&mut self.persisted.write().next_chart_tab_id)),
            StatsScope::Directory(id) => {
                self.directory_stats.get_mut(&id).map(|pane| take_next(&mut pane.next_chart_tab_id))
            }
        }
    }

    /// Move the pane's sub-dock out for drawing, which needs the rest of
    /// `self` borrowed alongside it.
    fn take_scoped_dock(&mut self, scope: StatsScope) -> Option<egui_dock::DockState<StatsSubTab>> {
        let empty = egui_dock::DockState::new(vec![]);
        match scope {
            // Untracked in both directions: taking the layout out and putting
            // it back leaves the persisted content as it was. The save task
            // re-writes that content on a five-second timer regardless; marking
            // it dirty every frame would add a debounced full save roughly every
            // second this tab is open on top of it.
            StatsScope::Session => {
                Some(std::mem::replace(&mut self.persisted.write_untracked().stats_dock_state, empty))
            }
            StatsScope::Directory(id) => {
                self.directory_stats.get_mut(&id).map(|pane| std::mem::replace(&mut pane.dock_state, empty))
            }
        }
    }

    /// Put a drawn sub-dock back. `changed` is whether the layout moved: a
    /// drag, a split, a sub-tab change or an added chart tab. Only then is the
    /// session pane's write worth saving.
    fn put_scoped_dock(&mut self, scope: StatsScope, dock: egui_dock::DockState<StatsSubTab>, changed: bool) {
        match scope {
            StatsScope::Session => {
                let mut persisted = if changed { self.persisted.write() } else { self.persisted.write_untracked() };
                persisted.stats_dock_state = dock;
            }
            StatsScope::Directory(id) => {
                if let Some(pane) = self.directory_stats.get_mut(&id) {
                    pane.dock_state = dock;
                }
            }
        }
    }
}

/// TabViewer for the stats sub-tabs (Overview / Charts).
///
/// This borrows everything it needs from TabState so that each sub-tab can
/// render its content inside the `egui_dock` DockArea.
struct StatsTabViewer<'a> {
    tab_state: &'a mut TabState,
    scope: StatsScope,
    /// Cached count of Charts tabs -- used to decide closability.
    chart_tab_count: usize,
    /// Pending tab additions (surface, node) -- applied after show_inside returns,
    /// because the dock state is swapped out during rendering.
    pending_adds: Vec<egui_dock::NodePath>,
}

impl TabViewer for StatsTabViewer<'_> {
    type Tab = StatsSubTab;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        // The scope keeps two panes' sub-tabs, and every chart plot inside
        // them, from sharing egui state when both are on screen.
        egui::Id::new(("stats_sub_tab", self.scope, *tab))
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            StatsSubTab::Overview => wt_translations::icon_t(icons::LIST, &t!("ui.stats.overview")).into(),
            StatsSubTab::Charts(id) => {
                let id = *id;
                match self.tab_state.read_scoped_chart_config(self.scope, id, |cfg| cfg.map(chart_title)).flatten() {
                    Some(title) => title.into(),
                    None => format!("{} Chart {}", icons::CHART_LINE, id).into(),
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match *tab {
            StatsSubTab::Overview => build_stats_overview(self.tab_state, self.scope, ui),
            StatsSubTab::Charts(id) => build_stats_charts(self.tab_state, self.scope, id, ui),
        }
    }

    fn closeable(&mut self, tab: &mut Self::Tab) -> bool {
        match tab {
            StatsSubTab::Overview => false,
            // Charts tabs are only closeable when there are more than one.
            StatsSubTab::Charts(_) => self.chart_tab_count > 1,
        }
    }

    fn on_close(&mut self, tab: &mut Self::Tab) -> OnCloseResponse {
        if let StatsSubTab::Charts(id) = tab {
            self.tab_state.remove_scoped_chart_config(self.scope, *id);
        }
        OnCloseResponse::Close
    }

    fn on_add(&mut self, path: egui_dock::NodePath) {
        self.pending_adds.push(path);
    }

    fn allowed_in_windows(&self, _tab: &mut Self::Tab) -> bool {
        false
    }
}

/// A chart sub-tab's title: its stat, and how the chart draws it.
fn chart_title(cfg: &SessionStatsChartConfig) -> String {
    let stat = cfg.selected_stat;
    let combined = cfg.combined;
    let rolling_average = cfg.rolling_average || combined;

    let title = match cfg.mode {
        ChartMode::Bar => match stat {
            ChartableStat::WinRate => stat.name().to_string(),
            _ => format!("Avg {}", stat.name()),
        },
        ChartMode::Line => {
            if combined {
                format!("{} (Combined)", stat.name())
            } else if rolling_average {
                format!("{} (Rolling Average)", stat.name())
            } else {
                stat.name().to_string()
            }
        }
    };

    let icon = match cfg.mode {
        ChartMode::Line => icons::CHART_LINE,
        ChartMode::Bar => icons::CHART_BAR,
    };

    format!("{icon} {title}")
}

impl ToolkitTabViewer<'_> {
    pub fn build_stats_tab(&mut self, ui: &mut egui::Ui) {
        self.build_stats_pane(ui, StatsScope::Session);
    }

    /// A `Tab::DirectoryStats` body: how far the read of the directory has
    /// got, above the same panes the session tab shows.
    pub fn build_directory_stats_tab(&mut self, ui: &mut egui::Ui, id: DirectoryStatsId) {
        // A tab outlives its pane by a frame on the close path.
        let Some(pane) = self.tab_state.directory_stats.get(&id) else {
            return;
        };
        let root = pane.root.display().to_string();
        let read = pane.read;
        let skips = pane.skips;

        let mut reload = false;
        ui.horizontal_wrapped(|ui| {
            ui.strong(wt_translations::icon_t(icons::FOLDER_OPEN, &root));
            ui.separator();
            match read {
                ReadState::Walking => {
                    ui.spinner();
                    ui.label(t!("ui.stats.directory_walking"));
                }
                ReadState::Scanning(progress) => {
                    ui.spinner();
                    ui.label(t!("ui.stats.directory_scanning", done = progress.done, total = progress.total));
                }
                ReadState::Reading(progress) => {
                    ui.spinner();
                    ui.label(t!("ui.stats.directory_reading", done = progress.done, total = progress.total));
                }
                ReadState::Finished => {
                    reload = ui
                        .button(wt_translations::icon_t(icons::ARROW_CLOCKWISE, &t!("ui.stats.directory_reload")))
                        .on_hover_text(t!("ui.stats.directory_reload_tooltip"))
                        .clicked();
                }
            }
            if skips.total() > 0 {
                ui.separator();
                ui.label(t!("ui.stats.directory_skipped", count = skips.total())).on_hover_text(t!(
                    "ui.stats.directory_skipped_detail",
                    unreadable = skips.unreadable,
                    missing_build = skips.missing_build,
                    no_results = skips.no_results
                ));
            }
        });
        if let ReadState::Scanning(progress) | ReadState::Reading(progress) = read {
            ui.add(egui::ProgressBar::new(progress.fraction()).desired_height(4.0));
        }
        if reload {
            self.tab_state.reload_directory_stats(id);
        }

        ui.separator();
        self.build_stats_pane(ui, StatsScope::Directory(id));
    }

    /// The filter bar and the Overview / Charts dock, over the games `scope`
    /// shows.
    fn build_stats_pane(&mut self, ui: &mut egui::Ui, scope: StatsScope) {
        let Some(mut filters) = self.tab_state.scoped_filters(scope) else {
            return;
        };
        let Some(all_modes) = self.tab_state.scoped_stats(scope, SessionStats::all_match_groups) else {
            return;
        };
        let filters_before = filters.clone();

        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut filters.limit_enabled, t!("ui.stats.limit"));
            let mut value = filters.game_count as u32;
            if ui
                .add_enabled(filters.limit_enabled, egui::DragValue::new(&mut value).range(1..=999).speed(0.2))
                .changed()
            {
                filters.game_count = value as usize;
            }

            ui.separator();

            ui.label(t!("ui.stats.division"));
            ui.selectable_value(&mut filters.division_filter, DivisionFilter::All, t!("ui.stats.div_all"));
            ui.selectable_value(&mut filters.division_filter, DivisionFilter::SoloOnly, t!("ui.stats.div_solo"));
            ui.selectable_value(&mut filters.division_filter, DivisionFilter::DivOnly, t!("ui.stats.div_div"));

            // A selection can outlive its mode's games (cleared games, a
            // reloaded directory); the buttons stay so it can be undone.
            if all_modes.len() > 1 || !filters.game_mode_filter.is_empty() {
                ui.separator();
                ui.label(t!("ui.stats.mode_label"));
                if ui.selectable_label(filters.game_mode_filter.is_empty(), t!("ui.stats.div_all")).clicked() {
                    filters.game_mode_filter.clear();
                }
                for mode in &all_modes {
                    let is_selected = filters.game_mode_filter.contains(mode);
                    let display = crate::data::session_stats::match_group_display_name(mode);
                    if ui.selectable_label(is_selected, display).clicked() {
                        if is_selected {
                            filters.game_mode_filter.remove(mode);
                        } else {
                            filters.game_mode_filter.insert(mode.clone());
                        }
                    }
                }
            }

            // Clearing forgets recorded games. A directory pane's games are its
            // replays, which it only reads.
            if scope == StatsScope::Session {
                ui.separator();

                if ui.button(wt_translations::icon_t(icons::ERASER, &t!("ui.stats.clear"))).clicked() {
                    self.tab_state.pending_confirmation = Some(crate::tab_state::ConfirmableAction::ClearSessionStats);
                }
            }
        });

        if filters != filters_before {
            self.tab_state.set_scoped_filters(scope, filters);
        }
        self.tab_state.sync_scoped_filters(scope);

        let Some(mut dock_state) = self.tab_state.take_scoped_dock(scope) else {
            return;
        };
        let layout_before = crate::tab_state::dock_layout_fingerprint(&dock_state);

        // The dock must hold an Overview and at least one Charts tab.
        let has_overview = dock_state.iter_all_tabs().any(|(_, t)| matches!(t, StatsSubTab::Overview));
        let has_chart = dock_state.iter_all_tabs().any(|(_, t)| matches!(t, StatsSubTab::Charts(_)));
        if !has_overview || !has_chart {
            dock_state = crate::tab_state::default_stats_dock_state();
        }

        let chart_tab_count =
            dock_state.iter_all_tabs().filter(|(_, tab)| matches!(tab, StatsSubTab::Charts(_))).count();

        let mut viewer = StatsTabViewer { tab_state: self.tab_state, scope, chart_tab_count, pending_adds: Vec::new() };

        DockArea::new(&mut dock_state)
            .id(egui::Id::new(("stats_dock", scope)))
            .style(egui_dock::Style::from_egui(ui.style().as_ref()))
            .show_add_buttons(true)
            .show_close_buttons(true)
            .show_leaf_collapse_buttons(false)
            .show_leaf_close_all_buttons(false)
            .allowed_splits(egui_dock::AllowedSplits::All)
            .show_inside(ui, &mut viewer);

        // Apply pending tab additions now that we have the real dock_state
        for path in viewer.pending_adds {
            let Some(id) = self.tab_state.next_scoped_chart_id(scope) else {
                continue;
            };
            if let Some(leaf) = dock_state[path.surface][path.node].get_leaf_mut() {
                leaf.append_tab(StatsSubTab::Charts(id));
            }
        }

        let changed = crate::tab_state::dock_layout_fingerprint(&dock_state) != layout_before;
        self.tab_state.put_scoped_dock(scope, dock_state, changed);
    }
}
/// Extract the game metadata provider from tab state (if game data is loaded).
fn get_metadata_provider(tab_state: &TabState) -> Option<Arc<GameMetadataProvider>> {
    tab_state.world_of_warships_data.as_ref().and_then(|wd| wd.read().game_metadata.clone())
}

/// The overview's pane-wide figures, read in one pass over the filtered games.
struct OverviewTotals {
    wins: usize,
    losses: usize,
    draws: usize,
    win_rate: f64,
    personal_rating: Option<PersonalRatingResult>,
    total_frags: i64,
    max_frags: Option<(GameParamId, i64)>,
    max_damage: Option<(GameParamId, u64)>,
    /// Each achievement once, with its count summed across the games.
    achievements: Vec<SerializableAchievement>,
}

impl OverviewTotals {
    fn from_stats(stats: &SessionStats, pr_data: &PersonalRatingData) -> Self {
        let mut achievements: Vec<SerializableAchievement> = Vec::new();
        for game in stats.filtered_games() {
            for achievement in &game.achievements {
                match achievements.iter_mut().find(|existing| existing.game_param_id == achievement.game_param_id) {
                    Some(existing) => {
                        existing.count += achievement.count;
                    }
                    None => achievements.push(achievement.clone()),
                }
            }
        }

        Self {
            wins: stats.games_won(),
            losses: stats.games_lost(),
            draws: stats.games_drawn(),
            win_rate: stats.win_rate().unwrap_or_default(),
            personal_rating: stats.calculate_pr(pr_data),
            total_frags: stats.total_frags(),
            max_frags: stats.max_frags(),
            max_damage: stats.max_damage(),
            achievements,
        }
    }
}

fn build_stats_overview(tab_state: &mut TabState, scope: StatsScope, ui: &mut egui::Ui) {
    let provider = get_metadata_provider(tab_state);
    let provider_ref = provider.as_deref();

    let pr_data_lock = Arc::clone(&tab_state.personal_rating_data);
    let Some(totals) = tab_state.scoped_stats(scope, |stats| OverviewTotals::from_stats(stats, &pr_data_lock.read()))
    else {
        return;
    };
    let OverviewTotals {
        wins,
        losses,
        draws,
        win_rate,
        personal_rating,
        total_frags,
        max_frags,
        max_damage,
        achievements: mut all_achievements,
    } = totals;
    let locale = tab_state.persisted.read().settings.app.locale.clone();
    let locale_ref = locale.as_deref();

    ui.horizontal_wrapped(|ui| {
        // Win rate
        let wld = if draws > 0 { format!("{wins}W/{losses}L/{draws}D") } else { format!("{wins}W/{losses}L") };
        ui.strong(format!("{wld} ({win_rate:.01}%)"));

        // PR
        if let Some(pr_result) = personal_rating {
            ui.separator();
            ui.label(t!("ui.stats.pr"));
            crate::ui::widgets::pr_chip(
                ui,
                pr_result.category,
                &format!("{:.0} ({})", pr_result.pr, pr_result.category.name()),
                true,
            );
        }

        // Total frags
        ui.separator();
        ui.label(t!("ui.stats.frags_count", count = total_frags));

        // Max frags
        if let Some((ship_id, max_frags)) = max_frags {
            let ship = resolve_ship_name(ship_id, provider_ref);
            ui.separator();
            ui.label(t!("ui.stats.best_frags", ship = ship, count = max_frags));
        }

        // Max damage
        if let Some((ship_id, max_damage)) = max_damage {
            let ship = resolve_ship_name(ship_id, provider_ref);
            ui.separator();
            ui.label(t!("ui.stats.max_damage", ship = ship, damage = separate_number(max_damage, locale_ref)));
        }
    });

    let loader: Option<&dyn wowsunpack::data::ResourceLoader> =
        provider_ref.map(|p| p as &dyn wowsunpack::data::ResourceLoader);

    all_achievements.sort_by(|a, b| {
        let name_a = a.resolved_name(loader);
        let name_b = b.resolved_name(loader);
        (Reverse(a.count), &name_a).cmp(&(Reverse(b.count), &name_b))
    });

    if !all_achievements.is_empty()
        && let Some(wows_data_lock) = tab_state.world_of_warships_data.as_ref()
    {
        let icons: Vec<Option<Arc<GameAsset>>> = {
            let wows_data = wows_data_lock.read();
            all_achievements.iter().map(|a| wows_data.cached_achievement_icon(&a.icon_key)).collect()
        };

        let icons: Vec<Option<Arc<GameAsset>>> = if icons.iter().any(|i| i.is_none()) {
            let mut wows_data = wows_data_lock.write();
            all_achievements
                .iter()
                .zip(icons)
                .map(|(a, cached)| cached.or_else(|| wows_data.load_achievement_icon(&a.icon_key)))
                .collect()
        } else {
            icons
        };

        ui.horizontal_wrapped(|ui| {
            for (achievement, icon) in all_achievements.iter().zip(icons) {
                let name = achievement.resolved_name(loader);
                let desc = achievement.resolved_description(loader);
                let tooltip = format!("{}: {}", name, desc);
                ui.vertical(|ui| {
                    ui.set_width(56.0);
                    if let Some(icon) = icon {
                        let image = Image::new(ImageSource::Bytes {
                            uri: icon.path.clone().into(),
                            bytes: icon.data.clone().into(),
                        })
                        .fit_to_exact_size((48.0, 48.0).into());
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.add(image);
                            ui.label(RichText::new(format!("x{}", achievement.count)).small().strong());
                            ui.label(RichText::new(&name).small().weak());
                        });
                    } else {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("x{}", achievement.count)).strong());
                            ui.label(RichText::new(&name).small().weak());
                        });
                    }
                })
                .response
                .on_hover_text(&tooltip);
            }
        });
    }

    ui.separator();

    ScrollArea::vertical().show(ui, |ui| {
        // Grouped and rated once, outside the loop: the loop needs the
        // persisted state mutably, and an immediate-mode redraw would
        // otherwise copy every recorded game on every frame.
        let grouped = tab_state.scoped_stats(scope, |stats| {
            let games: Vec<wows_toolkit_viewmodel::stats::PerGameStat> =
                stats.per_ship_limited_games().into_iter().map(PerGameStat::to_shared).collect();

            let mut games_by_ship: std::collections::HashMap<
                GameParamId,
                Vec<&wows_toolkit_viewmodel::stats::PerGameStat>,
            > = std::collections::HashMap::new();
            for game in &games {
                games_by_ship.entry(game.ship_id).or_default().push(game);
            }

            let pr_data = pr_data_lock.read();
            let mut aggregates: Vec<(GameParamId, PerformanceInfo)> = Vec::with_capacity(games_by_ship.len());
            let mut ratings: std::collections::HashMap<GameParamId, PrStats> = std::collections::HashMap::new();
            for (id, games) in games_by_ship {
                aggregates.push((id, PerformanceInfo::from_games(&games)));
                if let Some(rating) = PrStats::from_games(&games, &pr_data) {
                    ratings.insert(id, rating);
                }
            }
            (aggregates, ratings)
        });
        let Some((mut battle_results, pr_stats_by_ship)) = grouped else {
            return;
        };
        // The id breaks a tie so two ships whose newest game shares a
        // timestamp do not swap places from frame to frame.
        battle_results.sort_by(|a, b| b.1.last_played().cmp(a.1.last_played()).then_with(|| a.0.raw().cmp(&b.0.raw())));
        for (ship_id, perf_info) in &battle_results {
            let Some(win_rate) = perf_info.win_rate() else {
                continue;
            };

            let ship_name = resolve_ship_name(*ship_id, provider_ref);
            let locale = tab_state.persisted.read().settings.app.locale.clone();
            let locale_ref2 = locale.as_deref();
            let pr_stats = pr_stats_by_ship.get(ship_id);

            let wld = if perf_info.draws() > 0 {
                format!("{}W/{}L/{}D", perf_info.wins(), perf_info.losses(), perf_info.draws())
            } else {
                format!("{}W/{}L", perf_info.wins(), perf_info.losses())
            };
            let header = match pr_stats {
                Some(pr) => format!("{ship_name} {wld} ({win_rate:.0}%) - PR: {:.0}", pr.average.pr),
                None => format!("{ship_name} {wld} ({win_rate:.0}%)"),
            };

            let table_rows = stats_table::ship_rows(perf_info, pr_stats, locale_ref2);

            let collapsing_id = ui.make_persistent_id(("ship_stats_collapse", ship_id));
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), collapsing_id, false)
                .show_header(ui, |ui| {
                    ui.label(&header);
                    ui.menu_button(icons::COPY, |ui| {
                        if ui.button(t!("ui.stats.copy_markdown")).clicked() {
                            ui.ctx().copy_text(stats_table::to_markdown(
                                &header,
                                &table_rows,
                                translated_column,
                                translated_label,
                            ));
                            ui.close();
                        }
                        if ui.button(t!("ui.stats.copy_csv")).clicked() {
                            ui.ctx().copy_text(stats_table::to_csv(&table_rows, translated_column, translated_label));
                            ui.close();
                        }
                    });
                    // Removing games forgets recorded ones; a directory pane
                    // only reads its replays.
                    if scope == StatsScope::Session
                        && ui
                            .button(icons::TRASH)
                            .on_hover_text(t!("ui.stats.remove_games_hint", ship = ship_name))
                            .clicked()
                    {
                        if ui.input(|i| i.modifiers.ctrl) {
                            tab_state.persisted.write().session_stats.clear_ship(*ship_id);
                        } else {
                            tab_state.pending_confirmation =
                                Some(crate::tab_state::ConfirmableAction::ClearShipSessionStats { ship_id: *ship_id });
                        }
                    }
                })
                .body(|ui| {
                    egui::Grid::new(("ship_stats", ship_id)).num_columns(5).striped(true).show(ui, |ui| {
                        ui.strong("");
                        for column in stats_table::COLUMNS {
                            ui.strong(translated_column(column));
                        }
                        ui.end_row();

                        for row in &table_rows {
                            ui.label(translated_label(row.label));
                            for column in stats_table::COLUMNS {
                                let cell = row.cell(column);
                                // A rating is chipped by its own band; every
                                // other figure is printed plain, with the
                                // average emphasised.
                                match cell.rating.as_ref() {
                                    Some(rating) => {
                                        let emphasised = column == stats_table::Column::Average;
                                        crate::ui::widgets::pr_chip(ui, rating.category, &cell.text, emphasised)
                                            .on_hover_text(rating.category.name());
                                    }
                                    None if column == stats_table::Column::Average => {
                                        ui.strong(&cell.text);
                                    }
                                    None => {
                                        ui.label(&cell.text);
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });
                });
        }
    });
}
fn build_stats_charts(tab_state: &mut TabState, scope: StatsScope, chart_id: u64, ui: &mut egui::Ui) {
    let provider = get_metadata_provider(tab_state);
    let provider_ref = provider.as_deref();

    // Collect all session data into owned locals first to avoid borrow conflicts
    // with the chart settings, which are written through `tab_state`.
    // Resolve ship names from IDs for display, keyed by GameParamId.
    let Some(ship_stats) = tab_state.scoped_stats(scope, SessionStats::ship_stats_per_ship_limited) else {
        return;
    };
    let ship_stats: Vec<(GameParamId, String, PerformanceInfo)> = ship_stats
        .into_iter()
        .filter(|(_, perf)| perf.win_rate().is_some())
        .map(|(id, perf)| {
            let name = resolve_ship_name(id, provider_ref);
            (id, name, perf)
        })
        .collect();

    let Some(per_game_data) = tab_state
        .scoped_stats(scope, |stats| stats.per_ship_limited_games().into_iter().cloned().collect::<Vec<PerGameStat>>())
    else {
        return;
    };

    // Clone Arc so the read guard doesn't borrow tab_state
    let pr_data_lock = Arc::clone(&tab_state.personal_rating_data);
    let pr_data = pr_data_lock.read();

    let ctx = ui.ctx().clone();

    // Handle screenshot capture if one was requested
    tab_state.edit_scoped_chart_config(scope, chart_id, |cfg| {
        if !cfg.screenshot_requested {
            return;
        }
        let screenshot = ctx.input(|i| {
            for event in &i.raw.events {
                if let egui::Event::Screenshot { image, .. } = event {
                    return Some(image.clone());
                }
            }
            None
        });

        if let Some(screenshot) = screenshot {
            cfg.screenshot_requested = false;

            if let Some(plot_rect) = cfg.plot_rect {
                let pixels_per_point = ctx.pixels_per_point();
                let plot_image = screenshot.region(&plot_rect, Some(pixels_per_point));

                // Copy to clipboard using arboard
                if let Ok(mut clipboard) = arboard::Clipboard::new() {
                    let image_data = arboard::ImageData {
                        width: plot_image.width(),
                        height: plot_image.height(),
                        bytes: std::borrow::Cow::from(plot_image.as_raw().to_vec()),
                    };
                    let _ = clipboard.set_image(image_data);
                }
            }
        }
    });

    if ship_stats.is_empty() {
        match scope {
            StatsScope::Session => ui.label(t!("ui.stats.no_stats")),
            StatsScope::Directory(_) => ui.label(t!("ui.stats.directory_no_games")),
        };
        return;
    }

    // Build sorted list of (id, display_name) for selection UI
    let mut ship_entries: Vec<(GameParamId, String)> =
        ship_stats.iter().map(|(id, name, _)| (*id, name.clone())).collect();
    ship_entries.sort_by(|a, b| a.1.cmp(&b.1));
    let all_ship_ids: Vec<GameParamId> = ship_entries.iter().map(|(id, _)| *id).collect();

    // If no ships selected, select all by default
    tab_state.edit_scoped_chart_config(scope, chart_id, |cfg| {
        if !cfg.selected_ships_manually_changed {
            cfg.selected_ships = all_ship_ids.clone();
        }
    });

    // push_id with chart_id to guarantee unique popup IDs when the same
    // toolbar appears in multiple split panes of the stats dock.
    ui.push_id(chart_id, |ui| {
        ui.horizontal(|ui| {
            // Settings menu button with nested popup support
            ui.menu_button(wt_translations::icon_t(icons::GEAR_FINE, &t!("ui.stats.settings")), |ui| {
                tab_state.edit_scoped_chart_config(scope, chart_id, |cfg| {
                    ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                        ui.set_min_width(280.0);

                        ui.strong(t!("ui.stats.chart_type"));
                        ui.indent(("chart_type_indent", chart_id), |ui| {
                            ui.label(t!("ui.stats.stat_label"));
                            ui.horizontal_wrapped(|ui| {
                                for stat in ChartableStat::all() {
                                    if ui.selectable_label(cfg.selected_stat == *stat, stat.name()).clicked() {
                                        cfg.selected_stat = *stat;
                                        cfg.reset_plot = true;
                                    }
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.label(t!("ui.stats.mode_label"));
                                ui.add_enabled_ui(!cfg.combined, |ui| {
                                    if ui
                                        .selectable_value(&mut cfg.mode, ChartMode::Line, t!("ui.stats.chart_line"))
                                        .clicked()
                                    {
                                        cfg.reset_plot = true;
                                    }
                                    if ui
                                        .selectable_value(&mut cfg.mode, ChartMode::Bar, t!("ui.stats.chart_bar"))
                                        .clicked()
                                    {
                                        cfg.reset_plot = true;
                                    }
                                });
                            });
                        });

                        ui.separator();

                        ui.strong(t!("ui.stats.options"));
                        ui.indent(("options_indent", chart_id), |ui| {
                            if ui.checkbox(&mut cfg.combined, t!("ui.stats.combined")).changed() {
                                cfg.reset_plot = true;
                                if cfg.combined {
                                    cfg.mode = ChartMode::Line;
                                }
                            }
                            if cfg.mode == ChartMode::Line {
                                ui.add_enabled(
                                    !cfg.combined,
                                    egui::Checkbox::new(&mut cfg.rolling_average, t!("ui.stats.rolling_avg")),
                                );
                            }
                            ui.checkbox(&mut cfg.show_labels, t!("ui.stats.labels"));
                        });

                        ui.separator();

                        ui.strong(t!("ui.stats.ships"));
                        ui.indent(("ships_indent", chart_id), |ui| {
                            ui.horizontal(|ui| {
                                if ui.button(t!("ui.stats.all_ships")).clicked() {
                                    cfg.selected_ships = all_ship_ids.clone();
                                    cfg.selected_ships_manually_changed = true;
                                }
                                if ui.button(t!("ui.stats.no_ships")).clicked() {
                                    cfg.selected_ships.clear();
                                    cfg.selected_ships_manually_changed = true;
                                }
                            });
                            for (ship_id, ship_display_name) in &ship_entries {
                                let mut is_selected = cfg.selected_ships.contains(ship_id);
                                if ui.checkbox(&mut is_selected, ship_display_name.as_str()).changed() {
                                    if is_selected {
                                        cfg.selected_ships.push(*ship_id);
                                    } else {
                                        cfg.selected_ships.retain(|s| s != ship_id);
                                    }
                                    cfg.selected_ships_manually_changed = true;
                                }
                            }
                        });
                    });
                });
            });

            // Copy as Image button (stays outside popover)
            let has_plot = tab_state
                .read_scoped_chart_config(scope, chart_id, |cfg| cfg.and_then(|cfg| cfg.plot_rect).is_some())
                == Some(true);
            if has_plot && ui.button(wt_translations::icon_t(icons::CAMERA, &t!("ui.stats.copy_as_image"))).clicked() {
                tab_state.edit_scoped_chart_config(scope, chart_id, |cfg| cfg.screenshot_requested = true);
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
        });
    }); // push_id(chart_id)

    // A snapshot, so the plot draws without holding the settings borrowed. The
    // reset request is consumed: it applies to this frame's plot only.
    let Some(cfg) = tab_state.edit_scoped_chart_config(scope, chart_id, |cfg| {
        let snapshot = cfg.clone();
        cfg.reset_plot = false;
        snapshot
    }) else {
        return;
    };
    let rolling_average = cfg.rolling_average || cfg.combined;

    let mut plot_rect: Option<egui::Rect> = None;

    match cfg.mode {
        ChartMode::Line => {
            let filtered_data: Vec<&PerGameStat> =
                per_game_data.iter().filter(|g| cfg.selected_ships.contains(&g.ship_id)).collect();

            if !filtered_data.is_empty() {
                plot_rect = render_line_chart(
                    ui,
                    &filtered_data,
                    cfg.selected_stat,
                    &cfg.selected_ships,
                    &pr_data,
                    rolling_average,
                    cfg.combined,
                    cfg.show_labels,
                    cfg.reset_plot,
                    chart_id,
                    provider_ref,
                );
            }
        }
        ChartMode::Bar => {
            let mut selected_stats: Vec<(&String, &PerformanceInfo)> = ship_stats
                .iter()
                .filter(|(id, _, _)| cfg.selected_ships.contains(id))
                .map(|(_, name, perf)| (name, perf))
                .collect();

            selected_stats.sort_by_key(|a| a.0);

            if !selected_stats.is_empty() {
                plot_rect = Some(render_bar_chart(
                    ui,
                    &selected_stats,
                    cfg.selected_stat,
                    &pr_data,
                    cfg.show_labels,
                    cfg.reset_plot,
                    chart_id,
                ));
            }
        }
    }

    // Store the plot rect for screenshot cropping
    tab_state.edit_scoped_chart_config(scope, chart_id, |cfg| cfg.plot_rect = plot_rect);
}

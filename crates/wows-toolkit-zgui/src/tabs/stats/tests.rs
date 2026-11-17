use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::DockRegion;
use landaire_ui::DockState;
use landaire_ui::DockTarget;
use landaire_ui::PanelFactory;
use landaire_ui::PanelKind;
use landaire_ui::PanelRegistry;
use landaire_ui::ThemeRootProps;
use landaire_ui::ToastHostProps;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use rust_i18n::t;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::settings::keys;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart::ChartSettings;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use wows_toolkit_viewmodel::stats::session::AppOpened;
use wows_toolkit_viewmodel::stats::session::SessionScope;
use wows_toolkit_viewmodel::stats::setting_keys;
use zgui::component;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui::vocab::Toggled;

use super::CHART_KIND;
use super::StatsBodyProps;
use super::StatsScope;
use super::chart_entry;
use super::chart_title;
use super::layout_chart_ids;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;
use crate::interaction_tests::shell;
use crate::services::Services;
use crate::services::saved_for_test;
use crate::theme::SemanticTokensProps;

/// A session over three ships, two modes, solo and division games, oldest first.
fn sample_games() -> Vec<PerGameStat> {
    let ships = [("Yamato", 4_288_675_536_u64), ("Des Moines", 4_279_200_560), ("Shimakaze", 4_287_510_224)];
    (0..14_u32)
        .map(|index| {
            let (ship, id) = ships[index as usize % ships.len()];
            let damage = 40_000 + u64::from(index) * 9_500 % 110_000;
            let time = format!("2026-10-{:02} 20:{:02}", 1 + index / 3, index * 4 % 60);
            PerGameStat {
                ship_name: ship.to_owned(),
                ship_id: id.into(),
                game_time: time.clone(),
                sort_key: time,
                player_id: 1,
                damage,
                spotting_damage: damage / 3,
                frags: i64::from(index % 4),
                raw_xp: 900 + i64::from(index) * 70,
                base_xp: 600 + i64::from(index) * 40,
                is_win: index % 3 != 1,
                is_loss: index % 3 == 1,
                is_draw: false,
                is_div: index % 4 == 0,
                match_group: if index % 5 == 0 { "ranked" } else { "pvp" }.to_owned(),
                achievements: Vec::new(),
                match_contribution: Some(500 + damage as i64 / 120),
            }
        })
        .collect()
}

/// Writes the sample session into the shared test database, once per process.
fn seed_session() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let runtime = wows_toolkit_services::runtime::build().expect("a tokio runtime starts");
        // First: the seed must go to the test storage, never the reader's own database.
        crate::interaction_tests::returning_reader(&runtime);
        runtime.block_on(async {
            let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
            wows_toolkit_config::queries::clear_session_stats(&pool).await.expect("the session clears");
            for game in sample_games() {
                wows_toolkit_config::queries::add_session_stat(&pool, &game.to_row())
                    .await
                    .expect("a session row is written");
            }
        });
    });
}

/// The shell on the Stats tab over the sample session. Seeded first: the shell reads the saved
/// session as it starts.
fn open_stats(options: HeadlessOptions) -> HeadlessWindow {
    seed_session();
    let mut window = open_shell(options);
    let tab = window.by_test_id("stats").expect("the strip has a Stats tab");
    window.click(tab).expect("the Stats tab is clicked");
    settle_in_real_time(&mut window);
    window
}

fn checked(window: &mut HeadlessWindow, test_id: &str) -> Option<Toggled> {
    let element = window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is on screen"));
    window.a11y(element).and_then(|state| state.checked)
}

fn click(window: &mut HeadlessWindow, test_id: &str) {
    let element = window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is on screen"));
    window.click(element).unwrap_or_else(|err| panic!("{test_id} is clicked: {err}"));
    window.advance(Duration::from_millis(300));
}

fn dock_tab_selected(window: &mut HeadlessWindow, label: &str) -> Option<bool> {
    let tab = window.by_role_and_label(Role::Tab, label)?;
    window.a11y(tab).and_then(|state| state.selected)
}

#[test]
fn the_stats_dock_opens_on_the_overview_with_a_chart_beside_it() {
    let mut window = open_stats(HeadlessOptions::new(crate::interaction_tests::REVIEW_SIZE));

    let overview = t!("ui.stats.overview").into_owned();
    let ships = t!("ui.stats.ships").into_owned();
    assert_eq!(dock_tab_selected(&mut window, &overview), Some(true), "the tab opens on its overview");
    assert_eq!(dock_tab_selected(&mut window, &ships), Some(false));
    assert!(window.by_test_id("stats-chart").is_some(), "the chart is visible beside the overview");

    let tab = window.by_role_and_label(Role::Tab, &ships).expect("the Ships tab is in the dock");
    window.click(tab).expect("the Ships tab is clicked");
    window.advance(Duration::from_millis(300));
    assert_eq!(dock_tab_selected(&mut window, &ships), Some(true));
    assert_eq!(dock_tab_selected(&mut window, &overview), Some(false));
    assert!(window.by_test_id("stats-chart").is_some(), "the chart stays visible beside the ship listing");
}

#[test]
fn the_recent_games_limit_can_be_switched_on_and_off() {
    let mut window = open_stats(HeadlessOptions::new(crate::interaction_tests::REVIEW_SIZE));

    assert_eq!(checked(&mut window, "stats-limit-enabled"), Some(Toggled::False), "no limit is saved");
    assert!(window.by_test_id("stats-limit-count").is_some(), "the count box is present");

    click(&mut window, "stats-limit-enabled");
    assert_eq!(checked(&mut window, "stats-limit-enabled"), Some(Toggled::True));

    click(&mut window, "stats-limit-enabled");
    assert_eq!(checked(&mut window, "stats-limit-enabled"), Some(Toggled::False), "the limit toggles back off");
}

#[test]
fn the_stats_division_filter_is_single_select() {
    let mut window = open_stats(HeadlessOptions::new(crate::interaction_tests::REVIEW_SIZE));

    assert_eq!(checked(&mut window, "stats-division-all"), Some(Toggled::True), "All is the default");

    click(&mut window, "stats-division-solo");
    assert_eq!(checked(&mut window, "stats-division-solo"), Some(Toggled::True));
    assert_eq!(checked(&mut window, "stats-division-all"), Some(Toggled::False), "the filter is single-select");

    click(&mut window, "stats-division-div");
    assert_eq!(checked(&mut window, "stats-division-div"), Some(Toggled::True));
    assert_eq!(checked(&mut window, "stats-division-solo"), Some(Toggled::False));

    click(&mut window, "stats-division-all");
}

/// The mode filter's button stands out while it narrows the games, and stops once it does not.
#[test]
fn the_mode_filter_button_shows_when_it_narrows_the_games() {
    let mut window = open_stats(HeadlessOptions::new(crate::interaction_tests::REVIEW_SIZE));
    let narrowed = |window: &HeadlessWindow| {
        let trigger = window.by_test_id("stats-mode-filter-trigger").expect("two modes offer the mode filter");
        window.all_by_attribute("data-narrowed", "true").contains(&trigger)
    };
    assert!(!narrowed(&window), "every mode is shown to begin with");

    click(&mut window, "stats-mode-filter-trigger");
    click(&mut window, "stats-mode-pvp");
    assert!(narrowed(&window), "one mode ticked narrows the games");

    click(&mut window, "stats-mode-pvp");
    assert!(!narrowed(&window), "unticking it shows every mode again");
}

/// What a test drives a [`DirectoryRoot`] through: its games, and whether they are being read.
#[derive(Clone, Copy)]
struct DirectoryHandles {
    games: RwSignal<Arc<[PerGameStat]>>,
    reading: RwSignal<bool, LocalStorage>,
}

/// A directory's Stats body over games the test replaces, with the services and theme the shell
/// gives it.
#[component]
fn DirectoryRoot(handles: Rc<RefCell<Option<DirectoryHandles>>>) -> impl IntoView {
    Services::provide(None);
    let games: RwSignal<Arc<[PerGameStat]>> = RwSignal::new(Arc::from(sample_games()));
    let reading = RwSignal::new_local(false);
    *handles.borrow_mut() = Some(DirectoryHandles { games, reading });
    let rating = RwSignal::new(None::<Arc<PersonalRatingData>>);
    view! {
        ThemeRoot {
            SemanticTokens(choice = ThemeChoice::System)
            ToastHost {
                column(style:width = "100%", style:height = "100%") {
                    StatsBody(scope = StatsScope::Directory, games = games, personal_rating = rating, reading = reading)
                }
            }
        }
    }
}

/// A directory's Stats body over the sample games, with Ranked ticked in the mode filter.
fn open_directory_on_ranked() -> (HeadlessWindow, DirectoryHandles) {
    let handles = Rc::new(RefCell::new(None));
    let mut window = {
        let handles = Rc::clone(&handles);
        HeadlessWindow::open(HeadlessOptions::new(crate::interaction_tests::REVIEW_SIZE), move || {
            view! { DirectoryRoot(handles = Rc::clone(&handles)) }
        })
        .expect("the stats body opens headlessly")
    };
    window.advance(Duration::from_millis(300));
    let handles = handles.borrow_mut().take().expect("the root publishes its handles");

    click(&mut window, "stats-mode-filter-trigger");
    click(&mut window, "stats-mode-ranked");
    window.press_key(NamedKey::Escape, Modifiers::NONE);
    window.advance(Duration::from_millis(300));
    assert!(window.by_test_id("stats-overview-summary").is_some(), "the ranked games are summed");
    (window, handles)
}

#[test]
fn a_ticked_mode_whose_games_are_gone_is_dropped() {
    let (mut window, handles) = open_directory_on_ranked();

    let random: Vec<PerGameStat> = sample_games().into_iter().filter(|game| game.match_group == "pvp").collect();
    handles.games.set(Arc::from(random));
    window.advance(Duration::from_millis(300));
    assert!(window.by_test_id("stats-mode-filter-trigger").is_none(), "one mode left hides the mode menu");
    assert!(window.by_test_id("stats-overview-summary").is_some(), "the remaining games are summed");
}

/// A reload empties the games before it reads them again; the ticked mode outlasts that.
#[test]
fn a_ticked_mode_outlasts_a_directory_reload() {
    let (mut window, handles) = open_directory_on_ranked();

    handles.reading.set(true);
    handles.games.set(Arc::from([]));
    window.advance(Duration::from_millis(300));
    handles.games.set(Arc::from(sample_games()));
    handles.reading.set(false);
    window.advance(Duration::from_millis(300));

    let trigger = window.by_test_id("stats-mode-filter-trigger").expect("two modes offer the mode filter");
    assert!(window.all_by_attribute("data-narrowed", "true").contains(&trigger), "Ranked is still ticked");
    assert!(window.by_test_id("stats-overview-summary").is_some(), "the ranked games are summed again");
}

#[test]
fn adding_a_chart_opens_another_panel_on_the_asked_statistic() {
    let mut window = open_stats(HeadlessOptions::new(crate::interaction_tests::REVIEW_SIZE));
    let frags_title = chart_title(&ChartSettings { stat: ChartableStat::Frags, ..ChartSettings::default() });
    assert!(window.by_role_and_label(Role::Tab, &frags_title).is_none(), "one damage chart is open to begin with");

    click(&mut window, "stats-add-chart");
    click(&mut window, &format!("stats-add-chart-stat-{}", ChartableStat::Frags as usize));

    assert!(window.by_role_and_label(Role::Tab, &frags_title).is_some(), "the new panel plots frags");
}

#[test]
fn the_status_bar_hides_and_shows_the_chart_dock() {
    let mut window = open_shell(HeadlessOptions::new(crate::interaction_tests::REVIEW_SIZE));
    seed_session();
    // Other tests in the process save their own layouts; this one starts from the default.
    let runtime = wows_toolkit_services::runtime::build().expect("a tokio runtime starts");
    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
        sqlx::query("DELETE FROM settings WHERE key IN (?1, ?2)")
            .bind(super::CHARTS_SETTINGS_KEY)
            .bind(super::CHARTS_DOCK_LAYOUT_KEY)
            .execute(&pool)
            .await
            .expect("the saved charts are forgotten");
    });
    click(&mut window, "stats");
    // The saved charts are read on the tokio runtime and replace the dock when they land.
    settle_in_real_time(&mut window);
    assert!(window.by_test_id("stats-dock-toggle-left").is_none(), "an empty dock has no toggle");

    // A dock tab drawn in the right half of the window, below the filter bar, is the chart dock's.
    // A folded dock keeps its tabs laid out past the window's edge.
    let showing = |window: &mut HeadlessWindow| {
        let strip = window.by_test_id("stats-filter-bar").and_then(|bar| window.bounds(bar)).map(|bar| bar.bottom().0);
        let size = crate::interaction_tests::REVIEW_SIZE;
        window.by_role(Role::Tab).into_iter().any(|tab| {
            window.bounds(tab).is_some_and(|bounds| {
                bounds.width().0 > 0.0
                    && bounds.left().0 > size.width / 2.0
                    && bounds.right().0 <= size.width
                    && strip.is_some_and(|bottom| bounds.top().0 >= bottom)
            })
        })
    };
    assert!(showing(&mut window), "the default layout shows the chart dock");
    click(&mut window, "stats-dock-toggle-right");
    assert!(!showing(&mut window), "the toggle folds the chart dock away");
    click(&mut window, "stats-dock-toggle-right");
    assert!(showing(&mut window), "and opens it again");
}

/// Makes the database refuse a chart settings row naming `stat`, or stops it refusing. Other
/// tests share the database, so only the rows of a statistic they never chart are refused.
fn refuse_chart_rows_naming(stat: ChartableStat, refuse: bool) {
    let name = serde_json::to_string(&stat).expect("a statistic serializes");
    let runtime = wows_toolkit_services::runtime::build().expect("a tokio runtime starts");
    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
        for event in ["INSERT", "UPDATE"] {
            let trigger = format!("refuse_chart_rows_{}", event.to_lowercase());
            let statement = if refuse {
                format!(
                    "CREATE TRIGGER IF NOT EXISTS {trigger} BEFORE {event} ON settings \
                     WHEN NEW.key = '{key}' AND instr(NEW.value, '{name}') > 0 \
                     BEGIN SELECT RAISE(ABORT, 'refused by the test'); END",
                    key = super::CHARTS_SETTINGS_KEY,
                )
            } else {
                format!("DROP TRIGGER IF EXISTS {trigger}")
            };
            sqlx::query(&statement).execute(&pool).await.expect("the trigger is changed");
        }
    });
}

/// The stats of the saved chart row, or `None` when no row has been written.
fn saved_chart_stats() -> Option<Vec<ChartableStat>> {
    let runtime = wows_toolkit_services::runtime::build().expect("a tokio runtime starts");
    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
        let saved =
            wows_toolkit_config::queries::try_get_setting::<Vec<ChartSettings>>(&pool, super::CHARTS_SETTINGS_KEY)
                .await
                .expect("the saved charts are read")?;

        Some(saved.into_iter().map(|settings| settings.stat).collect())
    })
}

/// Adds a chart whose settings row the database refuses, and lets the write fail.
fn fail_a_chart_save(window: &mut HeadlessWindow) {
    refuse_chart_rows_naming(ChartableStat::SpottingDamage, true);
    click(window, "stats-add-chart");
    click(window, &format!("stats-add-chart-stat-{}", ChartableStat::SpottingDamage as usize));
    settle_in_real_time(window);
}

#[test]
fn a_failed_chart_save_is_shown_until_a_retry_writes_it() {
    let mut window = open_stats(HeadlessOptions::new(crate::interaction_tests::REVIEW_SIZE));
    fail_a_chart_save(&mut window);
    assert!(window.by_test_id("stats-charts-error").is_some(), "the refused write shows in the bar");
    let saved = saved_chart_stats();
    assert!(
        !saved.is_some_and(|stats| stats.contains(&ChartableStat::SpottingDamage)),
        "the refused write is not saved"
    );

    click(&mut window, "stats-retry-chart-load");
    settle_in_real_time(&mut window);
    assert!(window.by_test_id("stats-charts-error").is_some(), "a retry that fails again stays shown");

    refuse_chart_rows_naming(ChartableStat::SpottingDamage, false);
    click(&mut window, "stats-retry-chart-load");
    settle_in_real_time(&mut window);
    assert!(window.by_test_id("stats-charts-error").is_none(), "the error goes once the write lands");
    let saved = saved_chart_stats().expect("the retry wrote the chart row");
    assert!(saved.contains(&ChartableStat::SpottingDamage), "the retry wrote the charts on screen");
}

#[test]
fn a_chart_tab_names_its_statistic_and_how_it_is_plotted() {
    let damage = ChartableStat::Damage.label();
    let line = ChartSettings::default();
    assert_eq!(chart_title(&line), damage);
    let running = ChartSettings { running: true, ..ChartSettings::default() };
    assert_eq!(chart_title(&running), format!("{damage} {}", t!("chart.rolling_average_suffix")));
    let bar = ChartSettings { mode: ChartMode::Bar, ..ChartSettings::default() };
    assert_eq!(chart_title(&bar), t!("stat.avg_prefix", name = damage));
    let win_rate = ChartSettings { mode: ChartMode::Bar, stat: ChartableStat::WinRate, ..ChartSettings::default() };
    assert_eq!(chart_title(&win_rate), ChartableStat::WinRate.label(), "a win rate is not averaged twice");
}

#[test]
fn a_saved_layout_names_the_charts_it_holds() {
    let owner = zgui::reactive::Owner::new();
    owner.with(|| {
        let mut registry = PanelRegistry::new();
        registry.register(PanelKind::new(CHART_KIND), PanelFactory::new(|_| String::new(), |_| AnyView::new(())));
        let dock = DockState::new(registry);
        dock.add_panel(DockTarget::Region(DockRegion::Right), chart_entry(3)).expect("a chart is added");
        dock.add_panel(DockTarget::Region(DockRegion::Left), chart_entry(7)).expect("a second is added");
        let mut ids = layout_chart_ids(&dock.dump());
        ids.sort_unstable();
        assert_eq!(ids, vec![3, 7]);
    });
}

fn recorded_count() -> usize {
    let runtime = wows_toolkit_services::runtime::build().expect("a tokio runtime starts");
    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
        wows_toolkit_config::queries::get_all_session_stats(&pool).await.expect("the session is read").len()
    })
}

/// Opens the palette, types `query` and chooses the root entry `value`, or reports it is not
/// listed. The query brings the entry into the list's visible rows.
fn choose_command(window: &mut HeadlessWindow, value: &str, query: &str) -> bool {
    window.press_character('k', Modifiers::PRIMARY);
    window.advance(Duration::from_millis(400));
    window.type_text(query);
    window.advance(Duration::from_millis(400));
    let listed = window.by_test_id(&format!("palette-item-{value}"));
    match listed {
        Some(item) => {
            window.click(item).expect("the entry is clicked");
            settle_in_real_time(window);
            true
        }
        None => {
            // The first Escape empties the query, the second closes the palette.
            for _ in 0..2 {
                window.press_key(NamedKey::Escape, Modifiers::NONE);
                window.advance(Duration::from_millis(400));
            }
            false
        }
    }
}

#[test]
fn debug_mode_offers_sample_data_that_replaces_the_session_until_restored() {
    let mut window = open_stats(HeadlessOptions::new(crate::interaction_tests::REVIEW_SIZE));
    let recorded = recorded_count();

    assert!(!choose_command(&mut window, "sample-stats", "sample"), "the entry is hidden outside debug mode");

    window.press_character('d', Modifiers::PRIMARY | Modifiers::SHIFT);
    window.advance(Duration::from_millis(300));
    assert!(choose_command(&mut window, "sample-stats", "sample"), "debug mode lists the entry");

    assert!(window.by_test_id("stats-sample-notice").is_some(), "the bar says the games are made up");
    assert!(window.by_test_id("stats-sample-dock").is_some());
    assert!(window.by_test_id("stats-clear").is_none(), "made-up games cannot be cleared");
    assert!(window.by_test_id("stats-overview-summary").is_some(), "the overview sums the games");
    assert!(window.by_test_id("stats-chart").is_some(), "the chart plots them");
    let ships = t!("ui.stats.ships").into_owned();
    let tab = window.by_role_and_label(Role::Tab, &ships).expect("the Ships tab is in the dock");
    window.click(tab).expect("the Ships tab is clicked");
    window.advance(Duration::from_millis(300));
    let count = window.texts_by_class("wt-stats-ships__count").pop().unwrap_or_default();
    let listed: usize = count.split('/').next_back().and_then(|total| total.trim().parse().ok()).unwrap_or(0);
    assert!(listed >= 30, "ships of every class and tier are listed, not {count:?}");
    assert_eq!(recorded_count(), recorded, "nothing is written to the session");

    assert!(choose_command(&mut window, "restore-stats", "recorded"));
    assert!(window.by_test_id("stats-sample-notice").is_none(), "the recorded session is back");
    assert!(window.by_test_id("stats-dock").is_some());
    assert_eq!(recorded_count(), recorded);
}

/// The sample drawn from the ships of the install named by `WT_WOWS_DIR`.
#[test]
#[ignore = "needs a game install in WT_WOWS_DIR and draws on the graphics device"]
fn the_sample_stats_render_in_dark_and_light() {
    let wows_dir = std::env::var("WT_WOWS_DIR").expect("set WT_WOWS_DIR to a WoWs install directory");
    let runtime = wows_toolkit_services::runtime::build().expect("a tokio runtime starts");
    crate::interaction_tests::returning_reader(&runtime);
    runtime.block_on(async {
        let pool = wows_toolkit_config::open_db().await.expect("the settings database opens");
        wows_toolkit_config::queries::set_setting(&pool, keys::WOWS_DIR, &wows_dir)
            .await
            .expect("a setting is written");
    });

    for (appearance, name) in [(Appearance::Dark, "stats-sample-dark"), (Appearance::Light, "stats-sample-light")] {
        let mut window = open_stats(review_options(appearance));
        window.press_character('d', Modifiers::PRIMARY | Modifiers::SHIFT);
        window.advance(Duration::from_millis(300));
        // The sample takes its ships from the build once it loads, and made-up ones before.
        let mut from_build = false;
        for _ in 0..60 {
            assert!(choose_command(&mut window, "sample-stats", "sample"));
            let names = window.texts_by_class("wt-stats-achievement__name");
            if !names.is_empty() && !names.iter().any(|name| name.starts_with("Achievement ")) {
                from_build = true;
                break;
            }
            settle_in_real_time(&mut window);
        }
        assert!(from_build, "the build loads");
        println!("{}", save_shot(&mut window, name).display());
    }
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the tab"]
fn the_stats_tab_renders_in_dark_and_light() {
    for (appearance, name) in [(Appearance::Dark, "stats-dark"), (Appearance::Light, "stats-light")] {
        let mut window = open_stats(review_options(appearance));
        println!("{}", save_shot(&mut window, name).display());
    }
}

const YAMATO: u64 = 4_288_675_536;

/// When the run in the session tests opened.
fn opened() -> AppOpened {
    AppOpened::new(jiff::civil::date(2026, 11, 16).at(14, 32, 0, 0))
}

/// A Random battle in the Yamato that started at `game_time`.
fn yamato_at(game_time: &str, damage: u64, win: bool) -> PerGameStat {
    PerGameStat {
        ship_name: "Yamato".to_owned(),
        ship_id: YAMATO.into(),
        game_time: game_time.to_owned(),
        sort_key: wows_toolkit_viewmodel::stats::sortable_game_time(game_time),
        player_id: 1,
        damage,
        spotting_damage: 0,
        frags: 1,
        raw_xp: 1_000,
        base_xp: 700,
        is_win: win,
        is_loss: !win,
        is_draw: false,
        is_div: false,
        match_group: "pvp".to_owned(),
        achievements: Vec::new(),
        match_contribution: None,
    }
}

/// The Stats tab over the sample as the saved session, in a window that saves nothing, with this
/// run's replays read: one battle that started before the app opened and two after.
fn open_session_stats(options: HeadlessOptions) -> (HeadlessWindow, Services) {
    let mut window = open_stats(options);
    let services = shell().services();
    services.settings.use_private_database_for_test();
    services.session.choose_scope(SessionScope::Manual);
    services.session.restart_for_test(opened(), sample_games());
    for game in [
        yamato_at("16.11.2026 14:00:00", 120_000, true),
        yamato_at("16.11.2026 15:00:00", 90_000, true),
        yamato_at("16.11.2026 15:30:00", 50_000, false),
    ] {
        services.session.listed(game);
    }
    settle_in_real_time(&mut window);

    (window, services)
}

fn text_of(window: &HeadlessWindow, class: &str) -> String {
    window.texts_by_class(class).concat()
}

fn label_of(window: &mut HeadlessWindow, test_id: &str) -> String {
    let element = window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is on screen"));
    window.a11y(element).and_then(|state| state.label).unwrap_or_default()
}

/// The overview's record, the Ships panel's count and the chart's ships, in that order.
fn every_view(window: &mut HeadlessWindow) -> (String, String, String) {
    let overview = t!("ui.stats.overview").into_owned();
    let tab = window.by_role_and_label(Role::Tab, &overview).expect("the Overview tab is in the dock");
    window.click(tab).expect("the Overview tab is clicked");
    window.advance(Duration::from_millis(300));
    let record = text_of(window, "wt-stats-summary__record");

    let ships = t!("ui.stats.ships").into_owned();
    let tab = window.by_role_and_label(Role::Tab, &ships).expect("the Ships tab is in the dock");
    window.click(tab).expect("the Ships tab is clicked");
    window.advance(Duration::from_millis(300));
    let count = text_of(window, "wt-stats-ships__count");

    (record, count, label_of(window, "chart-ship-filter"))
}

#[test]
fn the_session_scope_narrows_every_stats_view_to_the_session_and_back() {
    let (mut window, services) = open_session_stats(HeadlessOptions::new(crate::interaction_tests::REVIEW_SIZE));
    let saved_view = every_view(&mut window);
    assert_eq!(saved_view.1, "3/3", "the saved session's three ships are listed");
    assert!(saved_view.2.ends_with(&t!("ui.stats.ship_selection_count", selected = 3, total = 3).into_owned()));

    click(&mut window, "stats-session-scope-since-opened");
    settle_in_real_time(&mut window);

    assert_eq!(services.session.scope_untracked(), SessionScope::SinceAppOpened);
    assert!(
        saved_for_test().contains(&(setting_keys::SESSION_SCOPE, "\"SinceAppOpened\"".to_owned())),
        "the scope is saved"
    );
    assert_eq!(
        text_of(&window, "wt-stats__session-count"),
        t!("ui.stats.session_chip_since", time = "14:32", battles = t!("ui.stats.session_battles_many", count = 2))
    );
    let (record, ships, chart) = every_view(&mut window);
    assert_eq!(record, "1W/1L (50.0%)", "the overview counts the two battles since the app opened");
    assert_eq!(ships, "1/1", "the Ships panel lists only their ship");
    assert!(chart.ends_with(&t!("ui.stats.ship_selection_count", selected = 1, total = 1).into_owned()), "{chart}");
    let clear = window.by_test_id("stats-clear").expect("the clear button is on screen");
    assert_eq!(
        window.a11y(clear).map(|state| state.disabled),
        Some(true),
        "a session since the app opened is not cleared"
    );

    click(&mut window, "stats-session-scope-manual");
    settle_in_real_time(&mut window);
    assert_eq!(every_view(&mut window), saved_view, "the saved session comes back unchanged");
}

#[test]
fn a_session_with_no_battles_says_what_fills_it() {
    let (mut window, services) = open_session_stats(HeadlessOptions::new(crate::interaction_tests::REVIEW_SIZE));
    services.session.restart_for_test(opened(), Vec::new());
    settle_in_real_time(&mut window);

    assert!(window.by_test_id("stats-session-empty").is_some(), "the empty saved session says so");
    assert!(window.by_test_id("stats-dock").is_none());

    click(&mut window, "stats-session-scope-since-opened");
    settle_in_real_time(&mut window);
    let said = window.texts_by_class("wt-stats__empty-text").concat();
    assert_eq!(said, t!("ui.stats.session_empty_since", time = "14:32"));

    services.session.listed(yamato_at("16.11.2026 15:00:00", 90_000, true));
    settle_in_real_time(&mut window);
    assert!(window.by_test_id("stats-session-empty").is_none(), "a battle fills the session at once");
    assert!(window.by_test_id("stats-dock").is_some());
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the session control"]
fn the_stats_session_control_renders_in_dark_and_light() {
    for (appearance, name) in [(Appearance::Dark, "stats-session-dark"), (Appearance::Light, "stats-session-light")] {
        let (mut window, _services) = open_session_stats(review_options(appearance));
        click(&mut window, "stats-session-scope-since-opened");
        settle_in_real_time(&mut window);
        println!("{}", save_shot(&mut window, name).display());
    }
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the tab"]
fn the_chart_save_error_renders() {
    let mut window = open_stats(review_options(Appearance::Dark));
    fail_a_chart_save(&mut window);
    println!("{}", save_shot(&mut window, "stats-save-error").display());
    refuse_chart_rows_naming(ChartableStat::SpottingDamage, false);
}

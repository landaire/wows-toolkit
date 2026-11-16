use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::DockRegion;
use landaire_ui::DockState;
use landaire_ui::DockTarget;
use landaire_ui::PanelFactory;
use landaire_ui::PanelKind;
use landaire_ui::PanelRegistry;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use rust_i18n::t;
use wows_toolkit_viewmodel::settings::keys;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::chart::ChartMode;
use wows_toolkit_viewmodel::stats::chart::ChartSettings;
use wows_toolkit_viewmodel::stats::chart::ChartableStat;
use zgui::view::AnyView;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;
use zgui::vocab::Toggled;

use super::CHART_KIND;
use super::chart_entry;
use super::chart_title;
use super::layout_chart_ids;
use crate::interaction_tests::open_shell;
use crate::interaction_tests::review_options;
use crate::interaction_tests::save_shot;
use crate::interaction_tests::settle_in_real_time;

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
            }
        })
        .collect()
}

/// Writes the sample session into the shared test database, once per process.
fn seed_session() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let runtime = wows_toolkit_services::runtime::build().expect("a tokio runtime starts");
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

/// The shell on the Stats tab over the sample session.
fn open_stats(options: HeadlessOptions) -> HeadlessWindow {
    let mut window = open_shell(options);
    seed_session();
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

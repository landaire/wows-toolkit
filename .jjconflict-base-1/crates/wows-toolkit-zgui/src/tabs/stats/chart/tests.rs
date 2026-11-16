use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::Scheme;
use landaire_ui::ThemeChoice;
use landaire_ui::ThemeRootProps;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::Element as TestElement;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_toolkit_viewmodel::stats::GameLimit;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::prelude::GetUntracked;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

use super::*;
use crate::theme::SemanticTokensProps;

/// A session of four ships over twenty games, oldest first, with the spread a real one has.
fn session() -> Arc<[PerGameStat]> {
    let ships = [
        ("Yamato", 3_763_255_248u64),
        ("Shimakaze", 3_762_174_960),
        ("Des Moines", 3_551_442_896),
        ("Petropavlovsk", 3_655_251_920),
    ];
    let damage = [
        84_210, 41_300, 127_850, 62_400, 98_120, 35_600, 152_300, 71_900, 56_000, 113_400, 48_250, 89_700, 134_100,
        27_800, 76_350, 101_900, 64_200, 118_600, 52_700, 93_400,
    ];
    damage
        .iter()
        .enumerate()
        .map(|(index, damage)| {
            let (ship, id) = ships[index % ships.len()];
            let time = format!("2026-11-{:02} 20:{:02}", 1 + index / 4, index * 3);
            PerGameStat {
                ship_name: ship.to_string(),
                ship_id: id.into(),
                game_time: time.clone(),
                sort_key: time,
                player_id: 1,
                damage: *damage,
                spotting_damage: damage / 3,
                frags: (index % 4) as i64,
                raw_xp: (*damage / 40) as i64,
                base_xp: (*damage / 90) as i64,
                is_win: index % 3 != 0,
                is_loss: index % 3 == 0,
                is_draw: false,
                is_div: index % 5 == 0,
                match_group: if index % 6 == 0 { "ranked" } else { "pvp" }.to_string(),
                achievements: Vec::new(),
            }
        })
        .collect()
}

/// The chart in a fixed-size box, themed as the shell themes it.
#[component]
fn Harness(settings: RwSignal<ChartSettings>, scheme: Scheme) -> impl IntoView {
    Services::provide(None);
    let choice = Signal::stored_local(ThemeChoice::Fixed(scheme));
    let games = Signal::stored(session());
    let filters = Signal::stored(StatsFilters { limit: GameLimit::All, ..StatsFilters::default() });
    let personal_rating = Signal::stored(None::<Arc<PersonalRatingData>>);

    view! {
        ThemeRoot(choice = choice) {
            SemanticTokens(choice = Signal::stored_local(match scheme {
                Scheme::Light => wows_toolkit_viewmodel::settings::ThemeChoice::Light,
                _ => wows_toolkit_viewmodel::settings::ThemeChoice::Dark,
            }))
            box(style:width = "900px", style:height = "480px", style:display = "flex", style:background-color = "var(--lui-color-panel)") {
                StatsChart(settings = settings, games = games, filters = filters, personal_rating = personal_rating)
            }
        }
    }
}

fn open(
    settings: ChartSettings,
    scheme: Scheme,
    drawing: Drawing,
) -> Option<(HeadlessWindow, RwSignal<ChartSettings>)> {
    let held = Rc::new(Cell::new(None));
    let mounted = Rc::clone(&held);
    let options = HeadlessOptions { drawing, ..HeadlessOptions::new(WindowSize { width: 900.0, height: 480.0 }) };
    let opened = HeadlessWindow::open(options, move || {
        let signal = RwSignal::new(settings.clone());
        mounted.set(Some(signal));
        view! { Harness(settings = signal, scheme = scheme) }
    });
    let mut window = match opened {
        Ok(window) => window,
        Err(
            landaire_ui::HeadlessError::NoWindow
            | landaire_ui::HeadlessError::App { source: zgui::runtime::AppError::GpuUnavailable(_) },
        ) => {
            eprintln!("skipped: no graphics adapter to draw on");
            return None;
        }
        Err(err) => panic!("the chart opens: {err}"),
    };
    window.advance(Duration::from_millis(50));
    let signal = held.get().expect("the harness was built");

    Some((window, signal))
}

fn element(window: &HeadlessWindow, id: &str) -> TestElement {
    window.by_test_id(id).unwrap_or_else(|| panic!("`{id}` is drawn"))
}

/// Whether `id` takes space, which an element styled away does not.
fn shown(window: &HeadlessWindow, id: &str) -> bool {
    let element = element(window, id);
    window.bounds(element).is_some_and(|bounds| bounds.size.width.0 > 0.0 && bounds.size.height.0 > 0.0)
}

fn press(window: &mut HeadlessWindow, id: &str) {
    let element = element(window, id);
    window.click(element).unwrap_or_else(|err| panic!("`{id}` is pressed: {err}"));
    window.advance(Duration::from_millis(50));
}

#[test]
fn picking_a_statistic_and_mode_writes_the_whole_settings_row() {
    let saved = ChartSettings { id: Some(4), ..ChartSettings::default() };
    let (mut window, settings) = open(saved, Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");

    press(&mut window, "chart-settings");
    press(&mut window, "chart-stat-stat.frags");
    press(&mut window, "chart-mode-Bar");
    press(&mut window, "chart-values");

    let written = settings.get_untracked();
    assert_eq!(written.id, Some(4), "the chart's id is kept");
    assert_eq!(written.stat, ChartableStat::Frags);
    assert_eq!(written.mode, ChartMode::Bar);
    assert!(written.show_values);
}

#[test]
fn taking_the_chart_off_the_tab_starts_from_the_tab_filters() {
    let (mut window, settings) =
        open(ChartSettings::default(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");

    press(&mut window, "chart-settings");
    press(&mut window, "chart-own-filters");
    assert_eq!(settings.get_untracked().own_filters, Some(StatsFilters::default()));

    press(&mut window, "chart-division-Division");
    assert_eq!(settings.get_untracked().own_filters.map(|filters| filters.division), Some(DivisionFilter::DivOnly));
}

#[test]
fn unticking_a_ship_remembers_the_pick() {
    let (mut window, settings) =
        open(ChartSettings::default(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");

    press(&mut window, "chart-ship-filter");
    press(&mut window, "chart-ship-3763255248");

    let written = settings.get_untracked();
    assert!(written.selection_touched);
    assert_eq!(written.selected_ships.len(), 3);
    assert!(!written.selected_ships.contains(&3_763_255_248));

    press(&mut window, "chart-ships-none");
    assert!(settings.get_untracked().selected_ships.is_empty());
    assert!(shown(&window, "chart-empty"), "no ship, nothing to plot");
}

/// The ship search marks the letters of each kept name it matched. Escape empties it before it
/// closes the menu.
#[test]
fn the_ship_search_marks_the_matched_letters_and_escape_empties_it() {
    let (mut window, _settings) =
        open(ChartSettings::default(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");

    press(&mut window, "chart-ship-filter");
    press(&mut window, "chart-ship-search");
    window.type_text("dm");
    window.advance(Duration::from_millis(50));

    // The popover draws its content twice, so the pieces come in equal pairs.
    let marked = window.texts_by_class(landaire_ui::MARK_CLASS);
    assert!(!marked.is_empty() && marked.chunks(2).all(|pair| pair == ["D", "M"]), "{marked:?}");
    assert!(window.by_test_id("chart-ship-3763255248").is_none(), "Yamato does not match");

    window.press_key(NamedKey::Escape, Modifiers::NONE);
    window.advance(Duration::from_millis(50));
    assert!(shown(&window, "chart-ship-filter-menu"), "Escape in the typed search leaves the menu open");
    assert!(window.by_test_id("chart-ship-3763255248").is_some(), "and empties the search");

    window.press_key(NamedKey::Escape, Modifiers::NONE);
    window.advance(Duration::from_millis(300));
    assert!(window.by_test_id("chart-ship-filter-menu").is_none(), "Escape in the empty search closes the menu");
}

#[test]
fn a_restored_row_is_followed() {
    let (mut window, settings) =
        open(ChartSettings::default(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");
    assert!(!shown(&window, "chart-empty"));

    settings.set(ChartSettings { selected_ships: Vec::new(), selection_touched: true, ..ChartSettings::default() });
    window.advance(Duration::from_millis(50));

    assert!(shown(&window, "chart-empty"), "the restored empty pick is drawn as nothing to plot");
}

#[test]
fn dragging_a_line_pans_and_a_double_click_resets_it() {
    let (mut window, _settings) =
        open(ChartSettings::default(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");
    assert!(!shown(&window, "chart-reset-view"), "the whole plot needs no reset");

    let plot = element(&window, "chart-plot");
    window.drag(plot, Point::new(DevicePx(200.0), DevicePx(200.0)), 4).expect("the plot is dragged");
    window.advance(Duration::from_millis(50));
    assert!(shown(&window, "chart-reset-view"), "a panned view offers its reset");

    window.double_click(plot).expect("the plot is double-clicked");
    window.advance(Duration::from_millis(50));
    assert!(!shown(&window, "chart-reset-view"), "a double click shows the whole plot again");
}

#[test]
fn a_bar_chart_does_not_pan() {
    let settings = ChartSettings { mode: ChartMode::Bar, ..ChartSettings::default() };
    let (mut window, _settings) = open(settings, Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");

    let plot = element(&window, "chart-plot");
    window.drag(plot, Point::new(DevicePx(200.0), DevicePx(200.0)), 4).expect("the plot is dragged");
    window.advance(Duration::from_millis(50));

    assert!(!shown(&window, "chart-reset-view"));
}

fn shot_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots").join(format!("{name}.png"))
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the chart"]
fn line_and_bar_charts_render_in_dark_and_light() {
    let line = ChartSettings::default();
    let bar = ChartSettings { mode: ChartMode::Bar, show_values: true, ..ChartSettings::default() };
    for (appearance, scheme) in [(Appearance::Dark, Scheme::Dark), (Appearance::Light, Scheme::Light)] {
        for (kind, settings) in [("line", line.clone()), ("bar", bar.clone())] {
            let Some((mut window, _)) = open(settings, scheme, Drawing::Device) else { return };
            window.advance(Duration::from_millis(200));
            let name = format!("stats-chart-{kind}-{}", if appearance == Appearance::Dark { "dark" } else { "light" });
            let path = shot_path(&name);
            window.save_png(&path).expect("the chart is drawn to a PNG");
            println!("{}", path.display());
        }
    }
}

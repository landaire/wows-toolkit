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
use wows_toolkit_viewmodel::stats::BattleDetails;
use wows_toolkit_viewmodel::stats::GameLimit;
use wows_toolkit_viewmodel::stats::plot::BandColors;
use wows_toolkit_viewmodel::stats::plot::PlotCanvas;
use wows_toolkit_viewmodel::stats::plot::PlotColor;
use wows_toolkit_viewmodel::stats::plot::PlotColors;
use wows_toolkit_viewmodel::stats::plot::draw;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::platform::CursorStyle;
use zgui::reactive::prelude::GetUntracked;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

use super::*;
use crate::theme::SemanticTokensProps;

/// A canvas that draws nothing and measures every label as empty.
struct Unmeasured;

impl PlotCanvas for Unmeasured {
    fn fill(&mut self, _bounds: PlotRect, _color: PlotColor, _radius: f32) {}

    fn polyline(&mut self, _points: &[PlotPoint], _width: f32, _color: PlotColor) {}

    fn measure(&mut self, _text: &str, _size: f32) -> f32 {
        0.0
    }

    fn text(&mut self, _origin: PlotPoint, _text: &str, _size: f32, _color: PlotColor) {}

    fn clipped(&mut self, _area: PlotRect, body: &mut dyn FnMut(&mut dyn PlotCanvas)) {
        body(self);
    }
}

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
                match_contribution: None,
                details: BattleDetails {
                    fires: Some((index % 3) as u64),
                    survived: Some(index % 2 == 0),
                    ..BattleDetails::default()
                },
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
        Err(err) if err.is_gpu_unavailable() => {
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

/// What the window's plot draws for `settings` through `view`, where the shared layout puts it
/// in the plot's box, and the box's top-left corner in device pixels.
///
/// Laid out without measuring labels, so a line chart's legend is at its narrowest and the area
/// is wider than the window's. The left edge, the heights and the bars do not depend on the
/// legend, so a line's first game and every bar sit where the window drew them.
fn drawn(
    window: &HeadlessWindow,
    settings: &ChartSettings,
    view: PlotView,
) -> (PlotData, PlotLayout, Point<DevicePx, Device>) {
    let filters = StatsFilters { limit: GameLimit::All, ..StatsFilters::default() };
    let model = ChartModel::new(settings, &session(), &filters, None);
    let data = PlotData { series: model.series, bars: model.bars, view, ..PlotData::default() };

    let plot = window.bounds(element(window, "chart-plot")).expect("the plot has a box");
    let bounds = PlotRect::new(0.0, 0.0, plot.size.width.0, plot.size.height.0);
    let color = PlotColor::new(0.0, 0.0, 0.0, 1.0);
    let colors = PlotColors { axis: color, grid: color, text: color, bands: BandColors::from_fn(|_| color) };
    let layout = draw(&data.plot(), bounds, colors, &mut Unmeasured).expect("the plot has room");

    (data, layout, plot.origin)
}

/// `at`, in the plot's logical pixels, in window device pixels. The test windows draw at scale
/// one.
fn on_window(origin: Point<DevicePx, Device>, at: PlotPoint) -> Point<DevicePx, Device> {
    Point::new(DevicePx(origin.x.0 + at.x), DevicePx(origin.y.0 + at.y))
}

/// The hover tooltip's name, value and game lines, in that order. Empty when it is not shown.
fn tooltip_lines(window: &HeadlessWindow) -> Vec<String> {
    ["wt-chart__tip-name", "wt-chart__tip-value", "wt-chart__tip-game"]
        .into_iter()
        .flat_map(|class| window.texts_by_class(class))
        .collect()
}

/// Where Des Moines' first game is drawn, on the value axis like every line's first game.
fn first_game(data: &PlotData, layout: &PlotLayout) -> PlotPoint {
    let series = data.series.iter().position(|series| series.name == "Des Moines").expect("Des Moines is plotted");

    layout
        .line_points(&data.plot())
        .nth(series)
        .and_then(|mut points| points.next())
        .expect("Des Moines has a first game")
}

/// Hovers a little right of `point`, inside the plot.
fn hover_near(window: &mut HeadlessWindow, origin: Point<DevicePx, Device>, point: PlotPoint) {
    window.hover_at(on_window(origin, PlotPoint::new(point.x + 3.0, point.y + 2.0)));
    window.advance(Duration::from_millis(50));
}

/// Hovers Des Moines' first game on the line chart.
fn hover_first_game(window: &mut HeadlessWindow, settings: &ChartSettings) {
    let (data, layout, origin) = drawn(window, settings, PlotView::default());
    hover_near(window, origin, first_game(&data, &layout));
}

/// Hovers the Yamato bar near its base.
fn hover_yamato_bar(window: &mut HeadlessWindow, settings: &ChartSettings) {
    let (data, layout, origin) = drawn(window, settings, PlotView::default());
    let index = data.bars.iter().position(|bar| bar.label == "Yamato").expect("Yamato has a bar");
    let rect = layout.bar_rects(&data.plot()).nth(index).expect("the bar is laid out");

    window.hover_at(on_window(origin, PlotPoint::new(rect.x + rect.width * 0.5, rect.bottom() - 4.0)));
    window.advance(Duration::from_millis(50));
}

/// The statistic search sits in the settings popover, and its list is drawn on a surface of its
/// own. A press on an option is a press outside the popover, so this proves the popover lets the
/// choice land and stays open. Frags is typed first: its option sits below the part of the list
/// that shows without scrolling.
#[test]
fn picking_a_statistic_and_mode_writes_the_whole_settings_row() {
    let saved = ChartSettings { id: Some(4), ..ChartSettings::default() };
    let (mut window, settings) = open(saved, Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");

    press(&mut window, "chart-settings");
    press(&mut window, "chart-stat");
    assert!(window.by_test_id("chart-stat-list").is_some(), "a press on the button opens the list");
    window.type_text("frag");
    window.advance(Duration::from_millis(50));
    press(&mut window, "chart-stat-Frags");
    assert_eq!(settings.get_untracked().stat, ChartableStat::Frags, "the choice lands");
    // Longer than the popover's exit, so a dismissed menu would be gone.
    window.advance(Duration::from_millis(300));
    assert!(window.by_test_id("chart-settings-menu").is_some(), "and the settings menu stays open");
    assert!(window.by_test_id("chart-stat-list").is_none(), "the list closes on the choice");
    press(&mut window, "chart-mode-Bar");
    press(&mut window, "chart-values");

    let written = settings.get_untracked();
    assert_eq!(written.id, Some(4), "the chart's id is kept");
    assert_eq!(written.stat, ChartableStat::Frags);
    assert_eq!(written.mode, ChartMode::Bar);
    assert!(written.show_values);
}

/// Typing narrows the statistics to the ones whose name holds what was typed, and picking one
/// plots it.
#[test]
fn typing_in_the_statistic_field_narrows_the_list_to_what_matches() {
    let (mut window, settings) =
        open(ChartSettings::default(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");

    press(&mut window, "chart-settings");
    press(&mut window, "chart-stat");
    assert!(window.by_test_id("chart-stat-Damage").is_some(), "every statistic is offered before typing");
    window.type_text("fire");
    window.advance(Duration::from_millis(50));

    assert!(window.by_test_id("chart-stat-Fires").is_some(), "Fires matches");
    assert!(window.by_test_id("chart-stat-Damage").is_none(), "Damage does not");
    assert!(window.by_test_id("chart-stat-WinRate").is_none(), "nor does a statistic a plain line cannot plot");

    press(&mut window, "chart-stat-Fires");
    assert_eq!(settings.get_untracked().stat, ChartableStat::Fires);
    assert_eq!(window.texts_by_class("wt-chart__title"), [ChartableStat::Fires.label()]);
    assert!(!shown(&window, "chart-empty"), "the games that recorded fires are plotted");
}

/// The chevron end of the statistic button is part of the button: over it the pointer is not a
/// text caret, a press there opens the search, and the caret lands in the search field.
#[test]
fn the_statistic_button_opens_from_its_chevron_and_takes_typing_at_once() {
    let (mut window, settings) =
        open(ChartSettings::default(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");

    press(&mut window, "chart-settings");
    let button = window.bounds(element(&window, "chart-stat")).expect("the statistic button has a box");
    let chevron = Point::new(
        DevicePx(button.origin.x.0 + button.size.width.0 - 8.0),
        DevicePx(button.origin.y.0 + button.size.height.0 * 0.5),
    );
    window.hover_at(chevron);
    window.advance(Duration::from_millis(50));
    assert_ne!(window.cursor(), Some(CursorStyle::Text), "the chevron is no text field");

    window.click_at(chevron);
    window.advance(Duration::from_millis(50));
    assert!(window.by_test_id("chart-stat-list").is_some(), "a press on the chevron opens the search");
    window.type_text("flood");
    window.advance(Duration::from_millis(50));
    assert!(window.by_test_id("chart-stat-Damage").is_none(), "what was typed narrows the list");
    press(&mut window, "chart-stat-Floods");
    assert_eq!(settings.get_untracked().stat, ChartableStat::Floods);
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

/// Dragging up on the value axis's gutter scales the values out about the press and leaves the
/// games alone, so Des Moines' first game stays on the value axis at the height the zoom moved it
/// to.
#[test]
fn dragging_the_value_axis_scales_the_values_alone() {
    let settings = ChartSettings::default();
    let (mut window, _) = open(settings.clone(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");
    let (data, layout, origin) = drawn(&window, &settings, PlotView::default());
    let first = first_game(&data, &layout);
    let centre = layout.area.y + layout.area.height * 0.5;
    let press = PlotPoint::new(layout.area.x - 20.0, first.y + 40.0f32.copysign(centre - first.y));

    window.hover_at(on_window(origin, press));
    window.advance(Duration::from_millis(50));
    assert_eq!(window.cursor(), Some(CursorStyle::ResizeNorthSouth), "the value axis offers to resize");

    let to = PlotPoint::new(press.x, press.y - 25.0);
    window.drag_from(on_window(origin, press), on_window(origin, to), 1);
    window.advance(Duration::from_millis(50));
    assert!(shown(&window, "chart-reset-view"), "a scaled axis offers its reset");

    let mut expected = PlotView::default();
    expected.drag_axis(PlotAxis::Y, to.y - press.y, press, &layout);
    let (data, zoomed, _) = drawn(&window, &settings, expected);
    let moved = first_game(&data, &zoomed);
    assert_eq!(moved.x, first.x, "the games are where they were");
    assert!((moved.y - first.y).abs() > 12.0, "the first game moved further than a pick reaches");

    let des_moines = ["Des Moines", "127,850", "Game 1, 2026-11-01 20:06"];
    hover_near(&mut window, origin, first);
    assert_ne!(tooltip_lines(&window), des_moines, "the first game left where it was");
    hover_near(&mut window, origin, moved);
    assert_eq!(tooltip_lines(&window), des_moines, "the first game is where the scaled values put it");

    let plot = element(&window, "chart-plot");
    window.double_click(plot).expect("the plot is double-clicked");
    window.advance(Duration::from_millis(50));
    assert!(!shown(&window, "chart-reset-view"), "a double click shows the whole plot again");
}

#[test]
fn dragging_the_game_axis_offers_its_reset() {
    let settings = ChartSettings::default();
    let (mut window, _) = open(settings.clone(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");
    let (_, layout, origin) = drawn(&window, &settings, PlotView::default());
    let press = PlotPoint::new(layout.area.x + 40.0, layout.area.bottom() + 10.0);

    window.hover_at(on_window(origin, press));
    window.advance(Duration::from_millis(50));
    assert_eq!(window.cursor(), Some(CursorStyle::ResizeEastWest), "the game axis offers to resize");

    window.drag_from(on_window(origin, press), on_window(origin, PlotPoint::new(press.x + 30.0, press.y)), 3);
    window.advance(Duration::from_millis(50));
    assert!(shown(&window, "chart-reset-view"), "a scaled axis offers its reset");
}

#[test]
fn a_bar_chart_does_not_scale_an_axis() {
    let settings = ChartSettings { mode: ChartMode::Bar, ..ChartSettings::default() };
    let (mut window, _) = open(settings.clone(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");
    let (_, layout, origin) = drawn(&window, &settings, PlotView::default());
    let press = PlotPoint::new(layout.area.x - 20.0, layout.area.y + layout.area.height * 0.5);

    window.hover_at(on_window(origin, press));
    window.advance(Duration::from_millis(50));
    assert_ne!(window.cursor(), Some(CursorStyle::ResizeNorthSouth), "a bar's axis offers no resize");

    window.drag_from(on_window(origin, press), on_window(origin, PlotPoint::new(press.x, press.y + 40.0)), 3);
    window.advance(Duration::from_millis(50));
    assert!(!shown(&window, "chart-reset-view"));
}

#[test]
fn hovering_a_line_point_shows_its_ship_value_and_game() {
    let settings = ChartSettings::default();
    let (mut window, _) = open(settings.clone(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");
    assert!(window.by_test_id("chart-hover-tooltip").is_none(), "no tooltip before the pointer comes");

    hover_first_game(&mut window, &settings);
    assert_eq!(tooltip_lines(&window), ["Des Moines", "127,850", "Game 1, 2026-11-01 20:06"]);

    window.hover_at(Point::new(DevicePx(1.0), DevicePx(1.0)));
    window.advance(Duration::from_millis(50));
    assert!(window.by_test_id("chart-hover-tooltip").is_none(), "leaving the plot takes the tooltip down");
}

#[test]
fn hovering_a_bar_shows_its_ship_and_average() {
    let settings = ChartSettings { mode: ChartMode::Bar, ..ChartSettings::default() };
    let (mut window, _) = open(settings.clone(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");

    hover_yamato_bar(&mut window, &settings);
    assert_eq!(tooltip_lines(&window), ["Yamato", "87,326"]);
}

/// A ship's average count keeps its fraction.
#[test]
fn hovering_a_bar_of_a_detail_shows_its_average() {
    let settings = ChartSettings { stat: ChartableStat::Fires, mode: ChartMode::Bar, ..ChartSettings::default() };
    let (mut window, _) = open(settings.clone(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");

    hover_yamato_bar(&mut window, &settings);
    assert_eq!(tooltip_lines(&window), ["Yamato", "0.80"]);
}

/// A win rate reads in the color of the account band it falls in, as the overview draws one.
#[test]
fn a_hovered_win_rate_is_colored_by_its_band() {
    let settings = ChartSettings { stat: ChartableStat::WinRate, mode: ChartMode::Bar, ..ChartSettings::default() };
    let (mut window, _) = open(settings.clone(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");

    hover_yamato_bar(&mut window, &settings);
    assert_eq!(tooltip_lines(&window), ["Yamato", "60.0%"]);
    let band = rating_band_name(wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory::Unicum);
    assert!(!window.all_by_attribute("data-band", band).is_empty(), "the value carries its band");

    let damage = ChartSettings { mode: ChartMode::Bar, ..ChartSettings::default() };
    let (mut window, _) = open(damage.clone(), Scheme::Dark, Drawing::Recorded).expect("a recorded window opens");
    hover_yamato_bar(&mut window, &damage);
    assert!(window.all_by_attribute("data-band", band).is_empty(), "another statistic carries none");
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

#[test]
#[ignore = "draws on the graphics device; run by hand to review the chart"]
fn hovered_line_and_bar_charts_render_in_dark_and_light() {
    let line = ChartSettings::default();
    let bar = ChartSettings { mode: ChartMode::Bar, ..ChartSettings::default() };
    for (appearance, scheme) in [(Appearance::Dark, Scheme::Dark), (Appearance::Light, Scheme::Light)] {
        for (kind, settings) in [("line", &line), ("bar", &bar)] {
            let Some((mut window, _)) = open(settings.clone(), scheme, Drawing::Device) else { return };
            window.advance(Duration::from_millis(200));
            match settings.mode {
                ChartMode::Line => hover_first_game(&mut window, settings),
                ChartMode::Bar => hover_yamato_bar(&mut window, settings),
            }
            window.advance(Duration::from_millis(200));
            let name =
                format!("stats-chart-{kind}-hover-{}", if appearance == Appearance::Dark { "dark" } else { "light" });
            let path = shot_path(&name);
            window.save_png(&path).expect("the chart is drawn to a PNG");
            println!("{}", path.display());
        }
    }
}

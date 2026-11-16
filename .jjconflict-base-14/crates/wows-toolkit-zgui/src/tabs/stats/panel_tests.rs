//! The Overview and Ships panels opened headlessly over sample games.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::HeadlessError;
use landaire_ui::ThemeRootProps;
use landaire_ui::ToastHostProps;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use wows_replays::types::GameParamId;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::test_support::FIXTURE_PR_SHIP_ID;
use wows_toolkit_replay_data::test_support::fixture_personal_rating_data;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingCategory;
use wows_toolkit_viewmodel::personal_rating::PersonalRatingData;
use wows_toolkit_viewmodel::settings::ThemeChoice;
use wows_toolkit_viewmodel::stats::PerGameStat;
use wows_toolkit_viewmodel::stats::PerformanceInfo;
use wows_toolkit_viewmodel::stats::SerializableAchievement;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::ParamData;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_params::types::Vehicle;
use wowsunpack::recognized::Recognized;
use zgui::component;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::reactive::RwSignal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

use crate::game_art;
use crate::interaction_tests::settle_in_real_time;
use crate::services::Services;
use crate::tabs::stats::overview::StatsOverviewProps;
use crate::tabs::stats::rating;
use crate::tabs::stats::ships::StatsShipsProps;
use crate::theme::SemanticTokensProps;
use crate::theme::rating_band_name;

const YAMATO: u64 = FIXTURE_PR_SHIP_ID;
const SHIMAKAZE: u64 = 4_181_604_048;
const DES_MOINES: u64 = 4_179_605_200;
const HALLAND: u64 = 4_076_746_448;

fn game(ship: &str, ship_id: u64, sort_key: &str, damage: u64, frags: i64, win: bool) -> PerGameStat {
    PerGameStat {
        ship_name: ship.to_owned(),
        ship_id: ship_id.into(),
        game_time: sort_key.to_owned(),
        sort_key: sort_key.to_owned(),
        player_id: 1,
        damage,
        spotting_damage: damage / 3,
        frags,
        raw_xp: 1_200 + damage as i64 / 100,
        base_xp: 700 + damage as i64 / 200,
        is_win: win,
        is_loss: !win,
        is_draw: false,
        is_div: false,
        match_group: "pvp".to_owned(),
        achievements: Vec::new(),
    }
}

fn achievement(id: u64, name: &str, icon_key: &str, count: usize) -> SerializableAchievement {
    SerializableAchievement {
        game_param_id: id.into(),
        display_name: name.to_owned(),
        description: String::new(),
        icon_key: icon_key.to_owned(),
        count,
    }
}

/// A session of four ships across an evening, oldest first.
fn evening() -> Arc<[PerGameStat]> {
    let mut games = vec![
        game("Yamato", YAMATO, "2026-02-13 18:02:11", 88_412, 1, true),
        game("Shimakaze", SHIMAKAZE, "2026-02-13 18:31:40", 61_203, 2, true),
        game("Des Moines", DES_MOINES, "2026-02-13 19:05:02", 47_950, 0, false),
        game("Yamato", YAMATO, "2026-02-13 19:40:57", 132_870, 3, true),
        game("Halland", HALLAND, "2026-02-13 20:12:33", 39_115, 1, false),
        game("Shimakaze", SHIMAKAZE, "2026-02-13 20:44:19", 23_480, 0, false),
        game("Yamato", YAMATO, "2026-02-13 21:20:08", 54_009, 0, false),
        game("Des Moines", DES_MOINES, "2026-02-13 21:51:45", 96_330, 2, true),
    ];
    games[3].achievements =
        vec![achievement(1, "Kraken Unleashed", "WARRIOR", 1), achievement(2, "High Caliber", "MAIN_CALIBER", 1)];
    games[7].achievements =
        vec![achievement(2, "High Caliber", "MAIN_CALIBER", 1), achievement(3, "Confederate", "SUPPORT", 1)];

    games.into()
}

/// What a test drives the panel through after it is open.
struct Handles {
    games: RwSignal<Arc<[PerGameStat]>>,
    rating: RwSignal<Option<Arc<PersonalRatingData>>>,
}

/// Which panel a window holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    Overview,
    Ships { clearable: bool },
}

/// The panels with the services and the theme the shell gives them, without the shell.
#[component]
fn PanelRoot(
    panel: Panel,
    appearance: Appearance,
    games: Arc<[PerGameStat]>,
    rating: Option<Arc<PersonalRatingData>>,
    game_data: Option<Arc<LoadedGameData>>,
    handles: Rc<RefCell<Option<Handles>>>,
    cleared: Rc<RefCell<Vec<GameParamId>>>,
) -> impl IntoView {
    let services = Services::provide(None);
    if let Some(data) = game_data {
        services.game_data.set_ready_for_test(data);
    }
    let dark = appearance == Appearance::Dark;
    install_stylesheet("wt-test-rating", &format!(":root {{ {} }}", rating::test_properties(dark)));
    let games = RwSignal::new(games);
    let rating = RwSignal::new(rating);
    *handles.borrow_mut() = Some(Handles { games, rating });
    let on_clear_ship = UnsyncCallback::new(move |ship: GameParamId| cleared.borrow_mut().push(ship));

    view! {
        ThemeRoot {
            SemanticTokens(choice = ThemeChoice::System)
            ToastHost {
                column(style:width = "100%", style:height = "100%") {
                    {match panel {
                        Panel::Overview => AnyView::new(view! { StatsOverview(games = games, personal_rating = rating) }),
                        Panel::Ships { clearable } => AnyView::new(view! {
                            StatsShips(games = games, personal_rating = rating, clearable = clearable, on_clear_ship = on_clear_ship)
                        }),
                    }}
                }
            }
        }
    }
}

struct Opened {
    window: HeadlessWindow,
    handles: Handles,
    cleared: Rc<RefCell<Vec<GameParamId>>>,
}

fn open(panel: Panel, drawing: Drawing, appearance: Appearance, rating: Option<Arc<PersonalRatingData>>) -> Opened {
    try_open(panel, drawing, appearance, rating, None).expect("the panel opens headlessly")
}

fn try_open(
    panel: Panel,
    drawing: Drawing,
    appearance: Appearance,
    rating: Option<Arc<PersonalRatingData>>,
    game_data: Option<Arc<LoadedGameData>>,
) -> Result<Opened, HeadlessError> {
    let handles = Rc::new(RefCell::new(None));
    let cleared = Rc::new(RefCell::new(Vec::new()));
    let options = HeadlessOptions {
        drawing,
        desktop: Some(appearance),
        ..HeadlessOptions::new(WindowSize { width: 900.0, height: 640.0 })
    };
    let window = {
        let handles = Rc::clone(&handles);
        let cleared = Rc::clone(&cleared);
        HeadlessWindow::open(options, move || {
            let handles = Rc::clone(&handles);
            let cleared = Rc::clone(&cleared);
            let rating = rating.clone();
            let game_data = game_data.clone();
            view! {
                PanelRoot(
                    panel = panel,
                    appearance = appearance,
                    games = evening(),
                    rating = rating,
                    game_data = game_data,
                    handles = handles,
                    cleared = cleared
                )
            }
        })?
    };
    let handles = handles.borrow_mut().take().expect("the panel publishes its handles");

    Ok(Opened { window, handles, cleared })
}

/// A build whose only ship is the Yamato, a battleship, and whose only file is the battleship
/// class icon, pointing up.
fn yamato_build() -> Arc<LoadedGameData> {
    let vehicle = Vehicle::builder()
        .level(10)
        .group("battleship".to_owned())
        .maybe_abilities(None)
        .upgrades(Vec::new())
        .maybe_config_data(None)
        .maybe_model_path(None)
        .maybe_armor(None)
        .maybe_hit_locations(None)
        .permoflages(Vec::new())
        .camera_trajectories(Vec::new())
        .maybe_ttx_components(None)
        .innate_skills(Vec::new())
        .build();
    let param = Param::builder()
        .id(GameParamId::from(YAMATO))
        .index("PJSB018".to_owned())
        .name("PJSB018_Yamato_1944".to_owned())
        .species(Recognized::Known(Species::Battleship))
        .nation("Japan".to_owned())
        .data(ParamData::Vehicle(vehicle))
        .build();
    let provider = GameMetadataProvider::from_params_no_specs(vec![param]).expect("a one-ship provider builds");

    Arc::new(LoadedGameData::for_test(provider, game_art::testing::upright_class_icon(Species::Battleship), 1))
}

fn open_ships(clearable: bool) -> Opened {
    open(Panel::Ships { clearable }, Drawing::Recorded, Appearance::Dark, None)
}

fn settle(window: &mut HeadlessWindow) {
    window.advance(Duration::from_millis(500));
}

fn expanded(window: &mut HeadlessWindow, ship: u64) -> Option<bool> {
    let toggle = window.by_test_id(&format!("ship-toggle-{ship}")).expect("the ship is listed");
    window.a11y(toggle).and_then(|state| state.expanded)
}

#[test]
fn clicking_a_toggle_opens_only_that_ship() {
    let Opened { mut window, .. } = open_ships(true);

    let toggle = window.by_test_id(&format!("ship-toggle-{SHIMAKAZE}")).expect("the ship is listed");
    window.click(toggle).expect("the toggle is drawn");
    settle(&mut window);

    assert_eq!(expanded(&mut window, SHIMAKAZE), Some(true), "the clicked ship opened");
    assert_eq!(expanded(&mut window, YAMATO), Some(false), "its neighbour stayed closed");
    assert!(window.by_test_id(&format!("ship-detail-{SHIMAKAZE}")).is_some(), "its table is shown");

    let toggle = window.by_test_id(&format!("ship-toggle-{SHIMAKAZE}")).expect("the ship is listed");
    window.click(toggle).expect("the toggle is drawn");
    settle(&mut window);

    assert_eq!(expanded(&mut window, SHIMAKAZE), Some(false), "clicking again closes it");
}

/// A press anywhere on a ship's header opens or closes it, as the chevron does; a double click
/// is two presses and leaves it as it was.
#[test]
fn the_whole_ship_header_opens_the_ship() {
    let Opened { mut window, .. } = open_ships(true);

    let header = window.by_test_id(&format!("ship-header-{SHIMAKAZE}")).expect("the ship is listed");
    window.click(header).expect("the header is drawn");
    settle(&mut window);
    assert_eq!(expanded(&mut window, SHIMAKAZE), Some(true), "a press on the header opens the ship");

    let header = window.by_test_id(&format!("ship-header-{SHIMAKAZE}")).expect("the ship is listed");
    window.double_click(header).expect("the header is drawn");
    settle(&mut window);
    assert_eq!(expanded(&mut window, SHIMAKAZE), Some(true), "a double click leaves it open");

    let header = window.by_test_id(&format!("ship-header-{SHIMAKAZE}")).expect("the ship is listed");
    window.click(header).expect("the header is drawn");
    settle(&mut window);
    assert_eq!(expanded(&mut window, SHIMAKAZE), Some(false), "another press closes it");
}

/// A press on any cell of a ship's line opens or closes it, as in GPUI; its buttons do not.
#[test]
fn a_number_cell_of_the_ship_line_opens_the_ship() {
    let Opened { mut window, .. } = open_ships(true);

    let header = window.by_test_id(&format!("ship-header-{SHIMAKAZE}")).expect("the ship is listed");
    let header = window.bounds(header).expect("the header has a box");
    let middle = header.origin.y.0 + header.size.height.0 / 2.0;
    let mut line: Vec<_> = window
        .all_by_class("lui-grid__cell")
        .into_iter()
        .filter_map(|cell| window.bounds(cell))
        .filter(|cell| cell.origin.y.0 <= middle && middle <= cell.origin.y.0 + cell.size.height.0)
        .collect();
    line.sort_by(|a, b| a.origin.x.0.total_cmp(&b.origin.x.0));
    let cell = line.get(1).expect("the line has a battles cell after the ship's");
    assert!(cell.origin.x.0 >= header.origin.x.0 + header.size.width.0, "{cell:?} is past the ship's cell");
    let battles = Point::new(
        DevicePx(cell.origin.x.0 + cell.size.width.0 / 2.0),
        DevicePx(cell.origin.y.0 + cell.size.height.0 / 2.0),
    );
    window.click_at(battles);
    settle(&mut window);
    assert_eq!(expanded(&mut window, SHIMAKAZE), Some(true), "a press on the battles cell opens the ship");

    window.click_at(battles);
    window.click_at(battles);
    settle(&mut window);
    assert_eq!(expanded(&mut window, SHIMAKAZE), Some(true), "a double click leaves it open");

    let copy = window.by_test_id(&format!("ship-copy-{SHIMAKAZE}")).expect("the ship has a copy menu");
    window.click(copy).expect("the copy button is drawn");
    settle(&mut window);
    window.press_key(NamedKey::Escape, Modifiers::NONE);
    settle(&mut window);
    assert_eq!(expanded(&mut window, SHIMAKAZE), Some(true), "the copy menu leaves it open");

    window.click_at(battles);
    settle(&mut window);
    assert_eq!(expanded(&mut window, SHIMAKAZE), Some(false), "another press closes it");
}

#[test]
fn a_ship_is_cleared_on_the_second_press_or_the_first_with_ctrl() {
    let Opened { mut window, cleared, .. } = open_ships(true);

    // A ship above the next one pressed, so its tooltip never covers that one's button.
    let clear = window.by_test_id(&format!("ship-clear-{SHIMAKAZE}")).expect("a recorded ship can be cleared");
    window.click_with(clear, Modifiers::PRIMARY).expect("the button is drawn");
    settle(&mut window);
    assert_eq!(*cleared.borrow(), [GameParamId::from(SHIMAKAZE)], "the modifier skips the confirmation");

    let clear = window.by_test_id(&format!("ship-clear-{HALLAND}")).expect("a recorded ship can be cleared");
    window.click(clear).expect("the button is drawn");
    settle(&mut window);
    assert_eq!(cleared.borrow().len(), 1, "one press only asks for confirmation");

    let clear = window.by_test_id(&format!("ship-clear-{HALLAND}")).expect("the button stays");
    window.click(clear).expect("the button is drawn");
    settle(&mut window);
    assert_eq!(*cleared.borrow(), [GameParamId::from(SHIMAKAZE), GameParamId::from(HALLAND)]);
}

#[test]
fn a_confirmation_does_not_outlive_a_change_of_games() {
    let Opened { mut window, handles, cleared } = open_ships(true);

    let clear = window.by_test_id(&format!("ship-clear-{HALLAND}")).expect("a recorded ship can be cleared");
    window.click(clear).expect("the button is drawn");
    settle(&mut window);
    handles.games.set(evening());
    settle(&mut window);

    let clear = window.by_test_id(&format!("ship-clear-{HALLAND}")).expect("the button stays");
    window.click(clear).expect("the button is drawn");
    settle(&mut window);
    assert!(cleared.borrow().is_empty(), "the new games ask again");
}

#[test]
fn a_directory_offers_no_clear() {
    let Opened { window, .. } = open_ships(false);

    assert!(window.by_test_id(&format!("ship-toggle-{HALLAND}")).is_some());
    assert!(window.by_test_id(&format!("ship-clear-{HALLAND}")).is_none());
}

#[test]
fn the_search_keeps_the_ships_whose_name_holds_it() {
    let Opened { mut window, .. } = open_ships(true);

    let search = window.by_test_id("stats-ships-search").expect("the search is shown");
    window.click(search).expect("the search is drawn");
    window.type_text("  MOINES");
    settle(&mut window);

    assert!(window.by_test_id(&format!("ship-toggle-{DES_MOINES}")).is_some());
    assert!(window.by_test_id(&format!("ship-toggle-{YAMATO}")).is_none());
    let marked = window.texts_by_class(landaire_ui::MARK_CLASS);
    assert!(!marked.is_empty() && marked.iter().all(|piece| piece == "Moines"), "{marked:?}");

    window.type_text("x");
    settle(&mut window);
    assert!(window.by_test_id("stats-ships-grid").is_none(), "no ship matches, so no grid");

    let clear = window.by_test_id("stats-ships-clear-search").expect("a search can be cleared");
    window.click(clear).expect("the button is drawn");
    settle(&mut window);
    assert!(window.by_test_id(&format!("ship-toggle-{YAMATO}")).is_some(), "every ship is back");

    let search = window.by_test_id("stats-ships-search").expect("the search is shown");
    window.click(search).expect("the search is drawn");
    window.type_text("moines");
    settle(&mut window);
    assert!(window.by_test_id(&format!("ship-toggle-{YAMATO}")).is_none());
    window.press_key(NamedKey::Escape, Modifiers::NONE);
    settle(&mut window);
    assert!(window.by_test_id(&format!("ship-toggle-{YAMATO}")).is_some(), "Escape empties the search");
}

#[test]
fn a_rating_table_arriving_late_still_rates_the_ships() {
    let Opened { mut window, handles, .. } = open_ships(true);

    let toggle = window.by_test_id(&format!("ship-toggle-{YAMATO}")).expect("the ship is listed");
    window.click(toggle).expect("the toggle is drawn");
    settle(&mut window);
    assert!(window.by_test_id(&format!("ship-row-{YAMATO}-PersonalRating")).is_none(), "nothing rates yet");

    handles.rating.set(Some(Arc::new(fixture_personal_rating_data())));
    settle(&mut window);
    assert!(
        window.by_test_id(&format!("ship-row-{YAMATO}-PersonalRating")).is_some(),
        "the late table reaches the rows"
    );
}

/// The game draws a class icon pointing up; a ship's row shows it pointing right.
#[test]
fn a_ships_class_icon_points_right() {
    let ships = Panel::Ships { clearable: false };
    let mut window = match try_open(ships, Drawing::Device, Appearance::Dark, None, Some(yamato_build())) {
        Ok(Opened { window, .. }) => window,
        Err(err) if game_art::testing::no_device(&err) => {
            eprintln!("skipped: no graphics adapter to draw the panel on");
            return;
        }
        Err(err) => panic!("the panel opens: {err}"),
    };
    settle_in_real_time(&mut window);

    game_art::testing::assert_points_right(&mut window, "wt-stats-ship__class");
}

#[test]
fn the_overview_rates_the_session_only_with_a_table() {
    let Opened { mut window, handles, .. } = open(Panel::Overview, Drawing::Recorded, Appearance::Dark, None);

    assert!(window.by_test_id("stats-overview-summary").is_some());
    assert!(window.by_test_id("stats-overview-rating").is_none(), "nothing rates without a table");
    assert!(window.by_test_id("stats-achievements").is_some(), "the evening earned achievements");

    handles.rating.set(Some(Arc::new(fixture_personal_rating_data())));
    settle(&mut window);
    assert!(window.by_test_id("stats-overview-rating").is_some());

    handles.games.set(Arc::from(Vec::new()));
    settle(&mut window);
    assert!(window.by_test_id("stats-overview-summary").is_none(), "no games, no summary");
}

/// The band a cell carries, read off the element `test_id` names.
fn band_of(window: &HeadlessWindow, test_id: &str) -> Option<&'static str> {
    let cell = window.by_test_id(test_id).expect("the cell is drawn");
    PersonalRatingCategory::ALL
        .into_iter()
        .map(rating_band_name)
        .find(|band| window.all_by_attribute("data-band", band).contains(&cell))
}

#[test]
fn the_overview_colours_each_win_rate_and_rating_by_its_band() {
    let Opened { mut window, handles, .. } = open(Panel::Overview, Drawing::Recorded, Appearance::Dark, None);

    // Two of three Yamato games won, one of two Shimakaze games, no Halland game.
    assert_eq!(band_of(&window, &format!("stats-overview-win-rate-{YAMATO}")), Some("super-unicum"));
    assert_eq!(band_of(&window, &format!("stats-overview-win-rate-{SHIMAKAZE}")), Some("average"));
    assert_eq!(band_of(&window, &format!("stats-overview-win-rate-{HALLAND}")), Some("bad"));
    assert_eq!(band_of(&window, &format!("stats-overview-pr-{YAMATO}")), None, "nothing rates without a table");

    let table = Arc::new(fixture_personal_rating_data());
    handles.rating.set(Some(Arc::clone(&table)));
    settle(&mut window);
    let yamato: Vec<PerGameStat> = evening().iter().filter(|game| game.ship_id.raw() == YAMATO).cloned().collect();
    let yamato: Vec<&PerGameStat> = yamato.iter().collect();
    let rated = PerformanceInfo::from_games(&yamato).personal_rating(&table).expect("the fixture rates the Yamato");
    assert_eq!(band_of(&window, &format!("stats-overview-pr-{YAMATO}")), Some(rating_band_name(rated.category)));
    assert_eq!(band_of(&window, &format!("stats-overview-pr-{HALLAND}")), None, "the fixture knows only the Yamato");
}

/// Writes `window` to `target/zgui-shots/<name>.png`.
fn save(window: &mut HeadlessWindow, name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/zgui-shots").join(format!("{name}.png"));
    window.save_png(&path).expect("the window is drawn to a PNG");

    path
}

#[test]
#[ignore = "draws on the graphics device; run by hand to review the panels"]
fn the_panels_render_in_dark_and_light() {
    for (appearance, suffix) in [(Appearance::Dark, "dark"), (Appearance::Light, "light")] {
        let rating = Some(Arc::new(fixture_personal_rating_data()));
        let Opened { mut window, .. } = open(Panel::Overview, Drawing::Device, appearance, rating.clone());
        settle(&mut window);
        println!("{}", save(&mut window, &format!("stats-overview-{suffix}")).display());
        drop(window);

        let Opened { mut window, .. } = open(Panel::Ships { clearable: true }, Drawing::Device, appearance, rating);
        let toggle = window.by_test_id(&format!("ship-toggle-{YAMATO}")).expect("the ship is listed");
        window.click(toggle).expect("the toggle is drawn");
        let clear = window.by_test_id(&format!("ship-clear-{HALLAND}")).expect("the ship can be cleared");
        window.click(clear).expect("the button is drawn");
        window.advance(Duration::from_secs(1));
        println!("{}", save(&mut window, &format!("stats-ships-{suffix}")).display());
    }
}

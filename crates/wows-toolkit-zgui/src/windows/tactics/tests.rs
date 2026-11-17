//! The board's window driven headlessly.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use landaire_ui::Appearance;
use landaire_ui::ThemeChoice;
use landaire_ui::ThemeRootProps;
use landaire_ui::ToastHostProps;
use landaire_ui::test_support::Drawing;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use tokio::runtime::Runtime;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::draw_command::ShipConfigCircleKind;
use wows_replays::types::GameParamId;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::load::LoadedGameData;
use wows_toolkit_replay_data::tactics::BoardTool;
use wows_toolkit_replay_data::tactics::ElementPos;
use wows_toolkit_replay_data::tactics::ElementSize;
use wows_toolkit_replay_data::tactics::MapChoice;
use wows_toolkit_replay_data::tactics::RANGE_CIRCLES;
use wows_toolkit_replay_data::tactics::RangeCircle;
use wows_toolkit_replay_data::tactics::ShipModifier;
use wows_toolkit_replay_data::tactics::board::BoardKey;
use wows_toolkit_replay_data::tactics::board::Modifiers as BoardModifiers;
use wows_toolkit_services::collab::CollabLink;
use wows_toolkit_viewmodel::text_filter::FilterQuery;
use wowsunpack::data::Version;
use wowsunpack::game_params::provider::GameMetadataProvider;
use wowsunpack::game_params::types::HullUpgradeConfig;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::Meters;
use wowsunpack::game_params::types::Param;
use wowsunpack::game_params::types::ParamData;
use wowsunpack::game_params::types::ShipConfigData;
use wowsunpack::game_params::types::Species;
use wowsunpack::game_params::types::Vehicle;
use wowsunpack::recognized::Recognized;
use wowsunpack::vfs::MemoryFS;
use wowsunpack::vfs::VfsPath;
use wt_collab_client::UserCursor;
use wt_collab_client::peer::LocalAnnotationEvent;
use wt_collab_client::peer::LocalEvent;
use wt_collab_client::types::Annotation;
use wt_collab_client::types::AnnotationShipConfig;
use zgui::component;
use zgui::reactive::Signal;
use zgui::view;
use zgui::view::IntoView;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;

use crate::services::Services;
use crate::theme::SemanticTokensProps;
use crate::windows::tactics::TacticsHost;
use crate::windows::tactics::TacticsWindowProps;
use crate::windows::tactics::model::BoardModel;

thread_local! {
    static MOUNTED: RefCell<Option<BoardModel>> = const { RefCell::new(None) };
}

pub(super) fn mounted(model: BoardModel) {
    MOUNTED.set(Some(model));
}

/// The board's window content, with the services and the theme a window gives it.
#[component]
fn BoardUnderTest(runtime: Option<Arc<Runtime>>) -> impl IntoView {
    Services::provide(runtime);
    let host = TacticsHost::new();

    view! {
        ThemeRoot(choice = Signal::stored_local(ThemeChoice::System)) {
            SemanticTokens(choice = Signal::stored_local(wows_toolkit_viewmodel::settings::ThemeChoice::System))
            ToastHost {
                TacticsWindow(host = host)
            }
        }
    }
}

pub(crate) struct BoardWindow {
    pub(crate) window: HeadlessWindow,
    pub(crate) model: BoardModel,
    _runtime: Arc<Runtime>,
}

impl BoardWindow {
    fn open() -> Self {
        Self::open_drawn(Drawing::Recorded, None)
    }

    fn open_drawn(drawing: Drawing, desktop: Option<Appearance>) -> Self {
        Self::open_with(HeadlessOptions {
            drawing,
            desktop,
            ..HeadlessOptions::new(WindowSize { width: 1200.0, height: 800.0 })
        })
    }

    pub(crate) fn open_with(options: HeadlessOptions) -> Self {
        let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
        let shown = Arc::clone(&runtime);
        let mut window = HeadlessWindow::open(options, move || {
            let runtime = Some(Arc::clone(&shown));
            view! { BoardUnderTest(runtime = runtime) }
        })
        .expect("the board opens headlessly");
        window.advance(Duration::from_millis(100));
        let model = MOUNTED.with_borrow(|model| *model).expect("the board was built");

        Self { window, model, _runtime: runtime }
    }

    pub(crate) fn settle(&mut self) {
        self.window.advance(Duration::from_millis(300));
    }

    /// Runs the window in real time until `done` holds: work handed to another thread lands only
    /// as time passes.
    fn wait_until(&mut self, done: impl Fn() -> bool) {
        for _ in 0..100 {
            if done() {
                return;
            }
            self.window.advance(Duration::from_millis(100));
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the board did not get there in time");
    }

    fn has(&self, test_id: &str) -> bool {
        self.window.by_test_id(test_id).is_some()
    }

    /// Whether the control `test_id` is shown taken up: a chosen control names itself in
    /// `data-chosen`.
    fn chosen(&self, test_id: &str) -> bool {
        self.window.by_attribute("data-chosen", test_id).is_some()
    }

    pub(crate) fn press(&mut self, test_id: &str) {
        let element = self.window.by_test_id(test_id).unwrap_or_else(|| panic!("{test_id} is shown"));
        self.window.click(element).unwrap_or_else(|err| panic!("{test_id} can be pressed: {err}"));
        self.settle();
    }

    fn set_map(&mut self) {
        self.model.update(|board| {
            board.set_map(MapChoice {
                map_id: Some(1),
                space: "spaces/01_solomon_islands".into(),
                label: "Solomon".into(),
            })
        });
        self.settle();
    }
}

pub(crate) fn tool_id(tool: BoardTool) -> String {
    format!("tactics-tool-{}", tool as u8)
}

/// A board with no map asks for one.
#[test]
fn a_board_with_no_map_asks_for_one() {
    let mut board = BoardWindow::open();
    assert!(board.has("tactics-pick-a-map"));
    assert!(board.has("tactics-map-select"));
    assert!(!board.has("tactics-mode-select"), "the mode picker waits for a map");

    board.set_map();
    assert!(board.has("tactics-mode-select"));
    assert!(!board.has("tactics-pick-a-map"));
}

/// A tool button takes its tool up, and a second press puts it down.
#[test]
fn a_tool_button_takes_its_tool_up_and_puts_it_down() {
    let mut board = BoardWindow::open();
    let line = tool_id(BoardTool::Line);
    assert!(!board.chosen(&line));
    board.press(&line);
    assert!(board.chosen(&line));
    board.press(&line);
    assert!(!board.chosen(&line));
}

/// The chords reach the board with nothing focused, and Escape puts the tool down.
#[test]
fn the_chords_take_up_tools() {
    let mut board = BoardWindow::open();
    board.window.press_character('4', Modifiers::PRIMARY);
    board.settle();
    assert!(board.chosen(&tool_id(BoardTool::Line)));

    board.window.press_character('m', Modifiers::PRIMARY);
    board.settle();
    assert!(board.chosen(&tool_id(BoardTool::Measurement)));

    board.window.press_key(NamedKey::Escape, Modifiers::NONE);
    board.settle();
    assert!(!board.chosen(&tool_id(BoardTool::Measurement)));
}

/// An ink pressed is the ink drawn in.
#[test]
fn an_ink_is_chosen() {
    let mut board = BoardWindow::open();
    assert!(board.chosen("tactics-ink-0"), "yellow is the ink a board starts with");
    board.press("tactics-ink-3");
    assert!(board.chosen("tactics-ink-3"));
    assert!(!board.chosen("tactics-ink-0"));
}

/// The nib steps within its range, and its buttons say when it can go no further.
#[test]
fn the_nib_steps_within_its_range() {
    let mut board = BoardWindow::open();
    board.press("tactics-nib-down");
    assert_eq!(board.model.with_untracked(|board| board.nib()), 1.0);
    board.press("tactics-nib-up");
    board.press("tactics-nib-up");
    assert_eq!(board.model.with_untracked(|board| board.nib()), 3.0);
}

/// Undo has nothing to take back on a fresh board, and the clear button waits for a drawing.
#[test]
fn a_fresh_board_has_nothing_to_take_back() {
    let board = BoardWindow::open();
    assert!(!board.has("tactics-clear-drawing"));
    assert!(!board.has("tactics-clear-caps"));
    assert!(board.model.with_untracked(|board| !board.can_undo()));
}

/// A range circle's name toggles it as its checkbox does.
#[test]
fn a_range_circle_name_toggles_it() {
    let mut board = BoardWindow::open();
    let (circle, _) = RANGE_CIRCLES[0];
    let on = |board: &BoardWindow| board.model.with_untracked(|board| circle.is_on(board.range_filter()));
    let was = on(&board);
    board.press("tactics-ranges");
    board.press(&format!("tactics-range-{}-name", circle as u8));
    assert_eq!(on(&board), !was, "the name toggled it");
    board.press(&format!("tactics-range-{}-name", circle as u8));
    assert_eq!(on(&board), was, "and back");
}

/// The shortcuts button opens the list of chords.
#[test]
fn the_shortcuts_button_opens_the_chords() {
    let mut board = BoardWindow::open();
    assert!(!board.has("tactics-shortcuts-menu"));
    board.press("tactics-shortcuts");
    assert!(board.has("tactics-shortcuts-menu"), "the press opens the list");
}

/// Escape in the ship search empties what was typed.
#[test]
fn escape_empties_the_ship_search() {
    let mut board = BoardWindow::open();
    board.set_map();
    let search = board.window.by_test_id("tactics-ship-search").expect("the ship search is shown");
    let typed = |window: &mut HeadlessWindow| window.a11y(search).and_then(|state| state.value).unwrap_or_default();
    board.press("tactics-ship-search");
    board.window.type_text("yam");
    board.settle();
    assert_eq!(typed(&mut board.window), "yam");

    board.window.press_key(NamedKey::Escape, Modifiers::NONE);
    board.settle();
    assert_eq!(typed(&mut board.window), "", "Escape empties the search");
}

/// The board's map at the minimap's own size, so an element point is the same map point.
fn show_at_minimap_size(board: &mut BoardWindow) {
    board.model.update(|board| {
        board.set_element(ElementSize { width: 768.0, height: 768.0 });
        board.drawn(Some((768.0, 768.0)));
    });
    board.settle();
}

/// A peer's pointer is drawn over the map where it points, and follows it as the session is
/// followed.
#[test]
fn a_peers_pointer_is_drawn_where_it_points() {
    let mut board = BoardWindow::open();
    board.set_map();
    show_at_minimap_size(&mut board);
    let state = Arc::default();
    let (link, _sent) = CollabLink::for_test(Arc::clone(&state));
    board.model.update(|board| board.set_collab(link));
    let cursor = |at: [f32; 2]| UserCursor {
        user_id: 9,
        name: "Peer".to_owned(),
        color: [0, 200, 255],
        pos: Some(at),
        last_update: std::time::Instant::now(),
    };
    let offset = |board: &BoardWindow| {
        let map = board.window.by_test_id("tactics-map").and_then(|map| board.window.bounds(map)).expect("a map");
        let mark = board
            .window
            .by_test_id("tactics-peer-cursor-9")
            .and_then(|mark| board.window.bounds(mark))
            .expect("a mark");
        (mark.origin.x.0 - map.origin.x.0, mark.origin.y.0 - map.origin.y.0)
    };
    let near = |(x, y): (f32, f32), to: (f32, f32)| (x - to.0).abs() < 1.0 && (y - to.1).abs() < 1.0;

    state.lock().cursors.push(cursor([300.0, 400.0]));
    board.settle();
    let at = offset(&board);
    assert!(near(at, (296.0, 396.0)), "the dot is centered on the pointer, at {at:?}");

    state.lock().cursors[0] = cursor([500.0, 100.0]);
    board.settle();
    let at = offset(&board);
    assert!(near(at, (496.0, 96.0)), "and follows it, to {at:?}");
}

/// The board's window from a real install, set on a map with a capture point added and a line
/// drawn, in both palettes. Reads the install named by `WT_WOWS_DIR`.
#[test]
#[ignore = "draws on the graphics device and reads a game install; run by hand to review the board"]
fn the_board_renders_in_dark_and_light() {
    let Some(dir) = std::env::var_os("WT_WOWS_DIR").map(PathBuf::from) else {
        panic!("WT_WOWS_DIR names a game install");
    };
    let cache = wows_toolkit_replay_data::load::GameDataCache::new(dir.clone());
    wows_toolkit_replay_data::load::load_current_build(&dir, &cache).expect("the build loads");

    for (appearance, name) in [(Appearance::Dark, "tactics-dark"), (Appearance::Light, "tactics-light")] {
        let mut board = BoardWindow::open_drawn(Drawing::Device, Some(appearance));
        let cache = cache.clone();
        board.model.update(move |board| board.set_game_data(Some(cache)));
        let map = board
            .model
            .with_untracked(|board| board.maps().iter().find(|map| map.space.contains("16_OC_bees_to_honey")).cloned())
            .or_else(|| board.model.with_untracked(|board| board.maps().first().cloned()))
            .expect("the install has maps");
        board.model.update(|board| board.set_map(map));
        for _ in 0..30 {
            board.window.advance(Duration::from_millis(100));
            std::thread::sleep(Duration::from_millis(50));
        }
        // A capture point placed in the middle of the map, then an arrow drawn out from it.
        board.press("tactics-add-cap");
        board.press("tactics-map");
        board.press(&tool_id(BoardTool::Arrow));
        let map = board.window.by_test_id("tactics-map").expect("the map is drawn");
        let bounds = board.window.bounds(map).expect("the map has a box");
        let to = zgui::geom::Point::new(
            zgui::geom::DevicePx(bounds.origin.x.0 + bounds.size.width.0 * 0.75),
            zgui::geom::DevicePx(bounds.origin.y.0 + bounds.size.height.0 * 0.3),
        );
        board.window.drag(map, to, 8).expect("the arrow is drawn");
        for _ in 0..20 {
            board.window.advance(Duration::from_millis(100));
            std::thread::sleep(Duration::from_millis(50));
        }
        let (caps, shapes) = board.model.with_untracked(|board| (board.caps().len(), board.annotation_count()));
        assert_eq!((caps, shapes), (1, 1), "the capture point and the arrow are on the board");
        let path = crate::interaction_tests::save_shot(&mut board.window, name);
        println!("{}", path.display());
    }
}

const HULL_A: &str = "PRUH610_Moskva_A";
const HULL_B: &str = "PRUH610_Moskva_B";

/// A build whose only ship is a cruiser with a main battery and two hulls that see differently
/// far.
fn two_hull_build() -> GameDataCache {
    let hull = |km: f32| HullUpgradeConfig { detection_km: Some(Km::new(km)), ..HullUpgradeConfig::default() };
    let config = ShipConfigData {
        hull_upgrades: HashMap::from([(HULL_A.to_owned(), hull(12.0)), (HULL_B.to_owned(), hull(10.0))]),
        main_battery_m: Some(Meters::new(15_000.0)),
        ..ShipConfigData::default()
    };
    let vehicle = Vehicle::builder()
        .level(10)
        .group("cruiser".to_owned())
        .maybe_abilities(None)
        .upgrades(Vec::new())
        .maybe_config_data(Some(config))
        .maybe_model_path(None)
        .maybe_armor(None)
        .maybe_hit_locations(None)
        .permoflages(Vec::new())
        .camera_trajectories(Vec::new())
        .maybe_ttx_components(None)
        .innate_skills(Vec::new())
        .build();
    let param = Param::builder()
        .id(GameParamId::from(1u32))
        .index("PRSC610".to_owned())
        .name("PRSC610_Moskva".to_owned())
        .species(Recognized::Known(Species::Cruiser))
        .nation("Russia".to_owned())
        .data(ParamData::Vehicle(vehicle))
        .build();
    let provider = GameMetadataProvider::from_params_no_specs(vec![param]).expect("a one-ship provider builds");
    let vfs: VfsPath = MemoryFS::new().into();

    GameDataCache::holding_for_test(Arc::new(LoadedGameData::for_test(provider, vfs, 1)))
}

/// A placed ship picked out alone is given its own hull, modifiers and circles through the
/// toolbar. Each change goes to the session and moves the circles drawn for that ship.
#[test]
fn a_picked_ship_takes_its_own_hull_modifiers_and_circles() {
    // The map's width in world units, which is what turns a range into a radius.
    const SPACE: f32 = 800.0;

    let mut board = BoardWindow::open();
    let cache = two_hull_build();
    board.model.update(move |board| {
        board.set_game_data(Some(cache));
        board.set_install(None, Some(Version::base(15, 0, 0)));
    });
    board.set_map();

    let state = Arc::default();
    let (link, sent) = CollabLink::for_test(Arc::clone(&state));
    board.model.update(|board| board.set_collab(link));
    // The board speaks to the session once it has told it which map it is on.
    board.wait_until(|| !state.lock().tactics_boards.is_empty());

    let ship = board
        .model
        .change(|board| board.matching_ships(&FilterQuery::new("moskva")))
        .and_then(|found| found.into_iter().next())
        .expect("the ship is found");
    // A build with no map art draws no frame, so the board is told the size of one.
    let at = ElementPos { x: 384.0, y: 384.0 };
    board.model.update(|board| {
        board.set_element(ElementSize { width: 768.0, height: 768.0 });
        board.drawn(Some((768.0, 768.0)));
        board.pick_ship(&ship);
        board.pointer_down(at, BoardModifiers::default());
        board.pointer_up(at);
        board.key(BoardKey::Escape, BoardModifiers::default());
        board.pointer_down(at, BoardModifiers::default());
        board.pointer_up(at);
    });
    board.settle();
    assert_eq!(board.model.with_untracked(|board| board.annotation_count()), 1, "the ship is placed");
    assert!(board.has("tactics-ship-config"), "the ship picked out alone shows its own controls");

    let radius = |board: &BoardWindow, kind: ShipConfigCircleKind| {
        board.model.with_untracked(|board| board.range_circles(SPACE)).into_iter().find_map(|command| match command {
            DrawCommand::ShipConfigCircle { kind: drawn, radius_px, .. } if drawn == kind => Some(radius_px),
            _ => None,
        })
    };
    assert_eq!(radius(&board, ShipConfigCircleKind::Detection), None, "a ship placed with every circle off draws none");

    board.press(&format!("tactics-ship-range-{}", RangeCircle::Detection as u8));
    board.press("tactics-ship-hull");
    board.press("tactics-ship-hull-1");
    let on_b = radius(&board, ShipConfigCircleKind::Detection).expect("the detection circle is drawn");
    board.press("tactics-ship-hull");
    board.press("tactics-ship-hull-0");
    let on_a = radius(&board, ShipConfigCircleKind::Detection).expect("the detection circle is drawn");
    assert!((on_b / on_a - 10.0 / 12.0).abs() < 1e-4, "the circle follows the hull: {on_b} against {on_a}");

    board.press(&format!("tactics-ship-range-{}", RangeCircle::MainBattery as u8));
    let stock = radius(&board, ShipConfigCircleKind::MainBattery).expect("the main battery circle is drawn");
    board.press(&format!("tactics-ship-modifier-{}", ShipModifier::GunRange as u8));
    let modded = radius(&board, ShipConfigCircleKind::MainBattery).expect("the main battery circle is drawn");
    assert!((modded / stock - 1.16).abs() < 1e-4, "the gun range mod widens the circle: {modded} against {stock}");

    let config = board.model.with_untracked(|board| board.picked_ship()).expect("the ship is still picked").config;
    assert_eq!(config.hull_name.as_deref(), Some(HULL_A));
    assert!(ShipModifier::GunRange.is_on(&config));
    assert!(!ShipModifier::ConcealmentExpert.is_on(&config));
    assert!(config.range_filter.detection && config.range_filter.main_battery);
    let told: Vec<AnnotationShipConfig> = sent
        .try_iter()
        .filter_map(|event| match event {
            LocalEvent::Annotation(LocalAnnotationEvent::Set {
                annotation: Annotation::Ship { config, .. }, ..
            }) => config,
            _ => None,
        })
        .collect();
    assert!(told.iter().any(|told| told.hull_name.as_deref() == Some(HULL_B)), "the session heard of each hull");
    assert_eq!(told.last(), Some(&config), "and the last thing it heard is the ship as it stands");
}

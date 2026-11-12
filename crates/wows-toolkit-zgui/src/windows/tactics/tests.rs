//! The board's window driven headlessly.

use std::cell::RefCell;
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
use wows_toolkit_replay_data::tactics::BoardTool;
use wows_toolkit_replay_data::tactics::MapChoice;
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

struct BoardWindow {
    window: HeadlessWindow,
    model: BoardModel,
    _runtime: Arc<Runtime>,
}

impl BoardWindow {
    fn open() -> Self {
        Self::open_drawn(Drawing::Recorded, None)
    }

    fn open_drawn(drawing: Drawing, desktop: Option<Appearance>) -> Self {
        let runtime = Arc::new(wows_toolkit_services::runtime::build().expect("a tokio runtime starts"));
        let shown = Arc::clone(&runtime);
        let options =
            HeadlessOptions { drawing, desktop, ..HeadlessOptions::new(WindowSize { width: 1200.0, height: 800.0 }) };
        let mut window = HeadlessWindow::open(options, move || {
            let runtime = Some(Arc::clone(&shown));
            view! { BoardUnderTest(runtime = runtime) }
        })
        .expect("the board opens headlessly");
        window.advance(Duration::from_millis(100));
        let model = MOUNTED.with_borrow(|model| *model).expect("the board was built");

        Self { window, model, _runtime: runtime }
    }

    fn settle(&mut self) {
        self.window.advance(Duration::from_millis(300));
    }

    fn has(&self, test_id: &str) -> bool {
        self.window.by_test_id(test_id).is_some()
    }

    /// Whether the control `test_id` is shown taken up: a chosen control names itself in
    /// `data-chosen`.
    fn chosen(&self, test_id: &str) -> bool {
        self.window.by_attribute("data-chosen", test_id).is_some()
    }

    fn press(&mut self, test_id: &str) {
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

fn tool_id(tool: BoardTool) -> String {
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

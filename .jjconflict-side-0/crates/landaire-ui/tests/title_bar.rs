//! A title bar and window frame driven over the headless platform.

#![cfg(feature = "test-support")]

use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use landaire_ui::FrameWindow;
use landaire_ui::ThemeRootProps;
use landaire_ui::TitleBarLeading;
use landaire_ui::TitleBarProps;
use landaire_ui::TitleButtonSide;
use landaire_ui::WindowFrameProps;
use landaire_ui::test_support::HeadlessOptions;
use landaire_ui::test_support::HeadlessWindow;
use landaire_ui::test_support::WindowSize;
use zgui::component;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::platform::CursorStyle;
use zgui::platform::DecorationSource;
use zgui::platform::FullscreenMode;
use zgui::platform::ResizeEdge;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::vocab::Modifiers;
use zgui::vocab::NamedKey;
use zgui::vocab::Role;

const SIZE: WindowSize = WindowSize { width: 600.0, height: 200.0 };
const SETTLE: Duration = Duration::from_millis(300);

/// What the bar asked of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Request {
    Minimize,
    ToggleMaximized,
    TitleBarDoubleClick,
    Close,
    MoveDrag,
    ResizeDrag(ResizeEdge),
}

/// A window that records each request and the cursor it was last told to show, and maximises on
/// the two requests that ask for it.
#[derive(Clone)]
struct Recorded {
    source: RwSignal<DecorationSource>,
    maximized: RwSignal<bool>,
    requests: Rc<RefCell<Vec<Request>>>,
    cursor: Rc<Cell<CursorStyle>>,
}

impl Recorded {
    fn record(&self, request: Request) {
        self.requests.borrow_mut().push(request);
    }

    fn taken(&self) -> Vec<Request> {
        std::mem::take(&mut *self.requests.borrow_mut())
    }
}

impl FrameWindow for Recorded {
    fn decoration_source(&self) -> Signal<DecorationSource> {
        self.source.into()
    }

    fn maximized(&self) -> Signal<bool> {
        self.maximized.into()
    }

    fn fullscreen(&self) -> Signal<Option<FullscreenMode>> {
        Signal::stored(None)
    }

    fn minimize(&self) {
        self.record(Request::Minimize);
    }

    fn toggle_maximized(&self) {
        self.record(Request::ToggleMaximized);
        self.maximized.update(|maximized| *maximized = !*maximized);
    }

    fn title_bar_double_click(&self) {
        self.record(Request::TitleBarDoubleClick);
        self.maximized.update(|maximized| *maximized = !*maximized);
    }

    fn close(&self) {
        self.record(Request::Close);
    }

    fn begin_move_drag(&self) {
        self.record(Request::MoveDrag);
    }

    fn begin_resize_drag(&self, edge: ResizeEdge) {
        self.record(Request::ResizeDrag(edge));
    }

    fn set_cursor(&self, cursor: CursorStyle) {
        self.cursor.set(cursor);
    }
}

#[component]
fn Framed(window: Recorded) -> impl IntoView {
    let window: Rc<dyn FrameWindow> = Rc::new(window);

    view! {
        ThemeRoot {
            WindowFrame(window = Rc::clone(&window)) {
                TitleBar(window = Rc::clone(&window), buttons = TitleButtonSide::Trailing) {"Replays"}
            }
        }
    }
}

/// A title bar whose leading content is far wider than the window.
#[component]
fn Crowded(window: Recorded) -> impl IntoView {
    let window: Rc<dyn FrameWindow> = Rc::new(window);

    view! {
        ThemeRoot {
            WindowFrame(window = Rc::clone(&window)) {
                TitleBar(window = Rc::clone(&window), buttons = TitleButtonSide::Trailing) {
                    TitleBarLeading(slot) {
                        row(style = "flex: none; width: 2000px; height: 20px") {}
                    }
                    "Replays"
                }
            }
        }
    }
}

#[component]
fn InWindow() -> impl IntoView {
    view! {
        ThemeRoot {
            TitleBar(buttons = TitleButtonSide::Trailing) {"Replays"}
        }
    }
}

fn open_recorded() -> (HeadlessWindow, Recorded) {
    open_with(|window| AnyView::new(view! { Framed(window = window) }))
}

fn open_with(build: fn(Recorded) -> AnyView) -> (HeadlessWindow, Recorded) {
    let handed = Rc::new(RefCell::new(None));
    let sent = Rc::clone(&handed);
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), move || {
        let recorded = Recorded {
            source: RwSignal::new(DecorationSource::Application),
            maximized: RwSignal::new(false),
            requests: Rc::new(RefCell::new(Vec::new())),
            cursor: Rc::new(Cell::new(CursorStyle::Default)),
        };
        *sent.borrow_mut() = Some(recorded.clone());
        build(recorded)
    })
    .expect("opens");
    window.advance(SETTLE);

    let recorded = handed.borrow_mut().take().expect("the view was built");
    (window, recorded)
}

fn open_in_window() -> HeadlessWindow {
    let mut window = HeadlessWindow::open(HeadlessOptions::new(SIZE), || view! { InWindow() }).expect("opens");
    window.advance(SETTLE);

    window
}

fn button(window: &mut HeadlessWindow, label: &str) -> Option<landaire_ui::test_support::Element> {
    window.by_role_and_label(Role::Button, label)
}

#[test]
fn the_bar_is_shown_only_while_the_application_draws_the_frame() {
    let mut window = open_in_window();
    assert!(window.by_class("lui-title-bar").is_none(), "the platform draws the frame");
    assert!(button(&mut window, "Close").is_none());

    window.set_decoration_source(DecorationSource::Application).expect("the window has a surface");
    window.advance(SETTLE);
    assert!(window.by_class("lui-title-bar").is_some(), "the application draws the frame");
    for label in ["Minimize", "Maximize", "Close"] {
        assert!(button(&mut window, label).is_some(), "the {label} button is shown");
    }

    window.set_decoration_source(DecorationSource::Platform).expect("the window has a surface");
    window.advance(SETTLE);
    assert!(window.by_class("lui-title-bar").is_none(), "the platform took the frame back");
}

#[test]
fn the_close_button_closes_the_window() {
    let mut window = open_in_window();
    window.set_decoration_source(DecorationSource::Application).expect("the window has a surface");
    window.advance(SETTLE);

    let close = button(&mut window, "Close").expect("the close button is shown");
    window.click(close).expect("the close button has a box");
    window.advance(SETTLE);
    assert!(!window.is_open());
}

#[test]
fn each_button_asks_for_its_action_and_none_moves_the_window() {
    let (mut window, recorded) = open_recorded();

    for label in ["Minimize", "Maximize", "Close"] {
        let pressed = button(&mut window, label).expect("the button is shown");
        window.click(pressed).expect("the button has a box");
    }

    assert_eq!(recorded.taken(), [Request::Minimize, Request::ToggleMaximized, Request::Close]);
}

#[test]
fn a_press_at_the_corner_of_the_close_button_closes_rather_than_resizes() {
    let (mut window, recorded) = open_recorded();
    let close = button(&mut window, "Close").expect("the close button is shown");
    let bounds = window.bounds(close).expect("the close button has a box");
    let corner = Point::new(DevicePx(bounds.origin.x.0 + bounds.size.width.0 - 2.0), DevicePx(bounds.origin.y.0 + 2.0));

    window.click_at(corner);
    assert_eq!(recorded.taken(), [Request::Close]);
}

#[test]
fn wide_leading_content_leaves_the_buttons_whole_and_in_the_window() {
    let (mut window, _) = open_with(|window| AnyView::new(view! { Crowded(window = window) }));

    for label in ["Minimize", "Maximize", "Close"] {
        let pressed = button(&mut window, label).expect("the button is shown");
        let bounds = window.bounds(pressed).expect("the button has a box");
        assert_eq!(bounds.size.width.0, 32.0, "the {label} button keeps its width");
        assert!(bounds.origin.x.0 + bounds.size.width.0 <= SIZE.width, "the {label} button is inside the window");
    }
}

#[test]
fn the_buttons_are_reached_and_pressed_from_the_keyboard() {
    let (mut window, recorded) = open_recorded();

    window.press_key(NamedKey::Tab, Modifiers::NONE);
    let minimize = button(&mut window, "Minimize").expect("the button is shown");
    assert!(window.a11y(minimize).is_some_and(|state| state.focused), "the first stop is the minimize button");

    window.press_key(NamedKey::Enter, Modifiers::NONE);
    assert_eq!(recorded.taken(), [Request::Minimize]);
}

#[test]
fn the_maximize_button_follows_the_window() {
    let (mut window, recorded) = open_recorded();
    assert!(button(&mut window, "Maximize").is_some());
    assert!(window.by_attribute("data-icon", "window-maximize").is_some());

    recorded.maximized.set(true);
    window.advance(SETTLE);
    assert!(button(&mut window, "Maximize").is_none());
    assert!(button(&mut window, "Restore").is_some());
    assert!(window.by_attribute("data-icon", "window-restore").is_some());
    assert!(window.by_attribute("data-icon", "window-maximize").is_none());
}

#[test]
fn a_double_press_on_the_title_maximizes_the_window() {
    let (mut window, recorded) = open_recorded();
    let title = window.by_class("lui-title-bar__title").expect("the title is shown");

    window.double_click(title).expect("the title has a box");
    assert_eq!(recorded.taken(), [Request::MoveDrag, Request::TitleBarDoubleClick]);
    assert!(recorded.maximized.get_untracked());

    window.advance(SETTLE);
    assert!(button(&mut window, "Restore").is_some(), "the button offers to restore it");
}

#[test]
fn the_frame_resizes_from_its_edges_only_while_it_is_framed() {
    let (mut window, recorded) = open_recorded();
    let north = window.by_attribute("data-edge", "n").expect("the top edge has a grip");
    let bounds = window.bounds(north).expect("the grip has a box");
    assert_eq!(bounds.size.height.0, 8.0);

    window.click(north).expect("the grip has a box");
    assert_eq!(recorded.taken(), [Request::ResizeDrag(ResizeEdge::North)]);
    assert_eq!(recorded.cursor.get(), CursorStyle::ResizeNorthSouth);

    recorded.maximized.set(true);
    window.advance(SETTLE);
    assert!(window.by_class("lui-window-frame__grip").is_none(), "a maximized window has no grips");
    assert_eq!(recorded.cursor.get(), CursorStyle::Default, "the grip under the pointer put the cursor back");

    recorded.maximized.set(false);
    recorded.source.set(DecorationSource::Platform);
    window.advance(SETTLE);
    assert!(window.by_class("lui-window-frame__grip").is_none(), "the platform's frame has its own");
    assert!(window.by_class("lui-title-bar").is_none());
}

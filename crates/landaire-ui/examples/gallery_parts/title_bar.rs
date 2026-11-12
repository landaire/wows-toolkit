//! The gallery's title bar section: a framed window with its buttons at the end, and a maximized
//! one with its buttons at the start and content beside its title.

use std::rc::Rc;

use landaire_ui::FrameWindow;
use landaire_ui::TitleBarLeading;
use landaire_ui::TitleBarProps;
use landaire_ui::TitleBarTrailing;
use landaire_ui::TitleButtonSide;
use landaire_ui::WindowFrameProps;
use landaire_ui::test_support::WindowSize;
use zgui::css;
use zgui::platform::CursorStyle;
use zgui::platform::DecorationSource;
use zgui::platform::FullscreenMode;
use zgui::platform::ResizeEdge;
use zgui::reactive::Signal;
use zgui::runtime::windows::WindowHandle;
use zgui::runtime::windows::use_window;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;
use zgui_ui_icons::set::ui::PANEL_LEFT;

use crate::Section;

pub const SECTION: Section =
    Section { name: "title_bar", title: "Title bar", page: WindowSize { width: 1280.0, height: 300.0 }, view };

const SHEET: &str = css!(
    ".gallery-title-bar { display: flex; flex-direction: row; gap: 16px; align-items: flex-start; }
     .gallery-title-bar__window { display: flex; flex-direction: column; width: 520px; height: 200px; }
     .gallery-title-bar__content { flex: 1 1 auto; padding: 12px; color: var(--lui-color-text-dim); }
     .gallery-title-bar__status { padding: 0 8px; color: var(--lui-color-text-dim); }"
);

/// The gallery's own window, with its frame left to the application whatever the platform says.
struct Undecorated {
    window: WindowHandle,
    maximized: Signal<bool>,
}

impl FrameWindow for Undecorated {
    fn decoration_source(&self) -> Signal<DecorationSource> {
        Signal::stored(DecorationSource::Application)
    }

    fn maximized(&self) -> Signal<bool> {
        self.maximized
    }

    fn fullscreen(&self) -> Signal<Option<FullscreenMode>> {
        self.window.fullscreen()
    }

    fn minimize(&self) {
        self.window.minimize();
    }

    fn toggle_maximized(&self) {
        self.window.toggle_maximized();
    }

    fn title_bar_double_click(&self) {
        self.window.title_bar_double_click();
    }

    fn close(&self) {
        self.window.close();
    }

    fn begin_move_drag(&self) {
        self.window.begin_move_drag();
    }

    fn begin_resize_drag(&self, edge: ResizeEdge) {
        self.window.begin_resize_drag(edge);
    }

    fn set_cursor(&self, cursor: CursorStyle) {
        self.window.set_cursor(cursor);
    }
}

fn view() -> AnyView {
    install_stylesheet("landaire-ui-gallery-title-bar", SHEET);

    let window = use_window();
    let framed: Rc<dyn FrameWindow> = Rc::new(Undecorated { maximized: window.maximized(), window: window.clone() });
    let filling: Rc<dyn FrameWindow> = Rc::new(Undecorated { maximized: Signal::stored(true), window });

    AnyView::new(view! {
        row(class = "gallery-title-bar") {
            box(class = "gallery-title-bar__window") {
                WindowFrame(window = Rc::clone(&framed)) {
                    TitleBar(window = Rc::clone(&framed), buttons = TitleButtonSide::Trailing) {"Replay viewer"}
                    box(class = "gallery-title-bar__content") {"Framed, with its buttons at the end"}
                }
            }
            box(class = "gallery-title-bar__window") {
                WindowFrame(window = Rc::clone(&filling)) {
                    TitleBar(window = Rc::clone(&filling), buttons = TitleButtonSide::Leading) {
                        TitleBarLeading(slot) {
                            Button(variant = ButtonVariant::Ghost, size = ButtonSize::IconSm, a11y:label = "Hide listing") {
                                Icon(icon = PANEL_LEFT, size = IconSize::Sm)
                            }
                        }
                        "Replay viewer"
                        TitleBarTrailing(slot) {
                            text(class = "gallery-title-bar__status") {"Connected"}
                        }
                    }
                    box(class = "gallery-title-bar__content") {"Maximized, with its buttons at the start"}
                }
            }
        }
    })
}

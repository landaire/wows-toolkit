//! Windows other than the main one.
//!
//! [`open`] is how a view opens one: the new window draws in the main window's palette, zoom and
//! motion, and its content finds [`Services`] through `Services::current()` as the main window's
//! does. It has no [`Shell`](crate::shell::Shell): the chrome belongs to the main window.

pub mod renderer;
pub mod tactics;

use landaire_ui::ThemeRootProps;
use landaire_ui::ToastHostProps;
use zgui::component;
use zgui::css;
use zgui::prelude::WindowHandle;
use zgui::prelude::WindowOptions;
use zgui::prelude::use_windows;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::provide_local_context;
use zgui::reactive::use_local_context;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::Children;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;

use crate::services::Services;
use crate::shell::Appearance;
use crate::theme::SemanticTokensProps;
use crate::theme::palette_choice;
use crate::window_shell::DecoratedWindowProps;

const SHEET: &str = "wt-window";

/// The window's content fills it, on the surface and in the type the main window's shell uses.
const CSS: &str = css!(
    ".wt-window {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        background-color: var(--lui-color-surface);
        color: var(--lui-color-text);
        font-family: var(--lui-font-family);
        font-size: var(--lui-font-size);
    }"
);

/// Opens a window over `content`, themed like the main one. Called from a component's body or
/// from a listener inside the main window.
///
/// # Panics
///
/// Outside the main window's `AppRoot`.
pub fn open<F, V>(options: WindowOptions, mut content: F) -> WindowHandle
where
    F: FnMut() -> V + 'static,
    V: IntoView + 'static,
{
    let services = Services::current();
    let appearance =
        use_local_context::<Appearance>().expect("a secondary window is opened from inside the main window");
    let title = options.attributes().title.to_string();
    use_windows().open(options, move || {
        let content = AnyView::new(content());
        let title = title.clone();
        view! { WindowRoot(services = services, appearance = appearance, title = title, content = content) }
    })
}

/// A secondary window's theme, frame and message host, with the process's services published to
/// it.
#[component]
fn WindowRoot(services: Services, appearance: Appearance, title: String, content: AnyView) -> impl IntoView {
    provide_local_context(services);
    provide_local_context(appearance);
    let choice = Signal::derive_local(move || palette_choice(appearance.shown_theme()));
    let shown = Signal::derive_local(move || appearance.shown_theme());
    let motion = Signal::derive_local(move || appearance.motion.get());
    let zoom = Signal::derive_local(move || crate::zoom::to_window(appearance.zoom.get()));

    view! {
        ThemeRoot(choice = choice, motion = motion, zoom = zoom) {
            SemanticTokens(choice = shown)
            ToastHost {
                DecoratedWindow(title = title) {
                    WindowSurface {
                        {content}
                    }
                }
            }
        }
    }
}

/// What fills a secondary window: its content, on the surface the main window's shell draws.
#[component]
pub(crate) fn WindowSurface(children: Children) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        column(class = "wt-window") {
            {children.into_view_once()}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Duration;

    use landaire_ui::Toasts;
    use landaire_ui::test_support::HeadlessOptions;
    use landaire_ui::test_support::HeadlessWindow;
    use landaire_ui::test_support::WindowSize;

    use super::*;

    thread_local! {
        static TOASTS: Cell<Option<Toasts>> = const { Cell::new(None) };
    }

    #[component]
    fn Panel() -> impl IntoView {
        TOASTS.set(Toasts::current());

        view! { box {} }
    }

    #[component]
    fn SecondaryUnderTest() -> impl IntoView {
        let services = Services::provide(None);
        let content = AnyView::new(view! { Panel() });

        view! { WindowRoot(services = services, appearance = Appearance::new(), title = "Panel".to_owned(), content = content) }
    }

    /// A secondary window has a message host of its own, so a toast raised in it reaches its screen.
    #[test]
    fn a_toast_reaches_a_secondary_windows_screen() {
        let mut window = HeadlessWindow::open(HeadlessOptions::new(WindowSize { width: 400.0, height: 200.0 }), || {
            view! { SecondaryUnderTest() }
        })
        .expect("the window opens headlessly");
        window.advance(Duration::from_millis(50));

        TOASTS.get().expect("the content sits inside a message host").ok("exported");
        window.advance(Duration::from_millis(400));

        let shown = window.texts_by_class("zui-toast").concat();
        assert!(shown.contains("exported"), "the toast is on screen, got {shown:?}");
    }
}

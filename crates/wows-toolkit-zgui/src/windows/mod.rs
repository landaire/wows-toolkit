//! Windows other than the main one.
//!
//! [`open`] is how a view opens one: the new window draws in the main window's palette, zoom and
//! motion, and its content finds [`Services`] through `Services::current()` as the main window's
//! does. It has no [`Shell`](crate::shell::Shell): the chrome belongs to the main window.

pub mod renderer;
pub mod tactics;

use landaire_ui::ThemeRootProps;
use landaire_ui::ToastHostProps;
use landaire_ui::Toasts;
use zgui::component;
use zgui::css;
use zgui::prelude::WindowHandle;
use zgui::prelude::WindowOptions;
use zgui::prelude::use_windows;
use zgui::reactive::Memo;
use zgui::reactive::Signal;
use zgui::reactive::prelude::Get;
use zgui::reactive::provide_local_context;
use zgui::reactive::use_local_context;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::Children;
use zgui::view::IntoView;
use zgui::view::NodeRef;
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
    let shown = follow_main(move || appearance.try_shown_theme());
    let motion = follow_main(move || appearance.motion.try_get());
    let zoom = follow_main(move || appearance.zoom.try_get());
    let choice = Signal::derive_local(move || palette_choice(shown.get()));
    let shown = Signal::derive_local(move || shown.get());
    let motion = Signal::derive_local(move || motion.get());
    let zoom = Signal::derive_local(move || crate::zoom::to_window(zoom.get()));

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

/// Follows a value of the main window's appearance, then holds the last one read once the main
/// window is gone: a secondary window can outlive it at exit.
fn follow_main<T>(read: impl Fn() -> Option<T> + Send + Sync + 'static) -> Memo<T>
where
    T: Clone + PartialEq + Send + Sync + 'static,
{
    let opened = read().expect("a secondary window opens from inside the main window");

    Memo::new(move |last: Option<&T>| read().or_else(|| last.cloned()).unwrap_or_else(|| opened.clone()))
}

/// What fills a secondary window: its content, on the surface the main window's shell draws. The
/// zoom shortcuts step every window's zoom from here as from the main window.
#[component]
pub(crate) fn WindowSurface(children: Children) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let surface = NodeRef::new();
    // A surface placed outside a window that `open` made, as a test fixture is, has no zoom to step.
    if let Some(services) = use_local_context::<Services>()
        && let Some(appearance) = use_local_context::<Appearance>()
        && let Some(toasts) = Toasts::current()
    {
        crate::zoom::hear_zoom_keys(surface, services, appearance, toasts);
    }

    view! {
        column(class = "wt-window", node_ref = surface) {
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
    use zgui::reactive::prelude::GetUntracked;
    use zgui::vocab::Modifiers;

    use super::*;

    thread_local! {
        static TOASTS: Cell<Option<Toasts>> = const { Cell::new(None) };
    }

    #[component]
    fn Panel() -> impl IntoView {
        TOASTS.set(Toasts::current());

        view! { box {} }
    }

    thread_local! {
        static OPENED: Cell<Option<(Services, Appearance)>> = const { Cell::new(None) };
    }

    #[component]
    fn SecondaryUnderTest() -> impl IntoView {
        let services = Services::provide(None);
        let appearance = Appearance::new();
        OPENED.set(Some((services, appearance)));
        let content = AnyView::new(view! { Panel() });

        view! { WindowRoot(services = services, appearance = appearance, title = "Panel".to_owned(), content = content) }
    }

    fn open_secondary() -> HeadlessWindow {
        let mut window = HeadlessWindow::open(HeadlessOptions::new(WindowSize { width: 400.0, height: 200.0 }), || {
            view! { SecondaryUnderTest() }
        })
        .expect("the window opens headlessly");
        window.advance(Duration::from_millis(50));

        window
    }

    /// A secondary window has a message host of its own, so a toast raised in it reaches its screen.
    #[test]
    fn a_toast_reaches_a_secondary_windows_screen() {
        let mut window = open_secondary();

        TOASTS.get().expect("the content sits inside a message host").ok("exported");
        window.advance(Duration::from_millis(400));

        let shown = window.texts_by_class("zui-toast").concat();
        assert!(shown.contains("exported"), "the toast is on screen, got {shown:?}");
    }

    /// The zoom shortcuts work in a secondary window too, and step the zoom every window follows.
    #[test]
    fn the_zoom_shortcuts_reach_a_secondary_window() {
        let mut window = open_secondary();
        let (services, appearance) = OPENED.get().expect("the window was built");
        services.settings.adopt_unsaved(crate::shell::settings::tests::test_settings());
        let before = appearance.zoom.get_untracked();

        window.press_character('=', Modifiers::PRIMARY);
        window.advance(Duration::from_millis(100));

        assert_eq!(appearance.zoom.get_untracked(), before.zoomed_in());
    }

    thread_local! {
        static MAIN: Cell<Option<(Services, Appearance)>> = const { Cell::new(None) };
    }

    #[component]
    fn SecondaryOfMain() -> impl IntoView {
        let (services, appearance) = MAIN.get().expect("the main window's state is made first");
        let content = AnyView::new(view! { Panel() });

        view! { WindowRoot(services = services, appearance = appearance, title = "Panel".to_owned(), content = content) }
    }

    /// The app's exit disposes the main window, which owns the services and the appearance, before
    /// the secondary windows. Until it closes, a secondary window keeps drawing in the last palette
    /// and zoom, and its zoom keys do nothing.
    #[test]
    fn a_secondary_window_outlives_the_main_windows_state() {
        zgui::reactive::install().expect("the reactive runtime installs");
        let main = zgui::reactive::Mounted::new();
        let (services, appearance) = main.with(|| (Services::provide(None), Appearance::new()));
        services.settings.adopt_unsaved(crate::shell::settings::tests::test_settings());
        MAIN.set(Some((services, appearance)));
        let mut window = HeadlessWindow::open(HeadlessOptions::new(WindowSize { width: 400.0, height: 200.0 }), || {
            view! { SecondaryOfMain() }
        })
        .expect("the window opens headlessly");
        window.advance(Duration::from_millis(50));

        main.unmount();
        window.press_character('=', Modifiers::PRIMARY);
        window.resize(WindowSize { width: 500.0, height: 300.0 }).expect("the window resizes");
        window.advance(Duration::from_millis(100));
        assert!(window.is_open(), "the window still runs");
    }
}

//! The gallery's floating window section: a window open over a content area.

use landaire_ui::FloatingSize;
use landaire_ui::FloatingWindowProps;
use landaire_ui::test_support::WindowSize;
use zgui::component;
use zgui::css;
use zgui::reactive::RwSignal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Set;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;

use crate::Section;

pub const SECTION: Section =
    Section { name: "floating", title: "Floating window", page: WindowSize { width: 1280.0, height: 300.0 }, view };

const SHEET: &str = css!(
    ".gallery-floating { display: flex; flex-direction: row; gap: 12px; align-items: flex-start; }
     .gallery-floating__frame { position: relative; display: flex; flex-direction: column; width: 520px;
                                height: 220px; overflow: hidden; border: 1px solid var(--lui-color-border);
                                border-radius: var(--lui-radius-lg); background-color: var(--lui-color-panel); }
     .gallery-floating__pane { display: flex; flex-direction: column; align-items: flex-start; gap: 4px;
                               padding: 8px; }
     .gallery-floating__dim { color: var(--lui-color-text-dim); }
     .gallery-floating__row { display: flex; flex-direction: row; gap: 6px; margin-top: 8px; }"
);

/// A content area with a floating window over it, which the close button shuts and the button
/// beneath opens again.
#[component]
fn Frame() -> impl IntoView {
    let open = RwSignal::new_local(true);
    let close = UnsyncCallback::new(move |()| open.set(false));

    view! {
        column(class = "gallery-floating__frame") {
            column(class = "gallery-floating__pane") {
                text {"Content beside the window"}
                text(class = "gallery-floating__dim") {"The window is not modal"}
                Button(variant = ButtonVariant::Outline, size = ButtonSize::Sm, on:click = move |_| open.set(true)) {
                    "Open the window"
                }
            }
            FloatingWindow(
                open = open,
                on_close = close,
                title = "Server details",
                default_size = {FloatingSize { width: 300.0, height: 170.0 }}
            ) {
                text {"Moved by its title bar, resized by its edges."}
                text(class = "gallery-floating__dim") {"It stays inside the frame it was opened in."}
                row(class = "gallery-floating__row") {
                    Button(size = ButtonSize::Sm) {"Install"}
                    Button(variant = ButtonVariant::Outline, size = ButtonSize::Sm, on:click = move |_| open.set(false)) {
                        "Close"
                    }
                }
            }
        }
    }
}

fn view() -> AnyView {
    install_stylesheet("landaire-ui-gallery-floating", SHEET);

    AnyView::new(view! {
        row(class = "gallery-floating") {
            Frame()
        }
    })
}

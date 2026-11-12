//! The gallery's chrome section: a tab strip, a side split, a status bar and a drawer, each at
//! rest, and the four tones of message.

use std::num::NonZeroU64;

use landaire_ui::CountUpProps;
use landaire_ui::DrawerProps;
use landaire_ui::FadeInProps;
use landaire_ui::Fraction;
use landaire_ui::Notice;
use landaire_ui::PanelWidth;
use landaire_ui::RevealProps;
use landaire_ui::RunningBarProps;
use landaire_ui::SidePanel;
use landaire_ui::SideSplitProps;
use landaire_ui::StatusBarEnd;
use landaire_ui::StatusBarProps;
use landaire_ui::StatusBarStart;
use landaire_ui::StatusToggleProps;
use landaire_ui::TabStripProps;
use landaire_ui::TabStripTabProps;
use landaire_ui::ToastHostProps;
use landaire_ui::ToastKey;
use landaire_ui::Toasts;
use landaire_ui::WidthRange;
use landaire_ui::test_support::WindowSize;
use zgui::component;
use zgui::css;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui_icons::set::status::CHECK_CIRCLE;
use zgui_ui_icons::set::status::INFO;
use zgui_ui_icons::set::ui::ELLIPSIS;
use zgui_ui_icons::set::ui::PANEL_LEFT;
use zgui_ui_icons::set::ui::SEARCH;

use crate::Section;

pub const SECTION: Section =
    Section { name: "chrome", title: "Chrome", page: WindowSize { width: 1280.0, height: 360.0 }, view };

const SHEET: &str = css!(
    ".gallery-chrome { display: flex; flex-direction: row; gap: 12px; align-items: flex-start; }
     .gallery-chrome__frame { display: flex; flex-direction: column; width: 430px; height: 240px;
                              overflow: hidden; border: 1px solid var(--lui-color-border);
                              border-radius: var(--lui-radius-lg); background-color: var(--lui-color-surface); }
     .gallery-chrome__body { position: relative; display: flex; flex: 1 1 auto; min-height: 0;
                             background-color: var(--lui-color-panel); }
     .gallery-chrome__pane { display: flex; flex-direction: column; gap: 4px; padding: 8px; }
     .gallery-chrome__dim { color: var(--lui-color-text-dim); }
     .gallery-chrome__drawer-head { display: flex; flex-direction: row; align-items: center;
                                    padding: 12px 12px 8px 16px; font-weight: 600;
                                    color: var(--lui-color-text-bright); }
     .gallery-chrome__grow { flex: 1 1 auto; }"
);

/// A tab strip over a side split, and a status bar under it.
#[component]
fn SplitFrame() -> impl IntoView {
    let selected = RwSignal::new_local("replays".to_owned());
    let open = RwSignal::new_local(true);
    let width = RwSignal::new_local(PanelWidth(180.0));
    let range = WidthRange { min: PanelWidth(120.0), max: PanelWidth(360.0) };
    let close = UnsyncCallback::new(|()| {});
    let done = RwSignal::new_local(12_u64);
    let total = NonZeroU64::new(30).expect("30 is not zero");
    let share = Signal::derive_local(move || Fraction::of(done.get(), total));

    view! {
        column(class = "gallery-chrome__frame") {
            TabStrip(value = selected, label = "Sections") {
                TabStripTab(value = "replays", icon = CHECK_CIRCLE) {"Replays"}
                TabStripTab(value = "search", icon = SEARCH) {"Search"}
                TabStripTab(value = "stats", icon = INFO) {"Statistics"}
                TabStripTab(value = "folder", on_close = close, close_label = "Close folder") {"Folder: recent"}
            }
            box(class = "gallery-chrome__body") {
                SideSplit(open = open, width = width, range = range, resize_label = "Resize listing") {
                    SidePanel(slot) {
                        column(class = "gallery-chrome__pane") {
                            text {"Listing"}
                            text(class = "gallery-chrome__dim") {"A panel at the left edge"}
                        }
                    }
                    column(class = "gallery-chrome__pane") {
                        text {"Main area"}
                        Reveal(open = true) {
                            text(class = "gallery-chrome__dim") {"Revealed content, open and at rest"}
                        }
                        FadeIn {
                            text(class = "gallery-chrome__dim") {"Faded in"}
                        }
                    }
                }
            }
            StatusBar {
                StatusBarStart(slot) {
                    StatusToggle(pressed = open, icon = PANEL_LEFT, label = "Hide listing")
                }
                text {"Shift: compare"}
                StatusBarEnd(slot) {
                    text {"Rendering "}
                    CountUp(value = done)
                    text {" of 30"}
                    RunningBar(done = share, label = "Rendering")
                    StatusToggle(pressed = RwSignal::new_local(false), icon = ELLIPSIS, label = "Show details")
                }
            }
        }
    }
}

/// A content area with a drawer open over it.
#[component]
fn DrawerFrame() -> impl IntoView {
    let open = RwSignal::new_local(true);

    view! {
        column(class = "gallery-chrome__frame") {
            box(class = "gallery-chrome__body") {
                column(class = "gallery-chrome__pane") {
                    text {"Content under the drawer"}
                    text(class = "gallery-chrome__dim") {"Dimmed by the scrim"}
                }
                Drawer(open = open, label = "Settings", width = PanelWidth(260.0)) {
                    row(class = "gallery-chrome__drawer-head") {
                        text(class = "gallery-chrome__grow") {"Settings"}
                        Button(variant = ButtonVariant::Ghost, size = ButtonSize::Sm) {"Close"}
                    }
                    column(class = "gallery-chrome__pane") {
                        text(class = "gallery-chrome__dim") {"The drawer's content"}
                    }
                }
            }
        }
    }
}

/// Puts one message of each tone up as it is built.
#[component]
fn Messages() -> impl IntoView {
    if let Some(toasts) = Toasts::current() {
        toasts.ok("Copied the path");
        toasts.info("Jumped to the first match");
        toasts.failed("The file could not be opened because the directory it is in is not readable");
        toasts.show(
            Notice::warn("No game directory is set").keyed(ToastKey::new("directory")).sticky().action("Choose", || {}),
        );
    }

    view! { box() }
}

/// Two frames of chrome side by side, and messages in the window's corner.
fn view() -> AnyView {
    install_stylesheet("landaire-ui-gallery-chrome", SHEET);

    AnyView::new(view! {
        ToastHost(limit = 4) {
            row(class = "gallery-chrome") {
                SplitFrame()
                DrawerFrame()
                Messages()
            }
        }
    })
}

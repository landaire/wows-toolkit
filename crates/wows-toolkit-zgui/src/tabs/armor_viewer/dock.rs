//! The tab's panes, each a closable tab of the dock, named for what it shows. A pane may share a
//! group with others or sit in a split of its own. With more than one pane, the active one gets an
//! accent border.

use landaire_ui::DockProps;
use landaire_ui::PanelFactory;
use landaire_ui::PanelId;
use landaire_ui::PanelKind;
use rust_i18n::t;
use zgui::component;
use zgui::css;
use zgui::reactive::Signal;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui::vocab::PointerButton;

use crate::tabs::armor_viewer::viewport::ArmorViewportProps;
use crate::tabs::armor_viewer::workspace::ArmorWorkspace;
use crate::tabs::armor_viewer::workspace::Pane;
use crate::tabs::armor_viewer::workspace::PaneId;
use crate::tabs::dock_menu_strings;

const SHEET: &str = "wt-armor-dock";

/// The kind every pane's panel is registered under.
pub(super) const PANE_KIND: &str = "armor-viewport";

const CSS: &str = css!(
    ".wt-armor-dock { flex: 1 1 auto; min-width: 0; min-height: 0; }
    /* Flush under the strip, so viewports in neighbouring groups line up. */
    .wt-armor-dock .lui-dock__body { padding-top: 0; }
    .wt-armor-pane {
        position: relative;
        display: flex;
        flex: 1 1 auto;
        height: 100%;
        min-width: 0;
        min-height: 0;
        box-sizing: border-box;
        border: 2px solid transparent;
    }
    .wt-armor-pane[data-single] { border-width: 0; }
    .wt-armor-pane[data-active] { border-color: var(--lui-color-accent); }"
);

/// Every pane of `workspace`, in its dock.
#[component]
pub fn ArmorDock(workspace: ArmorWorkspace) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let locale = workspace.services.locale;

    view! {
        Dock(
            state = workspace.dock,
            close_label = Signal::derive_local(locale.text("ui.tabs.close_panel")),
            tabs_label = Signal::derive_local(locale.text("ui.armor.viewports")),
            menu = dock_menu_strings(locale),
            class = "wt-armor-dock",
            attr:data-testid = "armor-viewport-dock"
        )
    }
}

/// Makes the dock's panes: each tab is named for the ship or model its pane shows, and holds the
/// pane's viewport. A pane's view is built again from the workspace whenever the dock builds its
/// panel again, so a pane moved to another group keeps its state.
pub(super) fn register(workspace: ArmorWorkspace) {
    let locale = workspace.services.locale;
    workspace.dock.register(
        PanelKind::new(PANE_KIND),
        PanelFactory::new(
            move |id: &PanelId| {
                locale.track();
                let shown = PaneId::from_panel(id)
                    .and_then(|pane| workspace.pane_untracked(pane))
                    .and_then(|viewport| viewport.shown());

                match shown {
                    Some(armor) => armor.display_name.clone(),
                    None => t!("ui.armor.empty").into_owned(),
                }
            },
            move |id: &PanelId| match PaneId::from_panel(id).and_then(|pane| workspace.find_pane(pane)) {
                Some(pane) => AnyView::new(view! { DockPane(workspace = workspace, pane = pane) }),
                None => AnyView::new(()),
            },
        ),
    );
}

/// One pane's viewport. A primary or secondary press inside it makes it the active pane; a middle
/// press only pans.
#[component]
fn DockPane(workspace: ArmorWorkspace, pane: Pane) -> impl IntoView {
    let id = pane.id;
    let single = move || workspace.panes().len() <= 1;
    let active = move || !single() && workspace.active_id() == id;
    let panel = id.panel_id().as_str().to_owned();

    view! {
        box(
            class = "wt-armor-pane",
            attr:data-testid = "armor-viewport-pane",
            attr:data-panel = panel,
            attr:data-single = move || single().then(String::new),
            attr:data-active = move || active().then(String::new),
            on:pointer_down:capture = move |ev| {
                if matches!(ev.button, Some(PointerButton::Primary | PointerButton::Secondary)) {
                    workspace.activate(id);
                }
            }
        ) {
            ArmorViewport(state = pane.viewport)
        }
    }
}

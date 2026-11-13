//! The tab's panes, laid out in resizable splits. One pane fills the area with no border. Once a
//! comparison pane is added, the panes share it along the split and the active pane gets an accent
//! border. Every pane carries a close button; closing the last one empties it.

use landaire_ui::TooltipContentProps;
use landaire_ui::TooltipProps;
use landaire_ui::TooltipTriggerProps;
use rust_i18n::t;
use zgui::component;
use zgui::css;
use zgui::reactive::prelude::Get;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui::vocab::PointerButton;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::ResizableHandleProps;
use zgui_ui::ResizablePanelGroupProps;
use zgui_ui::ResizablePanelProps;
use zgui_ui_primitives::Orientation;

use crate::icons::AppIconProps;
use crate::tabs::armor_viewer::viewport::ArmorViewportProps;
use crate::tabs::armor_viewer::workspace::ArmorWorkspace;
use crate::tabs::armor_viewer::workspace::Pane;
use crate::tabs::armor_viewer::workspace::PaneSplit;

const SHEET: &str = "wt-armor-dock";

const CSS: &str = css!(
    ".wt-armor-dock { display: flex; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .wt-armor-dock__group { flex: 1 1 auto; min-width: 0; min-height: 0; }
    .wt-armor-pane {
        position: relative;
        display: flex;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
        box-sizing: border-box;
        border: 2px solid transparent;
    }
    .wt-armor-pane[data-single] { border-width: 0; }
    .wt-armor-pane[data-active] { border-color: var(--lui-color-accent); }
    /* Small and in the corner, clear of the axis gizmo below it. */
    .wt-armor-pane__close { position: absolute; top: 2px; right: 2px; }"
);

/// Every pane of `workspace`, split as it says.
#[component]
pub fn ArmorDock(workspace: ArmorWorkspace) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    // A group is built for one direction and one set of panes: the dividers register in the order
    // they are made, so a pane coming or going, or a change of split, builds the group again.
    let group = move || {
        let direction = match workspace.split.get() {
            PaneSplit::SideBySide => Orientation::Horizontal,
            PaneSplit::Stacked => Orientation::Vertical,
        };
        let panes = workspace.panes();
        AnyView::new(view! {
            ResizablePanelGroup(
                direction = direction,
                class = "wt-armor-dock__group",
                attr:data-testid = "armor-viewport-dock"
            ) {
                {pane_views(workspace, panes)}
            }
        })
    };

    view! {
        box(class = "wt-armor-dock") {
            {group}
        }
    }
}

/// Each pane in a panel of an even share, with a divider between each two. Called inside the
/// group, which the panels and dividers register with as they are made.
fn pane_views(workspace: ArmorWorkspace, panes: Vec<Pane>) -> Vec<AnyView> {
    let share = 100.0 / panes.len().max(1) as f64;
    panes
        .into_iter()
        .enumerate()
        .flat_map(|(ix, pane)| {
            let handle = (ix > 0).then(|| AnyView::new(view! { ResizableHandle() }));
            let panel = AnyView::new(view! {
                ResizablePanel(default_size = share, min_pixels = 120.0) {
                    DockPane(workspace = workspace, pane = pane)
                }
            });
            handle.into_iter().chain(std::iter::once(panel))
        })
        .collect()
}

/// One pane: its viewport and close button. A primary or secondary press inside it makes it the
/// active pane; a middle press only pans.
#[component]
fn DockPane(workspace: ArmorWorkspace, pane: Pane) -> impl IntoView {
    let id = pane.id;
    let single = move || workspace.panes().len() <= 1;
    let active = move || !single() && workspace.active_id() == id;

    view! {
        box(
            class = "wt-armor-pane",
            attr:data-testid = "armor-viewport-pane",
            attr:data-single = move || single().then(String::new),
            attr:data-active = move || active().then(String::new),
            on:pointer_down:capture = move |ev| {
                if matches!(ev.button, Some(PointerButton::Primary | PointerButton::Secondary)) {
                    workspace.activate(id);
                }
            }
        ) {
            ArmorViewport(state = pane.viewport)
            box(class = "wt-armor-pane__close") {
                Tooltip {
                    TooltipTrigger {
                        Button(
                            variant = ButtonVariant::Ghost,
                            size = ButtonSize::IconXs,
                            attr:data-testid = "armor-viewport-pane-close",
                            a11y:label = t!("ui.armor.close_pane").into_owned(),
                            on:click = move |_| workspace.close_pane(id)
                        ) {
                            AppIcon(glyph = wows_toolkit_icons::Glyph::X)
                        }
                    }
                    TooltipContent {{t!("ui.armor.close_pane").into_owned()}}
                }
            }
        }
    }
}

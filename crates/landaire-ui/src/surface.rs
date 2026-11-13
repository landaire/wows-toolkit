//! The surfaces an interface is built from, as classes over the `--lui-*` tokens and as small
//! components that apply them.

use zgui::component;
use zgui::css;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::Children;
use zgui::view::Classes;
use zgui::view::IntoView;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonProps;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui_icons::IconData;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::IconSize;

use crate::tooltip::TooltipContentProps;
use crate::tooltip::TooltipProps;
use crate::tooltip::TooltipTriggerProps;

/// What the surface rules are installed under.
pub const SHEET: &str = "landaire-ui-surfaces";

/// The window ground with an inset panel in it.
pub const WORKSPACE_CLASS: &str = "lui-workspace";
/// The panel inside a [`WORKSPACE_CLASS`] element.
pub const WORKSPACE_INSET_CLASS: &str = "lui-workspace__inset";
/// One surface step above the panel, edged by a 1px rule.
pub const CARD_CLASS: &str = "lui-card";
/// A popover or tooltip surface an application builds itself.
pub const OVERLAY_CLASS: &str = "lui-overlay";
/// A container whose children are rows, every other one lifted starting with the first.
pub const STRIPED_CLASS: &str = "lui-striped";
/// One lifted row, for rows whose position among their siblings is not their index, such as a
/// virtualized list.
pub const STRIPE_CLASS: &str = "lui-stripe";
/// Figures of one width, so numbers in a column line up digit for digit.
pub const TABULAR_CLASS: &str = "lui-tabular";

pub const CSS: &str = css!(
    ".lui-workspace {
        display: flex;
        flex: 1 1 auto;
        box-sizing: border-box;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        padding: 4px;
        background-color: var(--lui-color-surface);
    }
    .lui-workspace__inset {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        border: 1px solid var(--lui-color-border);
        border-radius: 6px;
        background-color: var(--lui-color-panel);
        color: var(--lui-color-text);
    }
    .lui-card {
        background-color: var(--lui-color-card);
        border: 1px solid var(--lui-color-border);
        border-radius: var(--lui-radius-lg);
        box-shadow: var(--lui-shadow-card);
    }
    .lui-overlay {
        background-color: var(--lui-color-card);
        color: var(--lui-color-text);
        border: 1px solid var(--lui-color-border);
        border-radius: var(--lui-radius-lg);
        box-shadow: var(--lui-shadow-overlay);
    }
    .lui-striped > :nth-child(odd), .lui-stripe {
        background-color: var(--lui-color-faint);
    }
    .lui-indent-guides {
        display: flex;
        flex-direction: row;
        flex: none;
        align-self: stretch;
    }
    .lui-indent-guide {
        flex: none;
        border-right: 1px solid color-mix(in srgb, var(--lui-color-border) 70%, transparent);
    }
    .lui-expand-rail {
        display: flex;
        flex: none;
        padding: 4px 8px;
    }
    .lui-tabular {
        font-variant-numeric: tabular-nums;
    }"
);

/// A row's position in its list or table, from 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RowIndex(pub usize);

impl RowIndex {
    /// Whether the row takes [`STRIPE_CLASS`]: every other row, starting with the first.
    pub fn is_striped(self) -> bool {
        self.0.is_multiple_of(2)
    }
}

/// How many levels below the top of a tree a row sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TreeDepth(pub usize);

/// How far one tree level indents, in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct IndentWidth(pub f32);

/// How tall one row is, in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct RowHeight(pub f32);

/// The edge of the window a collapsible side panel is docked against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PanelSide {
    Left,
    Right,
    Bottom,
}

impl PanelSide {
    fn collapse_icon(self) -> IconData {
        match self {
            Self::Left => PANEL_LEFT_CLOSE,
            Self::Right => PANEL_RIGHT_CLOSE,
            Self::Bottom => PANEL_BOTTOM_CLOSE,
        }
    }

    fn expand_icon(self) -> IconData {
        match self {
            Self::Left => PANEL_LEFT_OPEN,
            Self::Right => PANEL_RIGHT_OPEN,
            Self::Bottom => PANEL_BOTTOM_OPEN,
        }
    }
}

/// The window ground with one inset, rounded panel in it.
#[component]
pub fn WorkspaceInset(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        box(class = WORKSPACE_CLASS) {
            box(class = WORKSPACE_INSET_CLASS, {..attrs}, class = class) {
                {children.into_view_once()}
            }
        }
    }
}

/// See [`CARD_CLASS`].
#[component]
pub fn CardSurface(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        box(class = CARD_CLASS, {..attrs}, class = class) {
            {children.into_view_once()}
        }
    }
}

/// See [`OVERLAY_CLASS`].
#[component]
pub fn OverlaySurface(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        box(class = OVERLAY_CLASS, {..attrs}, class = class) {
            {children.into_view_once()}
        }
    }
}

/// A column of rows, every other one lifted starting with the first. A long table of figures is
/// read across a row, and the lift keeps the eye on one.
#[component]
pub fn StripedRows(
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
    children: Children,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        column(class = STRIPED_CLASS, {..attrs}, class = class) {
            {children.into_view_once()}
        }
    }
}

/// One hairline per level for a tree row at `depth`, at the right edge of each level's indent,
/// so a deep row reads as belonging to the group above it.
#[component]
pub fn IndentGuides(depth: TreeDepth, indent: IndentWidth) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    let width = format!("{}px", indent.0);
    let guides: Vec<AnyView> = (0..depth.0)
        .map(|_| AnyView::new(view! { box(class = "lui-indent-guide", style:width = width.clone()) }))
        .collect();

    view! {
        row(class = "lui-indent-guides", a11y:hidden = true) {
            {guides}
        }
    }
}

/// The button in a side panel's header that collapses the panel.
///
/// Pair it with [`ExpandRail`] for the same `side`: the rail puts the expand control where this
/// button was, so collapsing does not move the control under the pointer.
#[component]
pub fn CollapseButton(
    side: PanelSide,
    /// Names the button for a screen reader and in its tooltip.
    #[prop(into)]
    label: String,
    /// Forwarded to the button, including its `on:click`.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    panel_toggle(side.collapse_icon(), label, attrs)
}

/// The narrow strip a collapsed side panel leaves behind, holding the control that expands it.
#[component]
pub fn ExpandRail(
    side: PanelSide,
    /// Names the button for a screen reader and in its tooltip.
    #[prop(into)]
    label: String,
    /// Forwarded to the button, including its `on:click`.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);

    view! {
        box(class = "lui-expand-rail") {
            {panel_toggle(side.expand_icon(), label, attrs)}
        }
    }
}

fn panel_toggle(icon: IconData, label: String, attrs: Attrs) -> impl IntoView {
    let tip = label.clone();

    view! {
        Tooltip {
            TooltipTrigger {
                Button(variant = ButtonVariant::Ghost, size = ButtonSize::IconSm, a11y:label = label, {..attrs}) {
                    Icon(icon = icon, size = IconSize::Sm)
                }
            }
            TooltipContent {{tip.clone()}}
        }
    }
}

/// A rounded frame whose inner edge winds against the outer one, so its inside stays empty. The
/// panel glyphs add a rule at the docked edge and a triangle pointing the way the panel moves,
/// both wound with the outer edge so they fill.
macro_rules! panel_frame {
    () => {
        "M5 3 L19 3 C20.105 3 21 3.895 21 5 L21 19 C21 20.105 20.105 21 19 21 L5 21 \
         C3.895 21 3 20.105 3 19 L3 5 C3 3.895 3.895 3 5 3 Z \
         M6 5 C5.448 5 5 5.448 5 6 L5 18 C5 18.552 5.448 19 6 19 L18 19 \
         C18.552 19 19 18.552 19 18 L19 6 C19 5.448 18.552 5 18 5 Z"
    };
}

const PANEL_LEFT_CLOSE: IconData = IconData::new(
    "panel-left-close",
    24.0,
    concat!(panel_frame!(), " M8 5 L10 5 L10 19 L8 19 Z M16 9 L16 15 L13 12 Z"),
);

const PANEL_LEFT_OPEN: IconData =
    IconData::new("panel-left-open", 24.0, concat!(panel_frame!(), " M8 5 L10 5 L10 19 L8 19 Z M13 9 L16 12 L13 15 Z"));

const PANEL_RIGHT_CLOSE: IconData = IconData::new(
    "panel-right-close",
    24.0,
    concat!(panel_frame!(), " M14 5 L16 5 L16 19 L14 19 Z M8 9 L11 12 L8 15 Z"),
);

const PANEL_RIGHT_OPEN: IconData = IconData::new(
    "panel-right-open",
    24.0,
    concat!(panel_frame!(), " M14 5 L16 5 L16 19 L14 19 Z M11 9 L11 15 L8 12 Z"),
);

const PANEL_BOTTOM_CLOSE: IconData = IconData::new(
    "panel-bottom-close",
    24.0,
    concat!(panel_frame!(), " M5 14 L19 14 L19 16 L5 16 Z M9 8 L15 8 L12 11 Z"),
);

const PANEL_BOTTOM_OPEN: IconData = IconData::new(
    "panel-bottom-open",
    24.0,
    concat!(panel_frame!(), " M5 14 L19 14 L19 16 L5 16 Z M12 8 L15 11 L9 11 Z"),
);

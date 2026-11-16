//! A dock drawn: the folding regions around the center, each region's splits, and each split's tab
//! groups.
//!
//! Each group's tabs are a [`TabStrip`](crate::TabStrip), as an application's own tabs are.

use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::Owner;
use zgui::reactive::RenderEffect;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::provide_local_context;
use zgui::reactive::use_local_context;
use zgui::view;
use zgui::view::AnyView;
use zgui::view::Attrs;
use zgui::view::Children;
use zgui::view::Classes;
use zgui::view::ForProps;
use zgui::view::IntoView;
use zgui::view::NodeRef;
use zgui::view::ShowProps;
use zgui::view::install_stylesheet;
use zgui_ui::ButtonSize;
use zgui_ui::ButtonVariant;
use zgui_ui::DropdownMenuContentProps;
use zgui_ui::DropdownMenuProps;
use zgui_ui::DropdownMenuTriggerProps;
use zgui_ui::MenuItemProps;
use zgui_ui::MenuSeparatorProps;
use zgui_ui::PanelBound;
use zgui_ui::ResizableContext;
use zgui_ui::ResizableHandleProps;
use zgui_ui::ResizablePanelGroupProps;
use zgui_ui::ResizablePanelProps;
use zgui_ui::TabsContentProps;
use zgui_ui_icons::IconProps;
use zgui_ui_icons::set::ui::ELLIPSIS;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Orientation;

use crate::chrome::tab_strip::TabStripEnd;
use crate::chrome::tab_strip::TabStripPanels;
use crate::chrome::tab_strip::TabStripProps;
use crate::chrome::tab_strip::TabStripStrings;
use crate::chrome::tab_strip::TabStripTabProps;
use crate::dock::drag::Area;
use crate::dock::drag::DragContext;
use crate::dock::drag::DragLayerProps;
use crate::dock::drag::draggable;
use crate::dock::kept;
use crate::dock::kept::KeptView;
use crate::dock::layout::CssLength;
use crate::dock::layout::DockRegion;
use crate::dock::layout::FoldingLimits;
use crate::dock::layout::FoldingRegion;
use crate::dock::layout::GroupId;
use crate::dock::layout::Layout;
use crate::dock::layout::NodeShape;
use crate::dock::layout::PanelEntry;
use crate::dock::layout::PanelId;
use crate::dock::layout::RegionExtent;
use crate::dock::layout::Share;
use crate::dock::layout::SplitAxis;
use crate::dock::layout::SplitId;
use crate::dock::state::DockState;
use crate::dock::state::LayoutGeneration;

/// What the dock rules are installed under.
const SHEET: &str = "landaire-ui-dock";

const CSS: &str = css!(
    ".lui-dock {
        position: relative;
        display: flex;
        flex: 1 1 auto;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background-color: var(--lui-color-surface);
    }
    .lui-dock .lui-dock__group[data-zoomed] {
        position: absolute;
        top: 0;
        right: 0;
        bottom: 0;
        left: 0;
        z-index: 20;
    }
    .lui-dock .lui-dock__group[data-covered] { display: none; }
    .lui-dock__slot {
        display: flex;
        flex-direction: column;
        flex: 0 0 auto;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
    }
    .lui-dock__slot-hidden > * { display: none; }
    .lui-dock .lui-dock__handle-hidden { display: none; }
    .lui-dock__region { display: flex; flex-direction: column; flex: 1 1 auto; min-width: 0; min-height: 0; }
    .lui-dock .lui-dock__group {
        flex: 1 1 auto;
        gap: 0;
        min-width: 0;
        min-height: 0;
        background-color: var(--lui-color-panel);
        color: var(--lui-color-text);
    }
    .lui-dock .lui-dock__group .lui-tab-strip__scroller,
    .lui-dock .lui-dock__group .lui-tab-strip__bar {
        flex: none;
        border-bottom: 1px solid var(--lui-color-border);
        background-color: var(--lui-color-surface);
    }
    .lui-dock .lui-dock__group .lui-tab-strip__bar .lui-tab-strip__scroller {
        border-bottom: none;
        background-color: transparent;
    }
    .lui-dock .lui-tab-strip__list .lui-tab-strip__tab.zui-tabs__trigger[data-state=\"active\"]:first-child {
        border-left-color: transparent;
    }
    .lui-dock .lui-dock__group-single .lui-tab-strip__scroller,
    .lui-dock .lui-dock__group-single .lui-tab-strip__bar {
        border-bottom: none;
        background-color: transparent;
    }
    .lui-dock .lui-dock__group-single .lui-tab-strip__underline { display: none; }
    .lui-dock .lui-dock__group-single .lui-tab-strip__list .lui-tab-strip__tab.zui-tabs__trigger[data-state=\"active\"] {
        border-color: transparent;
        background-color: transparent;
        color: var(--lui-color-text);
        font-weight: 400;
    }
    .lui-dock__menu.zui-button {
        margin: 0 4px;
        color: var(--lui-color-text-dim);
    }
    .lui-dock__body {
        display: flex;
        flex-direction: column;
        flex: 1 1 auto;
        min-height: 0;
        padding-top: 8px;
        overflow: hidden;
    }
    .lui-dock__group-single .lui-dock__body { padding-top: 0; }
    .lui-dock__content.lui-dock__content { flex: 1 1 auto; min-height: 0; overflow-x: hidden; overflow-y: auto; }
    .lui-dock .zui-resizable__line::after {
        content: \"\";
        position: absolute;
        left: -1px;
        top: 50%;
        width: 3px;
        height: 20px;
        margin-top: -10px;
        border-radius: 999px;
        background-color: var(--lui-color-text-dim);
        opacity: 0;
        transition: opacity var(--lui-motion-fade) var(--lui-motion-ease);
    }
    .lui-dock .zui-resizable__handle:hover > .zui-resizable__line::after { opacity: 0.35; }
    .lui-dock .zui-resizable__handle[data-dragging] > .zui-resizable__line::after {
        height: 44px;
        margin-top: -22px;
        opacity: 0.9;
    }
    .lui-dock .zui-resizable[data-direction=\"vertical\"] > .zui-resizable__handle > .zui-resizable__line::after {
        left: 50%;
        top: -1px;
        width: 20px;
        height: 3px;
        margin-left: -10px;
        margin-top: 0;
    }
    .lui-dock .zui-resizable[data-direction=\"vertical\"] > .zui-resizable__handle[data-dragging] > .zui-resizable__line::after {
        width: 44px;
        margin-left: -22px;
        margin-top: 0;
    }
    .lui-dock .lui-tab-strip__tab[data-lifted] { opacity: 0.5; cursor: grabbing; }
    .lui-dock__landing, .lui-dock__gap, .lui-dock__ghost {
        position: absolute;
        z-index: 40;
        pointer-events: none;
        box-sizing: border-box;
    }
    .lui-dock__landing {
        border: 1px solid color-mix(in srgb, var(--lui-color-accent) 70%, transparent);
        border-radius: 4px;
        background-color: color-mix(in srgb, var(--lui-color-accent) 20%, transparent);
    }
    .lui-dock__gap {
        border-radius: 1px;
        background-color: var(--lui-color-accent);
    }
    .lui-dock__ghost {
        z-index: 41;
        max-width: 220px;
        padding: 4px 12px;
        overflow: hidden;
        white-space: nowrap;
        text-overflow: ellipsis;
        border: 1px solid var(--lui-color-border-bright);
        border-radius: 6px;
        background-color: var(--lui-color-panel);
        color: var(--lui-color-text);
        opacity: 0.85;
    }"
);

/// The words of the menu at the end of each tab group's strip, which draws the group over the
/// whole dock and closes the panel it shows.
#[derive(Clone, Copy, Debug)]
pub struct DockMenuStrings {
    /// What the menu's button is called, for a screen reader.
    pub menu: Signal<String, LocalStorage>,
    pub zoom_in: Signal<String, LocalStorage>,
    pub zoom_out: Signal<String, LocalStorage>,
    pub close: Signal<String, LocalStorage>,
}

impl DockMenuStrings {
    /// The words in English.
    pub fn new() -> Self {
        Self { menu: "More".into(), zoom_in: "Zoom In".into(), zoom_out: "Zoom Out".into(), close: "Close".into() }
    }
}

impl Default for DockMenuStrings {
    fn default() -> Self {
        Self::new()
    }
}

/// What the dock's tab strips say to a screen reader, and the words of their context menus.
#[derive(Clone, Copy)]
struct DockLabels {
    /// Said before a panel's label to name its tab's close button.
    close: Signal<String, LocalStorage>,
    /// What each group's strip of tabs is called.
    tabs: Signal<String, LocalStorage>,
    strings: TabStripStrings,
    /// The words of each group's menu, or `None` for groups without one.
    menu: Option<DockMenuStrings>,
}

impl DockLabels {
    /// What a dock says when it is not told otherwise.
    fn english() -> Self {
        Self { close: "Close".into(), tabs: "Panels".into(), strings: TabStripStrings::new(), menu: None }
    }

    /// The labels of the dock the calling scope is in.
    fn current() -> Self {
        use_local_context::<Self>().unwrap_or_else(Self::english)
    }
}

/// The places in the dock's frame: three columns, and the center and bottom within the middle one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Slot {
    Left,
    Middle,
    Right,
    Center,
    Bottom,
}

impl Slot {
    /// The folding region the slot holds, or `None` for a slot that takes what is left.
    fn folding(self) -> Option<FoldingRegion> {
        match self {
            Self::Left => Some(FoldingRegion::Left),
            Self::Right => Some(FoldingRegion::Right),
            Self::Bottom => Some(FoldingRegion::Bottom),
            Self::Middle | Self::Center => None,
        }
    }
}

/// The slots of the frame's row, and of the column in its middle.
const ROW: [Slot; 3] = [Slot::Left, Slot::Middle, Slot::Right];
const COLUMN: [Slot; 2] = [Slot::Center, Slot::Bottom];

/// The least share a folding region keeps while it shows.
const FLOOR: f64 = 5.0;

/// Which slots of the frame take room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Showing {
    left: bool,
    middle: bool,
    right: bool,
    center: bool,
    bottom: bool,
}

impl Showing {
    fn of(layout: &Layout) -> Self {
        let center = layout.shows(DockRegion::Center);
        let bottom = layout.shows(DockRegion::Bottom);

        Self {
            left: layout.shows(DockRegion::Left),
            middle: center || bottom,
            right: layout.shows(DockRegion::Right),
            center,
            bottom,
        }
    }

    fn slot(self, slot: Slot) -> bool {
        match slot {
            Slot::Left => self.left,
            Slot::Middle => self.middle,
            Slot::Right => self.right,
            Slot::Center => self.center,
            Slot::Bottom => self.bottom,
        }
    }

    /// Whether the room `slot` is given is its own share: it shows beside the middle, and a
    /// bottom slot under the center.
    fn owns_share(self, slot: Slot) -> bool {
        self.slot(slot) && self.middle && (slot != Slot::Bottom || self.center)
    }
}

/// The length of a resizable group along its axis, in CSS pixels. Unknown until it is laid out.
#[derive(Clone, Copy, Debug, PartialEq)]
struct GroupLength(Option<f64>);

impl GroupLength {
    /// The share of the group `length` takes.
    fn share_of(self, length: CssLength) -> Option<f64> {
        let group = self.0.filter(|group| *group > 0.0)?;

        Some(length.pixels() / group * 100.0)
    }
}

/// How small and large the user may drag `slot` while it shows.
fn bound(layout: &Layout, slot: Slot, length: GroupLength) -> PanelBound {
    let Some(region) = slot.folding() else {
        return PanelBound::new(0.0, 100.0);
    };
    let limits = layout.limits.get(region);
    let min = limits.min.and_then(|min| length.share_of(min)).map_or(FLOOR, |min| min.max(FLOOR)).min(100.0);
    let max = limits.max.and_then(|max| length.share_of(max)).map_or(100.0, |max| max.min(100.0));

    PanelBound::new(min, max.max(min))
}

/// The share `slot` asks for while it shows, or `None` for the slot that takes what is left.
fn extent(layout: &Layout, slot: Slot, length: GroupLength) -> Option<f64> {
    let region = slot.folding()?;
    let share = match layout.extents.get(region) {
        RegionExtent::Share(share) => share.percent(),
        // Until the frame is laid out a length is shown at the region's default share.
        RegionExtent::Length(pixels) => length.share_of(pixels).unwrap_or(region.default_share().percent()),
    };

    Some(share)
}

/// The share each of `slots` takes, adding up to 100: a showing slot its extent inside its bound,
/// the slot without one what the others leave, and a hidden slot nothing. Extents that add up to
/// more than the whole, or that fill it with no slot left to take the rest, are scaled to fit.
fn targets(layout: &Layout, showing: Showing, slots: &[Slot], length: GroupLength) -> Vec<f64> {
    let mut sizes: Vec<Option<f64>> = slots
        .iter()
        .map(|slot| match extent(layout, *slot, length) {
            Some(extent) if showing.slot(*slot) => {
                let bound = bound(layout, *slot, length);
                Some(extent.clamp(bound.min, bound.max))
            }
            Some(_) => Some(0.0),
            None => None,
        })
        .collect();
    let fixed: f64 = sizes.iter().flatten().sum();
    let rest_shows = slots.iter().any(|slot| slot.folding().is_none() && showing.slot(*slot));

    if fixed > 100.0 || (!rest_shows && fixed > 0.0) {
        for size in sizes.iter_mut().flatten() {
            *size *= 100.0 / fixed;
        }
    }
    let fixed: f64 = sizes.iter().flatten().sum();
    // With nothing showing, the slot without an extent still takes the whole, so the group adds up.
    let rest = if rest_shows || fixed <= 0.0 { (100.0 - fixed).max(0.0) } else { 0.0 };

    sizes.into_iter().map(|size| size.unwrap_or(rest)).collect()
}

/// The bound each of `slots` is held to: its own while it shows, and zero while it does not.
fn bounds(layout: &Layout, showing: Showing, slots: &[Slot], length: GroupLength) -> Vec<PanelBound> {
    slots
        .iter()
        .map(|slot| if showing.slot(*slot) { bound(layout, *slot, length) } else { PanelBound::new(0.0, 0.0) })
        .collect()
}

/// Where the frame puts the panels of one group, and how far each may be dragged.
#[derive(Clone, Debug, PartialEq)]
struct Placement {
    targets: Vec<f64>,
    bounds: Vec<PanelBound>,
}

impl Placement {
    fn of(layout: &Layout, showing: Showing, slots: &[Slot], length: GroupLength) -> Self {
        Self { targets: targets(layout, showing, slots, length), bounds: bounds(layout, showing, slots, length) }
    }
}

/// Where the frame places the folding `region` of `layout` in `dock`, inside its limits and beside
/// the regions that show.
pub(crate) fn region_area(layout: &Layout, region: FoldingRegion, dock: Area) -> Area {
    let showing = Showing::of(layout);
    let row = targets(layout, showing, &ROW, GroupLength(Some(dock.width)));
    let part = |share: f64| share / 100.0;

    match region {
        FoldingRegion::Left => Area { width: dock.width * part(row[0]), ..dock },
        FoldingRegion::Right => {
            let width = dock.width * part(row[2]);
            Area { left: dock.left + dock.width - width, width, ..dock }
        }
        FoldingRegion::Bottom => {
            let column = targets(layout, showing, &COLUMN, GroupLength(Some(dock.height)));
            let height = dock.height * part(column[1]);
            Area {
                left: dock.left + dock.width * part(row[0]),
                top: dock.top + dock.height - height,
                width: dock.width * part(row[1]),
                height,
            }
        }
    }
}

/// What the frame sizes a group for. A change to any of it places the group's panels again.
#[derive(Clone, Debug, PartialEq)]
struct Sizing {
    showing: Showing,
    length: GroupLength,
    limits: FoldingLimits,
    extents: Vec<RegionExtent>,
}

/// Turns each length the folding regions among `slots` were given into the share of the group
/// it is placed at for `length`, inside its limits, so the regions keep their proportions from
/// then on. A region whose room is not its own share keeps the share its length is of the group.
fn settle_lengths(layout: &mut Layout, showing: Showing, slots: &[Slot], length: GroupLength) {
    if length.0.is_none_or(|group| group <= 0.0) {
        return;
    }

    let placed = targets(layout, showing, slots, length);
    for (slot, placed) in slots.iter().zip(placed) {
        let Some(region) = slot.folding() else {
            continue;
        };
        let RegionExtent::Length(pixels) = layout.extents.get(region) else {
            continue;
        };
        let share = if showing.owns_share(*slot) { Some(placed) } else { length.share_of(pixels) };
        if let Some(share) = share.and_then(|share| Share::new(share.min(100.0)).ok()) {
            layout.extents.set(region, RegionExtent::Share(share));
        }
    }
}

/// The panels of one resizable group of the frame, as they registered, whether their sizes are
/// being set by the frame rather than by the user, and the extents a drag last wrote.
#[derive(Clone, Copy)]
struct FrameGroup {
    slots: &'static [Slot],
    members: StoredValue<Vec<(Slot, u64)>, LocalStorage>,
    applying: StoredValue<bool, LocalStorage>,
    recorded: StoredValue<Option<Vec<RegionExtent>>, LocalStorage>,
}

impl FrameGroup {
    fn new(slots: &'static [Slot]) -> Self {
        Self {
            slots,
            members: StoredValue::new_local(Vec::new()),
            applying: StoredValue::new_local(false),
            recorded: StoredValue::new_local(None),
        }
    }

    /// The extents of the group's folding slots, in slot order.
    fn extents(self, layout: &Layout) -> Vec<RegionExtent> {
        self.slots.iter().filter_map(|slot| slot.folding()).map(|region| layout.extents.get(region)).collect()
    }

    /// Moves every panel to its target and leaves each with its bound.
    ///
    /// Each panel is pinned at its target in turn while the ones after it are free, so the group's
    /// own arithmetic gives the difference to the free ones and the last lands where it should.
    fn apply(self, context: ResizableContext, placement: &Placement) {
        self.applying.set_value(true);

        let members = self.members.get_value();
        for (_, id) in &members {
            context.set_bound(*id, PanelBound::new(0.0, 100.0));
        }
        for ((_, id), target) in members.iter().zip(&placement.targets) {
            context.set_bound(*id, PanelBound::new(*target, *target));
        }
        for ((_, id), bound) in members.iter().zip(&placement.bounds) {
            context.set_bound(*id, *bound);
        }

        self.applying.set_value(false);
    }

    /// Writes the shares the user dragged the folding slots to back into the layout.
    fn record(self, state: DockState, sizes: &[f64]) {
        if self.applying.get_value() {
            return;
        }

        let members = self.members.get_value();
        state.layout.update(|layout| {
            let showing = Showing::of(layout);
            for ((slot, _), size) in members.iter().zip(sizes) {
                let Some(region) = slot.folding() else {
                    continue;
                };
                // A side's share is only its own while the middle beside it shows.
                if !showing.owns_share(*slot) {
                    continue;
                }
                if let Ok(share) = Share::new(size.clamp(0.0, 100.0)) {
                    layout.extents.set(region, RegionExtent::Share(share));
                }
            }
            self.recorded.set_value(Some(self.extents(layout)));
        });
    }
}

/// Lays out a [`DockState`]: a left, center, right and bottom region, the side ones folding away
/// when closed or empty, each holding splits of tab groups.
///
/// A panel's view is built once and kept for as long as the panel is in the dock: while its tab
/// is not shown, while its region is closed, and when it moves to another group or region.
/// [`restore`](DockState::restore) builds every view again. One `Dock` draws a state at a time.
#[component]
pub fn Dock(
    state: DockState,
    /// Said before a panel's label to name its tab's close button. "Close" when not given.
    #[prop(into, optional)]
    close_label: Option<Signal<String, LocalStorage>>,
    /// What each group's strip of tabs is called. "Panels" when not given.
    #[prop(into, optional)]
    tabs_label: Option<Signal<String, LocalStorage>>,
    /// The words of each tab's context menu. English when not given.
    #[prop(optional)]
    strings: Option<TabStripStrings>,
    /// Gives each tab group a menu at the end of its strip, in these words. No menu when not
    /// given.
    #[prop(optional)]
    menu: Option<DockMenuStrings>,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    let english = DockLabels::english();
    provide_local_context(DockLabels {
        close: close_label.unwrap_or(english.close),
        tabs: tabs_label.unwrap_or(english.tabs),
        strings: strings.unwrap_or(english.strings),
        menu,
    });
    // Panel views are built in this scope, so they outlast the groups that show them.
    state.kept.update_value(|kept| kept.set_scope(Owner::current()));
    on_cleanup_local(move || kept::clear(state.kept));
    let drag = DragContext::new();
    provide_local_context(drag);

    view! {
        box(class = "lui-dock", node_ref = drag.dock, {..attrs}, class = class) {
            for _generation in move || vec![state.generation.get()], key = |generation: &LayoutGeneration| *generation {
                DockFrame(state = state)
            }
            DragLayer(drag = drag)
        }
    }
}

#[component]
fn DockFrame(state: DockState) -> impl IntoView {
    let showing = Memo::new(move |_| state.layout.with(Showing::of));
    let row = FrameGroup::new(&ROW);
    let column = FrameGroup::new(&COLUMN);
    let (row_start, column_start) = state.layout.with_untracked(|layout| {
        let now = Showing::of(layout);
        (
            Placement::of(layout, now, row.slots, GroupLength(None)),
            Placement::of(layout, now, column.slots, GroupLength(None)),
        )
    });
    let start = |placement: &Placement, index: usize| (placement.targets[index], placement.bounds[index]);
    let (left_share, left_bound) = start(&row_start, 0);
    let (middle_share, middle_bound) = start(&row_start, 1);
    let (right_share, right_bound) = start(&row_start, 2);
    let (center_share, center_bound) = start(&column_start, 0);
    let (bottom_share, bottom_bound) = start(&column_start, 1);

    // A zoomed group covers the frame, so its handles leave the focus order with the other groups.
    let zoomed = move || state.zoomed().is_some();
    let hidden_left = move || zoomed() || !showing.with(|showing| showing.left && showing.middle);
    let hidden_right = move || zoomed() || !showing.with(|showing| showing.right && showing.middle);
    let hidden_bottom = move || zoomed() || !showing.with(|showing| showing.center && showing.bottom);

    view! {
        ResizablePanelGroup(
            direction = Orientation::Horizontal,
            on_change = UnsyncCallback::new(move |sizes: Vec<f64>| row.record(state, &sizes))
        ) {
            FrameSlot(place = Slot::Left, group = row, showing = showing, share = left_share, bound = left_bound) {
                RegionView(state = state, region = DockRegion::Left)
            }
            ResizableHandle(class:lui-dock__handle-hidden = hidden_left)
            FrameSlot(place = Slot::Middle, group = row, showing = showing, share = middle_share, bound = middle_bound) {
                ResizablePanelGroup(
                    direction = Orientation::Vertical,
                    on_change = UnsyncCallback::new(move |sizes: Vec<f64>| column.record(state, &sizes))
                ) {
                    FrameSlot(place = Slot::Center, group = column, showing = showing, share = center_share, bound = center_bound) {
                        RegionView(state = state, region = DockRegion::Center)
                    }
                    ResizableHandle(class:lui-dock__handle-hidden = hidden_bottom)
                    FrameSlot(place = Slot::Bottom, group = column, showing = showing, share = bottom_share, bound = bottom_bound) {
                        RegionView(state = state, region = DockRegion::Bottom)
                    }
                    FrameSizer(state = state, group = column, showing = showing)
                }
            }
            ResizableHandle(class:lui-dock__handle-hidden = hidden_right)
            FrameSlot(place = Slot::Right, group = row, showing = showing, share = right_share, bound = right_bound) {
                RegionView(state = state, region = DockRegion::Right)
            }
            FrameSizer(state = state, group = row, showing = showing)
        }
    }
}

/// One panel of the frame's resizable groups. It registers with the group itself rather than
/// through `ResizablePanel`, so the frame can pin it while a region folds or unfolds.
#[component]
fn FrameSlot(
    place: Slot,
    group: FrameGroup,
    showing: Memo<Showing>,
    share: f64,
    bound: PanelBound,
    children: Children,
) -> impl IntoView {
    let context = ResizableContext::current();
    let id = context.map(|context| context.register_panel(bound, share));
    if let Some(id) = id {
        group.members.update_value(|members| members.push((place, id)));
    }

    let basis = move || {
        let (Some(context), Some(id)) = (context, id) else {
            return None;
        };
        Some(format!("{:.4}%", context.size_of(id)))
    };

    view! {
        box(
            class = "lui-dock__slot",
            class:lui-dock__slot-hidden = move || !showing.get().slot(place),
            style:flex-basis = basis
        ) {
            {children.into_view_once()}
        }
    }
}

/// Places the frame's panels again whenever a region starts or stops showing, the group is laid
/// out at a new length, or a region's limits change. Placed after the panels so every one of them
/// has registered when it first runs.
#[component]
fn FrameSizer(state: DockState, group: FrameGroup, showing: Memo<Showing>) -> impl IntoView {
    let context = ResizableContext::current();
    let node = context.map(ResizableContext::group);
    let size = node.map(|node| node.observe_border_size());
    let vertical = context.is_some_and(|context| matches!(context.direction(), Orientation::Vertical));
    let length = move || {
        let (node, size) = (node?, size?.get()?);
        let along = if vertical { size.height.0 } else { size.width.0 };
        let scale = node.scale();
        let scale = if scale > 0.0 { scale } else { 1.0 };

        Some(f64::from(along / scale))
    };
    let limits = Memo::new(move |_| state.layout.with(|layout| layout.limits));
    let extents = Memo::new(move |_| state.layout.with(|layout| group.extents(layout)));

    let sizing = RenderEffect::new(move |previous: Option<Sizing>| {
        let now = Sizing {
            showing: showing.get(),
            length: GroupLength(length()),
            limits: limits.get(),
            extents: extents.get(),
        };
        if now.length.0.is_some() && now.extents.iter().any(|extent| matches!(extent, RegionExtent::Length(_))) {
            state.layout.update(|layout| settle_lengths(layout, now.showing, group.slots, now.length));
        }
        // Extents a drag wrote are where the panels already stand.
        let dragged = group.recorded.try_update_value(Option::take).flatten().as_ref() == Some(&now.extents);
        let changed = match &previous {
            Some(previous) => {
                previous.showing != now.showing
                    || previous.length != now.length
                    || previous.limits != now.limits
                    || (previous.extents != now.extents && !dragged)
            }
            // The build registered every panel at its target for a frame not yet laid out.
            None => now.length.0.is_some(),
        };
        if changed && let Some(context) = context {
            let placement =
                state.layout.with_untracked(|layout| Placement::of(layout, now.showing, group.slots, now.length));
            group.apply(context, &placement);
        }

        now
    });
    on_cleanup_local(move || drop(sizing));
}

#[component]
fn RegionView(state: DockState, region: DockRegion) -> impl IntoView {
    let shape = Memo::new(move |_| state.layout.with(|layout| layout.shape(region)));
    view! {
        box(class = "lui-dock__region") {
            for shape in move || shape.get().into_iter().collect::<Vec<_>>(), key = |shape: &NodeShape| shape.clone() {
                NodeView(state = state, shape = shape)
            }
        }
    }
}

/// One node of a region's tree. Erased, because a split holds nodes.
#[component]
fn NodeView(state: DockState, shape: NodeShape) -> AnyView {
    match shape {
        NodeShape::Group(group) => AnyView::new(view! { GroupView(state = state, group = group) }),
        NodeShape::Split { id, axis, parts } => {
            AnyView::new(view! { SplitView(state = state, split = id, axis = axis, parts = parts) })
        }
    }
}

#[component]
fn SplitView(state: DockState, split: SplitId, axis: SplitAxis, parts: Vec<NodeShape>) -> AnyView {
    let shares = state.layout.with_untracked(|layout| layout.split_shares(split));
    let resized = UnsyncCallback::new(move |sizes: Vec<f64>| {
        let shares: Vec<Share> = sizes.iter().filter_map(|size| Share::new(size.clamp(0.0, 100.0)).ok()).collect();
        state.layout.update(|layout| layout.set_split_shares(split, &shares));
    });
    let direction = match axis {
        SplitAxis::Horizontal => Orientation::Horizontal,
        SplitAxis::Vertical => Orientation::Vertical,
    };

    // A zoomed group covers the split, so its handles leave the focus order too.
    let zoomed = move || state.zoomed().is_some();
    let mut children = Vec::new();
    for (index, (shape, share)) in parts.into_iter().zip(shares).enumerate() {
        if index > 0 {
            children.push(AnyView::new(view! { ResizableHandle(class:lui-dock__handle-hidden = zoomed) }));
        }
        children.push(AnyView::new(view! {
            ResizablePanel(default_size = share.percent(), min_size = 5.0) {
                NodeView(state = state, shape = shape)
            }
        }));
    }

    AnyView::new(view! {
        ResizablePanelGroup(direction = direction, on_change = resized) {{children}}
    })
}

/// A strip of tabs over the panel the selected one shows. With one panel the strip is a plain
/// title row.
#[component]
fn GroupView(state: DockState, group: GroupId) -> impl IntoView {
    // A group that has gone is about to be unmounted with its tree, and has nothing to show.
    let panels = Memo::new(move |_| {
        state.layout.with(|layout| layout.group(group).map(|group| group.panels.clone()).unwrap_or_default())
    });
    let active = Memo::new(move |_| state.layout.with(|layout| layout.group(group).map(|group| group.active.clone())));
    let shown = Binding::controlled(
        Signal::derive_local(move || active.get().map(|id| id.as_str().to_owned()).unwrap_or_default()),
        move |id: String| {
            // A tab chosen in the frame its panel was removed in has nothing left to show.
            let _ = state.activate(&PanelId::new(id));
        },
    );
    let single = move || panels.with(Vec::len) == 1;
    let labels = DockLabels::current();

    let on_close_tabs = UnsyncCallback::new(move |values: Vec<String>| {
        for value in values {
            // The strip offers only closable panels of this group.
            let _ = state.close(&PanelId::new(value));
        }
    });

    let body = NodeRef::new();
    if let Some(drag) = use_local_context::<DragContext>() {
        drag.register_body(group, body);
    }

    view! {
        TabStrip(
            value = shown,
            label = labels.tabs,
            strings = labels.strings,
            on_close_tabs = on_close_tabs,
            class = "lui-dock__group",
            class:lui-dock__group-single = single,
            attr:data-zoomed = move || (state.zoomed() == Some(group)).then(String::new),
            // Hidden rather than only drawn over, so the keyboard and a screen reader skip it too.
            attr:data-covered = move || state.zoomed().is_some_and(|zoomed| zoomed != group).then(String::new)
        ) {
            for entry in move || panels.get(), key = |entry: &PanelEntry| entry.id.clone() {
                DockTab(state = state, entry = entry)
            }
            TabStripEnd(slot) {
                {labels.menu.map(|strings| view! { GroupMenu(state = state, group = group, strings = strings) }.into_view())}
            }
            TabStripPanels(slot) {
                box(class = "lui-dock__body", node_ref = body) {
                    for entry in move || panels.get(), key = |entry: &PanelEntry| entry.id.clone() {
                        DockContent(state = state, entry = entry)
                    }
                }
            }
        }
    }
}

/// The menu at the end of a group's strip: draws the group over the whole dock or puts it back,
/// and closes the panel it shows when that panel is closable.
#[component]
fn GroupMenu(state: DockState, group: GroupId, strings: DockMenuStrings) -> impl IntoView {
    let zoomed = move || state.zoomed() == Some(group);
    let closable = Memo::new(move |_| {
        state.layout.with(|layout| {
            let group = layout.group(group)?;
            group.panels.iter().find(|entry| entry.id == group.active).map(|entry| entry.closable)
        }) == Some(true)
    });
    let toggle_zoom = UnsyncCallback::new(move |()| state.set_zoomed((!zoomed()).then_some(group)));
    let close_shown = UnsyncCallback::new(move |()| {
        if let Some(active) = state.active_panel(group) {
            let _ = state.close(&active);
        }
    });

    view! {
        DropdownMenu {
            DropdownMenuTrigger(
                variant = ButtonVariant::Ghost,
                size = ButtonSize::IconXs,
                class = "lui-dock__menu",
                a11y:label = move || strings.menu.get()
            ) {
                Icon(icon = ELLIPSIS)
            }
            DropdownMenuContent {
                MenuItem(on_select = toggle_zoom) {
                    {move || if zoomed() { strings.zoom_out.get() } else { strings.zoom_in.get() }}
                }
                if move || closable.get() {
                    MenuSeparator()
                    MenuItem(on_select = close_shown) {
                        {move || strings.close.get()}
                    }
                } else {}
            }
        }
    }
}

#[component]
fn DockTab(state: DockState, entry: PanelEntry) -> AnyView {
    let factory = state.factory(&entry.kind);
    let label = {
        let factory = factory.clone();
        let id = entry.id.clone();
        Signal::derive_local(move || factory.as_ref().map(|factory| factory.label(&id)).unwrap_or_default())
    };
    let title = factory.as_ref().map(|factory| factory.title(&entry.id));
    let value = entry.id.as_str().to_owned();
    let drag = draggable(state, entry.id.clone(), label);

    if !entry.closable {
        return AnyView::new(view! {
            TabStripTab(value = value, a11y:label = move || label.get(), {..drag}) {{title}}
        });
    }

    let close = DockLabels::current().close;
    let close_label = Signal::derive_local(move || format!("{} {}", close.get(), label.get()));
    let id = entry.id;
    let on_close = UnsyncCallback::new(move |()| {
        // A closable panel that is still in the dock always closes.
        let _ = state.close(&id);
    });

    AnyView::new(view! {
        TabStripTab(
            value = value,
            on_close = on_close,
            close_label = close_label,
            a11y:label = move || label.get(),
            {..drag}
        ) {{title}}
    })
}

#[component]
fn DockContent(state: DockState, entry: PanelEntry) -> impl IntoView {
    let value = entry.id.as_str().to_owned();

    view! {
        TabsContent(value = value, keep_mounted = true, class = "lui-dock__content") {
            {KeptView { state, entry: entry.clone() }}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::layout::RegionExtents;
    use crate::dock::layout::RegionLimits;

    const UNMEASURED: GroupLength = GroupLength(None);

    fn share(percent: f64) -> RegionExtent {
        RegionExtent::Share(Share::new(percent).expect("in range"))
    }

    fn length(pixels: f64) -> CssLength {
        CssLength::new(pixels).expect("a length")
    }

    fn with_extents(left: f64, right: f64, bottom: f64) -> Layout {
        let mut layout = Layout::new();
        layout.extents = RegionExtents { left: share(left), right: share(right), bottom: share(bottom) };

        layout
    }

    fn showing(left: bool, middle: bool, right: bool) -> Showing {
        Showing { left, middle, right, center: middle, bottom: false }
    }

    #[test]
    fn sides_that_ask_for_more_than_the_whole_are_scaled_to_fit_it() {
        let layout = with_extents(80.0, 80.0, 30.0);

        assert_eq!(targets(&layout, showing(true, true, true), &ROW, UNMEASURED), [50.0, 0.0, 50.0]);
    }

    #[test]
    fn a_side_below_its_floor_is_raised_to_it_and_the_center_gives_way() {
        let layout = with_extents(2.0, 0.0, 30.0);

        assert_eq!(targets(&layout, showing(true, true, false), &ROW, UNMEASURED), [5.0, 95.0, 0.0]);
    }

    #[test]
    fn with_the_center_hidden_the_sides_share_the_whole() {
        let layout = with_extents(20.0, 30.0, 30.0);

        assert_eq!(targets(&layout, showing(true, false, true), &ROW, UNMEASURED), [40.0, 0.0, 60.0]);
        assert_eq!(targets(&layout, showing(false, false, false), &ROW, UNMEASURED), [0.0, 100.0, 0.0]);
    }

    #[test]
    fn a_length_takes_its_share_of_the_measured_group_and_its_default_share_before() {
        let mut layout = with_extents(20.0, 25.0, 30.0);
        layout.extents.right = RegionExtent::Length(length(500.0));

        assert_eq!(targets(&layout, showing(false, true, true), &ROW, GroupLength(Some(1000.0))), [0.0, 50.0, 50.0]);
        assert_eq!(targets(&layout, showing(false, true, true), &ROW, UNMEASURED), [0.0, 75.0, 25.0]);
    }

    #[test]
    fn a_length_settles_into_the_share_it_takes_and_only_the_groups_own_regions_do() {
        let mut layout = with_extents(20.0, 25.0, 30.0);
        layout.extents.right = RegionExtent::Length(length(500.0));
        layout.extents.bottom = RegionExtent::Length(length(200.0));
        let all = Showing { left: false, middle: true, right: true, center: true, bottom: true };

        settle_lengths(&mut layout, all, &ROW, UNMEASURED);
        assert_eq!(layout.extents.right, RegionExtent::Length(length(500.0)), "nothing settles before a measure");

        settle_lengths(&mut layout, all, &ROW, GroupLength(Some(2000.0)));
        assert_eq!(layout.extents.right, share(25.0));
        assert_eq!(layout.extents.bottom, RegionExtent::Length(length(200.0)));

        settle_lengths(&mut layout, all, &COLUMN, GroupLength(Some(100.0)));
        assert_eq!(layout.extents.bottom, share(100.0), "a length past the whole takes the whole");
    }

    #[test]
    fn a_length_settles_at_the_share_its_limits_place_it_at() {
        let mut layout = with_extents(20.0, 25.0, 30.0);
        layout.extents.right = RegionExtent::Length(length(200.0));
        layout.limits.set(FoldingRegion::Right, RegionLimits { min: Some(length(280.0)), max: None });
        let measured = GroupLength(Some(800.0));

        settle_lengths(&mut layout, showing(false, true, true), &ROW, measured);
        assert_eq!(layout.extents.right, share(35.0));

        layout.extents.right = RegionExtent::Length(length(200.0));
        settle_lengths(&mut layout, showing(false, true, false), &ROW, measured);
        assert_eq!(layout.extents.right, share(25.0), "a hidden region keeps the share of its length");

        layout.extents.right = RegionExtent::Length(length(500.0));
        settle_lengths(&mut layout, showing(false, false, true), &ROW, measured);
        assert_eq!(layout.extents.right, share(62.5), "a side alone fills the row but keeps its own share");
    }

    #[test]
    fn pixel_limits_bound_a_side_once_the_group_is_measured() {
        let mut layout = with_extents(20.0, 10.0, 30.0);
        layout.limits.set(FoldingRegion::Right, RegionLimits { min: Some(length(200.0)), max: Some(length(400.0)) });
        let measured = GroupLength(Some(800.0));

        assert_eq!(targets(&layout, showing(false, true, true), &ROW, measured), [0.0, 75.0, 25.0]);
        assert_eq!(bound(&layout, Slot::Right, measured), PanelBound::new(25.0, 50.0));
        assert_eq!(bound(&layout, Slot::Right, UNMEASURED), PanelBound::new(FLOOR, 100.0));
    }

    #[test]
    fn a_least_above_the_most_or_the_group_wins() {
        let mut layout = Layout::new();
        layout.limits.set(FoldingRegion::Left, RegionLimits { min: Some(length(600.0)), max: Some(length(300.0)) });

        assert_eq!(bound(&layout, Slot::Left, GroupLength(Some(1000.0))), PanelBound::new(60.0, 60.0));
        assert_eq!(bound(&layout, Slot::Left, GroupLength(Some(400.0))), PanelBound::new(100.0, 100.0));
    }
}

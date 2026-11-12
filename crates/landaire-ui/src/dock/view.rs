//! A dock drawn: the folding regions around the center, each region's splits, and each split's tab
//! groups.
//!
//! Each group's tabs are a [`TabStrip`](crate::TabStrip), as an application's own tabs are.

use zgui::component;
use zgui::css;
use zgui::reactive::LocalStorage;
use zgui::reactive::Memo;
use zgui::reactive::RenderEffect;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
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
use zgui::view::install_stylesheet;
use zgui_ui::PanelBound;
use zgui_ui::ResizableContext;
use zgui_ui::ResizableHandleProps;
use zgui_ui::ResizablePanelGroupProps;
use zgui_ui::ResizablePanelProps;
use zgui_ui::TabsContentProps;
use zgui_ui_primitives::Binding;
use zgui_ui_primitives::Orientation;

use crate::chrome::tab_strip::TabStripPanels;
use crate::chrome::tab_strip::TabStripProps;
use crate::chrome::tab_strip::TabStripTabProps;
use crate::dock::layout::DockRegion;
use crate::dock::layout::GroupId;
use crate::dock::layout::Layout;
use crate::dock::layout::NodeShape;
use crate::dock::layout::PanelEntry;
use crate::dock::layout::PanelId;
use crate::dock::layout::Share;
use crate::dock::layout::SplitAxis;
use crate::dock::layout::SplitId;
use crate::dock::state::DockState;
use crate::dock::state::LayoutGeneration;

/// What the dock rules are installed under.
const SHEET: &str = "landaire-ui-dock";

const CSS: &str = css!(
    ".lui-dock {
        display: flex;
        flex: 1 1 auto;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background-color: var(--lui-color-surface);
    }
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
    .lui-dock .lui-dock__group .lui-tab-strip__scroller {
        flex: none;
        border-bottom: 1px solid var(--lui-color-border);
        background-color: var(--lui-color-surface);
    }
    .lui-dock .lui-tab-strip__list .lui-tab-strip__tab.zui-tabs__trigger[data-state=\"active\"]:first-child {
        border-left-color: transparent;
    }
    .lui-dock .lui-dock__group-single .lui-tab-strip__scroller {
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
    }"
);

/// What the dock's tab strips say to a screen reader.
#[derive(Clone)]
struct DockLabels {
    /// Said before a panel's label to name its tab's close button.
    close: String,
    /// What each group's strip of tabs is called.
    tabs: String,
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
    /// The room the slot is given while it shows, or `None` for the slot that takes what is left.
    fn extent(self, layout: &Layout) -> Option<Share> {
        match self {
            Self::Left => Some(layout.extents.left),
            Self::Right => Some(layout.extents.right),
            Self::Bottom => Some(layout.extents.bottom),
            Self::Middle | Self::Center => None,
        }
    }

    /// How small and large the user may drag the slot while it shows.
    fn bound(self) -> PanelBound {
        match self {
            Self::Left | Self::Right | Self::Bottom => PanelBound::new(5.0, 100.0),
            Self::Middle | Self::Center => PanelBound::new(0.0, 100.0),
        }
    }
}

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
}

/// The share each of `slots` takes, adding up to 100: a showing slot its extent inside its bound,
/// the slot without one what the others leave, and a hidden slot nothing. Extents that add up to
/// more than the whole, or that fill it with no slot left to take the rest, are scaled to fit.
fn targets(layout: &Layout, showing: Showing, slots: &[Slot]) -> Vec<f64> {
    let mut sizes: Vec<Option<f64>> = slots
        .iter()
        .map(|slot| match slot.extent(layout) {
            Some(extent) if showing.slot(*slot) => {
                let bound = slot.bound();
                Some(extent.percent().clamp(bound.min, bound.max))
            }
            Some(_) => Some(0.0),
            None => None,
        })
        .collect();
    let fixed: f64 = sizes.iter().flatten().sum();
    let rest_shows = slots.iter().any(|slot| slot.extent(layout).is_none() && showing.slot(*slot));

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

/// The panels of one resizable group of the frame, as they registered, and whether their sizes
/// are being set by the frame rather than by the user.
#[derive(Clone, Copy)]
struct FrameGroup {
    members: StoredValue<Vec<(Slot, u64)>, LocalStorage>,
    applying: StoredValue<bool, LocalStorage>,
}

impl FrameGroup {
    fn new() -> Self {
        Self { members: StoredValue::new_local(Vec::new()), applying: StoredValue::new_local(false) }
    }

    /// Moves every panel to its target and leaves each with its bound: pinned at zero when hidden.
    ///
    /// Each panel is pinned at its target in turn while the ones after it are free, so the group's
    /// own arithmetic gives the difference to the free ones and the last lands where it should.
    fn apply(self, context: ResizableContext, showing: Showing, targets: &[f64]) {
        self.applying.set_value(true);

        let members = self.members.get_value();
        for (_, id) in &members {
            context.set_bound(*id, PanelBound::new(0.0, 100.0));
        }
        for ((_, id), target) in members.iter().zip(targets) {
            context.set_bound(*id, PanelBound::new(*target, *target));
        }
        for (slot, id) in &members {
            let bound = if showing.slot(*slot) { slot.bound() } else { PanelBound::new(0.0, 0.0) };
            context.set_bound(*id, bound);
        }

        self.applying.set_value(false);
    }

    /// Writes the sizes the user dragged the folding slots to back into the layout.
    fn record(self, state: DockState, sizes: &[f64]) {
        if self.applying.get_value() {
            return;
        }

        let members = self.members.get_value();
        state.layout.update(|layout| {
            let showing = Showing::of(layout);
            for ((slot, _), size) in members.iter().zip(sizes) {
                // A side's share is only its own while the middle beside it shows.
                let Ok(share) = Share::new(size.clamp(0.0, 100.0)) else {
                    continue;
                };
                if !showing.slot(*slot) || !showing.middle || (*slot == Slot::Bottom && !showing.center) {
                    continue;
                }
                match slot {
                    Slot::Left => layout.extents.left = share,
                    Slot::Right => layout.extents.right = share,
                    Slot::Bottom => layout.extents.bottom = share,
                    Slot::Middle | Slot::Center => {}
                }
            }
        });
    }
}

/// Lays out a [`DockState`]: a left, center, right and bottom region, the side ones folding away
/// when closed or empty, each holding splits of tab groups.
///
/// Panels keep their views while their tab is not shown and while their region is closed. A
/// change to the shape of a region's tree builds that region's views again, and
/// [`restore`](DockState::restore) builds the whole dock again.
#[component]
pub fn Dock(
    state: DockState,
    /// Said before a panel's label to name its tab's close button.
    #[prop(into, default = "Close".to_owned())]
    close_label: String,
    /// What each group's strip of tabs is called.
    #[prop(into, default = "Panels".to_owned())]
    tabs_label: String,
    #[prop(into, optional)] class: Classes,
    #[prop(attrs)] attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, CSS);
    provide_local_context(DockLabels { close: close_label, tabs: tabs_label });

    view! {
        box(class = "lui-dock", {..attrs}, class = class) {
            for _generation in move || vec![state.generation.get()], key = |generation: &LayoutGeneration| *generation {
                DockFrame(state = state)
            }
        }
    }
}

#[component]
fn DockFrame(state: DockState) -> impl IntoView {
    let showing = Memo::new(move |_| state.layout.with(Showing::of));
    let row = FrameGroup::new();
    let column = FrameGroup::new();
    let (row_targets, column_targets) = state.layout.with_untracked(|layout| {
        let now = Showing::of(layout);
        (
            targets(layout, now, &[Slot::Left, Slot::Middle, Slot::Right]),
            targets(layout, now, &[Slot::Center, Slot::Bottom]),
        )
    });

    let hidden_left = move || !showing.with(|showing| showing.left && showing.middle);
    let hidden_right = move || !showing.with(|showing| showing.right && showing.middle);
    let hidden_bottom = move || !showing.with(|showing| showing.center && showing.bottom);

    view! {
        ResizablePanelGroup(
            direction = Orientation::Horizontal,
            on_change = UnsyncCallback::new(move |sizes: Vec<f64>| row.record(state, &sizes))
        ) {
            FrameSlot(place = Slot::Left, group = row, showing = showing, share = row_targets[0]) {
                RegionView(state = state, region = DockRegion::Left)
            }
            ResizableHandle(class:lui-dock__handle-hidden = hidden_left)
            FrameSlot(place = Slot::Middle, group = row, showing = showing, share = row_targets[1]) {
                ResizablePanelGroup(
                    direction = Orientation::Vertical,
                    on_change = UnsyncCallback::new(move |sizes: Vec<f64>| column.record(state, &sizes))
                ) {
                    FrameSlot(place = Slot::Center, group = column, showing = showing, share = column_targets[0]) {
                        RegionView(state = state, region = DockRegion::Center)
                    }
                    ResizableHandle(class:lui-dock__handle-hidden = hidden_bottom)
                    FrameSlot(place = Slot::Bottom, group = column, showing = showing, share = column_targets[1]) {
                        RegionView(state = state, region = DockRegion::Bottom)
                    }
                    FrameSizer(state = state, group = column, showing = showing, slots = vec![Slot::Center, Slot::Bottom])
                }
            }
            ResizableHandle(class:lui-dock__handle-hidden = hidden_right)
            FrameSlot(place = Slot::Right, group = row, showing = showing, share = row_targets[2]) {
                RegionView(state = state, region = DockRegion::Right)
            }
            FrameSizer(state = state, group = row, showing = showing, slots = vec![Slot::Left, Slot::Middle, Slot::Right])
        }
    }
}

/// One panel of the frame's resizable groups. It registers with the group itself rather than
/// through `ResizablePanel`, so the frame can pin it while a region folds or unfolds.
#[component]
fn FrameSlot(place: Slot, group: FrameGroup, showing: Memo<Showing>, share: f64, children: Children) -> impl IntoView {
    let context = ResizableContext::current();
    let shown = showing.get_untracked().slot(place);
    let bound = if shown { place.bound() } else { PanelBound::new(0.0, 0.0) };
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

/// Moves the frame's panels to their targets whenever a region starts or stops showing. Placed
/// after the panels so every one of them has registered when it first runs.
#[component]
fn FrameSizer(state: DockState, group: FrameGroup, showing: Memo<Showing>, slots: Vec<Slot>) -> impl IntoView {
    let context = ResizableContext::current();
    let sizing = RenderEffect::new(move |previous: Option<Showing>| {
        let now = showing.get();
        // The first run is the build, which registered every panel at its target already.
        if previous.is_some_and(|previous| previous != now)
            && let Some(context) = context
        {
            let shares = state.layout.with_untracked(|layout| targets(layout, now, &slots));
            group.apply(context, now, &shares);
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

    let mut children = Vec::new();
    for (index, (shape, share)) in parts.into_iter().zip(shares).enumerate() {
        if index > 0 {
            children.push(AnyView::new(view! { ResizableHandle() }));
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
    let label = use_local_context::<DockLabels>().map(|labels| labels.tabs).unwrap_or_default();

    view! {
        TabStrip(value = shown, label = label, class = "lui-dock__group", class:lui-dock__group-single = single) {
            for entry in move || panels.get(), key = |entry: &PanelEntry| entry.id.clone() {
                DockTab(state = state, entry = entry)
            }
            TabStripPanels(slot) {
                box(class = "lui-dock__body") {
                    for entry in move || panels.get(), key = |entry: &PanelEntry| entry.id.clone() {
                        DockContent(state = state, entry = entry)
                    }
                }
            }
        }
    }
}

#[component]
fn DockTab(state: DockState, entry: PanelEntry) -> AnyView {
    let factory = state.factory(&entry.kind);
    let label = factory.as_ref().map(|factory| factory.label(&entry.id)).unwrap_or_default();
    let title = factory.as_ref().map(|factory| factory.title(&entry.id));
    let value = entry.id.as_str().to_owned();

    if !entry.closable {
        return AnyView::new(view! { TabStripTab(value = value, a11y:label = label) {{title}} });
    }

    let close_label = match use_local_context::<DockLabels>() {
        Some(labels) => format!("{} {label}", labels.close),
        None => label.clone(),
    };
    let id = entry.id;
    let on_close = UnsyncCallback::new(move |()| {
        // A closable panel that is still in the dock always closes.
        let _ = state.close(&id);
    });

    AnyView::new(view! {
        TabStripTab(value = value, on_close = on_close, close_label = close_label, a11y:label = label) {{title}}
    })
}

#[component]
fn DockContent(state: DockState, entry: PanelEntry) -> impl IntoView {
    let factory = state.factory(&entry.kind);
    let id = entry.id.clone();

    view! {
        TabsContent(value = entry.id.as_str(), keep_mounted = true, class = "lui-dock__content") {
            {factory.as_ref().map(|factory| factory.content(&id))}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::layout::RegionExtents;

    const ROW: [Slot; 3] = [Slot::Left, Slot::Middle, Slot::Right];

    fn with_extents(left: f64, right: f64, bottom: f64) -> Layout {
        let share = |percent| Share::new(percent).expect("in range");
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

        assert_eq!(targets(&layout, showing(true, true, true), &ROW), [50.0, 0.0, 50.0]);
    }

    #[test]
    fn a_side_below_its_floor_is_raised_to_it_and_the_center_gives_way() {
        let layout = with_extents(2.0, 0.0, 30.0);

        assert_eq!(targets(&layout, showing(true, true, false), &ROW), [5.0, 95.0, 0.0]);
    }

    #[test]
    fn with_the_center_hidden_the_sides_share_the_whole() {
        let layout = with_extents(20.0, 30.0, 30.0);

        assert_eq!(targets(&layout, showing(true, false, true), &ROW), [40.0, 0.0, 60.0]);
        assert_eq!(targets(&layout, showing(false, false, false), &ROW), [0.0, 100.0, 0.0]);
    }
}

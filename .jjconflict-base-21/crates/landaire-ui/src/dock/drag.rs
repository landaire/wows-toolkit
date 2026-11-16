//! Moving a panel by dragging its tab.
//!
//! A tab pressed and moved [`THRESHOLD`] pixels is lifted, and follows the pointer until it is
//! released. Over a group's strip of tabs the pointer picks the gap between two tabs. Over a
//! group's content it picks one of five zones: the outer [`EDGE_SHARE`] of each side splits the
//! group there, and the middle joins the group's tabs. Along the edge of the dock, a side region
//! that takes no room is a target of its own. The dock draws where the panel would land, and a
//! release there moves it; a release anywhere else, or <kbd>Escape</kbd>, puts it back.

use std::collections::HashMap;

use zgui::component;
use zgui::geom::Css;
use zgui::geom::CssPx;
use zgui::geom::Device;
use zgui::geom::DevicePx;
use zgui::geom::Point;
use zgui::geom::Rect;
use zgui::reactive::LocalStorage;
use zgui::reactive::RenderEffect;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::reactive::use_local_context;
use zgui::view;
use zgui::view::AttrName;
use zgui::view::Attrs;
use zgui::view::EventCx;
use zgui::view::IntoView;
use zgui::view::ListenerGuard;
use zgui::view::NodeRef;
use zgui::view::events;
use zgui::vocab::Key;
use zgui::vocab::ListenerOptions;
use zgui::vocab::NamedKey;
use zgui::vocab::PointerButton;
use zgui_ui::TabsContext;

use crate::dock::layout::DockRegion;
use crate::dock::layout::DockTarget;
use crate::dock::layout::Edge;
use crate::dock::layout::FoldingRegion;
use crate::dock::layout::GroupId;
use crate::dock::layout::Layout;
use crate::dock::layout::PanelId;
use crate::dock::state::DockState;
use crate::dock::view::region_area;

/// How far a pressed tab moves, in CSS pixels, before it is lifted rather than clicked.
pub(crate) const THRESHOLD: f64 = 4.0;

/// The part of a group's width or height, at each side, whose drop splits the group there.
const EDGE_SHARE: f64 = 0.35;

/// How far in from the dock's edge a side region that takes no room catches a drop, in CSS
/// pixels.
const REGION_BAND: f64 = 32.0;

/// A point in CSS pixels from the window's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Spot {
    pub(crate) x: f64,
    pub(crate) y: f64,
}

impl Spot {
    pub(crate) fn distance_to(self, other: Self) -> f64 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

/// A box in CSS pixels from the window's top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Area {
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl Area {
    /// `rect`, a box in device pixels, at `scale` device pixels to the CSS pixel.
    fn of(rect: Rect<DevicePx, Device>, scale: f32) -> Self {
        let scale = f64::from(if scale > 0.0 { scale } else { 1.0 });

        Self {
            left: f64::from(rect.origin.x.0) / scale,
            top: f64::from(rect.origin.y.0) / scale,
            width: f64::from(rect.size.width.0) / scale,
            height: f64::from(rect.size.height.0) / scale,
        }
    }

    /// Where `node` is laid out, or `None` when it is not, or takes no room.
    fn of_node(node: NodeRef) -> Option<Self> {
        let area = Self::of(node.window_bounds()?, node.scale());

        (area.width > 0.0 && area.height > 0.0).then_some(area)
    }

    fn right(self) -> f64 {
        self.left + self.width
    }

    fn bottom(self) -> f64 {
        self.top + self.height
    }

    fn contains(self, at: Spot) -> bool {
        at.x >= self.left && at.x < self.right() && at.y >= self.top && at.y < self.bottom()
    }

    /// The part of the area at `edge`, `share` of it across.
    fn part(self, edge: Edge, share: f64) -> Self {
        match edge {
            Edge::Left => Self { width: self.width * share, ..self },
            Edge::Right => Self { left: self.right() - self.width * share, width: self.width * share, ..self },
            Edge::Top => Self { height: self.height * share, ..self },
            Edge::Bottom => Self { top: self.bottom() - self.height * share, height: self.height * share, ..self },
        }
    }
}

/// Where one tab group is drawn.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GroupArea {
    pub(crate) group: GroupId,
    /// The content under the strip.
    pub(crate) body: Area,
    /// Its tabs, in strip order.
    pub(crate) tabs: Vec<Area>,
}

impl GroupArea {
    /// The row the tabs are in: the group's width, the tabs' height.
    fn strip(&self) -> Option<Area> {
        let top = self.tabs.iter().map(|tab| tab.top).reduce(f64::min)?;
        let bottom = self.tabs.iter().map(|tab| tab.bottom()).reduce(f64::max)?;

        Some(Area { left: self.body.left, top, width: self.body.width, height: bottom - top })
    }
}

/// What the dock draws over where a dragged panel would land.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Preview {
    /// The room the panel would take.
    Room(Area),
    /// The gap between two tabs the panel would go into.
    Gap(Area),
}

/// Where a release would move a dragged panel, and what shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Landing {
    pub(crate) target: DockTarget,
    pub(crate) preview: Preview,
}

/// Where a release at `at` would move `panel`, given where the dock and its groups are drawn.
/// `None` where a release would leave the panel where it is.
pub(crate) fn landing(layout: &Layout, panel: &PanelId, dock: Area, groups: &[GroupArea], at: Spot) -> Option<Landing> {
    if !dock.contains(at) {
        return None;
    }
    let source = layout.group_of(panel)?;
    let tabs = &layout.group(source)?.panels;
    let from = tabs.iter().position(|entry| &entry.id == panel)?;

    for group in groups {
        let Some(strip) = group.strip().filter(|strip| strip.contains(at)) else {
            continue;
        };
        let index = group.tabs.iter().filter(|tab| tab.left + tab.width / 2.0 < at.x).count();
        if group.group == source && (index == from || index == from + 1) {
            return None;
        }
        let x = match group.tabs.get(index) {
            Some(tab) => tab.left,
            None => group.tabs.last().map_or(strip.left, |tab| tab.right()),
        };
        let gap = Area { left: x - 1.0, top: strip.top, width: 2.0, height: strip.height };
        return Some(Landing { target: DockTarget::Tab { group: group.group, index }, preview: Preview::Gap(gap) });
    }

    for region in [FoldingRegion::Left, FoldingRegion::Right, FoldingRegion::Bottom] {
        let docked = DockRegion::from(region);
        if layout.shows(docked) {
            continue;
        }
        let band = match region {
            FoldingRegion::Left => dock.part(Edge::Left, (REGION_BAND / dock.width).min(1.0)),
            FoldingRegion::Right => dock.part(Edge::Right, (REGION_BAND / dock.width).min(1.0)),
            FoldingRegion::Bottom => dock.part(Edge::Bottom, (REGION_BAND / dock.height).min(1.0)),
        };
        if !band.contains(at) {
            continue;
        }
        // The room it takes once the panel is in it, without a region the move empties.
        let mut moved = layout.clone();
        if moved.move_panel(panel, DockTarget::Region(docked)).is_err() {
            continue;
        }
        let room = region_area(&moved, region, dock);
        return Some(Landing { target: DockTarget::Region(docked), preview: Preview::Room(room) });
    }

    let group = groups.iter().find(|group| group.body.contains(at))?;
    let body = group.body;
    let across = (at.x - body.left) / body.width;
    let down = (at.y - body.top) / body.height;
    let edge = if across < EDGE_SHARE {
        Some(Edge::Left)
    } else if across > 1.0 - EDGE_SHARE {
        Some(Edge::Right)
    } else if down < EDGE_SHARE {
        Some(Edge::Top)
    } else if down > 1.0 - EDGE_SHARE {
        Some(Edge::Bottom)
    } else {
        None
    };

    match edge {
        None if group.group == source => None,
        None => Some(Landing { target: DockTarget::Group(group.group), preview: Preview::Room(body) }),
        Some(_) if group.group == source && tabs.len() == 1 => None,
        Some(edge) => Some(Landing {
            target: DockTarget::Beside { group: group.group, edge },
            preview: Preview::Room(body.part(edge, 0.5)),
        }),
    }
}

/// An element registered under a key, with the serial of the registration, so a registration
/// replaced by a newer one is not taken out by the older one's cleanup.
#[derive(Clone, Copy)]
struct Registered {
    node: NodeRef,
    serial: u64,
}

/// A tab being dragged.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Lifted {
    pub(crate) panel: PanelId,
    pub(crate) label: String,
    /// Where in the tab it was taken hold of, from the tab's corner.
    pub(crate) grab: Spot,
    /// Where the pointer is, from the dock's corner.
    pub(crate) at: Spot,
    /// Where a release would move it, with its preview from the dock's corner.
    pub(crate) landing: Option<Landing>,
}

/// What dragging a tab needs from the dock it is in: where the dock, each group's content and
/// each tab are drawn, and the tab being dragged.
#[derive(Clone, Copy)]
pub(crate) struct DragContext {
    pub(crate) dock: NodeRef,
    bodies: StoredValue<HashMap<GroupId, Registered>, LocalStorage>,
    tabs: StoredValue<HashMap<PanelId, Registered>, LocalStorage>,
    serial: StoredValue<u64, LocalStorage>,
    pub(crate) lifted: RwSignal<Option<Lifted>, LocalStorage>,
}

impl DragContext {
    pub(crate) fn new() -> Self {
        Self {
            dock: NodeRef::new(),
            bodies: StoredValue::new_local(HashMap::new()),
            tabs: StoredValue::new_local(HashMap::new()),
            serial: StoredValue::new_local(0),
            lifted: RwSignal::new_local(None),
        }
    }

    fn next_serial(self) -> u64 {
        let serial = self.serial.get_value() + 1;
        self.serial.set_value(serial);

        serial
    }

    /// Records where `group`'s content is drawn, until the calling scope is cleaned up.
    pub(crate) fn register_body(self, group: GroupId, node: NodeRef) {
        let serial = self.next_serial();
        self.bodies.update_value(|bodies| {
            bodies.insert(group, Registered { node, serial });
        });
        zgui::reactive::on_cleanup_local(move || {
            let _ = self.bodies.try_update_value(|bodies| {
                if bodies.get(&group).is_some_and(|known| known.serial == serial) {
                    bodies.remove(&group);
                }
            });
        });
    }

    /// Records where `panel`'s tab is drawn, until the calling scope is cleaned up.
    pub(crate) fn register_tab(self, panel: PanelId, node: NodeRef) {
        let serial = self.next_serial();
        self.tabs.update_value(|tabs| {
            tabs.insert(panel.clone(), Registered { node, serial });
        });
        zgui::reactive::on_cleanup_local(move || {
            let _ = self.tabs.try_update_value(|tabs| {
                if tabs.get(&panel).is_some_and(|known| known.serial == serial) {
                    tabs.remove(&panel);
                }
            });
        });
    }

    /// Where the dock and each group that takes room are drawn.
    fn areas(self, layout: &Layout) -> Option<(Area, Vec<GroupArea>)> {
        let dock = Area::of_node(self.dock)?;
        let groups = self.bodies.with_value(|bodies| {
            self.tabs.with_value(|tabs| {
                layout
                    .groups()
                    .into_iter()
                    .filter(|group| layout.shows_group(group.id))
                    .filter_map(|group| {
                        let body = Area::of_node(bodies.get(&group.id)?.node)?;
                        let tabs =
                            group.panels.iter().filter_map(|entry| Area::of_node(tabs.get(&entry.id)?.node)).collect();
                        Some(GroupArea { group: group.id, body, tabs })
                    })
                    .collect()
            })
        });

        Some((dock, groups))
    }

    /// Where `panel`'s tab is drawn.
    pub(crate) fn tab_area(self, panel: &PanelId) -> Option<Area> {
        let node = self.tabs.with_value(|tabs| tabs.get(panel).map(|known| known.node))?;

        Area::of_node(node)
    }

    /// Follows the pointer at `at` with `panel`'s tab, lifting it first when it is not yet.
    pub(crate) fn track(self, state: DockState, panel: &PanelId, label: impl FnOnce() -> String, grab: Spot, at: Spot) {
        let Some((dock, groups)) = state.layout.with_untracked(|layout| self.areas(layout)) else {
            return;
        };
        let landing = state.layout.with_untracked(|layout| landing(layout, panel, dock, &groups, at));
        let from_dock = |area: Area| Area { left: area.left - dock.left, top: area.top - dock.top, ..area };
        let landing = landing.map(|landing| Landing {
            preview: match landing.preview {
                Preview::Room(area) => Preview::Room(from_dock(area)),
                Preview::Gap(area) => Preview::Gap(from_dock(area)),
            },
            ..landing
        });
        let at = Spot { x: at.x - dock.left, y: at.y - dock.top };

        if self.lifted.with_untracked(|lifted| lifted.as_ref().is_some_and(|lifted| &lifted.panel == panel)) {
            self.lifted.update(|lifted| {
                if let Some(lifted) = lifted {
                    lifted.at = at;
                    lifted.landing = landing;
                }
            });
        } else {
            self.lifted.set(Some(Lifted { panel: panel.clone(), label: label(), grab, at, landing }));
        }
    }

    /// Puts the tab down: where it would land, or back where it was without a landing.
    pub(crate) fn release(self, state: DockState) {
        let Some(lifted) = self.lifted.try_update(Option::take).flatten() else {
            return;
        };
        if let Some(landing) = lifted.landing {
            // A target that went while the tab was held leaves the panel where it was.
            let _ = state.move_panel(&lifted.panel, landing.target);
        }
    }

    /// Puts the tab back where it was.
    pub(crate) fn cancel(self) {
        self.lifted.set(None);
    }

    /// Puts the tab back where it was, from a cleanup that may outlive the dock.
    fn drop_lifted(self) {
        let _ = self.lifted.try_update(|lifted| *lifted = None);
    }
}

/// Where a press on a tab started, where in the tab it took hold, and whether it has lifted the
/// tab. A lifted tab the dock no longer holds was put back.
#[derive(Clone, Copy, Debug)]
struct Press {
    from: Spot,
    grab: Spot,
    lifted: bool,
}

fn spot(position: Point<CssPx, Css>) -> Spot {
    Spot { x: f64::from(position.x.0), y: f64::from(position.y.0) }
}

/// What lets `panel`'s tab, inside its strip, be dragged: listeners, and a `data-lifted` mark
/// while it is held. Nothing outside a dock, or while a group is zoomed.
pub(crate) fn draggable(state: DockState, panel: PanelId, label: Signal<String, LocalStorage>) -> Attrs {
    let (Some(drag), Some(tabs)) = (use_local_context::<DragContext>(), TabsContext::current()) else {
        return Attrs::new();
    };
    drag.register_tab(panel.clone(), tabs.trigger_of(panel.as_str()));
    let press = StoredValue::new_local(None::<Press>);
    let held = {
        let panel = panel.clone();
        move || drag.lifted.with_untracked(|lifted| lifted.as_ref().is_some_and(|lifted| lifted.panel == panel))
    };
    // A tab taken away while it is held is never let go.
    zgui::reactive::on_cleanup_local({
        let held = held.clone();
        move || {
            if press.try_update_value(Option::take).flatten().is_some_and(|now| now.lifted) && held() {
                drag.drop_lifted();
            }
        }
    });

    let on_down = {
        let panel = panel.clone();
        move |ev: &mut EventCx<'_, events::PointerDown>| {
            if ev.button != Some(PointerButton::Primary) || !ev.primary || state.zoomed_untracked().is_some() {
                return;
            }
            let Some(tab) = drag.tab_area(&panel) else {
                return;
            };
            let from = spot(ev.position);
            let grab = Spot { x: from.x - tab.left, y: from.y - tab.top };
            press.set_value(Some(Press { from, grab, lifted: false }));
            // The press neither focuses the tab nor, through that, selects it. A release that
            // did not lift it does, so a drag leaves the selection as it was.
            ev.prevent_default();
            // Held, so the drag goes on once the pointer leaves the tab.
            ev.capture_pointer();
        }
    };
    let on_move = {
        let panel = panel.clone();
        let held = held.clone();
        move |ev: &mut EventCx<'_, events::PointerMove>| {
            let Some(mut now) = press.get_value() else {
                return;
            };
            if now.lifted && !held() {
                return;
            }
            let at = spot(ev.position);
            if !now.lifted {
                if at.distance_to(now.from) < THRESHOLD {
                    return;
                }
                now.lifted = true;
                press.set_value(Some(now));
            }
            drag.track(state, &panel, || label.get_untracked(), now.grab, at);
        }
    };
    let on_up = {
        let panel = panel.clone();
        let held = held.clone();
        move |ev: &mut EventCx<'_, events::PointerUp>| {
            let Some(now) = press.try_update_value(Option::take).flatten() else {
                return;
            };
            ev.release_pointer();
            if now.lifted {
                // Not a click on the tab.
                ev.prevent_default();
                if held() {
                    drag.release(state);
                }
                return;
            }
            if drag.tab_area(&panel).is_some_and(|tab| tab.contains(spot(ev.position))) {
                let tab = ev.current;
                ev.request_focus(tab);
            }
        }
    };
    let on_cancel = {
        move |ev: &mut EventCx<'_, events::PointerCancel>| {
            if press.try_update_value(Option::take).flatten().is_some() {
                ev.release_pointer();
                if held() {
                    drag.cancel();
                }
            }
        }
    };
    let lifted = move || {
        drag.lifted.with(|lifted| lifted.as_ref().is_some_and(|lifted| lifted.panel == panel)).then(String::new)
    };

    Attrs::new()
        .listener(events::PointerDown, ListenerOptions::DEFAULT, on_down)
        .listener(events::PointerMove, ListenerOptions::DEFAULT, on_move)
        .listener(events::PointerUp, ListenerOptions::DEFAULT, on_up)
        .listener(events::PointerCancel, ListenerOptions::DEFAULT, on_cancel)
        .attribute(AttrName::new("data-lifted"), lifted)
}

/// Puts a held tab back on <kbd>Escape</kbd>, wherever the focus is, for as long as the calling
/// scope lasts.
fn cancel_on_escape(drag: DragContext) {
    let guard = StoredValue::new_local(None::<ListenerGuard>);
    let attaching = RenderEffect::new(move |_| {
        if drag.dock.get().is_none() || guard.with_value(Option::is_some) {
            return;
        }
        let Some(window) = drag.dock.window_root() else {
            return;
        };
        let listener =
            window.listen(events::KEY_DOWN, ListenerOptions::CAPTURE, move |ev: &mut EventCx<'_, events::KeyDown>| {
                if ev.key != Key::Named(NamedKey::Escape) || drag.lifted.with_untracked(Option::is_none) {
                    return;
                }
                drag.cancel();
                ev.prevent_default();
                ev.stop_propagation();
            });
        guard.set_value(listener);
    });
    zgui::reactive::on_cleanup_local(move || {
        drop(attaching);
        guard.set_value(None);
    });
}

/// What the dock draws while a tab is dragged: where it would land, and the tab under the
/// pointer.
#[component]
pub(crate) fn DragLayer(drag: DragContext) -> impl IntoView {
    cancel_on_escape(drag);
    let px = |value: f64| format!("{value:.1}px");
    let room = move || match drag.lifted.with(|lifted| lifted.as_ref().and_then(|lifted| lifted.landing)) {
        Some(Landing { preview: Preview::Room(area), .. }) => Some(area),
        _ => None,
    };
    let gap = move || match drag.lifted.with(|lifted| lifted.as_ref().and_then(|lifted| lifted.landing)) {
        Some(Landing { preview: Preview::Gap(area), .. }) => Some(area),
        _ => None,
    };
    let ghost = move || {
        drag.lifted.with(|lifted| {
            lifted
                .as_ref()
                .map(|lifted| (lifted.label.clone(), lifted.at.x - lifted.grab.x, lifted.at.y - lifted.grab.y))
        })
    };

    view! {
        box(
            class = "lui-dock__landing",
            a11y:hidden = true,
            style:display = move || room().is_none().then(|| "none".to_owned()),
            style:left = move || room().map(|area| px(area.left)),
            style:top = move || room().map(|area| px(area.top)),
            style:width = move || room().map(|area| px(area.width)),
            style:height = move || room().map(|area| px(area.height))
        ) {}
        box(
            class = "lui-dock__gap",
            a11y:hidden = true,
            style:display = move || gap().is_none().then(|| "none".to_owned()),
            style:left = move || gap().map(|area| px(area.left)),
            style:top = move || gap().map(|area| px(area.top)),
            style:width = move || gap().map(|area| px(area.width)),
            style:height = move || gap().map(|area| px(area.height))
        ) {}
        box(
            class = "lui-dock__ghost",
            a11y:hidden = true,
            style:display = move || ghost().is_none().then(|| "none".to_owned()),
            style:left = move || ghost().map(|(_, left, _)| px(left)),
            style:top = move || ghost().map(|(_, _, top)| px(top))
        ) {
            {move || ghost().map(|(label, _, _)| label).unwrap_or_default()}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::layout::CssLength;
    use crate::dock::layout::PanelEntry;
    use crate::dock::layout::PanelKind;
    use crate::dock::layout::RegionExtent;
    use crate::dock::layout::RegionLimits;
    use crate::dock::layout::Share;

    fn entry(id: &str) -> PanelEntry {
        PanelEntry::new(PanelId::new(id), PanelKind::new("view"))
    }

    fn area(left: f64, top: f64, width: f64, height: f64) -> Area {
        Area { left, top, width, height }
    }

    const DOCK: Area = Area { left: 0.0, top: 0.0, width: 1000.0, height: 600.0 };

    /// A center of two groups side by side, `a` and `b` in the left one and `c` in the right, and
    /// where each is drawn: a 30px strip of 100px tabs over the content.
    fn two_groups() -> (Layout, Vec<GroupArea>) {
        let mut layout = Layout::new();
        let left = layout.add_panel(DockTarget::Region(DockRegion::Center), entry("a")).expect("adds");
        layout.add_panel(DockTarget::Group(left), entry("b")).expect("adds");
        let right = layout.add_panel(DockTarget::Beside { group: left, edge: Edge::Right }, entry("c")).expect("adds");
        let groups = vec![
            GroupArea {
                group: left,
                body: area(0.0, 30.0, 500.0, 570.0),
                tabs: vec![area(0.0, 0.0, 100.0, 30.0), area(100.0, 0.0, 100.0, 30.0)],
            },
            GroupArea {
                group: right,
                body: area(500.0, 30.0, 500.0, 570.0),
                tabs: vec![area(500.0, 0.0, 100.0, 30.0)],
            },
        ];

        (layout, groups)
    }

    fn landing_at(layout: &Layout, groups: &[GroupArea], panel: &str, x: f64, y: f64) -> Option<Landing> {
        landing(layout, &PanelId::new(panel), DOCK, groups, Spot { x, y })
    }

    fn target_at(layout: &Layout, groups: &[GroupArea], panel: &str, x: f64, y: f64) -> Option<DockTarget> {
        landing_at(layout, groups, panel, x, y).map(|landing| landing.target)
    }

    #[test]
    fn the_outer_part_of_a_groups_content_splits_it_and_the_middle_joins_its_tabs() {
        let (layout, groups) = two_groups();
        let right = groups[1].group;
        let at = |x, y| target_at(&layout, &groups, "a", x, y);

        assert_eq!(at(520.0, 300.0), Some(DockTarget::Beside { group: right, edge: Edge::Left }));
        assert_eq!(at(960.0, 300.0), Some(DockTarget::Beside { group: right, edge: Edge::Right }));
        assert_eq!(at(750.0, 40.0), Some(DockTarget::Beside { group: right, edge: Edge::Top }));
        assert_eq!(at(750.0, 560.0), Some(DockTarget::Beside { group: right, edge: Edge::Bottom }));
        assert_eq!(
            at(510.0, 40.0),
            Some(DockTarget::Beside { group: right, edge: Edge::Left }),
            "a side wins a corner"
        );
        assert_eq!(at(750.0, 300.0), Some(DockTarget::Group(right)));
        assert_eq!(at(1200.0, 300.0), None, "outside the dock");
    }

    #[test]
    fn a_split_shows_the_half_it_would_take_and_a_join_the_whole_group() {
        let (layout, groups) = two_groups();
        let preview = |x, y| landing_at(&layout, &groups, "a", x, y).map(|landing| landing.preview);

        assert_eq!(preview(960.0, 300.0), Some(Preview::Room(area(750.0, 30.0, 250.0, 570.0))));
        assert_eq!(preview(750.0, 560.0), Some(Preview::Room(area(500.0, 315.0, 500.0, 285.0))));
        assert_eq!(preview(750.0, 300.0), Some(Preview::Room(groups[1].body)));
    }

    #[test]
    fn a_drop_on_a_strip_goes_into_the_gap_nearest_the_pointer() {
        let (layout, groups) = two_groups();
        let (left, right) = (groups[0].group, groups[1].group);

        assert_eq!(target_at(&layout, &groups, "c", 20.0, 10.0), Some(DockTarget::Tab { group: left, index: 0 }));
        assert_eq!(target_at(&layout, &groups, "c", 120.0, 10.0), Some(DockTarget::Tab { group: left, index: 1 }));
        assert_eq!(target_at(&layout, &groups, "c", 400.0, 10.0), Some(DockTarget::Tab { group: left, index: 2 }));
        assert_eq!(target_at(&layout, &groups, "a", 800.0, 10.0), Some(DockTarget::Tab { group: right, index: 1 }));
        assert_eq!(target_at(&layout, &groups, "a", 400.0, 10.0), Some(DockTarget::Tab { group: left, index: 2 }));

        let gap = landing_at(&layout, &groups, "c", 160.0, 10.0).map(|landing| landing.preview);
        assert_eq!(gap, Some(Preview::Gap(area(199.0, 0.0, 2.0, 30.0))));
    }

    #[test]
    fn a_drop_that_would_leave_the_panel_where_it_is_is_no_drop() {
        let (layout, groups) = two_groups();

        assert_eq!(target_at(&layout, &groups, "a", 250.0, 300.0), None, "the middle of its own group");
        assert_eq!(target_at(&layout, &groups, "a", 20.0, 10.0), None, "the gap before itself");
        assert_eq!(target_at(&layout, &groups, "a", 120.0, 10.0), None, "the gap after itself");
        assert_eq!(target_at(&layout, &groups, "c", 960.0, 300.0), None, "a lone tab beside its own group");
        assert!(target_at(&layout, &groups, "a", 460.0, 300.0).is_some(), "a tab with company splits its group");
    }

    #[test]
    fn a_side_region_that_takes_no_room_catches_a_drop_at_the_docks_edge() {
        let (mut layout, groups) = two_groups();

        assert_eq!(target_at(&layout, &groups, "c", 10.0, 300.0), Some(DockTarget::Region(DockRegion::Left)));
        assert_eq!(target_at(&layout, &groups, "a", 995.0, 300.0), Some(DockTarget::Region(DockRegion::Right)));
        assert_eq!(target_at(&layout, &groups, "a", 750.0, 590.0), Some(DockTarget::Region(DockRegion::Bottom)));
        let room = landing_at(&layout, &groups, "c", 10.0, 300.0).map(|landing| landing.preview);
        assert_eq!(room, Some(Preview::Room(area(0.0, 0.0, 200.0, 600.0))), "it shows the room the region would take");

        layout.add_panel(DockTarget::Region(DockRegion::Left), entry("side")).expect("adds");
        let side = layout.group_of(&PanelId::new("side")).expect("docked");
        let groups =
            [vec![GroupArea { group: side, body: area(0.0, 30.0, 200.0, 570.0), tabs: vec![] }], groups].concat();
        assert_eq!(
            target_at(&layout, &groups, "c", 10.0, 300.0),
            Some(DockTarget::Beside { group: side, edge: Edge::Left }),
            "a region that takes room is reached through its groups"
        );
    }

    #[test]
    fn a_side_regions_room_is_held_to_its_limits_and_the_bottom_lies_under_the_center() {
        let (mut layout, groups) = two_groups();
        layout.extents.right = RegionExtent::Share(Share::new(20.0).expect("in range"));
        layout
            .limits
            .set(FoldingRegion::Right, RegionLimits { min: Some(CssLength::new(280.0).expect("a length")), max: None });
        let room = |x, y| landing_at(&layout, &groups, "a", x, y).map(|landing| landing.preview);

        assert_eq!(room(995.0, 300.0), Some(Preview::Room(area(720.0, 0.0, 280.0, 600.0))));

        layout.add_panel(DockTarget::Region(DockRegion::Left), entry("side")).expect("adds");
        let side = layout.group_of(&PanelId::new("side")).expect("docked");
        let groups =
            [vec![GroupArea { group: side, body: area(0.0, 30.0, 200.0, 570.0), tabs: vec![] }], groups].concat();
        let bottom = landing_at(&layout, &groups, "a", 750.0, 590.0).map(|landing| landing.preview);
        let Some(Preview::Room(bottom)) = bottom else {
            panic!("the bottom region catches the drop: {bottom:?}");
        };
        assert!(bottom.left > 0.0 && bottom.top > 0.0, "beside the left region, under the center: {bottom:?}");
        assert!((bottom.left + bottom.width - DOCK.width).abs() < 1e-9, "out to the dock's right: {bottom:?}");

        let emptied = landing_at(&layout, &groups, "side", 750.0, 590.0).map(|landing| landing.preview);
        let Some(Preview::Room(emptied)) = emptied else {
            panic!("the bottom region catches the drop: {emptied:?}");
        };
        assert_eq!(emptied.left, 0.0, "the left region the move empties gives up its room: {emptied:?}");
        assert!((emptied.width - DOCK.width).abs() < 1e-9, "{emptied:?}");
    }
}

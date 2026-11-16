//! Panel views kept for as long as their panel is in the dock, so a panel moved to another tab
//! group or region, or a region whose tree changed shape around it, keeps its state.
//!
//! A tab group places a panel's view with a [`KeptView`]. The view is built once, in the scope of
//! the [`Dock`](crate::Dock) that draws it, and each placement moves the same nodes in. A
//! placement taken down while its panel is still in the dock leaves the view in a node outside
//! the document until the next placement takes it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::view::Anchor;
use zgui::view::BuildCx;
use zgui::view::BuildCxOwned;
use zgui::view::DomHandle;
use zgui::view::ElementName;
use zgui::view::NodeId;
use zgui::view::View;
use zgui::view::view::AnyViewState;

use crate::dock::layout::PanelEntry;
use crate::dock::layout::PanelId;
use crate::dock::state::DockState;

/// One placement of a panel's view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlacementId(u64);

struct KeptPanel {
    view: AnyViewState,
    owner: Owner,
    /// The placement the view is in, or `None` while it waits outside the document.
    holder: Option<PlacementId>,
    /// Whether the view has been taken down for good.
    gone: bool,
}

impl KeptPanel {
    fn dispose(&mut self, dom: &DomHandle) {
        if self.gone {
            return;
        }
        self.gone = true;
        self.holder = None;
        self.view.unmount(dom);
        self.owner.cleanup();
    }
}

/// The views of one dock's panels.
#[derive(Default)]
pub(crate) struct KeptPanels {
    /// The scope views are built in: the drawing dock's.
    scope: Option<Owner>,
    /// The document the views were built for, once one has been.
    dom: Option<DomHandle>,
    /// Where a view waits between placements, made with the first wait.
    waiting_room: Option<NodeId>,
    panels: HashMap<PanelId, Rc<RefCell<KeptPanel>>>,
    next_placement: u64,
}

impl KeptPanels {
    /// Builds views in `scope` from now on.
    pub(crate) fn set_scope(&mut self, scope: Option<Owner>) {
        self.scope = scope;
    }

    /// Takes every view down, for a dock built again from nothing. The views are handed back to
    /// be disposed of outside the stored value, since disposing runs their cleanups.
    fn take_all(&mut self) -> (Option<DomHandle>, Vec<Rc<RefCell<KeptPanel>>>) {
        (self.dom.clone(), self.panels.drain().map(|(_, kept)| kept).collect())
    }

    fn next_placement(&mut self) -> PlacementId {
        self.next_placement += 1;

        PlacementId(self.next_placement)
    }

    /// The node views wait in while no placement holds them.
    fn waiting_room(&mut self, dom: &DomHandle) -> NodeId {
        *self.waiting_room.get_or_insert_with(|| dom.create_element(ElementName::new("box")))
    }
}

/// Takes down every view `kept` holds.
pub(crate) fn clear(kept: StoredValue<KeptPanels, LocalStorage>) {
    let Some((Some(dom), panels)) = kept.try_update_value(KeptPanels::take_all) else {
        return;
    };
    for panel in panels {
        panel.borrow_mut().dispose(&dom);
    }
}

/// The view of `entry`, built through `cx` in the dock's scope when there is none yet. `None` for
/// a panel whose kind has no factory, or a dock that has gone.
///
/// The view is built outside the stored value, since building it makes reactive values of its own.
fn view_of(state: DockState, entry: &PanelEntry, cx: &BuildCx<'_>) -> Option<Rc<RefCell<KeptPanel>>> {
    let (known, scope) = state.kept.try_with_value(|panels| {
        let known = panels.panels.get(&entry.id).filter(|kept| !kept.borrow().gone).map(Rc::clone);
        (known, panels.scope.clone())
    })?;
    if known.is_some() {
        return known;
    }

    let factory = state.factory(&entry.kind)?;
    let owner = scope.unwrap_or_else(|| cx.owner().clone()).child();
    let built = BuildCxOwned::new(cx.dom_handle(), cx.host_handle(), owner.clone(), cx.document());
    let view = owner.with(|| factory.content(&entry.id).build(&mut built.cx()));
    let kept = Rc::new(RefCell::new(KeptPanel { view, owner, holder: None, gone: false }));
    state.kept.try_update_value(|panels| {
        panels.dom.get_or_insert_with(|| cx.dom_handle());
        panels.panels.insert(entry.id.clone(), Rc::clone(&kept));
    })?;

    Some(kept)
}

/// Places the kept view of one panel.
pub(crate) struct KeptView {
    pub(crate) state: DockState,
    pub(crate) entry: PanelEntry,
}

pub(crate) struct KeptViewState {
    state: DockState,
    panel: PanelId,
    id: PlacementId,
    /// Where the placement sits; the view goes before it.
    marker: NodeId,
    parent: Option<NodeId>,
    kept: Option<Rc<RefCell<KeptPanel>>>,
}

impl KeptViewState {
    /// Moves the view in, from wherever it is.
    fn take_view(&self, dom: &DomHandle) {
        let (Some(parent), Some(kept)) = (self.parent, self.kept.as_ref()) else {
            return;
        };
        let mut kept = kept.borrow_mut();
        if kept.gone {
            return;
        }
        kept.view.mount(dom, parent, Some(self.marker));
        kept.holder = Some(self.id);
    }

    /// Lets go of the view this placement holds: to wait for the next placement while the panel
    /// is still in the dock, and otherwise for good.
    fn let_go(&self, dom: &DomHandle) {
        let Some(kept) = self.kept.as_ref() else {
            return;
        };
        if kept.borrow().holder != Some(self.id) {
            return;
        }
        kept.borrow_mut().holder = None;

        let state = self.state;
        let still_docked = state.layout.try_with_untracked(|layout| layout.group_of(&self.panel).is_some());
        if still_docked == Some(true)
            && let Some(room) = state.kept.try_update_value(|panels| panels.waiting_room(dom))
        {
            kept.borrow_mut().view.mount(dom, room, None);
            return;
        }

        let _ = state.kept.try_update_value(|panels| {
            if panels.panels.get(&self.panel).is_some_and(|known| Rc::ptr_eq(known, kept)) {
                panels.panels.remove(&self.panel);
            }
        });
        kept.borrow_mut().dispose(dom);
    }

    fn holds_view(&self) -> bool {
        self.kept.as_ref().is_some_and(|kept| kept.borrow().holder == Some(self.id))
    }
}

impl View for KeptView {
    type State = KeptViewState;

    fn build(self, cx: &mut BuildCx<'_>) -> KeptViewState {
        let state = self.state;
        let id = state.kept.try_update_value(KeptPanels::next_placement).unwrap_or(PlacementId(0));
        let kept = view_of(state, &self.entry, cx);

        KeptViewState { state, panel: self.entry.id, id, marker: cx.dom().create_marker(), parent: None, kept }
    }

    fn rebuild(self, view: &mut KeptViewState, cx: &mut BuildCx<'_>) {
        if self.entry.id == view.panel {
            return;
        }

        let dom = cx.dom_handle();
        view.let_go(&dom);
        view.kept = view_of(self.state, &self.entry, cx);
        view.panel = self.entry.id;
        view.take_view(&dom);
    }
}

impl Anchor for KeptViewState {
    fn mount(&mut self, dom: &DomHandle, parent: NodeId, before: Option<NodeId>) {
        dom.insert(parent, self.marker, before);
        self.parent = Some(parent);
        self.take_view(dom);
    }

    fn unmount(&mut self, dom: &DomHandle) {
        self.let_go(dom);
        dom.detach(self.marker);
        self.parent = None;
    }

    fn first_node(&self) -> Option<NodeId> {
        let view = self.kept.as_ref().filter(|_| self.holds_view()).and_then(|kept| kept.borrow().view.first_node());

        view.or(Some(self.marker))
    }
}

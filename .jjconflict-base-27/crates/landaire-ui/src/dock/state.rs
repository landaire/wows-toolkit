//! The handle an application drives a dock through, and the factories its panels are made by.

use std::collections::HashMap;
use std::rc::Rc;

use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::view::AnyView;

use crate::dock::kept;
use crate::dock::kept::KeptPanels;
use crate::dock::layout::DockRegion;
use crate::dock::layout::DockTarget;
use crate::dock::layout::FoldingRegion;
use crate::dock::layout::GroupId;
use crate::dock::layout::Layout;
use crate::dock::layout::PanelEntry;
use crate::dock::layout::PanelId;
use crate::dock::layout::PanelKind;
use crate::dock::layout::RegionExtent;
use crate::dock::layout::RegionLimits;
use crate::dock::layout::SplitAxis;
use crate::dock::saved::RestoreReport;
use crate::dock::saved::SavedLayout;
use crate::error::DockError;

/// Makes one view of a panel from its id.
type PanelView = Rc<dyn Fn(&PanelId) -> AnyView>;

/// Makes the parts of one kind of panel, given the panel's id.
#[derive(Clone)]
pub struct PanelFactory {
    label: Rc<dyn Fn(&PanelId) -> String>,
    title: Option<PanelView>,
    content: PanelView,
}

impl PanelFactory {
    /// `label` names the panel to a reader and is its tab's title; `content` is what the panel
    /// shows. The label is read where it is shown, so one that reads a signal follows it.
    pub fn new(label: impl Fn(&PanelId) -> String + 'static, content: impl Fn(&PanelId) -> AnyView + 'static) -> Self {
        Self { label: Rc::new(label), title: None, content: Rc::new(content) }
    }

    /// Draws the tab's title with `title` in place of the label text.
    pub fn with_title(self, title: impl Fn(&PanelId) -> AnyView + 'static) -> Self {
        Self { title: Some(Rc::new(title)), ..self }
    }

    pub(crate) fn label(&self, id: &PanelId) -> String {
        (self.label)(id)
    }

    pub(crate) fn title(&self, id: &PanelId) -> AnyView {
        match &self.title {
            Some(title) => title(id),
            None => {
                let label = Rc::clone(&self.label);
                let id = id.clone();
                AnyView::new(move || label(&id))
            }
        }
    }

    pub(crate) fn content(&self, id: &PanelId) -> AnyView {
        (self.content)(id)
    }
}

/// The factory for each kind of panel a dock can hold.
#[derive(Clone)]
pub struct PanelRegistry {
    factories: HashMap<PanelKind, PanelFactory>,
}

impl PanelRegistry {
    pub fn new() -> Self {
        Self { factories: HashMap::new() }
    }

    /// Makes panels of `kind` with `factory`, in place of any factory registered for it before.
    pub fn register(&mut self, kind: PanelKind, factory: PanelFactory) {
        self.factories.insert(kind, factory);
    }

    pub fn knows(&self, kind: &PanelKind) -> bool {
        self.factories.contains_key(kind)
    }

    pub(crate) fn factory(&self, kind: &PanelKind) -> Option<&PanelFactory> {
        self.factories.get(kind)
    }
}

impl Default for PanelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Which layout a dock's frame was built for. A restore starts a new one, so the frame is built
/// again at the restored sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LayoutGeneration(u64);

/// A dock's layout and panel factories, shared by the [`Dock`](crate::Dock) that draws it and the
/// application that changes it.
///
/// Reads subscribe, so an effect that calls [`dump`](Self::dump) runs again on every change to the
/// layout, which is where an application saves it.
#[derive(Clone, Copy)]
pub struct DockState {
    pub(crate) layout: RwSignal<Layout, LocalStorage>,
    registry: StoredValue<PanelRegistry, LocalStorage>,
    pub(crate) generation: RwSignal<LayoutGeneration, LocalStorage>,
    zoomed: RwSignal<Option<GroupId>, LocalStorage>,
    pub(crate) kept: StoredValue<KeptPanels, LocalStorage>,
}

impl DockState {
    /// An empty dock. Made inside the component that owns it, and gone with it.
    pub fn new(registry: PanelRegistry) -> Self {
        Self {
            layout: RwSignal::new_local(Layout::new()),
            registry: StoredValue::new_local(registry),
            generation: RwSignal::new_local(LayoutGeneration(0)),
            zoomed: RwSignal::new_local(None),
            kept: StoredValue::new_local(KeptPanels::default()),
        }
    }

    pub fn register(self, kind: PanelKind, factory: PanelFactory) {
        self.registry.update_value(|registry| registry.register(kind, factory));
    }

    /// Adds `entry` at `target`, shows it in its tab group and opens the region it lands in.
    pub fn add_panel(self, target: DockTarget, entry: PanelEntry) -> Result<GroupId, DockError> {
        let known = self.registry.try_with_value(|registry| registry.knows(&entry.kind)).ok_or(DockError::Gone)?;
        if !known {
            return Err(DockError::UnknownKind { kind: entry.kind });
        }

        self.change(|layout| layout.add_panel(target, entry))
    }

    /// Moves `panel` to `target` in one change, as dropping its tab there does. The panel keeps
    /// its id, its entry and its view, and shows where it lands. A group left empty is taken out.
    pub fn move_panel(self, panel: &PanelId, target: DockTarget) -> Result<GroupId, DockError> {
        self.change(|layout| layout.move_panel(panel, target))
    }

    /// Puts every tab group in `region` in one split along `axis`, in the order they are laid
    /// out, each keeping the room it had. Groups and panels keep their ids and views.
    pub fn line_up(self, region: DockRegion, axis: SplitAxis) {
        self.layout.update(|layout| layout.line_up(region, axis));
    }

    /// Shows `panel` in its tab group and opens the region it is in.
    pub fn activate(self, panel: &PanelId) -> Result<(), DockError> {
        self.change(|layout| layout.activate(panel))
    }

    /// Takes `panel` out as its tab's close button does, refusing one that is not closable.
    pub fn close(self, panel: &PanelId) -> Result<PanelEntry, DockError> {
        self.change(|layout| layout.close(panel))
    }

    /// Takes `panel` out whether or not it is closable.
    pub fn remove(self, panel: &PanelId) -> Result<PanelEntry, DockError> {
        self.change(|layout| layout.remove(panel))
    }

    pub fn set_region_open(self, region: DockRegion, open: bool) {
        self.layout.update(|layout| layout.set_open(region, open));
        if !open {
            self.unzoom_in(region);
        }
    }

    pub fn toggle_region(self, region: DockRegion) {
        let open = self.layout.with_untracked(|layout| layout.region(region).open);
        self.set_region_open(region, !open);
    }

    /// Puts back a zoomed group in `region`, so the region reopens as laid out.
    fn unzoom_in(self, region: DockRegion) {
        let Some(zoomed) = self.zoomed.with_untracked(|zoomed| *zoomed) else {
            return;
        };
        if self.layout.with_untracked(|layout| layout.region_of(zoomed)) == Some(region) {
            self.zoomed.set(None);
        }
    }

    /// Whether `region` is open. An open region with no panels in it takes no room either.
    pub fn is_region_open(self, region: DockRegion) -> bool {
        self.layout.with(|layout| layout.region(region).open)
    }

    /// Gives `region` `extent` of room while it shows. The extent is saved with the layout.
    pub fn set_region_extent(self, region: FoldingRegion, extent: RegionExtent) {
        self.layout.update(|layout| layout.extents.set(region, extent));
    }

    pub fn region_extent(self, region: FoldingRegion) -> RegionExtent {
        self.layout.with(|layout| layout.extents.get(region))
    }

    /// Keeps `region` within `limits` while it shows. Limits are not saved with the layout and
    /// outlast a [`restore`](Self::restore).
    pub fn set_region_limits(self, region: FoldingRegion, limits: RegionLimits) {
        self.layout.update(|layout| layout.limits.set(region, limits));
    }

    pub fn is_region_empty(self, region: DockRegion) -> bool {
        self.layout.with(|layout| layout.region(region).root.is_none())
    }

    /// Every panel, region by region (center, left, right, bottom) and in tab order.
    pub fn panels(self) -> Vec<PanelEntry> {
        self.layout.with(Layout::panels)
    }

    /// [`panels`](Self::panels) without subscribing the running effect to the layout.
    pub fn panels_untracked(self) -> Vec<PanelEntry> {
        self.layout.with_untracked(Layout::panels)
    }

    pub fn group_of(self, panel: &PanelId) -> Option<GroupId> {
        self.layout.with(|layout| layout.group_of(panel))
    }

    /// The panel `group` shows, or `None` when the group is gone.
    pub fn active_panel(self, group: GroupId) -> Option<PanelId> {
        self.layout.with(|layout| layout.group(group).map(|group| group.active.clone()))
    }

    /// The group drawn over the whole dock, or `None` while the dock is drawn as laid out. A
    /// group that has gone, or whose region does not show, is not zoomed.
    pub fn zoomed(self) -> Option<GroupId> {
        let zoomed = self.zoomed.get()?;

        self.layout.with(|layout| layout.shows_group(zoomed)).then_some(zoomed)
    }

    /// [`zoomed`](Self::zoomed) without subscribing the running effect.
    pub(crate) fn zoomed_untracked(self) -> Option<GroupId> {
        let zoomed = self.zoomed.with_untracked(|zoomed| *zoomed)?;

        self.layout.with_untracked(|layout| layout.shows_group(zoomed)).then_some(zoomed)
    }

    /// Draws `group` over the whole dock, or with `None` the dock as laid out. A group whose
    /// region does not show is not zoomed. The layout itself is unchanged, so this is not saved
    /// with it.
    pub fn set_zoomed(self, group: Option<GroupId>) {
        let group = group.filter(|group| self.layout.with_untracked(|layout| layout.shows_group(*group)));
        self.zoomed.set(group);
    }

    /// The panels on screen: the one each group shows, in the open regions, or only the zoomed
    /// group's. Reactive.
    pub fn shown_panels(self) -> Vec<PanelId> {
        match self.zoomed() {
            Some(group) => self.active_panel(group).into_iter().collect(),
            None => self.layout.with(Layout::shown_panels),
        }
    }

    pub fn dump(self) -> SavedLayout {
        self.layout.with(Layout::dump)
    }

    /// [`dump`](Self::dump) without subscribing the running effect to the layout.
    pub fn dump_untracked(self) -> SavedLayout {
        self.layout.with_untracked(Layout::dump)
    }

    /// Puts the dock back as `saved` describes it, building every panel's view again. Panels whose
    /// kind has no factory, and second panels with an id already used, are left out and listed in
    /// the report.
    pub fn restore(self, saved: &SavedLayout) -> Result<RestoreReport, DockError> {
        let (mut layout, skipped) = self
            .registry
            .try_with_value(|registry| Layout::restore(saved, |kind| registry.knows(kind)))
            .ok_or(DockError::Gone)?;
        layout.limits = self.layout.try_with_untracked(|current| current.limits).ok_or(DockError::Gone)?;

        self.layout.set(layout);
        self.zoomed.set(None);
        kept::clear(self.kept);
        self.generation.update(|generation| generation.0 += 1);

        Ok(RestoreReport { skipped })
    }

    pub(crate) fn factory(self, kind: &PanelKind) -> Option<PanelFactory> {
        self.registry.try_with_value(|registry| registry.factory(kind).cloned()).flatten()
    }

    fn change<T>(self, change: impl FnOnce(&mut Layout) -> Result<T, DockError>) -> Result<T, DockError> {
        self.layout.try_update(change).unwrap_or(Err(DockError::Gone))
    }
}

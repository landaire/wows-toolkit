//! The `Copy` handle a pane holds on one viewport, and what a change made through it carries out.

use std::cell::RefCell;
use std::rc::Rc;

use landaire_ui::MotionPreference;
use landaire_ui::motion_preference;
use zgui::reactive::LocalStorage;
use zgui::reactive::RwSignal;
use zgui::reactive::Signal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::on_cleanup_local;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::WithValue;
use zgui::surface::SurfaceInvalidator;
use zgui::view::AnyView;

use super::model::ViewportModel;
use super::types::ViewportChange;

/// What the viewport's context menu offers from the pane around it.
#[derive(Clone)]
pub struct PaneMenu {
    /// Whether the pane's options rail is open.
    pub options_open: Signal<bool, LocalStorage>,
    pub toggle_options: UnsyncCallback<()>,
    pub open_pen_check: UnsyncCallback<()>,
    /// The pane's quick toggles, appended to the menu.
    pub quick_toggles: UnsyncCallback<(), Vec<AnyView>>,
}

/// One viewport's state, shared by the viewport element, its pane and the panels around it.
///
/// Reads through [`Self::read`] follow every change a panel cares about; the pointer's hovers
/// and the camera move the overlay only, through [`Self::read_overlay`]. Every method is a no-op,
/// or answers with nothing, once the owner the state was made under is gone.
#[derive(Clone, Copy)]
pub struct ArmorViewportState {
    pub(super) model: StoredValue<ViewportModel, LocalStorage>,
    /// Moved by every change a panel reads.
    revision: RwSignal<u64, LocalStorage>,
    /// Moved by every change at all, which is what the overlay and the tooltips follow.
    overlay: RwSignal<u64, LocalStorage>,
    shared: StoredValue<Shared, LocalStorage>,
}

/// What the handle holds besides the model.
struct Shared {
    /// Asks the surface to draw the scene again.
    invalidator: SurfaceInvalidator,
    listeners: Vec<Rc<dyn Fn(ViewportChange)>>,
    pane_menu: Option<PaneMenu>,
    motion: Signal<MotionPreference, LocalStorage>,
}

/// What a change made through [`ArmorViewportState::update`] asks for once the model is no longer
/// borrowed.
pub(super) struct ViewCx {
    notify: bool,
    events: Vec<ViewportChange>,
    deferred: Vec<Box<dyn FnOnce(ArmorViewportState)>>,
    reduce_motion: bool,
}

impl ViewCx {
    /// The panels reading the state draw again.
    pub(super) fn notify(&mut self) {
        self.notify = true;
    }

    /// Tells the pane a user-driven change happened.
    pub(super) fn emit(&mut self, change: ViewportChange) {
        if !self.events.contains(&change) {
            self.events.push(change);
        }
    }

    /// Runs `then` with the handle once the change is done, for work that spawns a task or
    /// reaches other viewports.
    pub(super) fn defer(&mut self, then: impl FnOnce(ArmorViewportState) + 'static) {
        self.deferred.push(Box::new(then));
    }

    pub(super) fn reduce_motion(&self) -> bool {
        self.reduce_motion
    }

    #[cfg(test)]
    pub(super) fn for_test() -> Self {
        Self { notify: false, events: Vec::new(), deferred: Vec::new(), reduce_motion: false }
    }

    #[cfg(test)]
    pub(super) fn events(&self) -> &[ViewportChange] {
        &self.events
    }

    #[cfg(test)]
    pub(super) fn notified(&self) -> bool {
        self.notify
    }
}

thread_local! {
    /// Every live viewport, so a background stage shared by several of them reaches each.
    static LIVE: RefCell<Vec<ArmorViewportState>> = const { RefCell::new(Vec::new()) };
}

impl ArmorViewportState {
    /// A viewport showing nothing. Made under the owner of the pane it belongs to, whose disposal
    /// cancels its loads and lets its meshes go.
    pub fn new() -> Self {
        let state = Self {
            model: StoredValue::new_local(ViewportModel::new()),
            revision: RwSignal::new_local(0),
            overlay: RwSignal::new_local(0),
            shared: StoredValue::new_local(Shared {
                invalidator: SurfaceInvalidator::new(),
                listeners: Vec::new(),
                pane_menu: None,
                motion: motion_preference(),
            }),
        };
        LIVE.with(|live| live.borrow_mut().push(state));
        on_cleanup_local(move || {
            LIVE.with(|live| live.borrow_mut().retain(|held| !held.is(state)));
            state.model.try_update_value(|model| model.release());
        });

        state
    }

    /// Whether `other` is this same viewport.
    pub fn is(self, other: Self) -> bool {
        self.model == other.model
    }

    /// Every viewport still alive.
    pub(super) fn live() -> Vec<Self> {
        LIVE.with(|live| live.borrow().clone())
    }

    /// Reads the model, following every change a panel cares about.
    pub fn read<R>(self, read: impl FnOnce(&ViewportModel) -> R) -> Option<R> {
        self.revision.try_get()?;
        self.model.try_with_value(read)
    }

    /// Reads the model, following every change, the pointer's included.
    pub(super) fn read_overlay<R>(self, read: impl FnOnce(&ViewportModel) -> R) -> Option<R> {
        self.overlay.try_get()?;
        self.model.try_with_value(read)
    }

    /// Reads the model without following it.
    pub(super) fn read_untracked<R>(self, read: impl FnOnce(&ViewportModel) -> R) -> Option<R> {
        self.model.try_with_value(read)
    }

    /// Changes the model, then carries out what the change asked for: the panels and the overlay
    /// follow, the scene is drawn again when it changed, the pane hears of user-driven changes and
    /// deferred work runs. `None` once the state is gone.
    pub(super) fn update<R>(self, change: impl FnOnce(&mut ViewportModel, &mut ViewCx) -> R) -> Option<R> {
        let reduce_motion =
            self.shared.try_with_value(|shared| shared.motion.get_untracked() == MotionPreference::Reduced)?;
        let mut cx = ViewCx { notify: false, events: Vec::new(), deferred: Vec::new(), reduce_motion };
        let (result, dirty) = self.model.try_update_value(|model| {
            let result = change(model, &mut cx);
            (result, model.viewport.is_dirty())
        })?;
        self.settle(cx, dirty);
        Some(result)
    }

    fn settle(self, cx: ViewCx, dirty: bool) {
        let ViewCx { notify, events, deferred, .. } = cx;
        if notify {
            self.revision.try_update(|revision| *revision = revision.wrapping_add(1));
        }
        self.overlay.try_update(|revision| *revision = revision.wrapping_add(1));
        if dirty {
            self.shared.with_value(|shared| shared.invalidator.invalidate());
        }
        if !events.is_empty() {
            let listeners = self.shared.try_with_value(|shared| shared.listeners.clone()).unwrap_or_default();
            for event in events {
                for listener in &listeners {
                    listener(event);
                }
            }
        }
        for then in deferred {
            then(self);
        }
    }

    /// Moves the overlay without touching the model: a frame of an animation it draws.
    pub(super) fn repaint_overlay(self) {
        self.overlay.try_update(|revision| *revision = revision.wrapping_add(1));
    }

    /// The invalidator the surface drawing this viewport is bound to.
    pub(super) fn invalidator(self) -> Option<SurfaceInvalidator> {
        self.shared.try_with_value(|shared| shared.invalidator.clone())
    }

    /// Calls `listener` with every user-driven camera or settings change, for a pane that mirrors
    /// one viewport onto the others.
    pub fn on_change(self, listener: impl Fn(ViewportChange) + 'static) {
        self.shared.try_update_value(|shared| shared.listeners.push(Rc::new(listener)));
    }

    /// What the context menu offers from the pane.
    pub fn set_pane_menu(self, menu: PaneMenu) {
        self.shared.try_update_value(|shared| shared.pane_menu = Some(menu));
    }

    pub(super) fn pane_menu(self) -> Option<PaneMenu> {
        self.shared.try_with_value(|shared| shared.pane_menu.clone()).flatten()
    }

    /// Whether motion is reduced, followed.
    pub(super) fn motion(self) -> MotionPreference {
        self.shared.try_with_value(|shared| shared.motion).map_or(MotionPreference::Full, |motion| motion.get())
    }

    /// The panel revision, for a test that waits on a change.
    #[cfg(test)]
    pub(super) fn revision(self) -> u64 {
        self.revision.get_untracked()
    }
}

impl Default for ArmorViewportState {
    fn default() -> Self {
        Self::new()
    }
}

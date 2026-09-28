//! Single-pane Armor Viewer tab: the ship sidebar (`sidebar::Sidebar`) next
//! to the 3D viewport dock (`dock::ViewportDock`, wrapping `viewport_view::
//! ViewportView`) in a resizable split, matching the layout pattern
//! `replay_inspector::view::ReplayInspectorView` already uses for its browser
//! and dock split. Picking a ship in the sidebar starts a background armor
//! load (`load_ship::spawn_load_ship_armor`); when it completes, the loaded
//! armor is handed to the viewport (`ViewportView::show_armor`), which
//! uploads it and frames the camera.
//!
//! Owns the wgpu device shared by every viewport in the dock (`SharedGpu`,
//! Milestone 5 Task 9a): created once, off the UI thread, and handed down to
//! each viewport as an `Arc<GpuContext>`/`Arc<GpuPipeline>` via `ViewportView::
//! set_gpu` once ready -- see `SharedGpu`'s own doc. `ViewportView` no longer
//! creates its own device.
//!
//! Multi-pane comparison (Milestone 5 Task 9b): the sidebar's "Compare"
//! button (`sidebar::CompareSplit`) adds a new pane to `dock`, handing it the
//! shared device immediately (or leaving it `Initializing` if the device
//! itself isn't ready yet -- `apply_gpu_result` fans the result out to every
//! pane in `dock`, not just the first). A ship selection always routes to
//! `dock`'s *active* pane (`dock.active_viewport()`), not a fixed viewport;
//! there is still exactly one sidebar for the whole tab. `on_compare_split`
//! also clones the active pane's ship/camera/visibility/display/hull/lighting
//! state into the new pane (egui's own `CompareSettings`), so a fresh compare
//! pane starts as a copy rather than blank.
//!
//! Camera-mirror and settings-sync (Milestone 5 Task 9c): `mirror_cameras`/
//! `sync_options` gate two broadcasts, both driven by subscribing to every
//! pane's `viewport_view::ViewportEvent` (`on_viewport_event`, subscribed
//! alongside each pane's creation here and in `on_compare_split`). A
//! `CameraChanged`/`SettingsChanged` event from pane P re-reads P's camera/
//! `SyncedSettings` and applies it to every OTHER pane via `ViewportView`'s
//! SILENT setters (`set_camera`/`apply_synced`, which never emit) -- the
//! silence is what keeps this from echoing back into a loop. Toggling either
//! flag on immediately pushes the *active* pane's state to every other pane,
//! matching the egui app's own "sync on enable" feel. The toggles themselves
//! render in this pane's own chrome (see `Render`), gated on `dock.panes().
//! len() > 1`.
//!
//! Also owns the floating Armor Thickness legend (`legend.rs`): its state
//! (`legend: LegendState`) and drag mouse handlers live here rather than on
//! the legend panel itself, since dragging must keep tracking the pointer
//! past the panel's own small bounds -- see `legend.rs`'s module doc.

use std::collections::HashMap;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::resizable::h_resizable;
use gpui_kit::component::resizable::resizable_panel;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;

use crate::replay_inspector::load::LoadedGameData;
use crate::viewport::device::GpuContext;
use crate::viewport::renderer::GpuPipeline;

use super::assets::ArmorAssetsBundle;
use super::assets::ArmorAssetsError;
use super::assets::spawn_load_armor_assets;
use super::dock::ViewportDock;
use super::legend;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::SelectEvent;
use rust_i18n::t;
use wows_toolkit_viewmodel::armor::penetration::resolve_ship_shells;

use super::analysis;
use super::analysis::PenetrationState;
use super::dock::DockEvent;
use super::legend::LegendDrag;
use super::legend::LegendState;
use super::load_ship;
use super::load_ship::LoadedShipArmor;
use super::load_ship::ShipLoadError;
use super::load_ship::spawn_load_ship_armor;
use super::sidebar::CommonPaneSettings;
use super::sidebar::CompareSplit;
use super::sidebar::ExportModelRequested;
use super::sidebar::ShipSelected;
use super::sidebar::Sidebar;
use super::viewport_view::ViewportEvent;
use super::viewport_view::ViewportView;
use wowsunpack::game_params::types::Millimeters;

/// The status strip above the split. Fixed, so what it says never moves the
/// viewport.
const CHROME_HEIGHT: Pixels = px(20.);

/// Sidebar width, matching the Replay Inspector's own browser sidebar
/// (`replay_inspector::view::BROWSER_WIDTH`).
const SIDEBAR_WIDTH: Pixels = px(240.);
const SIDEBAR_MIN_WIDTH: Pixels = px(180.);
const SIDEBAR_MAX_WIDTH: Pixels = px(480.);

/// Load status of the shared ship-export assets/catalog/icons bundle.
enum BundleState {
    NotStarted,
    Loading,
    Ready(Arc<ArmorAssetsBundle>),
    Failed(String),
}

/// Load status of the currently selected ship's armor model.
enum ShipLoadState {
    Idle,
    Loading { display_name: String },
    Failed { display_name: String, reason: String },
}

/// Lifecycle of the wgpu device shared by every viewport in the pane's dock
/// (`ViewportDock`). Created once, off the UI thread, by `ArmorViewerPane`
/// itself (Milestone 5 Task 9a moved this up from `ViewportView`, which
/// previously stood up its own device per viewport) so the multi-pane split
/// (Task 9b) renders every pane through the same device instead of one per
/// pane.
enum SharedGpu {
    Initializing,
    Ready { ctx: Arc<GpuContext>, pipeline: Arc<GpuPipeline> },
    Failed(String),
}

pub struct ArmorViewerPane {
    sidebar: Entity<Sidebar>,
    dock: Entity<ViewportDock>,
    gpu: SharedGpu,
    bundle: BundleState,
    ship_load: ShipLoadState,
    /// Set once a ship's armor has been shown in `viewport` at least once,
    /// gating the legend overlay exactly like the egui app's `any_ship_loaded`
    /// check (`ui/tab.rs`'s `tab.loaded_armor.is_some()`).
    ship_loaded: bool,
    /// Hits a replay viewport asked for, waiting for the ship they belong on
    /// to finish loading.
    pending_hits: Vec<wows_replay_insights::timeline::PreExtractedHit>,
    /// The floating Armor Thickness legend's visibility/collapsed/position
    /// state; see the module doc and `legend.rs`.
    legend: LegendState,
    /// The fields of the shared defaults row this port has no control for,
    /// kept as read so a save here writes them back unchanged.
    unported_defaults: UnportedDefaults,
    /// Bumped by every `start_ship_load` call and captured into that load's
    /// background task; a completing task whose captured value no longer
    /// matches this field was superseded by a later ship selection and its
    /// result is discarded instead of overwriting `ship_load`/`viewport`.
    /// Guards against rapid clicks (A then B) applying out of order, since
    /// nothing otherwise constrains which of two in-flight loads finishes
    /// last.
    ship_load_generation: u64,
    /// Milestone 5 Task 9c: when set, every pane's camera is kept in lockstep
    /// with whichever pane last moved its own (`on_viewport_event`). Off by
    /// default, matching the egui app's own `mirror_cameras` default.
    mirror_cameras: bool,
    /// Milestone 5 Task 9c: when set, every pane's visibility/display/hull/
    /// lighting settings are kept in lockstep with whichever pane last
    /// changed its own (`on_viewport_event`). Off by default, matching the
    /// egui app's own `sync_options` default.
    sync_options: bool,
    /// The penetration checker's own state.
    pen: PenetrationState,
    /// Whether the checker's panel is up beside the viewport. It stays up
    /// while the pointer sweeps the hull, which is the whole interaction.
    show_analysis: bool,
    _subscriptions: Vec<Subscription>,
}

impl ArmorViewerPane {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sidebar = cx.new(|cx| Sidebar::new(window, cx));
        // Every viewport knows its pane, so the pane's own controls can sit
        // in the viewport's toolbar rather than on a strip above it.
        let this = cx.weak_entity();
        let viewport = cx.new(|cx| {
            let mut viewport = ViewportView::new(cx);
            viewport.set_pane(this);
            viewport
        });
        let viewport_event_sub = cx.subscribe(&viewport, Self::on_viewport_event);
        let dock = cx.new(|_| ViewportDock::new(viewport));
        let ship_selected_sub = cx.subscribe_in(&sidebar, window, Self::on_sidebar_event);
        let compare_split_sub = cx.subscribe_in(&sidebar, window, Self::on_compare_split);
        let export_requested_sub = cx.subscribe_in(&sidebar, window, Self::on_export_model_requested);
        let common_settings_sub = cx.subscribe_in(&sidebar, window, Self::on_common_settings);
        // `dock`'s own `cx.notify()` (add/close/activate pane) only
        // invalidates `ViewportDock`'s render; without this, the common-
        // settings toggle row below -- gated on `dock.panes().len() > 1` --
        // goes stale after a close (e.g. stays visible once back down to one
        // pane) until some unrelated event happens to re-render this pane.
        let dock_sub = cx.observe(&dock, |_this, _dock, cx| cx.notify());
        let dock_event_sub = cx.subscribe_in(&dock, window, Self::on_dock_event);

        let pen = PenetrationState::new(window, cx);
        let show_analysis = false;
        let pen_search_sub =
            cx.subscribe(&pen.search, |_pane, _state, _event: &gpui_kit::component::input::InputEvent, cx| cx.notify());

        let mut this = Self {
            sidebar,
            dock,
            gpu: SharedGpu::Initializing,
            bundle: BundleState::NotStarted,
            ship_load: ShipLoadState::Idle,
            ship_loaded: false,
            pending_hits: Vec::new(),
            legend: LegendState::default(),
            unported_defaults: UnportedDefaults::default(),
            ship_load_generation: 0,
            mirror_cameras: false,
            sync_options: false,
            pen,
            show_analysis,
            _subscriptions: vec![
                pen_search_sub,
                ship_selected_sub,
                compare_split_sub,
                export_requested_sub,
                common_settings_sub,
                viewport_event_sub,
                dock_sub,
                dock_event_sub,
            ],
        };
        this.start_gpu_init(cx);
        this
    }

    /// Stands up the shared wgpu device off the UI thread (device/adapter
    /// negotiation blocks on `pollster` internally), once, for the whole
    /// pane. Replaces `ViewportView`'s former per-viewport device init;
    /// [`Self::apply_gpu_result`] hands the result down to every current (and
    /// every later "Compare"-added) viewport in `dock`.
    fn start_gpu_init(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let created = cx
                .background_spawn(async move {
                    let ctx = GpuContext::new()?;
                    let pipeline = ctx.pipeline();
                    Ok::<_, anyhow::Error>((ctx, pipeline))
                })
                .await;
            let _ = this.update(cx, |this, cx| this.apply_gpu_result(created, cx));
        })
        .detach();
    }

    /// Applies the shared device's init result: on success, stores it as
    /// `SharedGpu::Ready` and hands the `Arc<GpuContext>`/`Arc<GpuPipeline>`
    /// down to every viewport currently in `dock` via `ViewportView::set_gpu`
    /// -- including any pane added by "Compare" while the device was still
    /// initializing (see `add_pane`'s doc); on failure, stores
    /// `SharedGpu::Failed` and propagates the same reason into every pane via
    /// `set_gpu_failed` so each shows the error instead of hanging on
    /// "Initializing...".
    fn apply_gpu_result(&mut self, result: anyhow::Result<(GpuContext, GpuPipeline)>, cx: &mut Context<Self>) {
        let panes = self.dock.read(cx).panes().to_vec();
        match result {
            Ok((ctx, pipeline)) => {
                let ctx = Arc::new(ctx);
                let pipeline = Arc::new(pipeline);
                self.gpu = SharedGpu::Ready { ctx: Arc::clone(&ctx), pipeline: Arc::clone(&pipeline) };
                for pane in panes {
                    pane.update(cx, |viewport, cx| viewport.set_gpu(Arc::clone(&ctx), Arc::clone(&pipeline), cx));
                }
            }
            Err(e) => {
                tracing::error!("armor viewer: failed to create shared wgpu device: {e:#}");
                let reason = format!("{e:#}");
                self.gpu = SharedGpu::Failed(reason.clone());
                for pane in panes {
                    pane.update(cx, |viewport, cx| viewport.set_gpu_failed(reason.clone(), cx));
                }
            }
        }
        cx.notify();
    }

    /// "Compare" handler (`sidebar::CompareSplit`, emitted by the sidebar
    /// header button): creates a new pane, hands it the shared GPU device if
    /// it's already ready (a pane added before the device finishes instead
    /// gets it from `apply_gpu_result`'s fan-out, same as this tab's first
    /// pane), clones the active pane's ship/camera/settings into it (egui's
    /// own `CompareSettings` -- see below), and pushes it into `dock` as the
    /// active pane so the next ship selection loads into it.
    ///
    /// The clone: the new pane `show_armor`s the SAME `Arc<LoadedShipArmor>`
    /// the active pane currently holds (immutable and shared, so this never
    /// re-exports the ship), then overwrites the camera `show_armor` just
    /// framed with the active pane's own camera (`set_camera`, silent) and
    /// applies its visibility/display/hull/lighting settings (`apply_synced`,
    /// silent), and copies its reload source so the new pane can
    /// independently switch hull/LOD/module afterward. If the active pane has
    /// no ship loaded yet, the new pane simply starts empty like before.
    /// Tells the sidebar what the panes share, so its header's own menu
    /// reports the current state.
    fn push_common_settings(&self, cx: &mut Context<Self>) {
        let dock = self.dock.read(cx);
        let common = CommonPaneSettings {
            comparing: dock.panes().len() > 1,
            mirror_cameras: self.mirror_cameras,
            sync_options: self.sync_options,
            stack_panes: dock.split_axis() == Axis::Vertical,
        };
        self.sidebar.update(cx, |sidebar, cx| sidebar.set_common(common, cx));
    }

    /// A change made in the sidebar's pane-sharing menu.
    /// Resolves the ship the penetration checker's combo just picked into the
    /// shells it brings, which is a GameParams walk rather than something to
    /// redo per render.
    /// The catalogue of ships this pane can open, once it has loaded.
    ///
    /// `None` while the assets are still being read, which is what the palette's
    /// armor mode says rather than opening an empty list.
    pub(crate) fn catalog(&self) -> Option<&crate::armor_viewer::catalog::ShipCatalog> {
        match &self.bundle {
            BundleState::Ready(bundle) => Some(&bundle.catalog),
            _ => None,
        }
    }

    /// Adds `param_index` to the ships being compared.
    ///
    /// The search field is cleared, as the egui panel clears it: the ship
    /// asked for is now in the list below, and leaving the text would keep
    /// offering it.
    pub(crate) fn add_comparison_ship(&mut self, param_index: &str, window: &mut Window, cx: &mut Context<Self>) {
        let BundleState::Ready(bundle) = &self.bundle else { return };
        let Some(ship) = resolve_ship_shells(bundle.assets.metadata(), param_index) else { return };
        self.pen.add(ship);
        self.pen.search.update(cx, |state, cx| state.set_value(String::new(), window, cx));
        self.push_cast_ship(cx);
        cx.notify();
    }

    pub(crate) fn remove_comparison_ship(&mut self, index: usize, cx: &mut Context<Self>) {
        self.pen.remove(index);
        self.push_cast_ship(cx);
        cx.notify();
    }

    pub(crate) fn clear_comparison_ships(&mut self, cx: &mut Context<Self>) {
        self.pen.clear();
        self.push_cast_ship(cx);
        cx.notify();
    }

    /// Hands the viewport the ship a trajectory cast fires.
    ///
    /// The first of the compared ships, which is the one the egui viewer
    /// casts the shared ray with; the rest contribute their own arcs there,
    /// which this port does not draw yet.
    fn push_cast_ship(&mut self, cx: &mut Context<Self>) {
        let first = self.pen.ships.first().cloned();
        let viewport = self.dock.read(cx).active_viewport().clone();
        viewport.update(cx, |view, cx| view.set_cast_ship(first, cx));
    }

    /// Whether the checker's panel is up.
    pub(crate) fn analysis_open(&self) -> bool {
        self.show_analysis
    }

    /// How many ships are being compared, which the toolbar button carries.
    pub(crate) fn comparison_count(&self) -> usize {
        self.pen.ships.len()
    }

    pub(crate) fn toggle_analysis(&mut self, cx: &mut Context<Self>) {
        self.show_analysis = !self.show_analysis;
        cx.notify();
    }

    /// Reads the plate the pointer is on into the checker.
    ///
    /// Called as the pane draws rather than when a panel opens: the verdicts
    /// are read against whatever the pointer is over, so they have to follow
    /// it. Returns whether it moved, so a caller can redraw on it.
    fn follow_pointer_plate(&mut self, cx: &mut Context<Self>) -> bool {
        let found = self.dock.read(cx).active_viewport().read(cx).last_plate().map(|(zone, thickness_mm)| {
            super::analysis::PlateUnderPointer { zone, thickness: Millimeters::from(thickness_mm) }
        });
        if found == self.pen.plate {
            return false;
        }
        self.pen.plate = found;
        true
    }

    /// Whether the armor legend is up. Read by the Display menu, which is
    /// where the egui app's own checkbox for it lives.
    pub(crate) fn legend_visible(&self) -> bool {
        self.legend.visible
    }

    /// Shows or hides the armor legend.
    pub(crate) fn set_legend_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        self.legend.visible = visible;
        self.save_defaults(cx);
        cx.notify();
    }

    /// The penetration checker's own state, which its popover reads.
    pub(crate) fn penetration(&self) -> &PenetrationState {
        &self.pen
    }

    /// The penetration checker's IFHE toggle, which its popover reaches
    /// through the pane rather than owning state of its own.
    pub(crate) fn set_pen_ifhe(&mut self, ifhe: bool, cx: &mut Context<Self>) {
        self.pen.ifhe = ifhe;
        cx.notify();
    }

    fn on_common_settings(
        &mut self,
        _sidebar: &Entity<Sidebar>,
        event: &CommonPaneSettings,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if event.mirror_cameras != self.mirror_cameras {
            self.set_mirror_cameras(event.mirror_cameras, cx);
        }
        if event.sync_options != self.sync_options {
            self.set_sync_options(event.sync_options, cx);
        }
        let split = if event.stack_panes { Axis::Vertical } else { Axis::Horizontal };
        self.dock.update(cx, |dock, cx| dock.set_split(split, cx));
        self.push_common_settings(cx);
    }

    /// What the dock asks of its owner.
    fn on_dock_event(
        &mut self,
        _dock: &Entity<ViewportDock>,
        event: &DockEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            DockEvent::ResetLastPane => {
                let viewport = self.fresh_viewport(cx);
                self.dock.update(cx, |dock, cx| dock.reset_to(viewport, cx));
                self.ship_loaded = false;
                self.push_common_settings(cx);
                cx.notify();
            }
        }
    }

    /// A new empty viewport, wired to this pane and handed whatever the
    /// shared GPU device has settled into so far.
    fn fresh_viewport(&mut self, cx: &mut Context<Self>) -> Entity<ViewportView> {
        let this = cx.weak_entity();
        let viewport = cx.new(|cx| {
            let mut viewport = ViewportView::new(cx);
            viewport.set_pane(this);
            viewport
        });
        let viewport_event_sub = cx.subscribe(&viewport, Self::on_viewport_event);
        self._subscriptions.push(viewport_event_sub);

        match &self.gpu {
            SharedGpu::Ready { ctx, pipeline } => {
                let ctx = Arc::clone(ctx);
                let pipeline = Arc::clone(pipeline);
                viewport.update(cx, |viewport, cx| viewport.set_gpu(ctx, pipeline, cx));
            }
            SharedGpu::Failed(reason) => {
                let reason = reason.clone();
                viewport.update(cx, |viewport, cx| viewport.set_gpu_failed(reason, cx));
            }
            SharedGpu::Initializing => {}
        }

        viewport
    }

    fn on_compare_split(
        &mut self,
        _sidebar: &Entity<Sidebar>,
        event: &CompareSplit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active = self.dock.read(cx).active_viewport();
        let clone_source = {
            let source = active.read(cx);
            source
                .current_armor()
                .map(|armor| (armor, source.camera(), source.synced_settings(), source.reload_source()))
        };

        let viewport = self.fresh_viewport(cx);

        if let Some((armor, camera, synced, reload_source)) = clone_source {
            viewport.update(cx, |view, cx| {
                if let Some((bundle, param_index, display_name)) = reload_source {
                    view.set_reload_source(bundle, param_index, display_name);
                }
                view.show_armor(armor, cx);
                view.set_camera(camera, cx);
                view.apply_synced(&synced, cx);
            });
        }

        self.dock.update(cx, |dock, cx| dock.add_pane(viewport, cx));

        // A compare asked for from a ship's own row opens that ship, rather
        // than a copy of the pane it was asked from. The pane added above is
        // the active one by now, so the load lands in it.
        if let Some(ship) = &event.ship {
            self.start_ship_load(ship.param_index.clone(), ship.display_name.clone(), cx);
        }
        self.push_common_settings(cx);
        cx.notify();
    }

    /// `ViewportEvent` handler, subscribed to every pane in `dock` (`new`,
    /// `on_compare_split`): broadcasts the source pane's camera/settings to
    /// every OTHER pane when the corresponding toggle is on. Uses
    /// `ViewportView`'s silent setters (`set_camera`/`apply_synced`), so this
    /// never re-triggers the event it's reacting to.
    fn on_viewport_event(&mut self, source: Entity<ViewportView>, event: &ViewportEvent, cx: &mut Context<Self>) {
        match event {
            ViewportEvent::CameraChanged if self.mirror_cameras => self.push_camera_from(&source, cx),
            ViewportEvent::SettingsChanged => {
                if self.sync_options {
                    self.push_settings_from(&source, cx);
                }
                // The display settings are a preference, so they outlive the
                // session that changed them.
                self.save_defaults(cx);
            }
            _ => {}
        }
    }

    /// Broadcasts `source`'s current camera to every other pane in `dock`.
    /// Called both from `on_viewport_event` (mirroring a user drag/zoom/etc.)
    /// and from `set_mirror_cameras` (pushing once, immediately, when the
    /// toggle is switched on).
    fn push_camera_from(&self, source: &Entity<ViewportView>, cx: &mut Context<Self>) {
        let panes = self.dock.read(cx).panes().to_vec();
        let Some(source_ix) = panes.iter().position(|pane| pane == source) else { return };
        let camera = source.read(cx).camera();
        for ix in other_pane_indices(panes.len(), source_ix) {
            panes[ix].update(cx, |view, cx| view.set_camera(camera.clone(), cx));
        }
    }

    /// Broadcasts `source`'s current visibility/display/hull/lighting
    /// settings to every other pane in `dock`. Called both from
    /// `on_viewport_event` (syncing a user visibility/display change) and
    /// from `set_sync_options` (pushing once, immediately, when the toggle is
    /// switched on).
    fn push_settings_from(&self, source: &Entity<ViewportView>, cx: &mut Context<Self>) {
        let panes = self.dock.read(cx).panes().to_vec();
        let Some(source_ix) = panes.iter().position(|pane| pane == source) else { return };
        let settings = source.read(cx).synced_settings();
        for ix in other_pane_indices(panes.len(), source_ix) {
            panes[ix].update(cx, |view, cx| view.apply_synced(&settings, cx));
        }
    }

    /// Common-settings toggle (`Render`): flips `mirror_cameras`. Turning it
    /// on immediately pushes the *active* pane's camera to every other pane,
    /// matching the egui app's own "sync on enable" feel rather than waiting
    /// for the active pane's next camera move.
    pub(crate) fn set_mirror_cameras(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.mirror_cameras == enabled {
            return;
        }
        self.mirror_cameras = enabled;
        if enabled {
            let active = self.dock.read(cx).active_viewport();
            self.push_camera_from(&active, cx);
        }
        cx.notify();
    }

    /// Common-settings toggle (`Render`): flips `sync_options`. Turning it on
    /// immediately pushes the *active* pane's settings to every other pane,
    /// same "sync on enable" rationale as `set_mirror_cameras`.
    pub(crate) fn set_sync_options(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.sync_options == enabled {
            return;
        }
        self.sync_options = enabled;
        if enabled {
            let active = self.dock.read(cx).active_viewport();
            self.push_settings_from(&active, cx);
        }
        cx.notify();
    }

    /// Seeds the legend's initial visibility/collapsed/position, and every
    /// current pane's display settings (plate edges, waterline, zero-mm
    /// plates, armor opacity -- Task 7b), from the persisted
    /// `armor_viewer_defaults` row, the same way `app.rs` threads the rest of
    /// `GpuiSettings` into its child views on startup. Called once, before
    /// the ship catalog itself has loaded, so `dock` only ever holds its
    /// single initial pane at this point -- no "Compare" pane can exist yet.
    pub fn apply_armor_defaults(&mut self, defaults: Option<&ArmorViewerDefaultsRow>, cx: &mut Context<Self>) {
        if let Some(defaults) = defaults {
            self.unported_defaults = UnportedDefaults { show_splash_boxes: defaults.show_splash_boxes };
        }
        self.legend = LegendState::from_defaults(defaults);
        let panes = self.dock.read(cx).panes().to_vec();
        for pane in panes {
            pane.update(cx, |viewport, cx| viewport.apply_armor_defaults(defaults, cx));
        }
        cx.notify();
    }

    /// Kicks off the Armor Viewer's ship-data load against `loaded` -- the
    /// SAME `Arc<LoadedGameData>` the Replay Inspector preloaded (see
    /// `App`'s wiring in `app.rs`) -- so this never triggers a second VFS/
    /// `GameParams` load for the same build. A no-op if a load already
    /// started (`apply_settings` can reasonably be called more than once by
    /// the app's own settings flow; only the first call should re-trigger).
    pub fn load_game_data(&mut self, loaded: Arc<LoadedGameData>, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.bundle, BundleState::NotStarted) {
            return;
        }
        self.bundle = BundleState::Loading;
        cx.notify();

        let task = spawn_load_armor_assets(loaded, cx);
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| this.apply_bundle_result(result, window, cx));
        })
        .detach();
    }

    fn apply_bundle_result(
        &mut self,
        result: Result<ArmorAssetsBundle, ArmorAssetsError>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(bundle) => {
                let bundle = Arc::new(bundle);
                self.sidebar.update(cx, |sidebar, cx| sidebar.set_bundle(Arc::clone(&bundle), cx));
                // The attacker combo is filled the moment the catalog lands,
                // not lazily in a render pass.
                self.pen.set_catalog(&bundle.catalog);
                self.bundle = BundleState::Ready(bundle);
            }
            Err(e) => {
                tracing::error!("armor viewer: failed to load ship assets: {e}");
                self.bundle = BundleState::Failed(e.to_string());
            }
        }
        cx.notify();
    }

    fn on_sidebar_event(
        &mut self,
        _sidebar: &Entity<Sidebar>,
        event: &ShipSelected,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Mirrors egui's `already_selected` guard (`ui/tab.rs:520-521`): a
        // re-click on the ship already loaded in the ACTIVE pane must not
        // re-trigger `start_ship_load`, which resets part/plate/hull
        // visibility, selected camo, and the undo stack. Only the active
        // pane is checked -- a Compare pane loading the same ship separately
        // is a legitimate, distinct load.
        let active = self.dock.read(cx).active_viewport();
        let loaded = active.read(cx).loaded_param_index();
        if is_ship_already_loaded(loaded, &event.param_index) {
            return;
        }
        self.start_ship_load(event.param_index.clone(), event.display_name.clone(), cx);
    }

    /// Sidebar per-ship "Export model" context-menu handler
    /// (`sidebar::ExportModelRequested`, Milestone 5 Task 10, item 4): asks
    /// where to write `{display_name}.glb` (`crate::dialog`, which keeps the
    /// app drawing while the dialog is open), then exports `event`'s ship at
    /// STOCK hull and default LOD on the background executor -- independent
    /// of whatever hull/LOD/module selection any pane currently displays,
    /// unlike the toolbar's own export (which exports a pane's LIVE
    /// selection). A cancelled dialog, or a request that arrives before the
    /// ship-asset bundle finishes loading, is a no-op.
    fn on_export_model_requested(
        &mut self,
        _sidebar: &Entity<Sidebar>,
        event: &ExportModelRequested,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let BundleState::Ready(bundle) = &self.bundle else {
            tracing::warn!("armor viewer: export requested before ship assets finished loading");
            return;
        };
        let bundle = Arc::clone(bundle);
        let param_index = event.param_index.clone();
        let display_name = event.display_name.clone();

        let asked = crate::dialog::save_file(
            None,
            &load_ship::default_export_filename(&display_name),
            Some(crate::dialog::GLB),
        );
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = asked.await else { return };
            let options = load_ship::export_options_from_selection(load_ship::DEFAULT_LOD, None, HashMap::new());
            let ship = display_name.clone();
            let written = cx
                .background_spawn(async move {
                    let result = load_ship::export_ship_glb(&bundle.assets, &param_index, &options, &path);
                    // The size is what the egui app reports beside the ship,
                    // and it is only knowable once the file is on disk.
                    result.map(|()| {
                        let size = std::fs::metadata(&path).map(|meta| meta.len()).ok();
                        (path, size)
                    })
                })
                .await;

            let _ = this.update_in(cx, |_this, window, cx| load_ship::report_export(written, &ship, window, cx));
        })
        .detach();
    }

    /// Shows `param_index`'s armor with the hits it had taken, for a replay
    /// viewport that asked for one.
    ///
    /// The hits are held rather than drawn here: the viewport they belong on
    /// only exists once the ship's armor has finished loading.
    pub fn show_with_hits(
        &mut self,
        param_index: String,
        display_name: String,
        hits: Vec<wows_replay_insights::timeline::PreExtractedHit>,
        cx: &mut Context<Self>,
    ) {
        self.pending_hits = hits;
        let active = self.dock.read(cx).active_viewport();
        if is_ship_already_loaded(active.read(cx).loaded_param_index(), &param_index) {
            // Already showing it, so only what it has taken has changed.
            let hits = std::mem::take(&mut self.pending_hits);
            active.update(cx, |view, cx| view.show_hits(hits, cx));
            return;
        }
        self.start_ship_load(param_index, display_name, cx);
    }

    /// Updates what the open viewer shows as playback moves.
    ///
    /// Nothing if no ship is loaded: a viewer that was never opened has no
    /// hull to put them on, and loading one here would drag the reader into
    /// a tab they did not ask for.
    pub fn follow_hits(
        &mut self,
        hits: Vec<wows_replay_insights::timeline::PreExtractedHit>,
        health: Option<f32>,
        cx: &mut Context<Self>,
    ) {
        if !self.ship_loaded {
            return;
        }
        let active = self.dock.read(cx).active_viewport();
        active.update(cx, |view, cx| {
            view.set_hit_health(health, cx);
            view.show_hits(hits, cx);
        });
    }

    fn start_ship_load(&mut self, param_index: String, display_name: String, cx: &mut Context<Self>) {
        let BundleState::Ready(bundle) = &self.bundle else {
            tracing::warn!("armor viewer: ship selected before ship assets finished loading");
            return;
        };
        let bundle = Arc::clone(bundle);
        // Captured now (the dock's *current* active pane), not re-read when
        // the load completes: a pane switch (or another "Compare") while
        // this load is in flight must not redirect its result to whatever
        // pane happens to be active later -- it always lands in the pane it
        // was requested for.
        let target_viewport = self.dock.read(cx).active_viewport();

        self.ship_load_generation += 1;
        let generation = self.ship_load_generation;

        self.ship_load = ShipLoadState::Loading { display_name: display_name.clone() };
        // The viewport shows nothing until a ship arrives, so it says what it
        // is waiting for rather than inviting a selection already made.
        target_viewport.update(cx, |viewport, cx| viewport.set_ship_loading(Some(display_name.clone().into()), cx));
        cx.notify();

        let task = spawn_load_ship_armor(Arc::clone(&bundle), param_index.clone(), display_name.clone(), cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.apply_ship_load_result(generation, param_index, display_name, bundle, target_viewport, result, cx)
            });
        })
        .detach();
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_ship_load_result(
        &mut self,
        generation: u64,
        param_index: String,
        display_name: String,
        bundle: Arc<ArmorAssetsBundle>,
        target_viewport: Entity<ViewportView>,
        result: Result<LoadedShipArmor, ShipLoadError>,
        cx: &mut Context<Self>,
    ) {
        if generation != self.ship_load_generation {
            // A newer ship selection superseded this load; discard the
            // result instead of clobbering the current selection's state.
            return;
        }
        match result {
            Ok(armor) => {
                self.ship_load = ShipLoadState::Idle;
                self.ship_loaded = true;
                target_viewport.update(cx, |viewport, cx| {
                    viewport.set_ship_loading(None, cx);
                    // Set before `show_armor`: a reload (Milestone 4 Task 8c)
                    // needs to know which bundle/param_index/display_name to
                    // re-export against, and `show_armor`'s own reset
                    // (`upload_armor_now`) is what clears the hull/LOD/module
                    // selection for this (possibly new) ship.
                    viewport.set_reload_source(bundle, param_index, display_name.clone());
                    viewport.show_armor(Arc::new(armor), cx);
                    // After the armor, because showing it starts the ship
                    // again and would drop them.
                    let hits = std::mem::take(&mut self.pending_hits);
                    if !hits.is_empty() {
                        viewport.show_hits(hits, cx);
                    }
                });
            }
            Err(e) => {
                tracing::error!("armor viewer: failed to load {display_name}: {e}");
                target_viewport.update(cx, |viewport, cx| viewport.set_ship_loading(None, cx));
                self.ship_load = ShipLoadState::Failed { display_name, reason: e.to_string() };
            }
        }
        cx.notify();
    }

    /// The status text shown above the split, if any. A ship load in flight
    /// is not in it: the pane it is loading into says so itself
    /// (`ViewportView::set_ship_loading`), where the user is looking.
    fn status_text(&self) -> Option<String> {
        match &self.bundle {
            BundleState::NotStarted | BundleState::Loading => Some(t!("ui.armor.loading_catalog").into_owned()),
            BundleState::Failed(reason) => Some(t!("ui.armor.catalog_failed", reason = reason).to_string()),
            BundleState::Ready(_) => match &self.ship_load {
                ShipLoadState::Idle | ShipLoadState::Loading { .. } => None,
                ShipLoadState::Failed { display_name, reason } => {
                    Some(t!("ui.armor.ship_load_failed", ship = display_name, reason = reason).into_owned())
                }
            },
        }
    }

    /// Header button handler (`legend::render_panel`): flips the legend
    /// between expanded and collapsed, matching the egui window's own
    /// collapse-triangle toggle.
    pub(crate) fn toggle_legend_collapsed(
        &mut self,
        _event: &ClickEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.legend.collapsed = !self.legend.collapsed;
        self.save_defaults(cx);
        cx.notify();
    }

    /// Header button handler (`legend::render_panel`): hides the legend,
    /// matching the egui window's own close (`open`) toggle.
    pub(crate) fn close_legend(&mut self, _event: &ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.legend.visible = false;
        self.save_defaults(cx);
        cx.notify();
    }

    /// Drag-handle mouse-down (`legend::render_panel`): captures the pointer
    /// and panel position so `drag_legend` can compute the new panel position
    /// from the pointer's total displacement.
    pub(crate) fn start_legend_drag(&mut self, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.legend.drag = Some(LegendDrag { pointer_start: event.position, panel_start: self.legend.pos });
        cx.notify();
    }

    /// Registered on the pane's full-size wrapping div (see `Render`) rather
    /// than the small legend panel, so the drag keeps tracking the pointer
    /// even once it moves past the panel's own bounds -- mirrors
    /// `viewport_view::ViewportView::handle_mouse_move`'s gizmo-drag pattern.
    fn drag_legend(&mut self, event: &MouseMoveEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = self.legend.drag else { return };
        let dx = event.position.x - drag.pointer_start.x;
        let dy = event.position.y - drag.pointer_start.y;
        self.legend.pos = point(drag.panel_start.x + dx, drag.panel_start.y + dy);
        cx.notify();
    }

    fn end_legend_drag(&mut self, _event: &MouseUpEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.legend.drag.take().is_some() {
            self.save_defaults(cx);
            cx.notify();
        }
    }

    /// Writes the viewport's display settings and the legend's placement
    /// back to the `armor_viewer_defaults` row, so the viewer opens the way
    /// it was left.
    ///
    /// The row is a single record covering both, which is why this writes
    /// the whole of it rather than the fields that changed. The active
    /// pane's settings are the ones kept: a comparison pane is a second view
    /// of the same ship, not a second preference.
    pub(crate) fn save_defaults(&self, cx: &mut Context<Self>) {
        let Some(pool) = crate::settings_store::pool(cx) else { return };
        let active = self.dock.read(cx).active_viewport();
        let display = active.read(cx).display_settings;
        // Derived from what the pane is showing, the way the egui app derives
        // them when it saves: they are a record of the current state rather
        // than a setting of their own.
        let all_visible = active.read(cx).all_visible();
        let (visible, collapsed, pos) = (self.legend.visible, self.legend.collapsed, self.legend.pos);
        let unported = self.unported_defaults;
        let row = ArmorViewerDefaultsRow {
            show_plate_edges: display.show_plate_edges,
            show_waterline: display.show_waterline,
            show_zero_mm: display.show_zero_mm,
            armor_opacity: display.armor_opacity as f64,
            waterline_opacity: display.waterline_opacity as f64,
            hull_opaque: display.hull_opaque,
            hull_all_visible: all_visible.hull,
            armor_all_visible: all_visible.armor,
            // Splash boxes belong to a mode the port does not draw, so this
            // is written back exactly as it was read and never overwrites
            // what the other app set.
            show_splash_boxes: unported.show_splash_boxes,
            show_legend: visible,
            legend_collapsed: collapsed,
            legend_pos_x: Some(f32::from(pos.x) as f64),
            legend_pos_y: Some(f32::from(pos.y) as f64),
        };

        cx.spawn(async move |_this, cx| {
            let written = crate::runtime::spawn(cx, async move {
                wows_toolkit_config::queries::save_armor_viewer_defaults(&pool, &row).await
            })
            .await;
            match written {
                Ok(Ok(())) => {}
                Ok(Err(err)) => tracing::warn!("armor viewer: the defaults could not be written: {err}"),
                Err(err) => tracing::warn!("armor viewer: the defaults write did not complete: {err}"),
            }
        })
        .detach();
    }
}

/// The parts of `armor_viewer_defaults` the port does not draw a control
/// for. Defaults match the row's own, for a database with no row yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct UnportedDefaults {
    show_splash_boxes: bool,
}

impl Render for ArmorViewerPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The checker reads its verdicts against whatever the pointer is on,
        // so the plate is taken here, every frame, rather than once when a
        // panel opens.
        if self.follow_pointer_plate(cx) && self.show_analysis {
            cx.notify();
        }

        let analysis = self.show_analysis.then(|| {
            resizable_panel()
                .size(analysis::PANEL_WIDTH)
                .size_range(analysis::PANEL_MIN_WIDTH..analysis::PANEL_MAX_WIDTH)
                .flex_none()
                .child(analysis::render_panel(self, &cx.entity(), cx))
        });

        let content = v_flex().size_full().child(
            div().flex_1().min_h(px(0.)).child(
                h_resizable("armor-viewer-split")
                    .child(
                        resizable_panel()
                            .size(SIDEBAR_WIDTH)
                            .size_range(SIDEBAR_MIN_WIDTH..SIDEBAR_MAX_WIDTH)
                            .flex_none()
                            .child(self.sidebar.clone()),
                    )
                    .child(resizable_panel().child(self.dock.clone()))
                    // `child`, not `children`: the group tracks its panels,
                    // and a plain `ParentElement` child is not one of them.
                    .when_some(analysis, |group, panel| group.child(panel)),
            ),
        );

        // Legend floats over the whole pane (not just the viewport), gated
        // on a ship being loaded, matching the egui app's `any_ship_loaded`
        // window gate (`ui/tab.rs` ~627-628).
        let show_legend = self.ship_loaded && self.legend.visible;
        let legend_panel = show_legend.then(|| legend::render_panel(&self.legend, cx));

        div()
            .id("armor-viewer-pane")
            .relative()
            .size_full()
            .on_mouse_move(cx.listener(Self::drag_legend))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::end_legend_drag))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::end_legend_drag))
            .child(content)
            .when_some(legend_panel, |this, panel| this.child(panel))
            .into_any_element()
    }
}

/// The pane indices, out of `panes_len` panes, that should receive a
/// camera-mirror/settings-sync broadcast from the pane at `source_ix`: every
/// index except the source's own -- a broadcast never re-applies to the pane
/// it came from. Factored out of `ArmorViewerPane::push_camera_from`/
/// `push_settings_from` so the "apply to others, never to self" rule is
/// unit-testable without a gpui_kit `Context`.
fn other_pane_indices(panes_len: usize, source_ix: usize) -> Vec<usize> {
    (0..panes_len).filter(|&ix| ix != source_ix).collect()
}

/// The egui-mirrored "already selected" decision (`ui/tab.rs:520-521`):
/// whether `requested_param_index` is the same ship already loaded in the
/// pane `loaded_param_index` came from. Factored out of `on_sidebar_event`
/// so the decision itself is unit-testable without a `ViewportView`/gpui_kit
/// `Context`. `loaded_param_index` is `None` both when no ship has finished
/// loading yet and when a load is still in flight (see `ViewportView::
/// loaded_param_index`'s doc) -- either way, a `None` never counts as
/// already-loaded, so a re-click always proceeds.
fn is_ship_already_loaded(loaded_param_index: Option<&str>, requested_param_index: &str) -> bool {
    loaded_param_index == Some(requested_param_index)
}

#[cfg(test)]
mod tests {
    use super::is_ship_already_loaded;
    use super::other_pane_indices;

    #[test]
    fn already_loaded_when_the_active_pane_holds_the_same_param_index() {
        assert!(is_ship_already_loaded(Some("PASC001"), "PASC001"));
    }

    #[test]
    fn not_already_loaded_when_the_active_pane_holds_a_different_ship() {
        assert!(!is_ship_already_loaded(Some("PASC001"), "PASC002"));
    }

    #[test]
    fn not_already_loaded_when_the_active_pane_has_no_ship_yet() {
        assert!(!is_ship_already_loaded(None, "PASC001"));
    }

    #[test]
    fn other_pane_indices_excludes_only_the_source() {
        assert_eq!(other_pane_indices(3, 1), vec![0, 2]);
    }

    #[test]
    fn other_pane_indices_empty_when_source_is_the_only_pane() {
        assert_eq!(other_pane_indices(1, 0), Vec::<usize>::new());
    }

    #[test]
    fn other_pane_indices_covers_every_pane_but_the_first() {
        assert_eq!(other_pane_indices(4, 0), vec![1, 2, 3]);
    }

    #[test]
    fn other_pane_indices_covers_every_pane_but_the_last() {
        assert_eq!(other_pane_indices(4, 3), vec![0, 1, 2]);
    }
}

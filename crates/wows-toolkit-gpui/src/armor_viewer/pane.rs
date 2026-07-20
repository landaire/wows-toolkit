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
//! there is still exactly one sidebar for the whole tab. Camera-mirror and
//! settings-sync across panes are Task 9c, not implemented here.
//!
//! Also owns the floating Armor Thickness legend (`legend.rs`): its state
//! (`legend: LegendState`) and drag mouse handlers live here rather than on
//! the legend panel itself, since dragging must keep tracking the pointer
//! past the panel's own small bounds -- see `legend.rs`'s module doc.

use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::h_flex;
use gpui_component::resizable::h_resizable;
use gpui_component::resizable::resizable_panel;
use gpui_component::v_flex;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;

use crate::replay_inspector::load::LoadedGameData;
use crate::viewport::device::GpuContext;
use crate::viewport::renderer::GpuPipeline;

use super::assets::ArmorAssetsBundle;
use super::assets::ArmorAssetsError;
use super::assets::spawn_load_armor_assets;
use super::dock::ViewportDock;
use super::legend;
use super::legend::LegendDrag;
use super::legend::LegendState;
use super::load_ship::LoadedShipArmor;
use super::load_ship::ShipLoadError;
use super::load_ship::spawn_load_ship_armor;
use super::sidebar::CompareSplit;
use super::sidebar::ShipSelected;
use super::sidebar::Sidebar;
use super::viewport_view::ViewportView;

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
    /// The floating Armor Thickness legend's visibility/collapsed/position
    /// state; see the module doc and `legend.rs`.
    legend: LegendState,
    /// Bumped by every `start_ship_load` call and captured into that load's
    /// background task; a completing task whose captured value no longer
    /// matches this field was superseded by a later ship selection and its
    /// result is discarded instead of overwriting `ship_load`/`viewport`.
    /// Guards against rapid clicks (A then B) applying out of order, since
    /// nothing otherwise constrains which of two in-flight loads finishes
    /// last.
    ship_load_generation: u64,
    _subscriptions: Vec<Subscription>,
}

impl ArmorViewerPane {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sidebar = cx.new(|cx| Sidebar::new(window, cx));
        let viewport = cx.new(ViewportView::new);
        let dock = cx.new(|_| ViewportDock::new(viewport));
        let ship_selected_sub = cx.subscribe_in(&sidebar, window, Self::on_sidebar_event);
        let compare_split_sub = cx.subscribe_in(&sidebar, window, Self::on_compare_split);

        let mut this = Self {
            sidebar,
            dock,
            gpu: SharedGpu::Initializing,
            bundle: BundleState::NotStarted,
            ship_load: ShipLoadState::Idle,
            ship_loaded: false,
            legend: LegendState::default(),
            ship_load_generation: 0,
            _subscriptions: vec![ship_selected_sub, compare_split_sub],
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
    /// pane), and pushes it into `dock` as the active pane so the next ship
    /// selection loads into it. The new pane starts empty until then.
    fn on_compare_split(
        &mut self,
        _sidebar: &Entity<Sidebar>,
        _event: &CompareSplit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let viewport = cx.new(ViewportView::new);
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
        self.dock.update(cx, |dock, cx| dock.add_pane(viewport, cx));
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
    pub fn load_game_data(&mut self, loaded: Arc<LoadedGameData>, cx: &mut Context<Self>) {
        if !matches!(self.bundle, BundleState::NotStarted) {
            return;
        }
        self.bundle = BundleState::Loading;
        cx.notify();

        let task = spawn_load_armor_assets(loaded, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| this.apply_bundle_result(result, cx));
        })
        .detach();
    }

    fn apply_bundle_result(&mut self, result: Result<ArmorAssetsBundle, ArmorAssetsError>, cx: &mut Context<Self>) {
        match result {
            Ok(bundle) => {
                let bundle = Arc::new(bundle);
                self.sidebar.update(cx, |sidebar, cx| sidebar.set_bundle(Arc::clone(&bundle), cx));
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
        self.start_ship_load(event.param_index.clone(), event.display_name.clone(), cx);
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
                    // Set before `show_armor`: a reload (Milestone 4 Task 8c)
                    // needs to know which bundle/param_index/display_name to
                    // re-export against, and `show_armor`'s own reset
                    // (`upload_armor_now`) is what clears the hull/LOD/module
                    // selection for this (possibly new) ship.
                    viewport.set_reload_source(bundle, param_index, display_name.clone());
                    viewport.show_armor(Arc::new(armor), cx);
                });
            }
            Err(e) => {
                tracing::error!("armor viewer: failed to load {display_name}: {e}");
                self.ship_load = ShipLoadState::Failed { display_name, reason: e.to_string() };
            }
        }
        cx.notify();
    }

    /// The status text shown above the split, if any: bundle load progress/
    /// failure takes priority over a ship load's own status. `None` once
    /// everything has settled into a normal idle state, matching
    /// `replay_inspector::view`'s `status_banner` pattern.
    fn status_text(&self) -> Option<String> {
        match &self.bundle {
            BundleState::NotStarted | BundleState::Loading => Some("Loading ship catalog...".to_string()),
            BundleState::Failed(reason) => Some(format!("Failed to load ship catalog: {reason}")),
            BundleState::Ready(_) => match &self.ship_load {
                ShipLoadState::Idle => None,
                ShipLoadState::Loading { display_name } => Some(format!("Loading {display_name}...")),
                ShipLoadState::Failed { display_name, reason } => {
                    Some(format!("Failed to load {display_name}: {reason}"))
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
        cx.notify();
    }

    /// Header button handler (`legend::render_panel`): hides the legend,
    /// matching the egui window's own close (`open`) toggle.
    pub(crate) fn close_legend(&mut self, _event: &ClickEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.legend.visible = false;
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
            cx.notify();
        }
        // TODO: persist `self.legend.pos`/`visible`/`collapsed` here once a
        // general settings write-back path exists. `settings.rs` is
        // documented read-only (the port has no shared DB pool handle this
        // pane could reuse, and `save_armor_viewer_defaults` writes the
        // whole `armor_viewer_defaults` row, not just the legend fields), so
        // legend placement/visibility is in-session only for now: it resets
        // to the persisted (or default) position/state on every restart.
    }
}

impl Render for ArmorViewerPane {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status_banner = self
            .status_text()
            .map(|text| h_flex().flex_none().px_2().py_1().child(div().text_xs().opacity(0.6).child(text)));

        let content = v_flex().size_full().when_some(status_banner, |this, banner| this.child(banner)).child(
            div().flex_1().min_h(px(0.)).child(
                h_resizable("armor-viewer-split")
                    .child(
                        resizable_panel()
                            .size(SIDEBAR_WIDTH)
                            .size_range(SIDEBAR_MIN_WIDTH..SIDEBAR_MAX_WIDTH)
                            .flex_none()
                            .child(self.sidebar.clone()),
                    )
                    .child(resizable_panel().child(self.dock.clone())),
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

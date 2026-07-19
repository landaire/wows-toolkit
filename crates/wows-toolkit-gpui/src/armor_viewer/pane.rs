//! Single-pane Armor Viewer tab: the ship sidebar (`sidebar::Sidebar`) next
//! to the 3D viewport (`viewport_view::ViewportView`) in a resizable split,
//! matching the layout pattern `replay_inspector::view::ReplayInspectorView`
//! already uses for its browser + dock split. Picking a ship in the sidebar
//! starts a background armor load (`load_ship::spawn_load_ship_armor`); when
//! it completes, the loaded armor is handed to the viewport
//! (`ViewportView::show_armor`), which uploads it and frames the camera.
//!
//! Multi-pane comparison (Milestone 5) does not exist yet, so there is
//! exactly one sidebar and one viewport for the whole tab.
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

use super::assets::ArmorAssetsBundle;
use super::assets::ArmorAssetsError;
use super::assets::spawn_load_armor_assets;
use super::legend;
use super::legend::LegendDrag;
use super::legend::LegendState;
use super::load_ship::LoadedShipArmor;
use super::load_ship::ShipLoadError;
use super::load_ship::spawn_load_ship_armor;
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

pub struct ArmorViewerPane {
    sidebar: Entity<Sidebar>,
    viewport: Entity<ViewportView>,
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
        let subscription = cx.subscribe_in(&sidebar, window, Self::on_sidebar_event);

        Self {
            sidebar,
            viewport,
            bundle: BundleState::NotStarted,
            ship_load: ShipLoadState::Idle,
            ship_loaded: false,
            legend: LegendState::default(),
            ship_load_generation: 0,
            _subscriptions: vec![subscription],
        }
    }

    /// Seeds the legend's initial visibility/collapsed/position, and the
    /// viewport's display settings (plate edges, waterline, zero-mm plates,
    /// armor opacity -- Task 7b), from the persisted `armor_viewer_defaults`
    /// row, the same way `app.rs` threads the rest of `GpuiSettings` into its
    /// child views on startup.
    pub fn apply_armor_defaults(&mut self, defaults: Option<&ArmorViewerDefaultsRow>, cx: &mut Context<Self>) {
        self.legend = LegendState::from_defaults(defaults);
        self.viewport.update(cx, |viewport, cx| viewport.apply_armor_defaults(defaults, cx));
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

        self.ship_load_generation += 1;
        let generation = self.ship_load_generation;

        self.ship_load = ShipLoadState::Loading { display_name: display_name.clone() };
        cx.notify();

        let task = spawn_load_ship_armor(Arc::clone(bundle), param_index, display_name.clone(), cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| this.apply_ship_load_result(generation, display_name, result, cx));
        })
        .detach();
    }

    fn apply_ship_load_result(
        &mut self,
        generation: u64,
        display_name: String,
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
                self.viewport.update(cx, |viewport, cx| viewport.show_armor(Arc::new(armor), cx));
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
                    .child(resizable_panel().child(self.viewport.clone())),
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

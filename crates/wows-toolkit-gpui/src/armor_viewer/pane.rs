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

use std::sync::Arc;

use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::h_flex;
use gpui_component::resizable::h_resizable;
use gpui_component::resizable::resizable_panel;
use gpui_component::v_flex;

use crate::replay_inspector::load::LoadedGameData;

use super::assets::ArmorAssetsBundle;
use super::assets::ArmorAssetsError;
use super::assets::spawn_load_armor_assets;
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
            _subscriptions: vec![subscription],
        }
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

        self.ship_load = ShipLoadState::Loading { display_name: display_name.clone() };
        cx.notify();

        let task = spawn_load_ship_armor(Arc::clone(bundle), param_index, display_name.clone(), cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| this.apply_ship_load_result(display_name, result, cx));
        })
        .detach();
    }

    fn apply_ship_load_result(
        &mut self,
        display_name: String,
        result: Result<LoadedShipArmor, ShipLoadError>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(armor) => {
                self.ship_load = ShipLoadState::Idle;
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
}

impl Render for ArmorViewerPane {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let status_banner = self
            .status_text()
            .map(|text| h_flex().flex_none().px_2().py_1().child(div().text_xs().opacity(0.6).child(text)));

        v_flex()
            .size_full()
            .when_some(status_banner, |this, banner| this.child(banner))
            .child(
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
            )
            .into_any_element()
    }
}

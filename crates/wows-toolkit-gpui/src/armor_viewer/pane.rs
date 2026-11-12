//! Single-pane Armor Viewer tab: the ship sidebar (`sidebar::Sidebar`) next
//! to the 3D viewport dock (`dock::ViewportDock`, wrapping `viewport_view::
//! ViewportView`) in a resizable split, matching the layout pattern
//! `replay_inspector::view::ReplayInspectorView` already uses for its browser
//! and dock split. Picking a ship in the sidebar starts a background armor
//! load (`load_ship::spawn_load_ship_armor`); when it completes, the loaded
//! armor is handed to the viewport (`ViewportView::show_armor`), which
//! uploads it and frames the camera.
//!
//! Every viewport in the dock draws on GPUI's own wgpu device through one
//! `SharedSceneDevice`, so they share one pipeline; the pane hands each new
//! viewport a clone of it. A lost device releases everything made on it at
//! once, and the viewports move to the new one when they next render.
//!
//! Multi-pane comparison (Milestone 5 Task 9b): the sidebar's "Compare"
//! button (`sidebar::CompareSplit`) adds a new pane to `dock`. A ship
//! selection always routes to
//! `dock`'s *active* pane (`dock.active_viewport()`), not a fixed viewport;
//! there is still exactly one sidebar for the whole tab. `on_compare_split`
//! also clones the active pane's ship/camera/visibility/display/hull/lighting
//! state into the new pane (egui's own `CompareSettings`), so a fresh compare
//! pane starts as a copy rather than blank.
//!
//! Camera-mirror and settings-sync (Milestone 5 Task 9c): `mirror_cameras`/
//! `sync_options` gate two broadcasts, both driven by subscribing to every
//! pane's `viewport_view::ViewportChange` (`on_viewport_event`, subscribed
//! alongside each pane's creation here and in `on_compare_split`). A
//! `Camera`/`Settings` event from pane P re-reads P's camera/
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
use std::collections::HashSet;
use std::sync::Arc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::component::Disableable;
use gpui_kit::component::Selectable;
use gpui_kit::component::Sizable;
use gpui_kit::component::button::Button;
use gpui_kit::component::button::ButtonVariants as _;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::h_flex;
use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;

use crate::motion;
use crate::replay_inspector::load::LoadedGameData;
use crate::side_split::SideSplit;
use crate::side_split::SideWidth;
use crate::status_bar;
use crate::status_bar::ModeHint;
use crate::status_bar::ModifierKey;
use crate::viewport::device::SharedSceneDevice;

use gpui_kit::component::WindowExt as _;

use super::assets::ArmorAssetsBundle;
use super::assets::ArmorAssetsError;
use super::assets::ModelAssets;
use super::assets::ModelLibraryLoadError;
use super::assets::spawn_build_model_library;
use super::assets::spawn_load_armor_assets;
use super::dock::ViewportDock;
use super::load_model::ModelLoadRequest;
use super::load_model::ModelSceneError;
use super::load_model::spawn_load_model_scene;
use super::model_refs::HardpointTarget;
use super::model_refs::ShipPartRef;
use super::sidebar::ExportCatalogModelRequested;
use super::sidebar::HardpointsToggled;
use super::sidebar::ModelSelected;
use super::viewport_view::SceneSource;
use wows_replays::types::EntityId;
use wows_toolkit_viewmodel::armor::incoming::IncomingFilter;
use wowsunpack::export::model_catalog::ModelPath;

use super::export_dialog;
use super::legend;
use gpui_kit::component::popover::Popover;
use gpui_kit::component::searchable_list::SearchableVec;
use gpui_kit::component::select::SelectEvent;
use rust_i18n::t;
use wows_toolkit_viewmodel::armor::penetration::resolve_ship_shells;
use wows_toolkit_viewmodel::settings::keys;

use super::analysis::PenetrationState;
use super::dock::DockEvent;
use super::dock::PanePosition;
use super::legend::LegendDrag;
use super::legend::LegendState;
use super::load_ship;
use super::load_ship::LoadedShipArmor;
use super::load_ship::ShipLoadError;
use super::load_ship::spawn_load_ship_armor;
use super::options_panel;
use super::sidebar::CommonPaneSettings;
use super::sidebar::CompareSplit;
use super::sidebar::ExportModelRequested;
use super::sidebar::ShipSelected;
use super::sidebar::Sidebar;
use super::viewport_view::CastShell;
use super::viewport_view::LoadGeneration;
use super::viewport_view::OpeningLayer;
use super::viewport_view::ViewportChange;
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

/// Load status of the model library and mount index that the Models list,
/// model loads and model exports read.
enum ModelsState {
    /// Waiting on the ship bundle it is built over.
    NotStarted,
    Loading,
    Ready(Arc<ModelAssets>),
    Failed(String),
}

/// The ship catalogue as a reader outside the pane sees it.
pub(crate) enum CatalogStatus<'a> {
    /// Not loaded yet; it arrives once the build's assets have been read.
    Pending,
    Ready(&'a crate::armor_viewer::catalog::ShipCatalog),
    Failed(&'a str),
}

/// Load status of the currently selected ship's armor model.
enum ShipLoadState {
    Idle,
    Loading { display_name: String },
    Failed { display_name: String, reason: String },
}

pub struct ArmorViewerPane {
    sidebar: Entity<Sidebar>,
    dock: Entity<ViewportDock>,
    scene: SharedSceneDevice,
    bundle: BundleState,
    /// The model library, built once the bundle has loaded.
    models: ModelsState,
    /// Whose hardpoints are marked, as the sidebar's checkboxes left them.
    /// Every viewport draws the ones that belong to its scene.
    hardpoint_targets: HashSet<HardpointTarget>,
    /// The export choices being collected, held so the read behind them can
    /// report into the dialog that is showing them.
    export_dialog: Option<Entity<export_dialog::ExportDialogState>>,
    /// Kept for as long as that dialog is open: it is what redraws the window
    /// when a read reports into it.
    _export_dialog_subscription: Option<Subscription>,
    ship_load: ShipLoadState,
    /// Set once a ship's armor has been shown in `viewport` at least once,
    /// gating the legend overlay exactly like the egui app's `any_ship_loaded`
    /// check (`ui/tab.rs`'s `tab.loaded_armor.is_some()`).
    ship_loaded: bool,
    /// Hits a replay viewport asked for, waiting for the ship load that will
    /// show them.
    pending_hits: Option<PendingHits>,
    /// What was fired at the ship on screen, for the log beside it. Empty for a
    /// ship opened from the catalogue rather than from a battle.
    incoming: IncomingFire,
    /// Where the playback feeding this viewer has reached, which the health
    /// strip marks.
    playback_at: Option<wows_replays::types::GameClock>,
    /// The floating Armor Thickness legend's visibility/collapsed/position
    /// state; see the module doc and `legend.rs`.
    legend: LegendState,
    /// The fields of the shared defaults row this port has no control for,
    /// kept as read so a save here writes them back unchanged.
    unported_defaults: UnportedDefaults,
    /// Milestone 5 Task 9c: when set, every pane's camera is kept in lockstep
    /// with whichever pane last moved its own (`on_viewport_event`). Off by
    /// default, matching the egui app's own `mirror_cameras` default.
    mirror_cameras: bool,
    /// Milestone 5 Task 9c: when set, every pane's visibility/display/hull/
    /// lighting settings are kept in lockstep with whichever pane last
    /// changed its own (`on_viewport_event`). Off by default, matching the
    /// egui app's own `sync_options` default.
    sync_options: bool,
    /// What a model opened in this pane shows first. Persisted under
    /// `keys::MODEL_VIEWER_SHOW_ARMOR`.
    opening_layer: OpeningLayer,
    /// The penetration checker's own state.
    pen: PenetrationState,
    /// Whether the options rail is up. Open by default: it holds the viewport
    /// tools, the trajectory and penetration sections and the options.
    show_options: bool,
    /// Whether the ships list is collapsed to its expand rail.
    sidebar_collapsed: bool,
    /// How wide the ships list is drawn, kept while it is collapsed.
    sidebar_width: Entity<SideWidth>,
    /// Which of the rail's sections are expanded.
    option_sections: OptionSectionsOpen,
    /// What the rail's filter field holds.
    options_filter: Entity<gpui_kit::component::input::InputState>,
    _subscriptions: Vec<Subscription>,
}

/// A section of the options rail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OptionSection {
    Trajectory,
    PenCheck,
    Visibility,
    Hull,
    Display,
    /// The camera ring and camera lock controls inside Display.
    CameraRings,
    /// The lighting controls inside Display.
    Lighting,
}

/// Which sections of the options rail are expanded.
///
/// A collapsed section is not built at all. The rail is rendered on every
/// window repaint -- a hull hover notifies -- and the two trees clone the
/// visibility maps and emit a row per zone, part and plate, which is what the
/// popovers only paid for while they were open.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OptionSectionsOpen {
    pub trajectory: bool,
    pub pen_check: bool,
    pub visibility: bool,
    pub hull: bool,
    pub display: bool,
    pub camera_rings: bool,
    pub lighting: bool,
}

impl Default for OptionSectionsOpen {
    /// Trajectory and Display: the arcs cast are read there, and Display is
    /// the section read most often and the cheapest of the trees to build.
    /// The camera and lighting groups start closed, since most readers only
    /// ever touch the model settings above them.
    fn default() -> Self {
        Self {
            trajectory: true,
            pen_check: false,
            visibility: false,
            hull: false,
            display: true,
            camera_rings: false,
            lighting: false,
        }
    }
}

impl OptionSectionsOpen {
    pub fn is_open(self, section: OptionSection) -> bool {
        match section {
            OptionSection::Trajectory => self.trajectory,
            OptionSection::PenCheck => self.pen_check,
            OptionSection::Visibility => self.visibility,
            OptionSection::Hull => self.hull,
            OptionSection::Display => self.display,
            OptionSection::CameraRings => self.camera_rings,
            OptionSection::Lighting => self.lighting,
        }
    }

    fn open_mut(&mut self, section: OptionSection) -> &mut bool {
        match section {
            OptionSection::Trajectory => &mut self.trajectory,
            OptionSection::PenCheck => &mut self.pen_check,
            OptionSection::Visibility => &mut self.visibility,
            OptionSection::Hull => &mut self.hull,
            OptionSection::Display => &mut self.display,
            OptionSection::CameraRings => &mut self.camera_rings,
            OptionSection::Lighting => &mut self.lighting,
        }
    }

    fn toggle(&mut self, section: OptionSection) {
        let open = self.open_mut(section);
        *open = !*open;
    }
}

impl ArmorViewerPane {
    /// The side panels this tab can show or put away, for the status bar.
    pub(crate) fn panel_toggles(&self, entity: &Entity<Self>) -> Vec<crate::status_bar::PanelToggle> {
        let ships = entity.downgrade();
        let options = entity.downgrade();
        vec![
            crate::status_bar::PanelToggle {
                id: "armor-ships-toggle".into(),
                name: t!("ui.armor.ships").into_owned().into(),
                side: crate::ui::PanelSide::Left,
                open: !self.sidebar_collapsed,
                toggle: std::rc::Rc::new(move |_window, cx| {
                    let _ = ships.update(cx, |pane, cx| {
                        let collapsed = pane.sidebar_collapsed;
                        pane.set_sidebar_collapsed(!collapsed, cx);
                    });
                }),
            },
            crate::status_bar::PanelToggle {
                id: "armor-options-toggle".into(),
                name: t!("ui.armor.options").into_owned().into(),
                side: crate::ui::PanelSide::Right,
                open: self.show_options,
                toggle: std::rc::Rc::new(move |_window, cx| {
                    let _ = options.update(cx, |pane, cx| pane.toggle_options(cx));
                }),
            },
        ]
    }

    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sidebar = cx.new(|cx| Sidebar::new(window, cx));
        // Every viewport knows its pane, so its context menu can offer the
        // pane's own controls.
        let this = cx.weak_entity();
        let scene = SharedSceneDevice::default();
        let viewport = cx.new(|cx| {
            let mut viewport = ViewportView::new(scene.clone(), window, cx);
            viewport.set_pane(this);
            viewport
        });
        let viewport_event_sub = cx.subscribe(&viewport, Self::on_viewport_event);
        let dock = cx.new(|_| ViewportDock::new(viewport));
        let ship_selected_sub = cx.subscribe_in(&sidebar, window, Self::on_sidebar_event);
        let compare_split_sub = cx.subscribe_in(&sidebar, window, Self::on_compare_split);
        let export_requested_sub = cx.subscribe_in(&sidebar, window, Self::on_export_model_requested);
        let common_settings_sub = cx.subscribe_in(&sidebar, window, Self::on_common_settings);
        let model_selected_sub = cx.subscribe_in(&sidebar, window, Self::on_model_selected);
        let hardpoints_sub = cx.subscribe_in(&sidebar, window, Self::on_hardpoints_toggled);
        let export_catalog_sub = cx.subscribe_in(&sidebar, window, Self::on_export_catalog_model_requested);
        // `dock`'s own `cx.notify()` (add/close/activate pane) only
        // invalidates `ViewportDock`'s render; without this, the common-
        // settings toggle row below -- gated on `dock.panes().len() > 1` --
        // goes stale after a close (e.g. stays visible once back down to one
        // pane) until some unrelated event happens to re-render this pane.
        let dock_sub = cx.observe(&dock, |this, _dock, cx| {
            this.push_open_target(cx);
            cx.notify();
        });
        let dock_event_sub = cx.subscribe_in(&dock, window, Self::on_dock_event);

        let pen = PenetrationState::new(window, cx);
        let pen_search_sub =
            cx.subscribe(&pen.search, |_pane, _state, _event: &gpui_kit::component::input::InputEvent, cx| cx.notify());
        let options_filter = cx.new(|cx| {
            gpui_kit::component::input::InputState::new(window, cx)
                .placeholder(t!("ui.armor.filter_options").into_owned())
        });
        let options_filter_sub = cx
            .subscribe(&options_filter, |_pane, _state, _event: &gpui_kit::component::input::InputEvent, cx| {
                cx.notify()
            });

        Self {
            sidebar,
            dock,
            scene,
            bundle: BundleState::NotStarted,
            models: ModelsState::NotStarted,
            hardpoint_targets: HashSet::new(),
            export_dialog: None,
            _export_dialog_subscription: None,
            ship_load: ShipLoadState::Idle,
            ship_loaded: false,
            pending_hits: None,
            incoming: IncomingFire::default(),
            playback_at: None,
            legend: LegendState::default(),
            unported_defaults: UnportedDefaults::default(),
            mirror_cameras: false,
            sync_options: false,
            opening_layer: OpeningLayer::default(),
            pen,
            show_options: true,
            sidebar_collapsed: false,
            sidebar_width: cx.new(|_| SideWidth::new(SIDEBAR_WIDTH, SIDEBAR_MIN_WIDTH..SIDEBAR_MAX_WIDTH)),
            option_sections: OptionSectionsOpen::default(),
            options_filter,
            _subscriptions: vec![
                pen_search_sub,
                options_filter_sub,
                ship_selected_sub,
                compare_split_sub,
                export_requested_sub,
                model_selected_sub,
                hardpoints_sub,
                export_catalog_sub,
                common_settings_sub,
                viewport_event_sub,
                dock_sub,
                dock_event_sub,
            ],
        }
    }

    /// "Compare" handler (`sidebar::CompareSplit`, emitted by the sidebar
    /// header button): creates a new pane, clones the active pane's
    /// ship/camera/settings into it (egui's own `CompareSettings` -- see
    /// below), and pushes it into `dock` as the active pane so the next ship
    /// selection loads into it.
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
            opening_layer: self.opening_layer,
        };
        self.sidebar.update(cx, |sidebar, cx| sidebar.set_common(common, cx));
    }

    /// Where playback is, so the health strip can mark it. `None` until the
    /// viewer is following one.
    pub(crate) fn playback_at(&self) -> Option<wows_replays::types::GameClock> {
        self.playback_at
    }

    /// Asks the playback feeding this viewer to move to `clock`.
    ///
    /// The pane owns neither the playback nor the tab it sits in, so it says
    /// where rather than moving anything itself.
    pub(crate) fn seek_to(&mut self, clock: wows_replays::types::GameClock, cx: &mut Context<Self>) {
        cx.emit(SeekRequested(clock));
    }

    /// Puts the export dialog's state away.
    pub(crate) fn forget_export_dialog(&mut self) {
        self.export_dialog = None;
        self._export_dialog_subscription = None;
    }

    /// What was fired at the ship on screen.
    pub(crate) fn incoming(&self) -> &IncomingFire {
        &self.incoming
    }

    /// Narrows the incoming-fire log to one attacker, or widens it to every
    /// enemy.
    pub(crate) fn set_incoming_attacker(&mut self, attacker: Option<EntityId>, cx: &mut Context<Self>) {
        if self.incoming.filter.attacker == attacker {
            return;
        }
        self.incoming.filter.attacker = attacker;
        cx.notify();
    }

    /// Whether the log counts secondary armament.
    pub(crate) fn set_incoming_secondaries(&mut self, secondaries: bool, cx: &mut Context<Self>) {
        if self.incoming.filter.secondaries == secondaries {
            return;
        }
        self.incoming.filter.secondaries = secondaries;
        cx.notify();
    }

    /// The viewport the reader is working in, which is where a cast arc lives.
    pub(crate) fn active_viewport(&self, cx: &App) -> Entity<ViewportView> {
        self.dock.read(cx).active_viewport()
    }

    /// Which of the dock's panes the options rail acts on.
    pub(crate) fn active_pane_position(&self, cx: &App) -> PanePosition {
        self.dock.read(cx).active_position()
    }

    fn set_sidebar_collapsed(&mut self, collapsed: bool, cx: &mut Context<Self>) {
        if self.sidebar_collapsed == collapsed {
            return;
        }
        self.sidebar_collapsed = collapsed;
        cx.notify();
    }

    /// The catalogue of ships this pane can open, once it has loaded.
    pub(crate) fn catalog(&self) -> Option<&crate::armor_viewer::catalog::ShipCatalog> {
        match self.catalog_status() {
            CatalogStatus::Ready(catalog) => Some(catalog),
            CatalogStatus::Pending | CatalogStatus::Failed(_) => None,
        }
    }

    /// Where the catalogue load stands, so a list waiting on it can say why
    /// it is empty.
    pub(crate) fn catalog_status(&self) -> CatalogStatus<'_> {
        match &self.bundle {
            BundleState::NotStarted | BundleState::Loading => CatalogStatus::Pending,
            BundleState::Ready(bundle) => CatalogStatus::Ready(&bundle.catalog),
            BundleState::Failed(reason) => CatalogStatus::Failed(reason),
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
        let first_ship = self.pen.ships.is_empty();
        self.pen.add(ship);
        if first_ship {
            self.pen.firing_ship = Some(param_index.to_string());
            self.pen.firing_shell = None;
        }
        self.pen.search.update(cx, |state, cx| state.set_value(String::new(), window, cx));
        self.push_cast_ship(cx);
        cx.notify();
    }

    pub(crate) fn remove_comparison_ship(&mut self, index: usize, cx: &mut Context<Self>) {
        self.pen.remove(index);
        if !self.pen.ships.iter().any(|ship| Some(ship.param_index.as_str()) == self.pen.firing_ship.as_deref()) {
            self.pen.firing_ship = None;
            self.pen.firing_shell = None;
        }
        self.push_cast_ship(cx);
        cx.notify();
    }

    pub(crate) fn clear_comparison_ships(&mut self, cx: &mut Context<Self>) {
        self.pen.clear();
        self.pen.firing_ship = None;
        self.pen.firing_shell = None;
        self.push_cast_ship(cx);
        cx.notify();
    }

    pub(crate) fn set_firing_ship(&mut self, param_index: &str, cx: &mut Context<Self>) {
        if !self.pen.ships.iter().any(|ship| ship.param_index == param_index) {
            return;
        }
        self.pen.firing_ship = Some(param_index.to_string());
        self.pen.firing_shell = None;
        self.push_cast_ship(cx);
        cx.notify();
    }

    pub(crate) fn set_firing_shell(&mut self, shell_index: usize, cx: &mut Context<Self>) {
        let Some(ship) =
            self.pen.ships.iter().find(|ship| Some(ship.param_index.as_str()) == self.pen.firing_ship.as_deref())
        else {
            return;
        };
        if shell_index >= ship.shells.len() || self.pen.firing_shell == Some(shell_index) {
            return;
        }
        self.pen.firing_shell = Some(shell_index);
        self.push_cast_ship(cx);
        cx.notify();
    }

    /// Hands every viewport the ship a trajectory cast fires: the controls
    /// are the pane's, so whichever viewport the reader casts in fires what
    /// they show.
    ///
    /// The selected ship supplies the trajectory shell; the first ship is the
    /// default until the reader chooses another.
    fn push_cast_ship(&mut self, cx: &mut Context<Self>) {
        let selected = self
            .pen
            .firing_ship
            .as_deref()
            .and_then(|param_index| self.pen.ships.iter().find(|ship| ship.param_index == param_index).cloned());
        let first = selected.or_else(|| self.pen.ships.first().cloned());
        self.pen.firing_ship = first.as_ref().map(|ship| ship.param_index.clone());
        let shell = first.map(|ship| CastShell { ship, shell_index: self.pen.firing_shell });
        for viewport in self.dock.read(cx).panes().to_vec() {
            viewport.update(cx, |view, cx| view.set_cast_shell(shell.clone(), cx));
        }
    }

    /// Whether the options rail is up.
    pub(crate) fn options_open(&self) -> bool {
        self.show_options
    }

    /// The rail's filter field.
    pub(crate) fn options_filter_input(&self) -> &Entity<gpui_kit::component::input::InputState> {
        &self.options_filter
    }

    /// What the rail's filter field asks for.
    pub(crate) fn options_filter(&self, cx: &App) -> super::options_filter::OptionsFilter {
        super::options_filter::OptionsFilter::new(&self.options_filter.read(cx).value())
    }

    /// Which of the rail's sections are expanded.
    pub(crate) fn option_sections_open(&self) -> OptionSectionsOpen {
        self.option_sections
    }

    pub(crate) fn toggle_option_section(&mut self, section: OptionSection, cx: &mut Context<Self>) {
        self.option_sections.toggle(section);
        cx.notify();
    }

    /// Brings the rail up with `section` expanded.
    pub(crate) fn open_option_section(&mut self, section: OptionSection, cx: &mut Context<Self>) {
        self.show_options = true;
        *self.option_sections.open_mut(section) = true;
        cx.notify();
    }

    pub(crate) fn toggle_options(&mut self, cx: &mut Context<Self>) {
        self.show_options = !self.show_options;
        // A zone row hovered in the rail highlights that zone on the hull. The
        // row leaves the tree with the rail rather than under the pointer, so
        // no `on_hover(false)` is dispatched and the highlight would outlive
        // what it describes. The popover triggers cleared it on close.
        if !self.show_options {
            for pane in self.dock.read(cx).panes().to_vec() {
                pane.update(cx, |view, cx| view.clear_sidebar_hover(cx));
            }
        }
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
        if event.opening_layer != self.opening_layer {
            self.opening_layer = event.opening_layer;
            crate::settings_store::save(keys::MODEL_VIEWER_SHOW_ARMOR, &event.opening_layer.shows_armor(), cx);
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            DockEvent::ResetLastPane => {
                let viewport = self.fresh_viewport(window, cx);
                self.dock.update(cx, |dock, cx| dock.reset_to(viewport, cx));
                self.ship_loaded = false;
                self.push_common_settings(cx);
                self.push_cast_ship(cx);
                cx.notify();
            }
        }
    }

    /// A new empty viewport, wired to this pane and drawing on its shared
    /// device.
    fn fresh_viewport(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<ViewportView> {
        let this = cx.weak_entity();
        let scene = self.scene.clone();
        let viewport = cx.new(|cx| {
            let mut viewport = ViewportView::new(scene, window, cx);
            viewport.set_pane(this);
            viewport
        });
        let viewport_event_sub = cx.subscribe(&viewport, Self::on_viewport_event);
        self._subscriptions.push(viewport_event_sub);
        let targets = self.hardpoint_targets.clone();
        viewport.update(cx, |view, cx| view.set_hardpoint_targets(targets, cx));

        viewport
    }

    fn on_compare_split(
        &mut self,
        _sidebar: &Entity<Sidebar>,
        event: &CompareSplit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let active = self.dock.read(cx).active_viewport();
        let clone_source = {
            let source = active.read(cx);
            source
                .current_armor()
                .map(|armor| (armor, source.camera(), source.synced_settings(), source.scene_source()))
        };

        let viewport = self.fresh_viewport(window, cx);

        if let Some((armor, camera, synced, scene_source)) = clone_source {
            viewport.update(cx, |view, cx| {
                if let Some(source) = scene_source {
                    view.set_scene_source(source);
                }
                view.show_armor(armor, self.opening_layer, cx);
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
        self.push_cast_ship(cx);
        cx.notify();
    }

    /// `ViewportChange` handler, subscribed to every pane in `dock` (`new`,
    /// `on_compare_split`): broadcasts the source pane's camera/settings to
    /// every OTHER pane when the corresponding toggle is on. Uses
    /// `ViewportView`'s silent setters (`set_camera`/`apply_synced`), so this
    /// never re-triggers the event it's reacting to.
    fn on_viewport_event(&mut self, source: Entity<ViewportView>, event: &ViewportChange, cx: &mut Context<Self>) {
        match event {
            ViewportChange::Camera if self.mirror_cameras => self.push_camera_from(&source, cx),
            ViewportChange::Settings => {
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

    /// Adopts the saved choice of what a model opens showing.
    pub fn set_opening_layer(&mut self, layer: OpeningLayer, cx: &mut Context<Self>) {
        self.opening_layer = layer;
        self.push_common_settings(cx);
    }

    /// What the next model opened in this pane shows first.
    pub fn opening_layer(&self) -> OpeningLayer {
        self.opening_layer
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
                self.start_model_library(Arc::clone(&bundle), cx);
                self.bundle = BundleState::Ready(bundle);
            }
            Err(e) => {
                tracing::error!("armor viewer: failed to load ship assets: {e}");
                self.bundle = BundleState::Failed(e.to_string());
            }
        }
        cx.notify();
    }

    /// Indexes every model over `bundle`'s assets in the background, for the
    /// sidebar's Models list and for opening one.
    fn start_model_library(&mut self, bundle: Arc<ArmorAssetsBundle>, cx: &mut Context<Self>) {
        if !matches!(self.models, ModelsState::NotStarted) {
            return;
        }
        self.models = ModelsState::Loading;
        let task = spawn_build_model_library(bundle, cx);
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| this.apply_model_library(result, cx));
        })
        .detach();
    }

    fn apply_model_library(&mut self, result: Result<ModelAssets, ModelLibraryLoadError>, cx: &mut Context<Self>) {
        match result {
            Ok(models) => {
                let library = Arc::clone(&models.library);
                self.sidebar.update(cx, |sidebar, cx| sidebar.set_model_library(library, cx));
                self.models = ModelsState::Ready(Arc::new(models));
            }
            Err(e) => {
                tracing::error!("model viewer: failed to build the model library: {e}");
                self.sidebar.update(cx, |sidebar, cx| sidebar.set_model_library_failed(cx));
                self.models = ModelsState::Failed(e.to_string());
            }
        }
        cx.notify();
    }

    fn on_model_selected(
        &mut self,
        _sidebar: &Entity<Sidebar>,
        event: &ModelSelected,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_model(event.model.clone(), event.from_ship.clone(), cx);
    }

    /// Opens one catalogued model in the active pane. A re-click on the model
    /// already shown there keeps it, cancelling any load started since.
    pub(crate) fn open_model(&mut self, model: ModelPath, from_ship: Option<ShipPartRef>, cx: &mut Context<Self>) {
        let active = self.dock.read(cx).active_viewport();
        let wanted = super::model_refs::OpenTarget::Model { model: model.clone(), from_ship: from_ship.clone() };
        if active.read(cx).open_target().as_ref() == Some(&wanted) {
            // The cancelled load may have been a replay's, whose log is not
            // about the scene that stays.
            if self.cancel_load(&active, cx) {
                self.forget_incoming();
            }
            return;
        }
        self.forget_incoming();
        self.start_model_load(ModelLoadRequest::new(model, from_ship), cx);
    }

    /// Drops a load in flight into `viewport`, so the scene it shows stays.
    /// Whether one was in flight.
    fn cancel_load(&mut self, viewport: &Entity<ViewportView>, cx: &mut Context<Self>) -> bool {
        if !viewport.update(cx, |view, cx| view.cancel_load(cx)) {
            return false;
        }

        self.ship_load = ShipLoadState::Idle;
        self.push_open_target(cx);
        cx.notify();

        true
    }

    fn start_model_load(&mut self, request: ModelLoadRequest, cx: &mut Context<Self>) {
        let (BundleState::Ready(bundle), ModelsState::Ready(models)) = (&self.bundle, &self.models) else {
            tracing::warn!("model viewer: model selected before the model library finished loading");
            return;
        };
        let bundle = Arc::clone(bundle);
        let models = Arc::clone(models);
        // Captured now, as a ship load captures it: the result lands in the
        // pane it was asked for even if another becomes active meanwhile.
        let target_viewport = self.dock.read(cx).active_viewport();

        let display_name = request.path.stem().to_string();
        self.ship_load = ShipLoadState::Loading { display_name: display_name.clone() };
        let generation = target_viewport.update(cx, |viewport, cx| viewport.begin_load(display_name.into(), cx));
        cx.notify();

        let task = spawn_load_model_scene(Arc::clone(&models), Arc::clone(&bundle), request.clone(), cx);
        let source = SceneSource::Model { bundle, models, request };
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this
                .update(cx, |this, cx| this.apply_model_load_result(generation, source, target_viewport, result, cx));
        })
        .detach();
    }

    fn apply_model_load_result(
        &mut self,
        generation: LoadGeneration,
        source: SceneSource,
        target_viewport: Entity<ViewportView>,
        result: Result<LoadedShipArmor, ModelSceneError>,
        cx: &mut Context<Self>,
    ) {
        // A newer selection in the same pane superseded this load.
        if !target_viewport.read(cx).is_current_load(generation) {
            return;
        }
        let display_name = source.display_name().to_string();
        match result {
            Ok(scene) => {
                self.ship_load = ShipLoadState::Idle;
                target_viewport.update(cx, |viewport, cx| {
                    viewport.set_ship_loading(None, cx);
                    viewport.set_scene_source(source);
                    viewport.show_armor(Arc::new(scene), self.opening_layer, cx);
                });
            }
            Err(e) => {
                tracing::error!("model viewer: failed to load {display_name}: {e}");
                target_viewport.update(cx, |viewport, cx| viewport.set_ship_loading(None, cx));
                self.ship_load = ShipLoadState::Failed { display_name, reason: e.to_string() };
            }
        }
        self.push_open_target(cx);
        cx.notify();
    }

    /// Tells the sidebar what the active pane shows, so its row is marked.
    fn push_open_target(&self, cx: &mut Context<Self>) {
        let open = self.dock.read(cx).active_viewport().read(cx).open_target();
        self.sidebar.update(cx, |sidebar, cx| sidebar.set_open_model(open, cx));
    }

    fn on_hardpoints_toggled(
        &mut self,
        _sidebar: &Entity<Sidebar>,
        event: &HardpointsToggled,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_hardpoints_visible(event.target.clone(), event.visible, cx);
    }

    /// Marks or unmarks `target`'s hardpoints in every pane, and tells the
    /// sidebar which are marked.
    pub(crate) fn set_hardpoints_visible(&mut self, target: HardpointTarget, visible: bool, cx: &mut Context<Self>) {
        let changed =
            if visible { self.hardpoint_targets.insert(target) } else { self.hardpoint_targets.remove(&target) };
        if !changed {
            return;
        }
        for viewport in self.dock.read(cx).panes().to_vec() {
            let targets = self.hardpoint_targets.clone();
            viewport.update(cx, |view, cx| view.set_hardpoint_targets(targets, cx));
        }
        self.sidebar.update(cx, |sidebar, cx| sidebar.set_hardpoint_visibility(&self.hardpoint_targets, cx));
        cx.notify();
    }

    fn on_sidebar_event(
        &mut self,
        _sidebar: &Entity<Sidebar>,
        event: &ShipSelected,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_ship(event.param_index.clone(), event.display_name.clone(), cx);
    }

    /// Opens a ship picked from the catalogue in the active pane.
    pub(crate) fn open_ship(&mut self, param_index: String, display_name: String, cx: &mut Context<Self>) {
        // Mirrors egui's `already_selected` guard (`ui/tab.rs:520-521`): a
        // re-click on the ship already loaded in the ACTIVE pane must not
        // re-trigger `start_ship_load`, which resets part/plate/hull
        // visibility, selected camo, and the undo stack. Only the active
        // pane is checked -- a Compare pane loading the same ship separately
        // is a legitimate, distinct load.
        let active = self.dock.read(cx).active_viewport();
        let loaded = active.read(cx).loaded_param_index();
        if is_ship_already_loaded(loaded, &param_index) {
            if self.cancel_load(&active, cx) {
                self.forget_incoming();
            }
            return;
        }
        self.forget_incoming();
        self.start_ship_load(param_index, display_name, cx);
    }

    /// Sidebar per-ship "Export model" context-menu handler
    /// (`sidebar::ExportModelRequested`): asks what the model should contain
    /// before asking where to write it, which is what the egui export dialog
    /// collects. A request that arrives before the ship-asset bundle finishes
    /// loading is a no-op.
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
        let assets = Arc::clone(bundle);
        // The hull the pane is showing, which is the one the reader is looking
        // at. A sidebar menu opened with no ship loaded seeds nothing.
        let seed_hull = self.dock.read(cx).active_viewport().read(cx).selected_hull();
        let request = ExportRequest::Ship {
            param_index: event.param_index.clone(),
            display_name: event.display_name.clone(),
            seed_hull,
        };
        self.ask_export(request, assets, window, cx);
    }

    fn on_export_catalog_model_requested(
        &mut self,
        _sidebar: &Entity<Sidebar>,
        event: &ExportCatalogModelRequested,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let request = ModelLoadRequest::new(event.model.clone(), event.from_ship.clone());
        self.export_catalog_model(request, window, cx);
    }

    /// Asks what one catalogued model's export should contain, then writes it
    /// with the armor thickness `request` picks. A request before the model
    /// library has loaded is a no-op.
    pub(crate) fn export_catalog_model(
        &mut self,
        request: ModelLoadRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (BundleState::Ready(bundle), ModelsState::Ready(models)) = (&self.bundle, &self.models) else {
            tracing::warn!("model viewer: export requested before the model library finished loading");
            return;
        };
        let assets = Arc::clone(bundle);
        let request = ExportRequest::Model { request, models: Arc::clone(models) };
        self.ask_export(request, assets, window, cx);
    }

    /// Reads the remembered export choices, then opens the dialog on them.
    fn ask_export(
        &mut self,
        request: ExportRequest,
        assets: Arc<ArmorAssetsBundle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let Ok(asked) = cx.update(|_window, cx| export_defaults(cx)) else { return };
            let defaults = asked.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.open_export_dialog(request, defaults, assets, window, cx);
            });
        })
        .detach();
    }

    /// Puts the export choices to the reader, and writes the model they confirm.
    fn open_export_dialog(
        &mut self,
        request: ExportRequest,
        defaults: wows_toolkit_viewmodel::armor::export::ExportDefaults,
        assets: Arc<ArmorAssetsBundle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = cx.new(|cx| match request {
            ExportRequest::Ship { param_index, display_name, seed_hull } => export_dialog::ExportDialogState::new(
                param_index,
                display_name,
                seed_hull,
                defaults,
                Arc::clone(&assets),
                cx,
            ),
            ExportRequest::Model { request, models } => {
                export_dialog::ExportDialogState::new_model(request, models, defaults, Arc::clone(&assets), cx)
            }
        });
        self.export_dialog = Some(state.clone());
        self._export_dialog_subscription =
            Some(cx.subscribe(&state, |_this, _state, _event: &export_dialog::Changed, cx| cx.notify()));
        let pane = cx.weak_entity();
        window.open_dialog(cx, move |dialog, window, cx| {
            let state = state.clone();
            let confirming = state.clone();
            let pane = pane.clone();
            let cancelling = pane.clone();
            let assets = Arc::clone(&assets);
            export_dialog::render(&state, dialog, window, cx).footer(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("armor-export-confirm")
                            .primary()
                            .label(t!("ui.armor.export_button").into_owned())
                            .small()
                            .disabled(!state.read(cx).is_ready())
                            .on_click(move |_event, window, cx: &mut gpui_kit::App| {
                                let Some(pane) = pane.upgrade() else { return };
                                let state = confirming.clone();
                                let assets = Arc::clone(&assets);
                                window.close_dialog(cx);
                                pane.update(cx, |pane, cx| pane.write_export(state, assets, window, cx));
                            }),
                    )
                    .child(
                        Button::new("armor-export-cancel")
                            .label(t!("ui.buttons.cancel").into_owned())
                            .small()
                            .on_click(move |_event, window, cx: &mut gpui_kit::App| {
                                window.close_dialog(cx);
                                // The dialog is gone, so neither it nor the
                                // redraw it asked for is wanted any more.
                                if let Some(pane) = cancelling.upgrade() {
                                    pane.update(cx, |pane, _cx| pane.forget_export_dialog());
                                }
                            }),
                    ),
            )
        });
    }

    /// Asks where to write the model the dialog described, then writes it.
    fn write_export(
        &mut self,
        state: Entity<export_dialog::ExportDialogState>,
        assets: Arc<ArmorAssetsBundle>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = state.read(cx);
        let (options, remember) = view.confirmed();
        let target = view.target().clone();
        let display_name = view.display_name().to_owned();

        // Remembered when the reader says yes, not when they pick a file: the
        // choices are theirs either way, and the egui dialog saves them here.
        remember_export_defaults(remember, cx);
        self.export_dialog = None;
        self._export_dialog_subscription = None;

        let asked = crate::dialog::save_file(
            None,
            &load_ship::default_export_filename(&display_name),
            Some(crate::dialog::GLB),
        );
        cx.spawn_in(window, async move |this, cx| {
            let Some(path) = crate::dialog::chosen(asked, &this, cx).await else { return };
            let ship = display_name.clone();
            let written = cx
                .background_spawn(async move {
                    let result = match target {
                        export_dialog::ExportTarget::Ship { param_index, .. } => {
                            load_ship::export_ship_glb(&assets.assets, &param_index, &options, &path)
                        }
                        export_dialog::ExportTarget::Model { request, models } => super::load_model::export_model_glb(
                            &models,
                            assets.assets.metadata(),
                            &request,
                            &options,
                            &path,
                        ),
                    };
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

    /// Puts the incoming-fire log away.
    ///
    /// A ship opened from the catalogue is not the ship the replay was firing
    /// at, so the log about that one is no longer about anything on screen.
    fn forget_incoming(&mut self) {
        self.incoming = IncomingFire::default();
        self.pending_hits = None;
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
        incoming: Box<crate::replay_renderer::IncomingContext>,
        cx: &mut Context<Self>,
    ) {
        self.playback_at = incoming.at;
        self.incoming =
            IncomingFire { context: *incoming, filter: IncomingFilter::default(), victim: Some(param_index.clone()) };
        let active = self.dock.read(cx).active_viewport();
        if is_ship_already_loaded(active.read(cx).loaded_param_index(), &param_index) {
            self.pending_hits = None;
            self.cancel_load(&active, cx);
            // Already showing it, so only what it has taken has changed.
            active.update(cx, |view, cx| {
                view.show_hits(hits, cx);
                let incoming = &self.incoming.context;
                view.compare_incoming(&incoming.taken, &incoming.shells, cx);
            });
            return;
        }
        self.pending_hits = self.start_ship_load(param_index.clone(), display_name, cx).map(|load| PendingHits {
            param_index,
            load,
            hits,
        });
    }

    /// Updates what the open viewer shows as playback moves.
    ///
    /// Nothing if no ship is loaded: a viewer that was never opened has no
    /// hull to put them on, and loading one here would drag the reader into
    /// a tab they did not ask for.
    pub fn follow_hits(
        &mut self,
        at: wows_replays::types::GameClock,
        hits: Vec<wows_replay_insights::timeline::PreExtractedHit>,
        health: Option<f32>,
        cx: &mut Context<Self>,
    ) {
        let active = self.dock.read(cx).active_viewport();
        // Hits are in a ship's space; a model opened since has none to take.
        if !self.ship_loaded || !active.read(cx).scene_features().is_some_and(|features| features.ship) {
            return;
        }
        self.playback_at = Some(at);
        active.update(cx, |view, cx| {
            view.set_hit_health(health, cx);
            view.follow_hits(hits, cx);
        });
    }

    /// Starts loading `param_index` into the active pane. `None` when the ship
    /// assets are not ready, so nothing was started.
    fn start_ship_load(
        &mut self,
        param_index: String,
        display_name: String,
        cx: &mut Context<Self>,
    ) -> Option<StartedLoad> {
        let BundleState::Ready(bundle) = &self.bundle else {
            tracing::warn!("armor viewer: ship selected before ship assets finished loading");
            return None;
        };
        let bundle = Arc::clone(bundle);
        // Captured now (the dock's *current* active pane), not re-read when
        // the load completes: a pane switch (or another "Compare") while
        // this load is in flight must not redirect its result to whatever
        // pane happens to be active later -- it always lands in the pane it
        // was requested for.
        let target_viewport = self.dock.read(cx).active_viewport();

        self.ship_load = ShipLoadState::Loading { display_name: display_name.clone() };
        // The viewport shows nothing until a ship arrives, so it says what it
        // is waiting for rather than inviting a selection already made.
        let generation =
            target_viewport.update(cx, |viewport, cx| viewport.begin_load(display_name.clone().into(), cx));
        cx.notify();

        let task = spawn_load_ship_armor(Arc::clone(&bundle), param_index.clone(), display_name.clone(), cx);
        let started = StartedLoad { viewport: target_viewport.entity_id(), generation };
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.apply_ship_load_result(generation, param_index, display_name, bundle, target_viewport, result, cx)
            });
        })
        .detach();

        Some(started)
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_ship_load_result(
        &mut self,
        generation: LoadGeneration,
        param_index: String,
        display_name: String,
        bundle: Arc<ArmorAssetsBundle>,
        target_viewport: Entity<ViewportView>,
        result: Result<LoadedShipArmor, ShipLoadError>,
        cx: &mut Context<Self>,
    ) {
        if !target_viewport.read(cx).is_current_load(generation) {
            // A newer selection in the same pane superseded this load; discard
            // the result instead of clobbering that selection's state.
            return;
        }
        match result {
            Ok(armor) => {
                self.ship_load = ShipLoadState::Idle;
                self.ship_loaded = true;
                let comparing = self.incoming.is_about(&param_index);
                let load = StartedLoad { viewport: target_viewport.entity_id(), generation };
                let hits = take_pending_hits(&mut self.pending_hits, &param_index, load);
                target_viewport.update(cx, |viewport, cx| {
                    viewport.set_ship_loading(None, cx);
                    // Set before `show_armor`: a reload (Milestone 4 Task 8c)
                    // needs to know which bundle/param_index/display_name to
                    // re-export against, and `show_armor`'s own reset
                    // (`upload_armor_now`) is what clears the hull/LOD/module
                    // selection for this (possibly new) ship.
                    viewport.set_scene_source(SceneSource::Ship {
                        bundle,
                        param_index,
                        display_name: display_name.clone(),
                    });
                    viewport.show_armor(Arc::new(armor), self.opening_layer, cx);
                    // After the armor, because showing it starts the ship
                    // again and would drop them.
                    if let Some(hits) = hits {
                        viewport.show_hits(hits, cx);
                    }
                    if comparing {
                        let incoming = &self.incoming.context;
                        viewport.compare_incoming(&incoming.taken, &incoming.shells, cx);
                    }
                });
            }
            Err(e) => {
                tracing::error!("armor viewer: failed to load {display_name}: {e}");
                target_viewport.update(cx, |viewport, cx| viewport.set_ship_loading(None, cx));
                self.ship_load = ShipLoadState::Failed { display_name, reason: e.to_string() };
            }
        }
        self.push_open_target(cx);
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The checker reads its verdicts against whatever the pointer is on,
        // so the plate is taken here, every frame, rather than once when its
        // section opens.
        if self.follow_pointer_plate(cx) && self.show_options && self.option_sections.pen_check {
            cx.notify();
        }

        // What a held key changes, for the status bar. Only with a ship
        // loaded: before that neither key does anything here.
        let mode = if self.ship_loaded { held_mode(window, cx) } else { None };
        status_bar::set_mode(cx.weak_entity().into(), mode, cx);

        // Deliberately NOT a resizable panel. The sections it holds are their
        // own fixed widths, so a drag handle could only overflow or pad them.
        //
        // `motion::slide` does not fit: its content closure would hold `cx`
        // while the call itself also borrows it.
        let options_shown = motion::slide_progress("armor-viewer-options-slide", self.show_options, window, cx);
        let options = (self.show_options || !motion::settled(options_shown)).then(|| {
            let panel =
                div().size_full().bg(cx.theme().background).child(options_panel::render_panel(self, &cx.entity(), cx));
            motion::slide_frame(
                options_panel::PANEL_WIDTH,
                options_shown.value,
                motion::Edge::Right,
                panel.into_any_element(),
            )
        });
        let options_inset = motion::slide_inset(options_panel::PANEL_WIDTH, options_shown, self.show_options);

        let split = SideSplit::new(
            "armor-viewer-ships",
            &self.sidebar_width,
            !self.sidebar_collapsed,
            self.sidebar.clone(),
            self.dock.clone(),
        );

        let content = v_flex().size_full().child(
            h_flex()
                .relative()
                .flex_1()
                .min_h(px(0.))
                .overflow_hidden()
                .child(div().flex_1().min_w(px(0.)).h_full().mr(options_inset).child(split))
                .children(options),
        );

        // Legend floats over the whole pane (not just the viewport), gated
        // on a ship being loaded, matching the egui app's `any_ship_loaded`
        // window gate (`ui/tab.rs` ~627-628).
        // A model without armor has no thicknesses for it to explain.
        let active_armor = self.dock.read(cx).active_viewport().read(cx).scene_features().map(|f| f.armor);
        let show_legend = self.legend.visible && active_armor.unwrap_or(self.ship_loaded);
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

/// What a held key changes in the viewer: Ctrl/Cmd makes a zone's checkbox
/// show that zone alone, and Shift makes a click on the hull cast a shell
/// instead of hiding the plate.
fn held_mode(window: &Window, cx: &App) -> Option<ModeHint> {
    if status_bar::is_held(ModifierKey::Secondary, window, cx) {
        return Some(ModeHint { key: ModifierKey::Secondary, name_key: "ui.armor.mode_solo_zone" });
    }

    status_bar::is_held(ModifierKey::Shift, window, cx)
        .then_some(ModeHint { key: ModifierKey::Shift, name_key: "ui.armor.mode_cast_trajectory" })
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

/// A ship load started into one viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StartedLoad {
    viewport: gpui_kit::EntityId,
    generation: LoadGeneration,
}

/// Hits a replay asked to see, held for the one load that will show them.
struct PendingHits {
    param_index: String,
    load: StartedLoad,
    hits: Vec<wows_replay_insights::timeline::PreExtractedHit>,
}

/// The hits held for `load` of `param_index`, if they are for it. Hits held
/// for any other load stay held.
fn take_pending_hits(
    pending: &mut Option<PendingHits>,
    param_index: &str,
    load: StartedLoad,
) -> Option<Vec<wows_replay_insights::timeline::PreExtractedHit>> {
    if !pending.as_ref().is_some_and(|held| held.param_index == param_index && held.load == load) {
        return None;
    }

    pending.take().map(|held| held.hits)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui_kit::AppContext as _;
    use gpui_kit::TestAppContext;
    use gpui_kit::px;
    use gpui_kit::size;
    use wowsunpack::export::hardpoint::Hardpoint;
    use wowsunpack::export::hardpoint::HardpointKind;
    use wowsunpack::export::hardpoint::HardpointName;
    use wowsunpack::export::model_catalog::ModelPath;

    use super::ArmorViewerPane;
    use super::LoadGeneration;
    use super::PendingHits;
    use super::StartedLoad;
    use super::is_ship_already_loaded;
    use super::other_pane_indices;
    use super::take_pending_hits;
    use crate::armor_viewer::load_model::fixtures;
    use crate::armor_viewer::model_refs::HardpointTarget;

    /// Clicking the model on screen while another loads keeps it on screen:
    /// the other load is dropped rather than replacing it when it lands.
    #[gpui_kit::test]
    fn a_click_on_the_shown_model_cancels_a_load_in_flight(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1200.), px(800.)), ArmorViewerPane::new);
        let turret = ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path");
        let scene = Arc::new(fixtures::model_scene(turret.clone(), Vec::new()));

        window
            .update(cx, |pane, _window, cx| {
                let layer = pane.opening_layer();
                let viewport = pane.active_viewport(cx);
                viewport.update(cx, |view, cx| view.show_armor(scene, layer, cx));
                let other = viewport.update(cx, |view, cx| view.begin_load("AGM058".into(), cx));
                let load = StartedLoad { viewport: viewport.entity_id(), generation: other };
                pane.pending_hits = Some(PendingHits { param_index: "PASC001".into(), load, hits: Vec::new() });

                pane.open_model(turret, None, cx);

                assert!(pane.pending_hits.is_none(), "the cancelled load's replay hits are put away");

                assert!(!viewport.read(cx).is_current_load(other), "the other load no longer lands");
                assert!(!viewport.update(cx, |view, cx| view.cancel_load(cx)), "nothing is loading");
            })
            .expect("the test window stays open");
    }

    /// Opening a model puts away hits a replay asked to show on a ship.
    #[gpui_kit::test]
    fn opening_a_model_drops_held_replay_hits(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1200.), px(800.)), ArmorViewerPane::new);
        let turret = ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path");

        window
            .update(cx, |pane, _window, cx| {
                let viewport = pane.active_viewport(cx).entity_id();
                let load = StartedLoad { viewport, generation: LoadGeneration::default() };
                pane.pending_hits = Some(PendingHits { param_index: "PASC001".into(), load, hits: Vec::new() });

                pane.open_model(turret, None, cx);

                assert!(pane.pending_hits.is_none());
            })
            .expect("the test window stays open");
    }

    /// A standalone model opens opaque, and its Opaque toggle leaves the
    /// saved setting for ships alone.
    #[gpui_kit::test]
    fn a_model_opens_opaque_apart_from_the_ship_setting(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1200.), px(800.)), ArmorViewerPane::new);
        let turret = ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path");
        let scene = Arc::new(fixtures::model_scene(turret, Vec::new()));

        window
            .update(cx, |pane, _window, cx| {
                let layer = pane.opening_layer();
                let viewport = pane.active_viewport(cx);
                viewport.update(cx, |view, cx| view.show_armor(scene, layer, cx));
                assert!(!viewport.read(cx).display_settings.hull_opaque, "ships are see-through by default");
                assert!(viewport.read(cx).hull_opaque(), "the model is opaque");

                viewport.update(cx, |view, cx| view.set_hull_opaque(false, cx));
                assert!(!viewport.read(cx).hull_opaque(), "the toggle is honoured");
                viewport.update(cx, |view, cx| view.set_hull_opaque(true, cx));
                assert!(!viewport.read(cx).display_settings.hull_opaque, "the ship setting is untouched");
            })
            .expect("the test window stays open");
    }

    #[gpui_kit::test]
    fn held_hits_go_only_to_the_load_they_were_held_for(cx: &mut TestAppContext) {
        let first = cx.new(|_| ()).entity_id();
        let second = cx.new(|_| ()).entity_id();
        let load = StartedLoad { viewport: first, generation: LoadGeneration::default() };
        let mut pending = Some(PendingHits { param_index: "PASC001".into(), load, hits: Vec::new() });

        let elsewhere = StartedLoad { viewport: second, ..load };
        assert!(take_pending_hits(&mut pending, "PASC001", elsewhere).is_none(), "another pane");
        assert!(take_pending_hits(&mut pending, "PASC002", load).is_none(), "another ship");
        assert!(pending.is_some(), "still held for its own load");

        assert!(take_pending_hits(&mut pending, "PASC001", load).is_some());
        assert!(pending.is_none());
    }

    /// Ticking a model's hardpoints in the sidebar marks them in the pane
    /// showing it, and unticking takes the markers away again.
    #[gpui_kit::test]
    fn a_hardpoint_toggle_marks_and_unmarks_the_open_model(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let window = cx.open_window(size(px(1200.), px(800.)), ArmorViewerPane::new);
        let turret = ModelPath::parse("content/gameplay/usa/gun/main/AGM034/AGM034.model").expect("a model path");
        let transform = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 1.0];
        let hardpoint = Hardpoint { name: HardpointName::new("HP_Muzzle"), kind: HardpointKind::Mount, transform };
        let scene = Arc::new(fixtures::model_scene(turret.clone(), vec![hardpoint]));

        let labels =
            |pane: &ArmorViewerPane, cx: &gpui_kit::App| pane.active_viewport(cx).read(cx).hardpoint_labels().len();
        window
            .update(cx, |pane, _window, cx| {
                let layer = pane.opening_layer();
                pane.active_viewport(cx).update(cx, |view, cx| view.show_armor(scene, layer, cx));
                assert_eq!(labels(pane, cx), 0, "nothing is marked yet");

                pane.set_hardpoints_visible(HardpointTarget::Model(turret.clone()), true, cx);
                assert_eq!(labels(pane, cx), 1, "the model's one hardpoint is marked");

                pane.set_hardpoints_visible(HardpointTarget::Model(turret), false, cx);
                assert_eq!(labels(pane, cx), 0, "and unmarked again");
            })
            .expect("the test window stays open");
    }

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

/// What the last export was asked to contain, for the next one.
///
/// The row is the one the egui app writes, so an export set up in either is what
/// the other offers next time.
fn export_defaults(cx: &gpui_kit::App) -> gpui_kit::Task<wows_toolkit_viewmodel::armor::export::ExportDefaults> {
    let pool = crate::settings_store::pool(cx);
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let (Some(pool), Some(runtime)) = (pool, runtime) else { return Default::default() };
        runtime
            .handle()
            .block_on(async move { wows_toolkit_viewmodel::armor::export::ExportDefaults::load(&pool).await })
    })
}

/// Remembers what an export was asked to contain.
fn remember_export_defaults(defaults: wows_toolkit_viewmodel::armor::export::ExportDefaults, cx: &gpui_kit::App) {
    let pool = crate::settings_store::pool(cx);
    let runtime = crate::runtime::runtime(cx);
    cx.background_spawn(async move {
        let (Some(pool), Some(runtime)) = (pool, runtime) else { return };
        let written = runtime.handle().block_on(async move { defaults.save(&pool).await });
        if let Err(err) = written {
            tracing::warn!("armor viewer: the export defaults were not saved: {err}");
        }
    })
    .detach();
}

/// The armor viewer asked for the playback behind it to move.
pub struct SeekRequested(pub wows_replays::types::GameClock);

impl EventEmitter<SeekRequested> for ArmorViewerPane {}

/// What was fired at the ship on screen.
#[derive(Default)]
pub(crate) struct IncomingFire {
    pub(crate) context: crate::replay_renderer::IncomingContext,
    pub(crate) filter: IncomingFilter,
    /// The ship the log is about. A comparison run against any other hull would
    /// be about plating those shells never met, which a compare split would
    /// otherwise do.
    pub(crate) victim: Option<String>,
}

impl IncomingFire {
    /// Whether the log is about `param_index`'s ship.
    fn is_about(&self, param_index: &str) -> bool {
        self.victim.as_deref() == Some(param_index)
    }

    /// What a viewer showing this ship should compare, where it is the ship the
    /// log is about.
    pub(crate) fn to_compare(
        &self,
        param_index: Option<&str>,
    ) -> Option<(
        Vec<wows_replay_insights::timeline::PreExtractedHit>,
        std::collections::HashMap<wows_replays::types::GameParamId, wowsunpack::game_params::types::ShellInfo>,
    )> {
        self.is_about(param_index?).then(|| (self.context.taken.clone(), self.context.shells.clone()))
    }
}

/// What an export dialog opens on.
enum ExportRequest {
    Ship {
        param_index: String,
        display_name: String,
        /// The hull the pane is showing, where there is one to seed from.
        seed_hull: Option<String>,
    },
    Model {
        request: ModelLoadRequest,
        models: Arc<ModelAssets>,
    },
}

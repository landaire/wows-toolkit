//! The Armor Viewer tab's state: its panes and which is active, what the panes share, the options
//! rail, and the loads that fill a pane.
//!
//! A selection always loads into the active pane, captured as the load starts, so a pane switch
//! while it runs cannot redirect it. Each pane's viewport lives under an owner of its own, which
//! is disposed as the pane closes: that cancels the pane's load and releases its meshes.
//!
//! Loads run off the UI thread with a job on the status bar. The first stage is shown as it lands
//! and the viewport runs the rest of the scene in the background.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use rust_i18n::t;
use wows_replays::types::GameClock;
use wows_toolkit_armor::load_model::ModelLoadRequest;
use wows_toolkit_armor::load_model::ProblemReason;
use wows_toolkit_armor::load_model::SceneProblem;
use wows_toolkit_armor::load_model::load_model_scene;
use wows_toolkit_armor::load_ship::load_ship_armor_by_param;
use wows_toolkit_armor::model_refs::HardpointTarget;
use wows_toolkit_armor::model_refs::OpenTarget;
use wows_toolkit_armor::model_refs::ShipPartRef;
use wows_toolkit_armor::scene_load::FirstStage;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wows_toolkit_replay_data::replay_render::armor::ArmorFollowed;
use wows_toolkit_replay_data::replay_render::armor::ShowArmor;
use wows_toolkit_viewmodel::settings::keys;
use wowsunpack::export::model_catalog::ModelPath;
use wowsunpack::game_types::GameParamId;
use zgui::reactive::Callable;
use zgui::reactive::LocalStorage;
use zgui::reactive::Owner;
use zgui::reactive::RwSignal;
use zgui::reactive::StoredValue;
use zgui::reactive::UnsyncCallback;
use zgui::reactive::prelude::Get;
use zgui::reactive::prelude::GetUntracked;
use zgui::reactive::prelude::GetValue;
use zgui::reactive::prelude::Set;
use zgui::reactive::prelude::SetValue;
use zgui::reactive::prelude::Update;
use zgui::reactive::prelude::UpdateValue;
use zgui::reactive::prelude::With;
use zgui::reactive::prelude::WithUntracked;
use zgui::reactive::prelude::WithValue;
use zgui::task::blocking;
use zgui::task::spawn_local;

use crate::services::Job;
use crate::services::Services;
use crate::tabs::armor_viewer::library::ArmorBundle;
use crate::tabs::armor_viewer::library::ArmorLibrary;
use crate::tabs::armor_viewer::library::FeedSource;
use crate::tabs::armor_viewer::options::OptionSection;
use crate::tabs::armor_viewer::options::OptionSectionsOpen;
use crate::tabs::armor_viewer::panels::incoming::IncomingFire;
use crate::tabs::armor_viewer::panels::legend::LegendState;
use crate::tabs::armor_viewer::panels::pen::PenetrationState;
use crate::tabs::armor_viewer::sidebar::CommonPaneSettings;
use crate::tabs::armor_viewer::viewport::ArmorViewportState;
use crate::tabs::armor_viewer::viewport::CastShell;
use crate::tabs::armor_viewer::viewport::LoadGeneration;
use crate::tabs::armor_viewer::viewport::OpeningLayer;
use crate::tabs::armor_viewer::viewport::SceneSource;
use crate::tabs::armor_viewer::viewport::ViewportChange;

/// Names one pane for as long as it is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaneId(u64);

/// Which way the comparison panes are laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaneSplit {
    /// Side by side, which compares two ships' beams.
    #[default]
    SideBySide,
    /// One above the other, which compares their lengths.
    Stacked,
}

/// Where the active pane sits among the panes, as the reader counts them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanePosition {
    /// From 1.
    pub number: usize,
    pub count: usize,
}

/// One pane: its viewport and the owner its viewport lives under.
#[derive(Clone)]
pub struct Pane {
    pub id: PaneId,
    pub viewport: ArmorViewportState,
    owner: Owner,
}

/// A ship load started into one pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StartedLoad {
    pane: PaneId,
    generation: LoadGeneration,
}

/// Hits a replay asked to see, held for the one load that will show them. Playback moving while it
/// runs replaces them, so the ship lands showing where playback has reached.
struct PendingHits {
    param_index: String,
    load: StartedLoad,
    hits: Vec<wows_replay_insights::timeline::PreExtractedHit>,
    health: Option<f32>,
}

/// The replay viewport a pane was shown from, whose playback moves that pane's hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Following {
    source: FeedSource,
    pane: PaneId,
}

/// The parts of `armor_viewer_defaults` the port has no control for, kept as read so a save
/// writes them back unchanged.
#[derive(Debug, Clone, Copy, PartialEq)]
struct UnportedDefaults {
    show_splash_boxes: bool,
    show_waterline: bool,
    waterline_opacity: f64,
}

impl Default for UnportedDefaults {
    /// The row's own column defaults, for a database with no row yet.
    fn default() -> Self {
        Self { show_splash_boxes: false, show_waterline: true, waterline_opacity: 0.3 }
    }
}

/// What decides the ship a trajectory cast fires and its shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CastKey {
    firing_ship: Option<String>,
    firing_shell: Option<usize>,
    /// The first ship the checker holds, which fires when none is chosen.
    first_ship: Option<String>,
}

/// The tab's state, as one `Copy` handle.
#[derive(Clone, Copy)]
pub struct ArmorWorkspace {
    pub services: Services,
    pub library: ArmorLibrary,
    panes: RwSignal<Vec<Pane>, LocalStorage>,
    /// The tab's owner. Panes are its children whatever owner a compare or close runs under, so
    /// they are disposed with the tab and not with the control that asked for them.
    owner: StoredValue<Owner>,
    active: RwSignal<PaneId, LocalStorage>,
    next_pane: StoredValue<u64>,
    pub split: RwSignal<PaneSplit, LocalStorage>,
    mirror_cameras: RwSignal<bool, LocalStorage>,
    sync_options: RwSignal<bool, LocalStorage>,
    /// What a model opened in a pane shows first. Persisted under `MODEL_VIEWER_SHOW_ARMOR`.
    pub opening_layer: RwSignal<OpeningLayer, LocalStorage>,
    /// Set once the reader picks the opening layer, which the saved setting read after must not undo.
    opening_layer_picked: StoredValue<bool>,
    /// Whose hardpoints are marked, as the sidebar's checkboxes left them.
    pub hardpoints: RwSignal<Rc<HashSet<HardpointTarget>>, LocalStorage>,
    /// Set once a ship has been shown, which the legend and the held-key modes wait for.
    pub ship_loaded: RwSignal<bool, LocalStorage>,
    pending_hits: StoredValue<Option<PendingHits>, LocalStorage>,
    /// The replay viewport whose playback the viewer shows, and the pane it is shown in. Another
    /// viewport's moves are not this ship's.
    following: StoredValue<Option<Following>, LocalStorage>,
    /// What was fired at the ship on screen. Empty for a ship opened from the catalogue.
    pub incoming: RwSignal<IncomingFire, LocalStorage>,
    /// Where the playback feeding the viewer has reached.
    pub playback_at: RwSignal<Option<GameClock>, LocalStorage>,
    /// Moves that playback.
    seek: StoredValue<Option<UnsyncCallback<GameClock>>, LocalStorage>,
    pub pen: RwSignal<PenetrationState, LocalStorage>,
    pub legend: RwSignal<LegendState, LocalStorage>,
    unported: StoredValue<UnportedDefaults>,
    /// Whether the ships list is open, which the status bar toggles.
    pub sidebar_open: RwSignal<bool, LocalStorage>,
    /// Whether the options rail is open, which the status bar toggles.
    pub options_open: RwSignal<bool, LocalStorage>,
    pub sections: RwSignal<OptionSectionsOpen, LocalStorage>,
    pub options_filter: RwSignal<String, LocalStorage>,
    pub zone_filter: RwSignal<String, LocalStorage>,
    pub part_filter: RwSignal<String, LocalStorage>,
}

impl ArmorWorkspace {
    /// The tab's state, with one empty pane. Called from the tab's body.
    pub fn new(services: Services, library: ArmorLibrary) -> Self {
        let workspace = Self {
            services,
            library,
            panes: RwSignal::new_local(Vec::new()),
            owner: StoredValue::new(Owner::current().expect("a component body runs under an owner")),
            active: RwSignal::new_local(PaneId(0)),
            next_pane: StoredValue::new(0),
            split: RwSignal::new_local(PaneSplit::default()),
            mirror_cameras: RwSignal::new_local(false),
            sync_options: RwSignal::new_local(false),
            opening_layer: RwSignal::new_local(OpeningLayer::default()),
            opening_layer_picked: StoredValue::new(false),
            hardpoints: RwSignal::new_local(Rc::new(HashSet::new())),
            ship_loaded: RwSignal::new_local(false),
            pending_hits: StoredValue::new_local(None),
            following: StoredValue::new_local(None),
            incoming: RwSignal::new_local(IncomingFire::default()),
            playback_at: RwSignal::new_local(None),
            seek: StoredValue::new_local(None),
            pen: RwSignal::new_local(PenetrationState::default()),
            legend: RwSignal::new_local(LegendState::default()),
            unported: StoredValue::new(UnportedDefaults::default()),
            sidebar_open: RwSignal::new_local(true),
            options_open: RwSignal::new_local(true),
            sections: RwSignal::new_local(OptionSectionsOpen::default()),
            options_filter: RwSignal::new_local(String::new()),
            zone_filter: RwSignal::new_local(String::new()),
            part_filter: RwSignal::new_local(String::new()),
        };
        let first = workspace.fresh_pane();
        workspace.active.set(first.id);
        workspace.panes.set(vec![first]);

        workspace
    }

    /// Every pane, in display order. Tracked.
    pub fn panes(self) -> Vec<Pane> {
        self.panes.get()
    }

    pub fn active_id(self) -> PaneId {
        self.active.get()
    }

    /// The pane the options rail acts on and a selection loads into. Tracked.
    pub fn active(self) -> ArmorViewportState {
        let active = self.active.get();
        self.panes.with(|panes| pane_of(panes, active))
    }

    pub fn active_untracked(self) -> ArmorViewportState {
        let active = self.active.get_untracked();
        self.panes.with_untracked(|panes| pane_of(panes, active))
    }

    /// Where the active pane sits. Tracked.
    pub fn active_position(self) -> PanePosition {
        let active = self.active.get();
        self.panes.with(|panes| PanePosition {
            number: panes.iter().position(|pane| pane.id == active).map_or(1, |ix| ix + 1),
            count: panes.len(),
        })
    }

    fn viewports_untracked(self) -> Vec<ArmorViewportState> {
        self.panes.with_untracked(|panes| panes.iter().map(|pane| pane.viewport).collect())
    }

    /// Makes `id` the active pane: interacting with a pane is what makes it the target.
    pub fn activate(self, id: PaneId) {
        if self.active.get_untracked() == id {
            return;
        }
        if self.panes.with_untracked(|panes| panes.iter().any(|pane| pane.id == id)) {
            self.active.set(id);
        }
    }

    /// A new empty pane, wired to the tab, under an owner of its own.
    fn fresh_pane(self) -> Pane {
        let id = PaneId(self.next_pane.get_value());
        self.next_pane.update_value(|next| *next += 1);
        let owner = self.owner.with_value(Owner::child);
        let viewport = owner.with(ArmorViewportState::new);
        viewport.set_hardpoint_targets(self.hardpoints.get_untracked().as_ref().clone());
        viewport.set_pane_menu(super::options::pane_menu(self, viewport));
        viewport.on_change(move |change| self.on_viewport_change(viewport, change));

        Pane { id, viewport, owner }
    }

    /// Closes pane `id`. The last pane is emptied rather than removed, since a viewer with no
    /// viewport would draw nothing.
    pub fn close_pane(self, id: PaneId) {
        let Some(ix) = self.panes.with_untracked(|panes| panes.iter().position(|pane| pane.id == id)) else { return };
        let len = self.panes.with_untracked(Vec::len);
        let closed = if len > 1 {
            let mut closed = None;
            self.panes.update(|panes| closed = Some(panes.remove(ix)));
            if self.active.get_untracked() == id {
                let next = self.panes.with_untracked(|panes| panes[ix.min(panes.len() - 1)].id);
                self.active.set(next);
            }
            closed
        } else {
            let fresh = self.fresh_pane();
            let mut closed = None;
            self.active.set(fresh.id);
            self.panes.update(|panes| closed = Some(std::mem::replace(&mut panes[0], fresh)));
            self.ship_loaded.set(false);
            closed
        };
        if let Some(closed) = closed {
            closed.owner.cleanup();
        }
        // A replay followed in the closed pane has nowhere left to show its moves.
        if self.following.get_value().is_some_and(|following| following.pane == id) {
            self.following.set_value(None);
        }
        self.pending_hits.update_value(|pending| {
            if pending.as_ref().is_some_and(|held| held.load.pane == id) {
                *pending = None;
            }
        });
        self.push_cast_ship();
    }

    /// Adds a comparison pane copying the active one, or opening `ship` when the compare was asked
    /// from a ship's own row. The new pane becomes the active one.
    pub fn compare(self, ship: Option<(String, String)>) {
        let source = self.active_untracked();
        let fresh = self.fresh_pane();
        if let Some(armor) = source.shown() {
            let viewport = fresh.viewport;
            if let Some(scene_source) = source.scene_source() {
                viewport.set_scene_source(scene_source);
            }
            viewport.show_armor(armor, self.opening_layer.get_untracked());
            // The copy holds the background stage too, so it runs on if the source closes first.
            if let Some(run) = source.scene_rest() {
                viewport.share_scene_rest(run);
            }
            viewport.set_camera(source.camera());
            if let Some(synced) = source.synced_settings() {
                viewport.apply_synced(&synced);
            }
        }
        let id = fresh.id;
        self.panes.update(|panes| panes.push(fresh));
        self.active.set(id);

        if let Some((param_index, display_name)) = ship {
            self.forget_incoming();
            self.start_ship_load(param_index, display_name);
        }
        self.push_cast_ship();
    }

    /// What the panes share, as the sidebar's header reports it. Tracked.
    pub fn common(self) -> CommonPaneSettings {
        CommonPaneSettings {
            comparing: self.panes.with(|panes| panes.len() > 1),
            mirror_cameras: self.mirror_cameras.get(),
            sync_options: self.sync_options.get(),
            stack_panes: self.split.get() == PaneSplit::Stacked,
            opening_layer: self.opening_layer.get(),
        }
    }

    /// Adopts what the sidebar's header changed.
    pub fn set_common(self, common: CommonPaneSettings) {
        self.set_mirror_cameras(common.mirror_cameras);
        self.set_sync_options(common.sync_options);
        if common.opening_layer != self.opening_layer.get_untracked() {
            self.opening_layer.set(common.opening_layer);
            self.opening_layer_picked.set_value(true);
            self.save_opening_layer();
        }
        let split = if common.stack_panes { PaneSplit::Stacked } else { PaneSplit::SideBySide };
        if self.split.get_untracked() != split {
            self.split.set(split);
        }
    }

    /// Keeps every pane's camera with whichever pane moves its own. Turning it on pushes the active
    /// pane's camera at once.
    pub fn set_mirror_cameras(self, on: bool) {
        if self.mirror_cameras.get_untracked() == on {
            return;
        }
        self.mirror_cameras.set(on);
        if on {
            self.push_camera_from(self.active_untracked());
        }
    }

    /// Keeps every pane's visibility, display, hull and lighting settings with whichever pane
    /// changes its own. Turning it on pushes the active pane's at once.
    pub fn set_sync_options(self, on: bool) {
        if self.sync_options.get_untracked() == on {
            return;
        }
        self.sync_options.set(on);
        if on {
            self.push_settings_from(self.active_untracked());
        }
    }

    fn on_viewport_change(self, source: ArmorViewportState, change: ViewportChange) {
        match change {
            ViewportChange::Camera if self.mirror_cameras.get_untracked() => self.push_camera_from(source),
            ViewportChange::Settings => {
                if self.sync_options.get_untracked() {
                    self.push_settings_from(source);
                }
                // The display settings are a preference, so they outlive the session.
                self.save_defaults();
            }
            _ => {}
        }
    }

    /// The setters used here are silent, which keeps a broadcast from echoing back.
    fn push_camera_from(self, source: ArmorViewportState) {
        let camera = source.camera();
        for viewport in self.viewports_untracked().into_iter().filter(|viewport| *viewport != source) {
            viewport.set_camera(camera.clone());
        }
    }

    fn push_settings_from(self, source: ArmorViewportState) {
        let Some(settings) = source.synced_settings() else { return };
        for viewport in self.viewports_untracked().into_iter().filter(|viewport| *viewport != source) {
            viewport.apply_synced(&settings);
        }
    }

    /// Seeds the legend and every pane's display settings from the saved defaults row, and what a
    /// model opens showing from its setting. A layer the reader picked while the settings were read
    /// is kept, and saved in place of the one read.
    pub fn apply_defaults(self, defaults: Option<&ArmorViewerDefaultsRow>, show_armor: bool) {
        if let Some(defaults) = defaults {
            self.unported.set_value(UnportedDefaults {
                show_splash_boxes: defaults.show_splash_boxes,
                show_waterline: defaults.show_waterline,
                waterline_opacity: defaults.waterline_opacity,
            });
        }
        self.legend.set(LegendState::from_defaults(defaults));
        for viewport in self.viewports_untracked() {
            viewport.apply_armor_defaults(defaults);
        }
        if self.opening_layer_picked.get_value() {
            self.save_opening_layer();
        } else {
            self.opening_layer.set(OpeningLayer::from_show_armor(show_armor));
        }
    }

    fn save_opening_layer(self) {
        let shows_armor = self.opening_layer.get_untracked().shows_armor();
        self.services.settings.edit(keys::MODEL_VIEWER_SHOW_ARMOR, |settings| {
            settings.model_show_armor = shows_armor;
            settings.model_show_armor
        });
    }

    /// Writes the active pane's display settings and the legend back to the defaults row. The row
    /// is one record covering both, so all of it is written.
    pub fn save_defaults(self) {
        let active = self.active_untracked();
        let display = active.display_settings();
        let all_visible = active.all_visible();
        let legend = self.legend.get_untracked();
        let unported = self.unported.get_value();
        let row = ArmorViewerDefaultsRow {
            show_plate_edges: display.show_plate_edges,
            show_waterline: unported.show_waterline,
            show_zero_mm: display.show_zero_mm,
            armor_opacity: f64::from(display.armor_opacity),
            waterline_opacity: unported.waterline_opacity,
            hull_opaque: display.hull_opaque,
            hull_all_visible: all_visible.hull,
            armor_all_visible: all_visible.armor,
            // Splash boxes and the water plane have no control here, so they are written back
            // as read and never overwrite what the other app set.
            show_splash_boxes: unported.show_splash_boxes,
            show_legend: legend.visible,
            legend_collapsed: legend.collapsed,
            legend_pos_x: Some(f64::from(legend.pos.x)),
            legend_pos_y: Some(f64::from(legend.pos.y)),
        };
        self.services.settings.save_armor_defaults(row);
    }

    /// Opens the rail with `section` expanded.
    pub fn open_section(self, section: OptionSection) {
        self.options_open.set(true);
        self.sections.update(|sections| sections.open(section));
    }

    /// Shows or hides the options rail. A zone row hovered in the rail highlights its zone, and the
    /// row leaves with the rail rather than from under the pointer, so the highlight is cleared.
    pub fn toggle_options(self) {
        let open = !self.options_open.get_untracked();
        self.options_open.set(open);
        if !open {
            for viewport in self.viewports_untracked() {
                viewport.clear_sidebar_hover();
            }
        }
    }

    /// Marks or unmarks `target`'s hardpoints in every pane.
    pub fn set_hardpoints_visible(self, target: HardpointTarget, visible: bool) {
        let mut targets = self.hardpoints.get_untracked().as_ref().clone();
        let changed = if visible { targets.insert(target) } else { targets.remove(&target) };
        if !changed {
            return;
        }
        for viewport in self.viewports_untracked() {
            viewport.set_hardpoint_targets(targets.clone());
        }
        self.hardpoints.set(Rc::new(targets));
    }

    /// What the active pane shows, which the sidebar marks. Tracked.
    pub fn open_target(self) -> Option<OpenTarget> {
        self.active().open_target()
    }

    /// The ship a trajectory cast fires and its chosen shell: the selected ship, else the first.
    pub fn cast_shell_untracked(self) -> Option<CastShell> {
        self.pen.with_untracked(|pen| {
            let selected =
                pen.firing_ship.as_deref().and_then(|wanted| pen.ships.iter().find(|ship| ship.param_index == wanted));
            let shell_index = if selected.is_some() { pen.firing_shell } else { None };
            selected.or_else(|| pen.ships.first()).cloned().map(|ship| CastShell { ship, shell_index })
        })
    }

    /// What decides [`Self::cast_shell_untracked`], tracked.
    pub fn cast_key(self) -> CastKey {
        self.pen.with(|pen| CastKey {
            firing_ship: pen.firing_ship.clone(),
            firing_shell: pen.firing_shell,
            first_ship: pen.ships.first().map(|ship| ship.param_index.clone()),
        })
    }

    /// Hands every pane the ship a trajectory cast fires: the controls are the tab's, so whichever
    /// pane the reader casts in fires what they show.
    pub fn push_cast_ship(self) {
        let shell = self.cast_shell_untracked();
        for viewport in self.viewports_untracked() {
            viewport.set_cast_shell(shell.clone());
        }
    }

    /// Reads the plate under the pointer into the checker, so its verdicts follow it.
    pub fn follow_pointer_plate(self) {
        let found = self.active().last_plate().map(|(zone, thickness_mm)| {
            crate::tabs::armor_viewer::panels::pen::PlateUnderPointer {
                zone,
                thickness: wowsunpack::game_params::types::Millimeters::from(thickness_mm),
            }
        });
        if self.pen.with_untracked(|pen| pen.plate != found) {
            self.pen.update(|pen| pen.plate = found);
        }
    }

    /// Asks the playback feeding the viewer to move to `clock`.
    pub fn seek_to(self, clock: GameClock) {
        if let Some(seek) = self.seek.get_value() {
            seek.run(clock);
        }
    }

    /// Puts the incoming-fire log away: a ship opened from the catalogue is not the one the replay
    /// was firing at.
    fn forget_incoming(self) {
        self.incoming.set(IncomingFire::default());
        self.pending_hits.set_value(None);
        self.following.set_value(None);
    }

    /// Opens a ship in the active pane. A re-click on the ship already there keeps it, cancelling
    /// any load started since.
    pub fn open_ship(self, param_index: String, display_name: String) {
        let active = self.active_untracked();
        if active.loaded_param_index().as_deref() == Some(param_index.as_str()) {
            if active.cancel_load() {
                self.forget_incoming();
            }
            return;
        }
        self.forget_incoming();
        self.start_ship_load(param_index, display_name);
    }

    /// Opens one catalogued model in the active pane. A re-click on the model already shown keeps
    /// it, cancelling any load started since.
    pub fn open_model(self, model: ModelPath, from_ship: Option<ShipPartRef>) {
        let active = self.active_untracked();
        let wanted = OpenTarget::Model { model: model.clone(), from_ship: from_ship.clone() };
        if active.open_target().as_ref() == Some(&wanted) {
            if active.cancel_load() {
                self.forget_incoming();
            }
            return;
        }
        self.forget_incoming();
        self.start_model_load(ModelLoadRequest::new(model, from_ship));
    }

    /// Opens an armored building with its mounts in the active pane. One with no hull model says so
    /// in the pane.
    pub fn open_building(self, building: GameParamId) {
        let Some(models) = self.library.models_ready_untracked() else {
            tracing::warn!("model viewer: building selected before the model library finished loading");
            return;
        };
        let Some(entry) = models.buildings.building(building) else {
            tracing::error!("model viewer: building {building} is not in the armored building index");
            return;
        };
        let active = self.active_untracked();
        if active.open_target() == Some(OpenTarget::Building(building)) {
            if active.cancel_load() {
                self.forget_incoming();
            }
            return;
        }
        self.forget_incoming();

        let Some(hull) = entry.model().cloned() else {
            tracing::error!("model viewer: building {} names no hull model", entry.param().name());
            let problem = SceneProblem {
                subject: entry.param().name().to_string().into(),
                reason: ProblemReason::GeometryAbsent,
            };
            // A load still running would otherwise land over the message.
            active.cancel_load();
            active.fail_load(problem);
            return;
        };
        self.start_model_load(ModelLoadRequest::for_building(hull, building));
    }

    fn start_model_load(self, request: ModelLoadRequest) {
        let (Some(bundle), Some(models)) = (self.library.ready_untracked(), self.library.models_ready_untracked())
        else {
            tracing::warn!("model viewer: model selected before the model library finished loading");
            return;
        };
        let target = self.active_untracked();
        let display_name = request.path.stem().to_string();
        let opening = target.begin_load(display_name.clone());
        let generation = opening.generation;
        let cancel = opening.cancel;
        let services = self.services;

        spawn_local(async move {
            let _job = services.jobs.start(Job {
                name: t!("ui.armor.loading_named_ship", ship = display_name).into_owned(),
                progress: None,
            });
            let asked = request.clone();
            let assets = Arc::clone(&bundle.assets);
            let landed = blocking(move || {
                load_model_scene(&models, assets.metadata(), &asked, &cancel).map(|first| (first, models))
            })
            .await;
            if !target.is_current_load(generation) {
                return;
            }
            let outcome = match landed {
                Ok((first, models)) => drawable_scene(first).map(|first| (first, models)),
                Err(err) => {
                    tracing::error!("model viewer: failed to load {display_name}: {err}");
                    Err(err.problem())
                }
            };
            target.set_ship_loading(None);
            match outcome {
                Ok((FirstStage { scene, rest }, models)) => {
                    target.set_scene_source(SceneSource::Model { assets: Arc::clone(&bundle.assets), models, request });
                    let scene = Arc::new(scene);
                    target.show_armor(Arc::clone(&scene), self.opening_layer.get_untracked());
                    target.start_scene_rest(scene, rest);
                }
                Err(reason) => target.fail_load(SceneProblem { subject: display_name.into(), reason }),
            }
        });
    }

    /// Starts loading `param_index` into the active pane. `None` when the ship data is not ready,
    /// so nothing was started.
    fn start_ship_load(self, param_index: String, display_name: String) -> Option<StartedLoad> {
        let Some(bundle) = self.library.ready_untracked() else {
            tracing::warn!("armor viewer: ship selected before ship assets finished loading");
            return None;
        };
        let pane = self.active.get_untracked();
        let target = self.active_untracked();
        let opening = target.begin_load(display_name.clone());
        let started = StartedLoad { pane, generation: opening.generation };
        let cancel = opening.cancel;
        let services = self.services;

        spawn_local(async move {
            let _job = services.jobs.start(Job {
                name: t!("ui.armor.loading_named_ship", ship = display_name).into_owned(),
                progress: None,
            });
            let assets = Arc::clone(&bundle.assets);
            let (asked, named) = (param_index.clone(), display_name.clone());
            let landed = blocking(move || load_ship_armor_by_param(&assets, &asked, &named, &cancel)).await;
            if !target.is_current_load(started.generation) {
                // A newer selection in the same pane superseded this load.
                return;
            }
            let outcome = match landed {
                Ok(first) => drawable_scene(first),
                Err(err) => {
                    tracing::error!("armor viewer: failed to load {display_name}: {err}");
                    Err(ProblemReason::ShipUnreadable)
                }
            };
            self.land_ship(started, target, &bundle, param_index, display_name, outcome);
        });

        Some(started)
    }

    fn land_ship(
        self,
        started: StartedLoad,
        target: ArmorViewportState,
        bundle: &ArmorBundle,
        param_index: String,
        display_name: String,
        outcome: Result<FirstStage, ProblemReason>,
    ) {
        target.set_ship_loading(None);
        let FirstStage { scene, rest } = match outcome {
            Ok(first) => first,
            Err(reason) => {
                // No ship landed to show them on, and playback moving must not keep them waiting.
                self.pending_hits.update_value(|pending| drop(take_pending_hits(pending, &param_index, started)));
                target.fail_load(SceneProblem { subject: display_name.into(), reason });
                return;
            }
        };
        self.ship_loaded.set(true);
        let comparing = self.incoming.with_untracked(|incoming| incoming.is_about(&param_index));
        let held = self.pending_hits.try_update_value(|pending| take_pending_hits(pending, &param_index, started));
        // Set before `show_armor`, which resets the hull, LOD and module choice for the new ship.
        target.set_scene_source(SceneSource::Ship { assets: Arc::clone(&bundle.assets), param_index, display_name });
        let scene = Arc::new(scene);
        target.show_armor(Arc::clone(&scene), self.opening_layer.get_untracked());
        target.start_scene_rest(scene, rest);
        // After the armor, because showing it starts the ship again and would drop them.
        if let Some(Some(held)) = held {
            target.show_hits(held.hits);
            target.set_hit_health(held.health);
        }
        if comparing {
            self.incoming
                .with_untracked(|incoming| target.compare_incoming(&incoming.context.taken, &incoming.context.shells));
        }
    }

    /// Shows a ship with the hits it had taken, for the replay viewport `source`, which the viewer
    /// follows from then on. The hits are held until the load that will show them lands.
    pub fn show_with_hits(self, source: FeedSource, shown: ShowArmor, seek: Option<UnsyncCallback<GameClock>>) {
        let ShowArmor { target, hits, incoming } = shown;
        self.following.set_value(Some(Following { source, pane: self.active.get_untracked() }));
        self.seek.set_value(seek);
        self.playback_at.set(incoming.at);
        let param_index = target.param_index;
        self.incoming.set(IncomingFire::about(*incoming, param_index.clone()));
        let active = self.active_untracked();
        if active.loaded_param_index().as_deref() == Some(param_index.as_str()) {
            self.pending_hits.set_value(None);
            active.cancel_load();
            // Already showing it, so only what it has taken has changed.
            active.show_hits(hits);
            self.incoming
                .with_untracked(|incoming| active.compare_incoming(&incoming.context.taken, &incoming.context.shells));
            return;
        }
        let started = self.start_ship_load(param_index.clone(), target.display_name);
        self.pending_hits.set_value(started.map(|load| PendingHits { param_index, load, hits, health: None }));
    }

    /// Updates the hits in the pane `source` was shown in as its playback moves. Nothing when that
    /// pane shows no ship and none is loading: loading one here would pull the reader into a tab
    /// they did not ask for.
    pub fn follow_hits(self, source: FeedSource, followed: ArmorFollowed) {
        let Some(following) = self.following.get_value().filter(|following| following.source == source) else {
            return;
        };

        let ArmorFollowed { hits, health, at } = followed;
        let loading = self.pending_hits.with_value(Option::is_some);
        if loading {
            self.playback_at.set(Some(at));
            self.pending_hits.update_value(|pending| {
                if let Some(pending) = pending {
                    pending.hits = hits;
                    pending.health = health;
                }
            });
            return;
        }

        let Some(viewport) = self.pane_untracked(following.pane) else { return };
        if !viewport.scene_features().is_some_and(|features| features.ship) {
            return;
        }
        self.playback_at.set(Some(at));
        viewport.set_hit_health(health);
        viewport.follow_hits(hits);
    }

    fn pane_untracked(self, id: PaneId) -> Option<ArmorViewportState> {
        self.panes.with_untracked(|panes| panes.iter().find(|pane| pane.id == id).map(|pane| pane.viewport))
    }

    #[cfg(test)]
    pub fn hold_hits_for_test(self, source: FeedSource, param_index: &str, generation: LoadGeneration) {
        let pane = self.active.get_untracked();
        let load = StartedLoad { pane, generation };
        let held = PendingHits { param_index: param_index.to_owned(), load, hits: Vec::new(), health: None };
        self.following.set_value(Some(Following { source, pane }));
        self.pending_hits.set_value(Some(held));
    }

    #[cfg(test)]
    pub fn holds_hits_for_test(self) -> bool {
        self.pending_hits.with_value(Option::is_some)
    }

    #[cfg(test)]
    pub fn land_ship_for_test(
        self,
        generation: LoadGeneration,
        bundle: &ArmorBundle,
        outcome: Result<FirstStage, wows_toolkit_armor::load_ship::ShipLoadError>,
    ) {
        let target = self.active_untracked();
        if !target.is_current_load(generation) {
            return;
        }
        let started = StartedLoad { pane: self.active.get_untracked(), generation };
        let outcome = outcome.map_err(|_| ProblemReason::ShipUnreadable).and_then(drawable_scene);
        self.land_ship(started, target, bundle, "PASB028".to_owned(), "Iowa".to_owned(), outcome);
    }
}

fn pane_of(panes: &[Pane], id: PaneId) -> ArmorViewportState {
    panes
        .iter()
        .find(|pane| pane.id == id)
        .or_else(|| panes.first())
        .map(|pane| pane.viewport)
        .expect("the workspace always holds a pane")
}

/// `first` when its scene has something to draw, else why it has nothing. The background stage
/// adds no geometry, so the first stage decides.
fn drawable_scene(first: FirstStage) -> Result<FirstStage, ProblemReason> {
    match first.scene.emptiness() {
        None => Ok(first),
        Some(empty) => {
            tracing::warn!("armor viewer: {} has nothing to draw: {empty:?}", first.scene.display_name);
            Err(ProblemReason::Empty(empty))
        }
    }
}

/// The hits held for `load` of `param_index`, if they are for it. Hits held for any other load
/// stay held.
fn take_pending_hits(pending: &mut Option<PendingHits>, param_index: &str, load: StartedLoad) -> Option<PendingHits> {
    if !pending.as_ref().is_some_and(|held| held.param_index == param_index && held.load == load) {
        return None;
    }

    pending.take()
}

#[cfg(test)]
mod tests {
    use super::LoadGeneration;
    use super::PaneId;
    use super::PendingHits;
    use super::StartedLoad;
    use super::take_pending_hits;

    #[test]
    fn held_hits_go_only_to_the_load_they_were_held_for() {
        let load = StartedLoad { pane: PaneId(0), generation: LoadGeneration::default() };
        let mut pending = Some(PendingHits { param_index: "PASC001".into(), load, hits: Vec::new(), health: None });

        let elsewhere = StartedLoad { pane: PaneId(1), ..load };
        assert!(take_pending_hits(&mut pending, "PASC001", elsewhere).is_none(), "another pane");
        assert!(take_pending_hits(&mut pending, "PASC002", load).is_none(), "another ship");
        assert!(pending.is_some(), "still held for its own load");

        assert!(take_pending_hits(&mut pending, "PASC001", load).is_some());
        assert!(pending.is_none());
    }
}

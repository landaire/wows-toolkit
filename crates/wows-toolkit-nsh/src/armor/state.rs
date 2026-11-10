//! The viewer's process-wide state: the shared device, the ship data bundle,
//! the panes and everything the panels around them hold.
//!
//! Every change goes through [`update`], which bumps the view revision the
//! `watch` command follows and wakes the pane render threads. Pointer motion
//! goes through [`drive`] instead, which bumps the view only when something a
//! panel shows changed and reports hover on its own channel.

use std::fmt;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;
use std::sync::mpsc;
use std::time::Duration;

use wows_replay_insights::timeline::PreExtractedHit;
use wows_replays::types::GameClock;
use wows_toolkit_armor::assets::load_ship_assets;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::PlateKey;
use wows_toolkit_armor::load_ship::ShipLoadError;
use wows_toolkit_armor::load_ship::build_reload_options;
use wows_toolkit_armor::load_ship::load_ship_armor_by_param;
use wows_toolkit_armor::load_ship::load_ship_armor_with_options;
use wows_toolkit_armor::realtime::IncomingContext;
use wows_toolkit_armor::scene::ArmorScene;
use wows_toolkit_armor::scene::Changes;
use wows_toolkit_armor::scene::GizmoView;
use wows_toolkit_armor::scene::Gpu;
use wows_toolkit_armor::scene::PaneCommand;
use wows_toolkit_armor::viewport::types::ViewRect;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wows_toolkit_viewmodel::armor::incoming::IncomingFilter;
use wows_toolkit_viewmodel::ship_catalog::ShipCatalog;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::ship::ShipAssets;

use crate::frames;
use crate::frames::PixelSize;
use crate::frames::SurfaceId;
use crate::game_data;
use crate::game_data::GameDataError;

use super::defaults;
use super::export::ExportDialog;
use super::frame::Readback;
use super::input::SceneInput;
use super::pen::PenState;
use super::render;
use super::render::Signal;
use super::view::HoverView;
use super::view::hover_view;

/// Where the legend opens before it has been moved.
const LEGEND_DEFAULT_POSITION: (f32, f32) = (10.0, 100.0);

/// One pane, by the number its surface is named with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct PaneId(u32);

impl PaneId {
    pub fn new(raw: u32) -> Self {
        Self(raw)
    }

    pub fn raw(self) -> u32 {
        self.0
    }

    pub fn surface(self) -> SurfaceId {
        SurfaceId::new(format!("armor-pane-{}", self.0))
    }
}

impl fmt::Display for PaneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The shared device every pane renders through.
pub(super) enum GpuState {
    Initializing,
    Ready(Gpu),
    Failed(String),
}

/// The ship data every pane loads from: the export assets and the catalog.
pub(super) struct Bundle {
    pub assets: ShipAssets,
    pub catalog: ShipCatalog,
}

pub(super) enum BundleState {
    NotStarted,
    /// The settings name no World of Warships directory.
    NotConfigured,
    Loading,
    Ready(Arc<Bundle>),
    Failed(String),
}

/// Where a pane's ship load stands.
pub(super) enum PaneLoad {
    Idle,
    Loading { display_name: String },
    Failed { display_name: String, reason: String },
}

/// The ship a pane shows, which is what a hull change reloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ShipSource {
    pub param_index: String,
    pub display_name: String,
}

pub(super) struct Pane {
    pub id: PaneId,
    pub scene: ArmorScene,
    signals: mpsc::Sender<Signal>,
    pub load: PaneLoad,
    /// Bumped by every load into this pane, so a load that lands after a
    /// later one started is dropped.
    load_generation: u64,
    pub source: Option<ShipSource>,
    /// Bumped whenever the pane starts showing another ship, so a tree's
    /// expansion can start over.
    pub ship_generation: u32,
    /// The inline export confirmation is up.
    pub export_confirm: bool,
}

/// The floating thickness legend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Legend {
    pub visible: bool,
    pub collapsed: bool,
    pub x: f32,
    pub y: f32,
}

impl Legend {
    fn from_defaults(row: Option<&ArmorViewerDefaultsRow>) -> Self {
        let Some(row) = row else { return Self::new() };
        let (x, y) = match (row.legend_pos_x, row.legend_pos_y) {
            (Some(x), Some(y)) => (x as f32, y as f32),
            _ => LEGEND_DEFAULT_POSITION,
        };
        Self { visible: row.show_legend, collapsed: row.legend_collapsed, x, y }
    }

    fn new() -> Self {
        let (x, y) = LEGEND_DEFAULT_POSITION;
        Self { visible: true, collapsed: false, x, y }
    }
}

/// What was fired at the ship on screen.
#[derive(Default)]
pub(super) struct IncomingFire {
    pub context: IncomingContext,
    pub filter: IncomingFilter,
    /// The ship the log is about; another hull never met those shells.
    pub victim: Option<String>,
}

impl IncomingFire {
    fn is_about(&self, param_index: &str) -> bool {
        self.victim.as_deref() == Some(param_index)
    }

    pub fn is_empty(&self) -> bool {
        self.context.taken.is_empty() && self.context.attackers.is_empty()
    }
}

/// A ship asked for before the bundle loaded, opened once it has.
struct PendingOpen {
    pane: PaneId,
    source: ShipSource,
}

type SeekHandler = Arc<dyn Fn(GameClock) + Send + Sync>;

pub(super) struct State {
    started: bool,
    pub gpu: GpuState,
    pub bundle: BundleState,
    /// The defaults row as last read or written. `None` before one exists.
    pub defaults: Option<ArmorViewerDefaultsRow>,
    /// The row has been read, so a save will not overwrite one not yet seen.
    defaults_read: bool,
    pub panes: Vec<Pane>,
    pub active: PaneId,
    next_pane: u32,
    pub mirror_cameras: bool,
    pub sync_options: bool,
    pub stack_panes: bool,
    pub options_open: bool,
    pub pen_open: bool,
    /// A ship has been shown at least once, which is what the legend waits for.
    pub ship_loaded: bool,
    pub legend: Legend,
    pub pen: PenState,
    /// The active pane's last plate as the checker last read it.
    pen_plate: Option<(String, f32)>,
    pub incoming: IncomingFire,
    pub playback_at: Option<GameClock>,
    /// Hits a replay asked for, waiting for their ship to load.
    pending_hits: Vec<PreExtractedHit>,
    pending_open: Option<PendingOpen>,
    pub reveal_request: u32,
    pub export: Option<ExportDialog>,
    seek: Option<SeekHandler>,
    pub hover: HoverView,
}

struct Service {
    state: Mutex<State>,
    view: tokio::sync::watch::Sender<u64>,
    hover: tokio::sync::watch::Sender<u64>,
}

static SERVICE: LazyLock<Service> = LazyLock::new(|| {
    let mut state = State {
        started: false,
        gpu: GpuState::Initializing,
        bundle: BundleState::NotStarted,
        defaults: None,
        defaults_read: false,
        panes: Vec::new(),
        active: PaneId(1),
        next_pane: 1,
        mirror_cameras: false,
        sync_options: false,
        stack_panes: false,
        options_open: true,
        pen_open: false,
        ship_loaded: false,
        legend: Legend::new(),
        pen: PenState::default(),
        pen_plate: None,
        incoming: IncomingFire::default(),
        playback_at: None,
        pending_hits: Vec::new(),
        pending_open: None,
        reveal_request: 0,
        export: None,
        seek: None,
        hover: HoverView::default(),
    };
    state.active = state.add_pane();
    Service {
        state: Mutex::new(state),
        view: tokio::sync::watch::channel(0).0,
        hover: tokio::sync::watch::channel(0).0,
    }
});

fn lock() -> MutexGuard<'static, State> {
    SERVICE.state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn bump(sender: &tokio::sync::watch::Sender<u64>) {
    sender.send_modify(|revision| *revision = revision.wrapping_add(1));
}

/// Applies `change`, tells the view watchers and wakes every pane.
pub(super) fn update<R>(change: impl FnOnce(&mut State) -> R) -> R {
    let result = {
        let mut state = lock();
        let result = change(&mut state);
        state.wake_panes();
        result
    };
    bump(&SERVICE.view);

    result
}

/// Reads the state without changing it.
pub(super) fn read<R>(look: impl FnOnce(&State) -> R) -> R {
    look(&lock())
}

pub(super) fn subscribe_view() -> tokio::sync::watch::Receiver<u64> {
    SERVICE.view.subscribe()
}

pub(super) fn subscribe_hover() -> tokio::sync::watch::Receiver<u64> {
    SERVICE.hover.subscribe()
}

pub(super) fn spawn_named(name: &str, work: impl FnOnce() + Send + 'static) {
    if let Err(error) = std::thread::Builder::new().name(name.to_owned()).spawn(work) {
        tracing::warn!("armor viewer: no thread for {name}: {error}");
    }
}

/// What a pane's render thread has to present.
pub(super) struct Driven {
    pub readback: Option<Readback>,
    pub gizmo: Option<GizmoView>,
    pub wants_tick: bool,
}

/// Feeds pane `id` its input and `step` of motion, then renders it at `view`
/// when it is dirty or `resized`. `None` once the pane is gone.
pub(super) fn drive(
    id: PaneId,
    inputs: Vec<SceneInput>,
    view: Option<(PixelSize, f32)>,
    step: Option<Duration>,
    resized: bool,
) -> Option<Driven> {
    let (driven, view_changed, hover_changed) = {
        let mut state = lock();
        let index = state.index_of(id)?;
        let mut view_changed = false;
        let mut hover_changed = false;

        if let Some((size, scale)) = view {
            let scene = &mut state.panes[index].scene;
            scene.set_rect(ViewRect::new(0.0, 0.0, size.width as f32 / scale, size.height as f32 / scale));
            if resized {
                scene.mark_dirty();
            }
        }

        for input in inputs {
            let (view, hover) = state.apply_input(index, input);
            view_changed |= view;
            hover_changed |= hover;
        }

        if let Some(step) = step
            && state.panes[index].scene.wants_tick()
        {
            state.panes[index].scene.tick(step);
            let changes = state.absorb(index);
            view_changed |= changes.settings || changes.trajectory_mode;
        }

        let scene = &mut state.panes[index].scene;
        let readback = match view {
            Some((size, _)) if !size.is_empty() => scene
                .render((size.width, size.height))
                .map(|(width, height, pixels)| Readback { width, height, pixels }),
            _ => None,
        };
        let gizmo = if readback.is_some() { scene.gizmo() } else { None };
        let driven = Driven { readback, gizmo, wants_tick: scene.wants_tick() };
        if view_changed {
            state.wake_panes();
        }

        (driven, view_changed, hover_changed)
    };
    if view_changed {
        bump(&SERVICE.view);
    }
    if hover_changed {
        bump(&SERVICE.hover);
    }

    Some(driven)
}

/// Shows `param_index` with the hits it had taken, for a replay that asked
/// for it, and raises the viewer.
pub fn show_realtime(param_index: String, display_name: String, hits: Vec<PreExtractedHit>, incoming: IncomingContext) {
    update(|state| {
        state.ensure_started();
        state.reveal_request = state.reveal_request.wrapping_add(1);
        state.pending_hits = hits;
        state.playback_at = incoming.at;
        state.incoming =
            IncomingFire { context: incoming, filter: IncomingFilter::default(), victim: Some(param_index.clone()) };

        let index = state.active_index();
        if state.shows(index, &param_index) {
            // Already showing it, so only what it has taken has changed.
            let hits = std::mem::take(&mut state.pending_hits);
            let incoming = &state.incoming.context;
            let scene = &mut state.panes[index].scene;
            scene.show_hits(hits);
            scene.compare_incoming(&incoming.taken, &incoming.shells);
            state.absorb(index);
            return;
        }
        let active = state.active;
        state.start_ship_load(active, ShipSource { param_index, display_name });
    });
}

/// Updates what the viewer shows as playback moves. Nothing until a ship has
/// been shown: following never opens one.
pub fn follow_realtime(hits: Vec<PreExtractedHit>, health: Option<f32>, at: GameClock) {
    update(|state| {
        if !state.ship_loaded {
            return;
        }
        state.playback_at = Some(at);
        let index = state.active_index();
        let scene = &mut state.panes[index].scene;
        scene.set_hit_health(health);
        scene.show_hits(hits);
        state.absorb(index);
    });
}

/// Where the viewer asks playback to move.
pub fn set_seek_handler(handler: impl Fn(GameClock) + Send + Sync + 'static) {
    update(|state| state.seek = Some(Arc::new(handler)));
}

/// Asks playback to move to `clock`. The handler runs outside the lock: it
/// may well call back into the viewer.
pub(super) fn seek(clock: GameClock) {
    let handler = read(|state| state.seek.clone());
    match handler {
        Some(handler) => handler(clock),
        None => tracing::debug!("armor viewer: no playback to seek"),
    }
}

impl State {
    /// Starts the device and the ship data, once. A bundle the settings did
    /// not allow, or that failed, is tried again.
    pub fn ensure_started(&mut self) {
        if !self.started {
            self.started = true;
            spawn_named("armor-gpu", || {
                let created = Gpu::new();
                update(|state| state.adopt_gpu(created));
            });
            spawn_named("armor-defaults-read", || {
                let read = defaults::load_viewer_defaults();
                update(|state| state.adopt_defaults(read));
            });
        }
        if matches!(self.bundle, BundleState::NotStarted | BundleState::NotConfigured | BundleState::Failed(_)) {
            self.bundle = BundleState::Loading;
            spawn_named("armor-bundle", || {
                let loaded = load_bundle();
                update(|state| state.adopt_bundle(loaded));
            });
        }
    }

    fn adopt_gpu<E>(&mut self, created: Result<Gpu, E>)
    where
        E: fmt::Display,
    {
        match created {
            Ok(gpu) => {
                for pane in &mut self.panes {
                    pane.scene.set_gpu(gpu.clone());
                }
                self.gpu = GpuState::Ready(gpu);
            }
            Err(error) => {
                tracing::error!("armor viewer: failed to create the shared wgpu device: {error:#}");
                self.gpu = GpuState::Failed(format!("{error:#}"));
            }
        }
    }

    fn adopt_defaults(&mut self, read: Result<Option<ArmorViewerDefaultsRow>, super::error::ArmorError>) {
        // A row that could not be read is not overwritten: it may hold what
        // the reader set.
        let row = match read {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!("armor viewer: the defaults could not be read: {error}");
                return;
            }
        };
        self.defaults_read = true;
        self.legend = Legend::from_defaults(row.as_ref());
        for pane in &mut self.panes {
            pane.scene.apply_defaults(row.as_ref());
        }
        self.defaults = row;
    }

    fn adopt_bundle(&mut self, loaded: Result<Bundle, BundleState>) {
        match loaded {
            Ok(bundle) => {
                self.bundle = BundleState::Ready(Arc::new(bundle));
                if let Some(PendingOpen { pane, source }) = self.pending_open.take() {
                    self.start_ship_load(pane, source);
                }
            }
            Err(state) => self.bundle = state,
        }
    }

    pub fn bundle(&self) -> Option<&Arc<Bundle>> {
        match &self.bundle {
            BundleState::Ready(bundle) => Some(bundle),
            _ => None,
        }
    }

    pub fn index_of(&self, id: PaneId) -> Option<usize> {
        self.panes.iter().position(|pane| pane.id == id)
    }

    /// The active pane's index. A pane is always active.
    pub fn active_index(&self) -> usize {
        self.index_of(self.active).unwrap_or(0)
    }

    pub fn active_pane(&self) -> &Pane {
        &self.panes[self.active_index()]
    }

    /// Whether pane `index` shows `param_index`, fully loaded.
    fn shows(&self, index: usize, param_index: &str) -> bool {
        let pane = &self.panes[index];
        matches!(pane.load, PaneLoad::Idle)
            && pane.scene.has_armor()
            && pane.source.as_ref().is_some_and(|source| source.param_index == param_index)
    }

    fn wake_panes(&self) {
        for pane in &self.panes {
            // A pane whose thread has stopped is being closed.
            let _ = pane.signals.send(Signal::Wake);
        }
    }

    /// A new empty pane, with its render thread running.
    fn add_pane(&mut self) -> PaneId {
        let id = PaneId(self.next_pane);
        self.next_pane += 1;
        let surface = frames::surface(&id.surface());
        let signals = render::spawn(id, surface);
        let mut scene = ArmorScene::new();
        if let GpuState::Ready(gpu) = &self.gpu {
            scene.set_gpu(gpu.clone());
        }
        scene.apply_defaults(self.defaults.as_ref());
        self.panes.push(Pane {
            id,
            scene,
            signals,
            load: PaneLoad::Idle,
            load_generation: 0,
            source: None,
            ship_generation: 0,
            export_confirm: false,
        });

        id
    }

    /// Runs `change` on the active pane's scene and settles what it changed.
    pub fn act(&mut self, change: impl FnOnce(&mut ArmorScene)) {
        let index = self.active_index();
        change(&mut self.panes[index].scene);
        self.absorb(index);
    }

    /// Settles what pane `index` changed: mirrors its camera, syncs its
    /// settings and saves them, and opens the rail for trajectory mode.
    pub fn absorb(&mut self, index: usize) -> Changes {
        let changes = self.panes[index].scene.take_changes();
        if changes.camera && self.mirror_cameras {
            self.push_camera_from(index);
        }
        if changes.settings {
            if self.sync_options {
                self.push_settings_from(index);
            }
            self.save_defaults();
        }
        if changes.trajectory_mode && self.panes[index].scene.trajectory_state().shown {
            self.options_open = true;
        }

        changes
    }

    fn push_camera_from(&mut self, source: usize) {
        let camera = self.panes[source].scene.camera();
        for (index, pane) in self.panes.iter_mut().enumerate() {
            if index != source {
                pane.scene.set_camera(camera.clone());
            }
        }
    }

    fn push_settings_from(&mut self, source: usize) {
        let settings = self.panes[source].scene.synced_settings();
        for (index, pane) in self.panes.iter_mut().enumerate() {
            if index != source {
                pane.scene.apply_synced(&settings);
            }
        }
    }

    /// Writes the active pane's display settings and the legend back to the
    /// defaults row. The active pane's are kept: a comparison pane is a
    /// second view, not a second preference.
    pub fn save_defaults(&mut self) {
        if !self.defaults_read {
            return;
        }
        let row = self.defaults_row();
        self.defaults = Some(self.defaults_row());
        defaults::save_viewer_defaults(row);
    }

    fn defaults_row(&self) -> ArmorViewerDefaultsRow {
        let scene = &self.active_pane().scene;
        let display = scene.display_settings();
        let all_visible = scene.all_visible();
        ArmorViewerDefaultsRow {
            show_plate_edges: display.show_plate_edges,
            show_waterline: display.show_waterline,
            show_zero_mm: display.show_zero_mm,
            armor_opacity: display.armor_opacity as f64,
            waterline_opacity: display.waterline_opacity as f64,
            hull_opaque: display.hull_opaque,
            hull_all_visible: all_visible.hull,
            armor_all_visible: all_visible.armor,
            // No control here sets it, so it is written back as read; a
            // database with no row yet holds the column's own default.
            show_splash_boxes: self.defaults.as_ref().is_some_and(|row| row.show_splash_boxes),
            show_legend: self.legend.visible,
            legend_collapsed: self.legend.collapsed,
            legend_pos_x: Some(self.legend.x as f64),
            legend_pos_y: Some(self.legend.y as f64),
        }
    }

    /// One input on pane `index`. Returns whether the view and the hover
    /// changed.
    fn apply_input(&mut self, index: usize, input: SceneInput) -> (bool, bool) {
        let mut view = false;
        let id = self.panes[index].id;
        let motion = matches!(input, SceneInput::Moved { .. } | SceneInput::Left);
        let pressed = matches!(input, SceneInput::Down { .. });
        let scene = &mut self.panes[index].scene;
        let command = match input {
            SceneInput::Resize { .. } => None,
            SceneInput::Down { at, button, clicks } => {
                scene.pointer_down(at, button, clicks);
                None
            }
            SceneInput::Up { at, button, shift } => {
                scene.pointer_up(at, button, shift);
                None
            }
            SceneInput::Moved { at } => {
                scene.pointer_moved(at);
                None
            }
            SceneInput::Left => {
                scene.pointer_left();
                None
            }
            SceneInput::Wheel { dy } => {
                scene.wheel(dy);
                None
            }
            SceneInput::Pinch { factor } => {
                scene.pinch(factor);
                None
            }
            SceneInput::KeyDown { key, command, shift } => scene.key_down(&key, command, shift),
            SceneInput::KeyUp { key } => {
                scene.key_up(&key);
                None
            }
            SceneInput::ReleaseKeys => {
                scene.release_keys();
                None
            }
        };
        // Any press makes its pane the one the panels act on.
        if pressed && self.active != id {
            self.activate(id);
            view = true;
        }
        if command == Some(PaneCommand::ToggleOptions) {
            self.set_options_open(!self.options_open);
            view = true;
        }

        let changes = self.absorb(index);
        // Pointer motion changes the hover, which has its own channel.
        view |= changes.settings || changes.trajectory_mode || (changes.view && !motion);
        view |= self.refresh_pen_plate();
        let hover = self.refresh_hover(index);

        (view, hover)
    }

    /// Reads the active pane's last plate into the checker. Returns whether
    /// it moved.
    fn refresh_pen_plate(&mut self) -> bool {
        let plate = self.active_pane().scene.last_plate().map(|(zone, mm)| (zone.to_owned(), mm));
        if plate == self.pen_plate {
            return false;
        }
        self.pen_plate = plate;
        true
    }

    /// Reads pane `index`'s hover into the hover channel. Returns whether it
    /// changed.
    fn refresh_hover(&mut self, index: usize) -> bool {
        let id = self.panes[index].id;
        let metadata = self.bundle().map(|bundle| bundle.assets.metadata());
        let hover = hover_view(id, &self.panes[index].scene, metadata);
        let empty = hover.plate.is_none() && hover.ring.is_none();
        if empty && self.hover.pane_id != Some(id.raw()) {
            return false;
        }
        let hover = if empty { HoverView::default() } else { hover };
        if hover == self.hover {
            return false;
        }
        self.hover = hover;
        true
    }

    pub fn activate(&mut self, id: PaneId) {
        if self.index_of(id).is_none() || self.active == id {
            return;
        }
        self.active = id;
        self.push_cast_ship();
        self.refresh_pen_plate();
    }

    pub fn set_options_open(&mut self, open: bool) {
        self.options_open = open;
        // A hovered tree row leaves with the rail, not under the pointer, so
        // its highlight would outlive it.
        if !open {
            for pane in &mut self.panes {
                pane.scene.clear_sidebar_hover();
            }
        }
    }

    /// Hands the active pane the ship and shell a trajectory cast fires.
    pub fn push_cast_ship(&mut self) {
        let (ship, shell) = self.pen.cast_ship();
        self.act(|scene| scene.set_cast_ship(ship, shell));
    }

    /// Opens a catalog ship in the active pane, unless it already shows it.
    pub fn open_ship(&mut self, source: ShipSource) {
        let index = self.active_index();
        if self.shows(index, &source.param_index) {
            return;
        }
        // The log was about the replay's ship, not this one.
        self.incoming = IncomingFire::default();
        let active = self.active;
        self.start_ship_load(active, source);
    }

    /// Loads `source` into pane `id` off-thread. Waits for the bundle when it
    /// is still loading.
    pub fn start_ship_load(&mut self, id: PaneId, source: ShipSource) {
        let Some(bundle) = self.bundle().cloned() else {
            self.ensure_started();
            self.pending_open = Some(PendingOpen { pane: id, source });
            return;
        };
        let Some(index) = self.index_of(id) else { return };
        let pane = &mut self.panes[index];
        pane.load_generation += 1;
        let generation = pane.load_generation;
        pane.load = PaneLoad::Loading { display_name: source.display_name.clone() };

        spawn_named("armor-ship-load", move || {
            let loaded = load_ship_armor_by_param(&bundle.assets, &source.param_index, &source.display_name);
            update(|state| state.finish_ship_load(id, generation, source, loaded));
        });
    }

    fn finish_ship_load(
        &mut self,
        id: PaneId,
        generation: u64,
        source: ShipSource,
        loaded: Result<LoadedShipArmor, ShipLoadError>,
    ) {
        let Some(index) = self.index_of(id) else { return };
        if self.panes[index].load_generation != generation {
            return;
        }
        let armor = match loaded {
            Ok(armor) => Arc::new(armor),
            Err(error) => {
                tracing::error!("armor viewer: failed to load {}: {error}", source.display_name);
                self.panes[index].load =
                    PaneLoad::Failed { display_name: source.display_name, reason: error.to_string() };
                return;
            }
        };

        self.ship_loaded = true;
        let comparing = self.incoming.is_about(&source.param_index);
        let hits = std::mem::take(&mut self.pending_hits);
        let incoming = &self.incoming.context;
        let pane = &mut self.panes[index];
        pane.load = PaneLoad::Idle;
        pane.source = Some(source);
        pane.ship_generation = pane.ship_generation.wrapping_add(1);
        pane.scene.show_armor(armor);
        // After the armor: showing a ship starts it over and would drop them.
        if !hits.is_empty() {
            pane.scene.show_hits(hits);
        }
        if comparing {
            pane.scene.compare_incoming(&incoming.taken, &incoming.shells);
        }
        self.absorb(index);
        if self.active == id {
            self.push_cast_ship();
        }
    }

    /// Reloads the active pane's ship under its hull, LOD and module choice.
    pub fn reload_active(&mut self) {
        let index = self.active_index();
        let Some(bundle) = self.bundle().cloned() else { return };
        let pane = &mut self.panes[index];
        let Some(source) = pane.source.clone() else { return };
        let selection = pane.scene.hull_selection();
        let generation = pane.scene.begin_reload();
        let id = pane.id;
        let options = match build_reload_options(
            &bundle.assets,
            &source.param_index,
            source.display_name.clone(),
            selection.lod,
            selection.hull,
            selection.modules,
        ) {
            Ok(options) => options,
            Err(error) => {
                tracing::error!("armor viewer: cannot reload {}: {error}", source.display_name);
                return;
            }
        };

        spawn_named("armor-ship-reload", move || {
            let reloaded = load_ship_armor_with_options(&bundle.assets, &source.param_index, options);
            update(|state| {
                let Some(index) = state.index_of(id) else { return };
                match reloaded {
                    Ok(armor) => state.panes[index].scene.apply_reload(generation, Arc::new(armor)),
                    Err(error) => tracing::error!("armor viewer: failed to reload {}: {error}", source.display_name),
                }
                state.absorb(index);
            });
        });
    }

    /// Selects a camo on the active pane, decoding it off-thread.
    pub fn select_camo(&mut self, id: Option<CamoSchemeId>) {
        let index = self.active_index();
        let pane_id = self.panes[index].id;
        let job = self.panes[index].scene.select_camo(id);
        self.absorb(index);
        let Some(job) = job else { return };

        spawn_named("armor-camo", move || {
            let decoded = job.run();
            update(|state| {
                let Some(index) = state.index_of(pane_id) else { return };
                state.panes[index].scene.apply_camo(decoded);
                state.absorb(index);
            });
        });
    }

    /// A new pane copying the active one, made active. With a ship, that
    /// ship loads into it instead.
    pub fn compare(&mut self, ship: Option<ShipSource>) {
        let active = self.active_pane();
        let copy = active
            .scene
            .current_armor()
            .cloned()
            .map(|armor| (armor, active.scene.camera(), active.scene.synced_settings(), active.source.clone()));

        let id = self.add_pane();
        let index = self.panes.len() - 1;
        if let Some((armor, camera, synced, source)) = copy {
            let pane = &mut self.panes[index];
            pane.source = source;
            pane.scene.show_armor(armor);
            pane.scene.set_camera(camera);
            pane.scene.apply_synced(&synced);
        }
        self.active = id;
        self.push_cast_ship();
        self.refresh_pen_plate();

        if let Some(ship) = ship {
            self.start_ship_load(id, ship);
        }
    }

    /// Closes pane `id`. The last pane is emptied instead.
    pub fn close_pane(&mut self, id: PaneId) {
        let Some(index) = self.index_of(id) else { return };
        if self.panes.len() == 1 {
            let pane = &mut self.panes[index];
            pane.scene.clear();
            pane.scene.apply_defaults(self.defaults.as_ref());
            pane.load_generation += 1;
            pane.load = PaneLoad::Idle;
            pane.source = None;
            pane.export_confirm = false;
            pane.ship_generation = pane.ship_generation.wrapping_add(1);
            self.ship_loaded = false;
            self.refresh_pen_plate();
            return;
        }

        let pane = self.panes.remove(index);
        let _ = pane.signals.send(Signal::Stop);
        frames::release(&pane.id.surface());
        if self.active == id {
            let next = self.panes[index.min(self.panes.len() - 1)].id;
            self.active = next;
            self.push_cast_ship();
        }
        self.refresh_pen_plate();
    }

    pub fn set_mirror_cameras(&mut self, on: bool) {
        if self.mirror_cameras == on {
            return;
        }
        self.mirror_cameras = on;
        if on {
            let index = self.active_index();
            self.push_camera_from(index);
        }
    }

    pub fn set_sync_options(&mut self, on: bool) {
        if self.sync_options == on {
            return;
        }
        self.sync_options = on;
        if on {
            let index = self.active_index();
            self.push_settings_from(index);
        }
    }

    /// The active pane's ship, as a reload or an export needs it.
    pub fn active_source(&self) -> Option<&ShipSource> {
        self.active_pane().source.as_ref()
    }

    /// Hides the hovered plate, or shows it again.
    pub fn toggle_plate(&mut self, key: PlateKey) {
        self.act(|scene| scene.toggle_plate(key));
    }

    /// The thicknesses a part checkbox covers on the active pane.
    pub fn part_thicknesses(&self, zone: &str, part: &str) -> Vec<i32> {
        let scene = &self.active_pane().scene;
        let show_zero_mm = scene.display_settings().show_zero_mm;
        scene
            .current_armor()
            .and_then(|armor| armor.zone_part_plates.iter().find(|candidate| candidate.name == zone))
            .and_then(|zone| zone.parts.iter().find(|candidate| candidate.name == part))
            .map(|part| super::tree::shown_thicknesses(part, show_zero_mm))
            .unwrap_or_default()
    }

    /// The hull meshes of every group, or of one.
    pub fn hull_meshes(&self, group: Option<&str>) -> Vec<String> {
        let Some(armor) = self.active_pane().scene.current_armor() else { return Vec::new() };
        armor
            .hull_part_groups
            .iter()
            .filter(|(name, _)| group.is_none_or(|group| group == name))
            .flat_map(|(_, meshes)| meshes.iter().cloned())
            .collect()
    }

    pub fn set_hull_visible(&mut self, meshes: Vec<String>, visible: bool) {
        self.act(|scene| {
            scene.mutate_hull_visibility(|hull| {
                for mesh in meshes {
                    hull.insert(mesh, visible);
                }
            })
        });
    }

    pub fn set_legend(&mut self, change: impl FnOnce(&mut Legend)) {
        change(&mut self.legend);
        self.save_defaults();
    }
}

/// Reads the installed build's ship data. The error is the state to show.
fn load_bundle() -> Result<Bundle, BundleState> {
    let loaded = match game_data::wait_current_build() {
        Ok(loaded) => loaded,
        Err(GameDataError::NotConfigured) => return Err(BundleState::NotConfigured),
        Err(error) => return Err(BundleState::Failed(error.to_string())),
    };
    let assets =
        load_ship_assets(loaded.vfs_with_assets(), loaded.assets_bin_bytes(), loaded.provider()).map_err(|error| {
            tracing::error!("armor viewer: failed to load ship assets: {error}");
            BundleState::Failed(error.to_string())
        })?;
    let catalog = ShipCatalog::build(loaded.provider());

    Ok(Bundle { assets, catalog })
}

//! One armor viewport: the offscreen scene, the ship shown in it, everything
//! the reader set on it, and what the pointer and keys do to it.
//!
//! A front end owns one [`ArmorScene`] per pane, forwards its pointer, wheel
//! and key input in logical pixels relative to the viewport, renders it when
//! [`ArmorScene::is_dirty`], and reads [`ArmorScene::take_changes`] to learn
//! what it has to redraw around the image or pass on to other panes. Work
//! that must not block the caller (a ship reload, a camo decode) is handed
//! back as a job the front end runs off its UI thread and returns.

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use wows_replay_insights::timeline::PreExtractedHit;
use wows_replays::analyzer::battle_controller::state::ResolvedShotHit;
use wows_replays::types::GameParamId;
use wows_replays::types::ShotId;
use wows_toolkit_config::queries::ArmorViewerDefaultsRow;
use wows_toolkit_viewmodel::armor::arc::ArcOutcome;
use wows_toolkit_viewmodel::armor::arc::ArcPlate;
use wows_toolkit_viewmodel::armor::arc::ComparisonVerdict;
use wows_toolkit_viewmodel::armor::arc::PlateKeyParts;
use wows_toolkit_viewmodel::armor::arc::Strike;
use wows_toolkit_viewmodel::armor::arc::ZoneShape;
use wows_toolkit_viewmodel::armor::arc::compare_with_server;
use wows_toolkit_viewmodel::armor::arc::describe_arc;
use wows_toolkit_viewmodel::armor::arc::isolate;
use wows_toolkit_viewmodel::armor::camera_perspective::CameraPerspective;
use wows_toolkit_viewmodel::armor::camera_perspective::LookMode;
use wows_toolkit_viewmodel::armor::camera_perspective::water_aim_point;
use wows_toolkit_viewmodel::armor::incoming::ServerOutcome;
use wows_toolkit_viewmodel::armor::penetration::ComparisonShip;
use wowsunpack::ballistics::ImpactResult;
use wowsunpack::ballistics::ShellParams;
use wowsunpack::ballistics::solve_for_range;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::SchemeTextures;
use wowsunpack::export::camouflage::UvTransform;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::types::AmmoType;
use wowsunpack::game_params::types::CameraTrajectory;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::Millimeters;
use wowsunpack::game_params::types::ShellInfo;

use crate::camera_rings;
use crate::camo::CamoDecode;
use crate::camo::CamoTextureCache;
use crate::camo::CamoTextures;
use crate::camo::decode_active_camo;
use crate::camo::recompute_active_camo;
use crate::camo::remap_camo_selection;
use crate::gaps;
use crate::highlight;
use crate::hits;
use crate::load_ship::ArmorTriangleTooltip;
use crate::load_ship::DEFAULT_LOD;
use crate::load_ship::LoadedShipArmor;
use crate::load_ship::PlateKey;
use crate::splash;
use crate::trajectory;
use crate::upload;
use crate::upload::DisplaySettings;
use crate::upload_hull;
use crate::viewport::camera;
use crate::viewport::camera::ArcballCamera;
use crate::viewport::camera::Axis;
use crate::viewport::device::GpuContext;
use crate::viewport::gizmo;
use crate::viewport::renderer::GpuPipeline;
use crate::viewport::renderer::LAYER_OVERLAY;
use crate::viewport::renderer::Viewport3D;
use crate::viewport::types::LightingSettings;
use crate::viewport::types::MeshId;
use crate::viewport::types::Vec2;
use crate::viewport::types::Vec3;
use crate::viewport::types::ViewRect;
use crate::visibility;
use crate::visibility::SidebarHighlightKey;
use crate::visibility::VisibilityFilter;
use crate::visibility::VisibilitySnapshot;
use crate::visibility::VisibilityUndoStack;

/// How far the locked camera's aim marker reaches, as a fraction of the model's
/// diagonal, and the span it is held to so it is legible on any ship.
const AIM_MARKER_FRACTION: f32 = 0.03;
const AIM_MARKER_MIN: f32 = 1.0;
const AIM_MARKER_MAX: f32 = 8.0;

/// What the locked camera's aim marker is coloured (`armor_viewer/ui/tab.rs`).
const AIM_MARKER_COLOR: [f32; 4] = [1.0, 0.6, 0.1, 0.85];

/// How far along the look direction the water is searched for, so a camera
/// aimed near the horizon marks a far point rather than none.
const AIM_MARKER_REACH: f32 = 5000.0;

/// What a drag and the wheel do to the locked camera, per pixel and per notch.
const PERSPECTIVE_YAW_PER_PX: f32 = 0.005;
const PERSPECTIVE_PITCH_PER_PX: f32 = 0.005;
const PERSPECTIVE_ZOOM_PER_NOTCH: f32 = 0.001;

/// How far outside the hull a cast ray starts, in model units, so it crosses
/// every plate on the way in rather than only those past the point clicked.
const CAST_STANDOFF: f32 = 50.0;

/// The camera distance trajectory markers are sized against, so one stays
/// readable as the view pulls back.
const CAST_SCALE_REFERENCE: f32 = 200.0;

/// How wide a ship is assumed to be before its bounds are known, so an arc
/// drawn early has a length rather than none.
const DEFAULT_MODEL_EXTENT: f32 = 20.0;

/// The range a cast shell is fired from until one is chosen.
pub const DEFAULT_CAST_RANGE: Km = Km::new(10.0);

/// The span a cast range is chosen from, and its step.
pub const CAST_RANGE_MIN: f32 = 0.0;
pub const CAST_RANGE_MAX: f32 = 30.0;
pub const CAST_RANGE_STEP: f32 = 0.5;

/// The colours cast shells are drawn in, in the order they were cast.
pub const TRAJECTORY_PALETTE: [[f32; 4]; 4] =
    [[0.30, 0.75, 1.00, 1.0], [1.00, 0.65, 0.20, 1.0], [0.65, 1.00, 0.45, 1.0], [0.95, 0.45, 0.85, 1.0]];

/// How far the hull may be heeled over, in degrees, matching the egui
/// slider's own range.
pub const ROLL_LIMIT_DEG: f32 = 25.0;

/// How long a gizmo-snap animation runs.
const GIZMO_SNAP_DURATION_SECS: f32 = 0.35;

/// Total pointer displacement since a press, in logical pixels, before it
/// counts as a drag rather than a click, matching egui's own threshold.
const DRAG_THRESHOLD_PX: f32 = 4.0;

/// How many lines of the shot log are shown at once.
const SHOT_LOG_LINES: usize = 12;

/// The mode a ship opens on: its first, which is the one the game itself
/// uses. `None` for a ship whose GameParams name none.
fn first_camera_mode(armor: &LoadedShipArmor) -> Option<String> {
    armor.camera_trajectories.first().map(|(name, _)| name.clone())
}

/// The shared device every pane renders through.
#[derive(Clone)]
pub struct Gpu {
    pub ctx: Arc<GpuContext>,
    pub pipeline: Arc<GpuPipeline>,
}

impl Gpu {
    /// Stands up the owned device and its pipeline. Blocks; run it off the UI
    /// thread.
    pub fn new() -> anyhow::Result<Self> {
        let ctx = GpuContext::new()?;
        let pipeline = ctx.pipeline();
        Ok(Self { ctx: Arc::new(ctx), pipeline: Arc::new(pipeline) })
    }
}

/// How the camera orbits are drawn.
///
/// `mode` is `None` until a ship names one, and is corrected whenever the
/// loaded ship changes: a mode name belongs to a ship's own GameParams.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CameraRingSettings {
    pub shown: bool,
    pub mode: Option<String>,
    /// Where between the field-of-view extremes the camera sits, 0 to 1.
    pub fov: f32,
    /// How far the camera is raised, -1 to 1.
    pub height: f32,
    pub zoom_path: bool,
    pub zoom_path_at_fov: bool,
    pub zoom_path_at_max_fov: bool,
}

/// The camera locked onto one of the ship's own orbits.
///
/// While it is on, a drag turns the eye on its orbit and tilts it, and the
/// wheel moves between the inner and outer orbit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PerspectiveSettings {
    pub enabled: bool,
    pub camera: CameraPerspective,
}

/// Which of the two isolations an arc is being asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Isolate {
    /// Only the plates the shell crossed.
    Plates,
    /// Every part of the zones it entered.
    Zones,
}

/// One cast arc, as the analysis panel shows it.
#[derive(Clone, Debug)]
pub struct ArcSummary {
    /// The colour the arc is drawn in, so the row and the line match.
    pub color: [f32; 4],
    pub hits: usize,
    /// The plating the shell crossed, added up regardless of angle.
    pub total_armor: Millimeters,
    pub range: Km,
    pub isolating_plates: bool,
    pub isolating_zones: bool,
    pub outcome: ArcOutcome,
}

/// Whether the armor's openings are marked, and how many were found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GapState {
    pub shown: bool,
    pub count: usize,
}

/// Whether clicks burst shells, and what the last burst reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplashState {
    pub shown: bool,
    pub boxes_shown: bool,
    /// Whether this hull ships a splash file at all. The mode is refused
    /// without one rather than placing a burst against nothing.
    pub has_data: bool,
    pub zones_reached: usize,
}

/// Whether clicks cast shells, and what has been cast so far.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrajectoryState {
    pub shown: bool,
    pub count: usize,
    pub continue_on_ricochet: bool,
}

/// What a pane is currently showing all of, which is what the next ship
/// opens showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllVisible {
    pub hull: bool,
    pub armor: bool,
}

impl AllVisible {
    /// Read against this port's absence rules: a part with no entry is
    /// visible and a hull mesh with no entry is not.
    pub fn of(
        part: &HashMap<(String, String), bool>,
        plate: &HashMap<PlateKey, bool>,
        hull: &HashMap<String, bool>,
    ) -> Self {
        Self {
            hull: !hull.is_empty() && hull.values().all(|visible| *visible),
            armor: part.values().all(|visible| *visible) && !plate.values().any(|hidden| *hidden),
        }
    }
}

/// What one pane hands another while "sync options" is on, matching the egui
/// app's `SyncedPaneSettings`.
///
/// The selected camo is not among them: a `CamoSchemeId` is an index within
/// one ship's own scheme list.
#[derive(Clone, Debug, PartialEq)]
pub struct SyncedSettings {
    pub part_visibility: HashMap<(String, String), bool>,
    pub plate_visibility: HashMap<PlateKey, bool>,
    pub hull_visibility: HashMap<String, bool>,
    pub display_settings: DisplaySettings,
    pub lighting: LightingSettings,
    pub camera_rings: CameraRingSettings,
    pub perspective: PerspectiveSettings,
}

/// What changed since the front end last asked, beyond the image itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// The reader moved the camera; a mirrored pane follows.
    pub camera: bool,
    /// The reader changed a setting other panes sync and the defaults save.
    pub settings: bool,
    /// Trajectory mode went on or off, which opens the options rail.
    pub trajectory_mode: bool,
    /// Something drawn around the image changed: a tooltip, a count, a list.
    pub view: bool,
}

impl Changes {
    pub fn any(self) -> bool {
        self.camera || self.settings || self.trajectory_mode || self.view
    }
}

/// A pointer button, as far as the viewport cares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Primary,
    Middle,
    Secondary,
}

/// Something a key asks of the pane around the viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneCommand {
    ToggleOptions,
}

/// The hovered plate: its key, its tooltip, and where the pointer is.
#[derive(Clone, Debug)]
pub struct HoverInfo {
    pub key: PlateKey,
    pub tooltip: ArmorTriangleTooltip,
    pub cursor: Vec2,
}

/// The camera orbit under the pointer: what it reads as, and where.
#[derive(Clone, Debug, PartialEq)]
pub struct RingHover {
    pub label: String,
    pub cursor: Vec2,
}

/// One gizmo ball to draw, in draw order (farthest first).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GizmoBall {
    pub axis: Axis,
    pub positive: bool,
    pub tip: Vec2,
    pub hovered: bool,
}

/// The navigation gizmo as it should be drawn this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct GizmoView {
    pub center: Vec2,
    pub balls: Vec<GizmoBall>,
}

/// The colour a gizmo axis is drawn in, as 0xRRGGBB.
pub fn axis_rgb(axis: Axis) -> u32 {
    match axis {
        Axis::X => 0xDC4646,
        Axis::Y => 0x5AC85A,
        Axis::Z => 0x5082E6,
    }
}

/// The label drawn on a positive gizmo ball.
pub fn axis_label(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "X",
        Axis::Y => "Y",
        Axis::Z => "Z",
    }
}

/// Movement keys tracked for continuous camera motion while held.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum MoveKey {
    Forward,
    Back,
    Left,
    Right,
    Up,
    Down,
    RotateLeft,
    RotateRight,
    PanLeft,
    PanRight,
    PanUp,
    PanDown,
}

impl MoveKey {
    /// `key` is a lowercase key name (`"w"`, `"up"`). Shift turns the arrows
    /// from a vertical move and an orbit into a pan.
    pub fn from_key(key: &str, shift: bool) -> Option<Self> {
        match (key, shift) {
            ("w", _) => Some(Self::Forward),
            ("s", _) => Some(Self::Back),
            ("a", _) => Some(Self::Left),
            ("d", _) => Some(Self::Right),
            ("up", false) => Some(Self::Up),
            ("down", false) => Some(Self::Down),
            ("left", false) => Some(Self::RotateLeft),
            ("right", false) => Some(Self::RotateRight),
            ("up", true) => Some(Self::PanUp),
            ("down", true) => Some(Self::PanDown),
            ("left", true) => Some(Self::PanLeft),
            ("right", true) => Some(Self::PanRight),
            _ => None,
        }
    }

    /// Every motion a released key may have started: Shift can be let go
    /// before the arrow it modified.
    pub fn released_by(key: &str) -> impl Iterator<Item = Self> {
        [Self::from_key(key, false), Self::from_key(key, true)].into_iter().flatten()
    }
}

/// One tick's camera motion from the held keys, each axis summed over the
/// keys that drive it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeldMotion {
    pub forward: f32,
    pub right: f32,
    pub vertical: f32,
    pub rotate: f32,
    /// Screen space, +x right and +y up.
    pub pan_x: f32,
    pub pan_y: f32,
}

impl HeldMotion {
    pub fn from_keys<'a>(keys: impl IntoIterator<Item = &'a MoveKey>) -> Self {
        let mut motion = Self::default();
        for key in keys {
            match key {
                MoveKey::Forward => motion.forward += 1.0,
                MoveKey::Back => motion.forward -= 1.0,
                MoveKey::Left => motion.right -= 1.0,
                MoveKey::Right => motion.right += 1.0,
                MoveKey::Up => motion.vertical += 1.0,
                MoveKey::Down => motion.vertical -= 1.0,
                MoveKey::RotateLeft => motion.rotate += 1.0,
                MoveKey::RotateRight => motion.rotate -= 1.0,
                MoveKey::PanLeft => motion.pan_x -= 1.0,
                MoveKey::PanRight => motion.pan_x += 1.0,
                MoveKey::PanUp => motion.pan_y += 1.0,
                MoveKey::PanDown => motion.pan_y -= 1.0,
            }
        }
        motion
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum DragKind {
    Orbit,
    Pan,
}

#[derive(Clone, Copy, Debug)]
struct DragState {
    kind: DragKind,
    last: Vec2,
    start: Vec2,
    /// Whether the pointer has moved past [`DRAG_THRESHOLD_PX`] since the
    /// press, which tells a click from a drag.
    moved: bool,
}

/// The hull, LOD and module choice a reload loads under.
#[derive(Clone, Debug, PartialEq)]
pub struct HullSelection {
    pub lod: usize,
    pub hull: Option<String>,
    pub modules: HashMap<ComponentType, String>,
}

/// A camo decode to run off the caller's thread, then hand back to
/// [`ArmorScene::apply_camo`].
pub struct CamoJob {
    generation: u64,
    id: CamoSchemeId,
    cached: Option<Arc<SchemeTextures>>,
    armor: Arc<LoadedShipArmor>,
}

/// A finished [`CamoJob`].
pub struct CamoResult {
    generation: u64,
    id: CamoSchemeId,
    decode: CamoDecode,
}

impl CamoJob {
    /// Decodes and composites the scheme. Slow; run it off the UI thread.
    pub fn run(self) -> CamoResult {
        let decode = decode_active_camo(self.id, self.cached, &self.armor);
        CamoResult { generation: self.generation, id: self.id, decode }
    }
}

/// One armor viewport. See the module doc.
pub struct ArmorScene {
    viewport: Viewport3D,
    gpu: Option<Gpu>,
    /// The viewport's rectangle in the front end's logical coordinates.
    rect: Option<ViewRect>,
    model_bounds: Option<(Vec3, Vec3)>,
    drag: Option<DragState>,
    gizmo_hover: Option<(Axis, bool)>,
    gizmo_press_in_box: bool,
    held_keys: HashSet<MoveKey>,
    /// A ship shown before the device was ready, uploaded once it is.
    pending_armor: Option<Arc<LoadedShipArmor>>,
    current_armor: Option<Arc<LoadedShipArmor>>,
    /// Absent means visible; a value says whether the part is shown.
    part_visibility: HashMap<(String, String), bool>,
    armor_all_visible: bool,
    /// An entry means the plate is hidden.
    plate_visibility: HashMap<PlateKey, bool>,
    /// Absent means hidden; a value says whether the hull mesh is shown.
    hull_visibility: HashMap<String, bool>,
    hull_all_visible: bool,
    hull_mesh_ids: Vec<MeshId>,
    selected_hull: Option<String>,
    hull_lod: usize,
    selected_modules: HashMap<ComponentType, String>,
    /// Bumped by every reload and every new ship, so a reload that lands
    /// after another started is dropped.
    reload_generation: u64,
    selected_camo: Option<CamoSchemeId>,
    camo_texture_cache: CamoTextureCache,
    camo_generation: u64,
    active_camo_textures: CamoTextures,
    active_camo_uvs: HashMap<String, UvTransform>,
    undo_stack: VisibilityUndoStack,
    hovered: Option<HoverInfo>,
    hits: Vec<PreExtractedHit>,
    sim_verdicts: HashMap<ShotId, ComparisonVerdict>,
    sim_unchecked: usize,
    hit_health: Option<f32>,
    ring_hovers: Vec<camera_rings::RingHover>,
    hovered_ring: Option<RingHover>,
    hover_highlight: Option<(PlateKey, MeshId)>,
    sidebar_highlight: Option<(SidebarHighlightKey, MeshId)>,
    mesh_triangle_info: Vec<(MeshId, Vec<ArmorTriangleTooltip>)>,
    /// The zone and thickness of the last plate the pointer was over, kept
    /// after the hover ends so the penetration checker opens on it.
    last_plate: Option<(String, f32)>,
    display_settings: DisplaySettings,
    camera_rings: CameraRingSettings,
    perspective: PerspectiveSettings,
    camera_before_lock: Option<ArcballCamera>,
    aim_mesh_ids: Vec<MeshId>,
    show_gaps: bool,
    gap_count: usize,
    trajectory_mode: bool,
    trajectories: Vec<trajectory::Trajectory>,
    continue_on_ricochet: bool,
    cast_ship: Option<ComparisonShip>,
    cast_shell_index: Option<usize>,
    cast_range: Km,
    splash_mode: bool,
    splash_result: Option<splash::SplashResult>,
    show_splash_boxes: bool,
    show_hidden_only: bool,
    changes: Changes,
}

impl Default for ArmorScene {
    fn default() -> Self {
        Self::new()
    }
}

impl ArmorScene {
    pub fn new() -> Self {
        Self {
            viewport: Viewport3D::new(),
            gpu: None,
            rect: None,
            model_bounds: None,
            drag: None,
            gizmo_hover: None,
            gizmo_press_in_box: false,
            held_keys: HashSet::new(),
            pending_armor: None,
            current_armor: None,
            part_visibility: HashMap::new(),
            armor_all_visible: true,
            plate_visibility: HashMap::new(),
            hull_visibility: HashMap::new(),
            hull_all_visible: false,
            hull_mesh_ids: Vec::new(),
            selected_hull: None,
            hull_lod: DEFAULT_LOD,
            selected_modules: HashMap::new(),
            reload_generation: 0,
            selected_camo: None,
            camo_texture_cache: CamoTextureCache::default(),
            camo_generation: 0,
            active_camo_textures: HashMap::new(),
            active_camo_uvs: HashMap::new(),
            undo_stack: VisibilityUndoStack::default(),
            hovered: None,
            hits: Vec::new(),
            sim_verdicts: HashMap::new(),
            sim_unchecked: 0,
            hit_health: None,
            ring_hovers: Vec::new(),
            hovered_ring: None,
            hover_highlight: None,
            sidebar_highlight: None,
            mesh_triangle_info: Vec::new(),
            last_plate: None,
            display_settings: DisplaySettings::default(),
            camera_rings: CameraRingSettings::default(),
            perspective: PerspectiveSettings::default(),
            camera_before_lock: None,
            aim_mesh_ids: Vec::new(),
            show_gaps: false,
            gap_count: 0,
            trajectory_mode: false,
            trajectories: Vec::new(),
            continue_on_ricochet: false,
            cast_ship: None,
            cast_shell_index: None,
            cast_range: DEFAULT_CAST_RANGE,
            splash_mode: false,
            splash_result: None,
            show_splash_boxes: false,
            show_hidden_only: false,
            changes: Changes::default(),
        }
    }

    /// What changed since the last call.
    pub fn take_changes(&mut self) -> Changes {
        std::mem::take(&mut self.changes)
    }

    fn changed_view(&mut self) {
        self.changes.view = true;
    }

    fn changed_settings(&mut self) {
        self.changes.settings = true;
        self.changes.view = true;
    }

    fn changed_camera(&mut self) {
        self.changes.camera = true;
    }

    /// Adopts the shared device, uploading a ship shown before it was ready.
    pub fn set_gpu(&mut self, gpu: Gpu) {
        self.gpu = Some(gpu);
        if let Some(armor) = self.pending_armor.take() {
            self.upload_armor_now(armor);
        }
        self.viewport.mark_dirty();
        self.changed_view();
    }

    pub fn gpu_ready(&self) -> bool {
        self.gpu.is_some()
    }

    /// Places the viewport. Pointer positions are read in the same
    /// coordinates.
    pub fn set_rect(&mut self, rect: ViewRect) {
        let same = self.rect.is_some_and(|held| {
            held.left() == rect.left()
                && held.top() == rect.top()
                && held.width() == rect.width()
                && held.height() == rect.height()
        });
        if !same {
            self.rect = Some(rect);
            self.viewport.mark_dirty();
        }
    }

    pub fn rect(&self) -> Option<ViewRect> {
        self.rect
    }

    fn viewport_size(&self) -> (f32, f32) {
        self.rect.map(|rect| (rect.width(), rect.height())).unwrap_or((1.0, 1.0))
    }

    pub fn is_dirty(&self) -> bool {
        self.viewport.is_dirty()
    }

    pub fn mark_dirty(&mut self) {
        self.viewport.mark_dirty();
    }

    /// Renders at `size` device pixels when anything changed, returning the
    /// tightly packed sRGB RGBA8 readback.
    pub fn render(&mut self, size: (u32, u32)) -> Option<(u32, u32, Vec<u8>)> {
        let gpu = self.gpu.clone()?;
        if !self.viewport.is_dirty() {
            return None;
        }
        let size = (size.0.max(1), size.1.max(1));
        let frame = self.viewport.render_offscreen_rgba(&gpu.ctx.device, &gpu.ctx.queue, &gpu.pipeline, size)?;
        self.viewport.clear_dirty();
        Some(frame)
    }

    /// Seeds the display settings and the visibility a new ship opens with
    /// from the saved defaults.
    pub fn apply_defaults(&mut self, defaults: Option<&ArmorViewerDefaultsRow>) {
        self.display_settings = DisplaySettings::from_defaults(defaults);
        self.armor_all_visible = defaults.map(|d| d.armor_all_visible).unwrap_or(true);
        self.hull_all_visible = defaults.map(|d| d.hull_all_visible).unwrap_or(false);
        self.reupload_current_armor();
    }

    pub fn current_armor(&self) -> Option<&Arc<LoadedShipArmor>> {
        self.current_armor.as_ref()
    }

    pub fn has_armor(&self) -> bool {
        self.current_armor.is_some()
    }

    pub fn camera(&self) -> ArcballCamera {
        self.viewport.camera.clone()
    }

    /// Sets the camera without reporting a camera change, for a mirror.
    pub fn set_camera(&mut self, camera: ArcballCamera) {
        self.viewport.camera = camera;
        self.viewport.mark_dirty();
    }

    pub fn synced_settings(&self) -> SyncedSettings {
        SyncedSettings {
            part_visibility: self.part_visibility.clone(),
            plate_visibility: self.plate_visibility.clone(),
            hull_visibility: self.hull_visibility.clone(),
            display_settings: self.display_settings,
            lighting: self.viewport.lighting.clone(),
            camera_rings: self.camera_rings.clone(),
            perspective: self.perspective,
        }
    }

    /// Applies another pane's settings without reporting a settings change
    /// and without an undo entry, matching the egui app's
    /// `SyncedPaneSettings::apply_to`.
    pub fn apply_synced(&mut self, settings: &SyncedSettings) {
        self.part_visibility = settings.part_visibility.clone();
        self.plate_visibility = settings.plate_visibility.clone();
        self.hull_visibility = settings.hull_visibility.clone();
        self.display_settings = settings.display_settings;
        self.viewport.lighting = settings.lighting.clone();
        self.camera_rings = settings.camera_rings.clone();
        self.perspective.camera = settings.perspective.camera;
        self.set_perspective_enabled_inner(settings.perspective.enabled);
        self.reupload_current_armor();
        self.viewport.mark_dirty();
        self.changed_view();
    }

    /// Shows `armor`, resetting everything that belonged to the previous
    /// ship and framing the camera on this one. Waits for the device when it
    /// is not ready yet.
    pub fn show_armor(&mut self, armor: Arc<LoadedShipArmor>) {
        if self.gpu.is_some() {
            self.upload_armor_now(armor);
            self.pending_armor = None;
        } else {
            self.pending_armor = Some(armor);
        }
        self.changed_view();
    }

    /// Drops the ship shown, as a pane that is reset to empty.
    pub fn clear(&mut self) {
        *self = Self { gpu: self.gpu.take(), rect: self.rect, ..Self::new() };
        self.viewport.mark_dirty();
        self.changed_view();
    }

    fn upload_armor_now(&mut self, armor: Arc<LoadedShipArmor>) {
        let Some(gpu) = self.gpu.clone() else { return };
        self.reload_generation += 1;
        self.part_visibility.clear();
        self.plate_visibility.clear();
        self.hull_visibility.clear();
        self.hull_mesh_ids.clear();
        self.selected_hull = None;
        self.hull_lod = DEFAULT_LOD;
        self.selected_modules.clear();
        self.camera_rings.mode = first_camera_mode(&armor);
        self.selected_camo = None;
        self.camo_generation = self.camo_generation.wrapping_add(1);
        self.camo_texture_cache.clear();
        self.active_camo_textures.clear();
        self.active_camo_uvs.clear();
        self.undo_stack.clear();
        self.hovered = None;
        self.hover_highlight = None;
        self.hovered_ring = None;
        self.ring_hovers.clear();
        self.sidebar_highlight = None;
        self.trajectories.clear();
        self.splash_result = None;
        // Only the non-default outcome needs explicit entries: an absent part
        // is shown and an absent hull mesh is not.
        if !self.armor_all_visible {
            for (zone, parts) in &armor.zone_parts {
                for part in parts {
                    self.part_visibility.insert((zone.clone(), part.clone()), false);
                }
            }
        }
        if self.hull_all_visible {
            for (_group, names) in &armor.hull_part_groups {
                for name in names {
                    self.hull_visibility.insert(name.clone(), true);
                }
            }
        }
        let visibility = VisibilityFilter {
            part: &self.part_visibility,
            plate: &self.plate_visibility,
            show_hidden_only: self.show_hidden_only,
        };
        self.mesh_triangle_info = upload::upload_armor_to_viewport(
            &mut self.viewport,
            &gpu.ctx.device,
            &armor,
            visibility,
            self.display_settings,
        );
        upload_hull::upload_hull_meshes(
            &mut self.viewport,
            &gpu.ctx.device,
            &gpu.ctx.queue,
            &gpu.pipeline,
            &armor,
            &mut self.hull_mesh_ids,
            &self.hull_visibility,
            self.display_settings.hull_opaque,
            &self.active_camo_textures,
            &self.active_camo_uvs,
        );
        self.model_bounds = Some(armor.bounds);
        // The camera the lock puts back is the one framed on this ship. A ship
        // that names no orbit has nothing to lock onto, so the lock comes off.
        if self.perspective.enabled {
            if armor.camera_trajectories.is_empty() {
                self.perspective.enabled = false;
                self.camera_before_lock = None;
            } else {
                self.camera_before_lock = Some(self.viewport.camera.clone());
            }
        }
        self.current_armor = Some(armor);
        // Overlays carried over from the previous ship are drawn on this one.
        if self.has_overlays() {
            self.reupload_current_armor();
        }
    }

    /// Whether anything is drawn over the armor and hull.
    fn has_overlays(&self) -> bool {
        self.camera_rings.shown
            || self.perspective.enabled
            || !self.hits.is_empty()
            || self.show_gaps
            || self.show_splash_boxes
            || self.display_settings.show_ship_center
    }

    pub fn model_roll_deg(&self) -> f32 {
        self.viewport.model_roll.to_degrees()
    }

    /// Heels the hull over. Only the model turns; the waterline and the
    /// world-space overlays stay level.
    pub fn set_model_roll_deg(&mut self, degrees: f32) {
        let radians = degrees.clamp(-ROLL_LIMIT_DEG, ROLL_LIMIT_DEG).to_radians();
        if (self.viewport.model_roll - radians).abs() < f32::EPSILON {
            return;
        }
        self.viewport.model_roll = radians;
        self.viewport.mark_dirty();
        self.changed_view();
    }

    pub fn all_visible(&self) -> AllVisible {
        AllVisible::of(&self.part_visibility, &self.plate_visibility, &self.hull_visibility)
    }

    pub fn camera_rings(&self) -> &CameraRingSettings {
        &self.camera_rings
    }

    /// The camera modes this ship's own GameParams name, in their own order.
    pub fn camera_modes(&self) -> Vec<String> {
        self.current_armor
            .as_ref()
            .map(|armor| armor.camera_trajectories.iter().map(|(name, _)| name.clone()).collect())
            .unwrap_or_default()
    }

    pub fn set_camera_rings(&mut self, settings: CameraRingSettings) {
        if self.camera_rings == settings {
            return;
        }
        self.camera_rings = settings;
        self.reupload_current_armor();
        self.changed_settings();
    }

    pub fn perspective(&self) -> PerspectiveSettings {
        self.perspective
    }

    /// Locks the camera to the ship's own orbit, or lets it go again. The free
    /// camera is kept while locked and put back when unlocked.
    pub fn set_perspective_enabled(&mut self, on: bool) {
        if on == self.perspective.enabled {
            return;
        }
        self.set_perspective_enabled_inner(on);
        self.reupload_current_armor();
        self.changed_settings();
    }

    fn set_perspective_enabled_inner(&mut self, on: bool) {
        if on == self.perspective.enabled {
            return;
        }
        if on {
            self.camera_before_lock = Some(self.viewport.camera.clone());
            self.perspective.camera.yaw = self.viewport.camera.azimuth;
            self.perspective.camera.pitch = self.viewport.camera.elevation;
            self.perspective.camera.clamp();
        } else if let Some(camera) = self.camera_before_lock.take() {
            self.viewport.camera = camera;
        }
        self.perspective.enabled = on;
        // Held keys and a drag belong to the camera that was just replaced.
        self.held_keys.clear();
    }

    pub fn set_perspective_fov(&mut self, degrees: f32) {
        if (self.perspective.camera.fov_deg - degrees).abs() < f32::EPSILON {
            return;
        }
        self.perspective.camera.fov_deg = degrees;
        self.perspective.camera.clamp();
        self.reupload_current_armor();
        self.changed_settings();
    }

    pub fn set_perspective_look_mode(&mut self, mode: LookMode) {
        if self.perspective.camera.look_mode == mode {
            return;
        }
        self.perspective.camera.look_mode = mode;
        self.reupload_current_armor();
        self.changed_settings();
    }

    /// The trajectory the locked camera and the drawn orbits follow.
    fn current_trajectory(&self) -> Option<&CameraTrajectory> {
        let armor = self.current_armor.as_ref()?;
        let wanted = self.camera_rings.mode.as_ref();
        armor
            .camera_trajectories
            .iter()
            .find(|(name, _)| Some(name) == wanted)
            .or_else(|| armor.camera_trajectories.first())
            .map(|(_, trajectory)| trajectory)
    }

    /// Points the camera where the lock says and marks what it is aimed at.
    /// A no-op while the lock is off or the ship names no orbit.
    fn aim_perspective(&mut self, device: Option<&wgpu::Device>, bounds: (Vec3, Vec3)) {
        for id in self.aim_mesh_ids.drain(..) {
            self.viewport.remove_mesh(id);
        }
        if !self.perspective.enabled {
            return;
        }
        let Some(trajectory) = self.current_trajectory().cloned() else { return };

        // The port's armor carries no waterline offset.
        let waterline_dy = 0.0;
        let (fov, height) = (self.camera_rings.fov, self.camera_rings.height);
        self.perspective.camera.clamp_pitch_to_far_side(&trajectory, fov, height, waterline_dy);
        let (eye_model, look_model) = self.perspective.camera.eye_and_look_dir(&trajectory, fov, height, waterline_dy);
        let eye = self.viewport.pos_to_world_space(eye_model);
        let look = self.viewport.pos_to_world_space(look_model);
        let target = water_aim_point(eye, look, AIM_MARKER_REACH);

        let (min, max) = bounds;
        let diagonal = (max - min).norm();
        let camera = &mut self.viewport.camera;
        camera.set_eye_and_target(eye, target);
        camera.fov = self.perspective.camera.fov_deg.to_radians();
        // Near enough to stand on the ship, far enough to keep the water the
        // eye is aimed at in the frustum.
        camera.near = 0.05;
        camera.far = (diagonal * 8.0).max(10.0);

        let Some(device) = device else { return };
        let radius = (diagonal * AIM_MARKER_FRACTION).clamp(AIM_MARKER_MIN, AIM_MARKER_MAX);
        let (vertices, indices) = camera_rings::build_water_marker(target, radius, AIM_MARKER_COLOR);
        if !indices.is_empty() {
            let id = self.viewport.add_world_space_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            self.aim_mesh_ids.push(id);
        }
    }

    /// Re-aims the locked camera after the lock itself moved.
    fn refresh_perspective(&mut self) {
        let gpu = self.gpu.clone();
        let Some(bounds) = self.model_bounds else { return };
        self.aim_perspective(gpu.as_ref().map(|gpu| gpu.ctx.device.as_ref()), bounds);
        self.viewport.mark_dirty();
    }

    /// Whether the free camera is the reader's to move.
    fn camera_is_free(&self) -> bool {
        !self.perspective.enabled
    }

    fn steer_perspective(&mut self, dx: f32, dy: f32) -> bool {
        if !self.perspective.enabled || (dx == 0.0 && dy == 0.0) {
            return false;
        }
        self.perspective.camera.yaw -= dx * PERSPECTIVE_YAW_PER_PX;
        self.perspective.camera.pitch += dy * PERSPECTIVE_PITCH_PER_PX;
        self.perspective.camera.clamp();
        true
    }

    fn zoom_perspective(&mut self, notches: f32) -> bool {
        if !self.perspective.enabled || notches == 0.0 {
            return false;
        }
        self.perspective.camera.zoom -= notches * PERSPECTIVE_ZOOM_PER_NOTCH;
        self.perspective.camera.clamp();
        true
    }

    /// Draws what the ship had taken, replacing whatever was drawn before.
    pub fn show_hits(&mut self, hits: Vec<PreExtractedHit>) {
        self.hits = hits;
        self.reupload_current_armor();
        self.changed_view();
    }

    pub fn hits_drawn(&self) -> usize {
        self.hits.len()
    }

    /// What this app's own simulation made of a landed shell, against what the
    /// server said.
    pub fn sim_verdict(&self, shot: ShotId) -> Option<&ComparisonVerdict> {
        self.sim_verdicts.get(&shot)
    }

    /// How many armour-piercing shells the comparison could not answer for.
    pub fn sim_unchecked(&self) -> usize {
        self.sim_unchecked
    }

    /// Sends each landed shell through this hull and compares the result with
    /// what the server said the shell did.
    pub fn compare_incoming(&mut self, taken: &[PreExtractedHit], shells: &HashMap<GameParamId, ShellInfo>) {
        let had = !self.sim_verdicts.is_empty() || self.sim_unchecked > 0;
        self.sim_verdicts.clear();
        self.sim_unchecked = 0;
        if let Some(armor) = self.current_armor.clone() {
            let center = armor.center();
            for taken in taken {
                let hit = &taken.hit;
                let Some(shell) = hit.salvo.as_ref().and_then(|salvo| shells.get(&salvo.params_id)) else { continue };
                // The plate chain simulates AP only.
                if shell.ammo_type != AmmoType::AP {
                    continue;
                }
                match self.compare_one(hit, shell, center) {
                    Some(verdict) => {
                        self.sim_verdicts.insert(hit.hit.shot_id, verdict);
                    }
                    None => self.sim_unchecked += 1,
                }
            }
        }
        if had || !self.sim_verdicts.is_empty() || self.sim_unchecked > 0 {
            self.changed_view();
        }
    }

    /// One landed shell, cast back along the way it came.
    fn compare_one(&self, hit: &ResolvedShotHit, shell: &ShellInfo, center: Vec3) -> Option<ComparisonVerdict> {
        let arrival = wows_replay_insights::hull_impact::shell_arrival(hit)?;
        // Placed without the hull's bounds: a strike dragged to the hull's edge
        // would be simulated through plating the shell never met.
        let at = wows_replay_insights::hull_impact::hull_impact(hit, [center.x, center.y, center.z], None)?;

        let params = ShellParams::from_shell_info(shell)?;
        let impact = solve_for_range(&params, arrival.travelled)?;
        let fall = impact.impact_angle_horizontal;
        let (cos, sin) = (fall.cos(), fall.sin());
        let bearing = arrival.bearing;
        let shell_dir = Vec3::new(bearing.x * cos, -sin, bearing.z * cos).normalize();

        let through = Vec3::new(at[0], at[1], at[2]);
        let ray_hits = self.viewport.pick_all_ray_model_space(through - shell_dir * CAST_STANDOFF, shell_dir);
        let hits = trajectory::build_hits(&ray_hits, &self.mesh_triangle_info, &shell_dir);
        let first = hits.first()?;
        let strike = Strike { angle: first.angle_from_normal, thickness: first.thickness };
        let plates: Vec<ArcPlate> = hits
            .iter()
            .map(|hit| ArcPlate { zone: hit.zone.clone(), thickness: hit.thickness, material: hit.material })
            .collect();

        let sim = trajectory::simulate(&params, &impact, &hits, self.continue_on_ricochet);
        let server = ServerOutcome::from_shell_hit_type(&hit.hit.hit_type.shell_hit);
        compare_with_server(&sim, &plates, strike, &server, &params)
    }

    /// Says what the ship's health was at the moment being shown.
    pub fn set_hit_health(&mut self, health: Option<f32>) {
        if self.hit_health == health {
            return;
        }
        self.hit_health = health;
        self.changed_view();
    }

    pub fn hit_health(&self) -> Option<f32> {
        self.hit_health
    }

    /// The shot log beside the hull: what landed, newest first, bounded.
    pub fn shot_log(&self) -> Vec<wows_replay_insights::hull_impact::LoggedHit> {
        self.hits.iter().rev().take(SHOT_LOG_LINES).map(wows_replay_insights::hull_impact::log_line).collect()
    }

    pub fn gaps(&self) -> GapState {
        GapState { shown: self.show_gaps, count: self.gap_count }
    }

    pub fn set_show_gaps(&mut self, show: bool) {
        if self.show_gaps == show {
            return;
        }
        self.show_gaps = show;
        self.reupload_current_armor();
        self.changed_view();
    }

    pub fn trajectory_state(&self) -> TrajectoryState {
        TrajectoryState {
            shown: self.trajectory_mode,
            count: self.trajectories.len(),
            continue_on_ricochet: self.continue_on_ricochet,
        }
    }

    /// Adopts the attacker casts are made with, re-running what was cast.
    pub fn set_cast_ship(&mut self, ship: Option<ComparisonShip>, shell_index: Option<usize>) {
        if self.cast_ship.as_ref().map(|held| &held.param_index) == ship.as_ref().map(|next| &next.param_index)
            && self.cast_shell_index == shell_index
        {
            return;
        }
        self.cast_ship = ship;
        self.cast_shell_index = shell_index;
        self.recast_trajectories();
    }

    pub fn cast_range(&self) -> Km {
        self.cast_range
    }

    pub fn set_cast_range(&mut self, range: Km) {
        if (self.cast_range.value() - range.value()).abs() < f32::EPSILON {
            return;
        }
        self.cast_range = range;
        self.recast_trajectories();
    }

    /// Turns trajectory casting on or off. Leaving keeps what was cast;
    /// turning it on leaves splash mode, since both want the click.
    pub fn set_trajectory_mode(&mut self, on: bool) {
        if self.trajectory_mode == on {
            return;
        }
        self.trajectory_mode = on;
        if on {
            self.splash_mode = false;
        }
        self.changes.trajectory_mode = true;
        self.changed_view();
    }

    pub fn splash_state(&self) -> SplashState {
        SplashState {
            shown: self.splash_mode,
            boxes_shown: self.show_splash_boxes,
            has_data: self.current_armor.as_ref().is_some_and(|armor| armor.splash_data.is_some()),
            zones_reached: self.splash_result.as_ref().map(|result| result.hit_zones.len()).unwrap_or(0),
        }
    }

    /// Turns splash bursting on or off, leaving trajectory mode if it was on.
    pub fn set_splash_mode(&mut self, on: bool) {
        if self.splash_mode == on {
            return;
        }
        self.splash_mode = on;
        if on && self.trajectory_mode {
            self.trajectory_mode = false;
            self.changes.trajectory_mode = true;
        }
        self.changed_view();
    }

    pub fn set_show_splash_boxes(&mut self, show: bool) {
        if self.show_splash_boxes == show {
            return;
        }
        self.show_splash_boxes = show;
        self.reupload_current_armor();
        self.changed_view();
    }

    /// The zones the burst on screen reached.
    pub fn splash_zones(&self) -> &[splash::SplashZoneHit] {
        self.splash_result.as_ref().map(|result| result.hit_zones.as_slice()).unwrap_or(&[])
    }

    /// Bursts a shell where the pointer is; past the ship clears the burst.
    fn burst_at(&mut self, pointer: Vec2) {
        let Some(rect) = self.rect else { return };
        let Some(surface) = self.viewport.pick(pointer, rect) else {
            self.splash_result = None;
            self.reupload_current_armor();
            self.changed_view();
            return;
        };
        let Some(armor) = self.current_armor.clone() else { return };
        let Some(data) = armor.splash_data.as_ref() else { return };
        let point: [f32; 3] = surface.world_position.into();
        let reach = self.burst_reach();
        self.splash_result = Some(splash::compute_splash(point, reach, data, armor.hit_locations.as_ref()));
        self.reupload_current_armor();
        self.changed_view();
    }

    /// How far the chosen attacker's largest HE or SAP burst reaches.
    fn burst_reach(&self) -> splash::ModelUnit {
        let largest = self.cast_ship.as_ref().and_then(|ship| {
            ship.shells
                .iter()
                .filter(|shell| matches!(shell.ammo_type, AmmoType::HE | AmmoType::SAP))
                .map(|shell| shell.caliber)
                .max_by(|a, b| a.value().total_cmp(&b.value()))
        });
        match largest {
            Some(caliber) => splash::splash_half_extent(caliber),
            None => splash::ModelUnit::new(0.0),
        }
    }

    pub fn set_continue_on_ricochet(&mut self, on: bool) {
        if self.continue_on_ricochet == on {
            return;
        }
        self.continue_on_ricochet = on;
        self.recast_trajectories();
    }

    /// Drops every cast shell, giving back what their isolation hid.
    pub fn clear_trajectories(&mut self) {
        if self.trajectories.is_empty() {
            return;
        }
        let was_isolating = self.trajectories.iter().any(|cast| cast.isolating_plates || cast.isolating_zones);
        self.trajectories.clear();
        if was_isolating {
            self.apply_arc_isolation();
            return;
        }
        self.reupload_current_armor();
        self.changed_view();
    }

    pub fn show_hidden_only(&self) -> bool {
        self.show_hidden_only
    }

    /// Shows only the plates the game's own viewer hides, or everything again.
    pub fn set_show_hidden_only(&mut self, hidden_only: bool) {
        if self.show_hidden_only == hidden_only {
            return;
        }
        self.show_hidden_only = hidden_only;
        self.reupload_current_armor();
        self.changed_view();
    }

    /// The zone and thickness of the last plate the pointer was over.
    pub fn last_plate(&self) -> Option<(&str, f32)> {
        self.last_plate.as_ref().map(|(zone, mm)| (zone.as_str(), *mm))
    }

    pub fn hovered(&self) -> Option<&HoverInfo> {
        self.hovered.as_ref()
    }

    pub fn hovered_ring(&self) -> Option<&RingHover> {
        self.hovered_ring.as_ref()
    }

    /// Casts a shell at whatever the pointer is over, along the camera's
    /// flattened bearing. Past the ship clears what was cast.
    fn cast_at(&mut self, pointer: Vec2, keep_existing: bool) {
        let Some(rect) = self.rect else { return };
        let Some(surface) = self.viewport.pick(pointer, rect) else {
            if !keep_existing {
                self.clear_trajectories();
            }
            return;
        };
        let Some((_, ray)) = self.viewport.screen_to_ray(pointer, rect) else { return };
        let approach = trajectory::approach_xz(&ray);
        let shell_dir = self.shell_direction(approach);

        let origin = surface.world_position - shell_dir * CAST_STANDOFF;
        let ray_hits = self.viewport.pick_all_ray(origin, shell_dir);
        let hits = trajectory::build_hits(&ray_hits, &self.mesh_triangle_info, &shell_dir);
        if hits.is_empty() {
            if !keep_existing {
                self.clear_trajectories();
            }
            return;
        }
        let cast = self.build_cast_at(hits, shell_dir, self.cast_range);
        if !keep_existing {
            self.trajectories.clear();
        }
        self.trajectories.push(cast);
        self.reupload_current_armor();
        self.changed_view();
    }

    /// Every cast arc, as the analysis panel reads them.
    pub fn arcs(&self) -> Vec<ArcSummary> {
        self.trajectories
            .iter()
            .enumerate()
            .map(|(index, cast)| {
                let plates: Vec<ArcPlate> = cast
                    .hits
                    .iter()
                    .map(|hit| ArcPlate { zone: hit.zone.clone(), thickness: hit.thickness, material: hit.material })
                    .collect();
                ArcSummary {
                    color: TRAJECTORY_PALETTE[index % TRAJECTORY_PALETTE.len()],
                    hits: cast.hits.len(),
                    total_armor: Millimeters::from(cast.hits.iter().map(|hit| hit.thickness.value()).sum::<f32>()),
                    range: cast.range,
                    isolating_plates: cast.isolating_plates,
                    isolating_zones: cast.isolating_zones,
                    outcome: describe_arc(cast.sim.as_ref(), &plates),
                }
            })
            .collect()
    }

    pub fn remove_trajectory(&mut self, index: usize) {
        if index >= self.trajectories.len() {
            return;
        }
        let dropped = self.trajectories.remove(index);
        if dropped.isolating_plates || dropped.isolating_zones {
            self.apply_arc_isolation();
        } else {
            self.reupload_current_armor();
            self.changed_view();
        }
    }

    /// Fires one arc from a different range, leaving the others.
    pub fn set_trajectory_range(&mut self, index: usize, range: Km) {
        let Some(old) = self.trajectories.get(index) else { return };
        if (old.range.value() - range.value()).abs() < f32::EPSILON {
            return;
        }
        let (hits, shell_dir) = (old.hits.clone(), old.shell_dir);
        let (isolating_plates, isolating_zones) = (old.isolating_plates, old.isolating_zones);
        let mut cast = self.build_cast_at(hits, shell_dir, range);
        cast.isolating_plates = isolating_plates;
        cast.isolating_zones = isolating_zones;
        let isolating = isolating_plates || isolating_zones;
        self.trajectories[index] = cast;
        if isolating {
            self.apply_arc_isolation();
        } else {
            self.reupload_current_armor();
            self.changed_view();
        }
    }

    /// Hides what one arc did not cross, or stops hiding it. Plates and zones
    /// exclude each other.
    pub fn isolate_arc(&mut self, index: usize, what: Isolate, on: bool) {
        let Some(cast) = self.trajectories.get_mut(index) else { return };
        match what {
            Isolate::Plates => {
                cast.isolating_plates = on;
                cast.isolating_zones = false;
            }
            Isolate::Zones => {
                cast.isolating_zones = on;
                cast.isolating_plates = false;
            }
        }
        self.apply_arc_isolation();
    }

    pub fn isolate_all_arcs(&mut self, what: Isolate) {
        for cast in &mut self.trajectories {
            cast.isolating_plates = what == Isolate::Plates;
            cast.isolating_zones = what == Isolate::Zones;
        }
        self.apply_arc_isolation();
    }

    /// Rebuilds the visibility the arcs' isolation asks for, from scratch.
    fn apply_arc_isolation(&mut self) {
        let mut hit_zones: HashSet<String> = HashSet::new();
        let mut hit_plates: HashSet<PlateKeyParts> = HashSet::new();
        for cast in &self.trajectories {
            for hit in &cast.hits {
                if cast.isolating_zones {
                    hit_zones.insert(hit.zone.clone());
                }
                if cast.isolating_plates {
                    hit_plates.insert((
                        hit.zone.clone(),
                        hit.material.name().to_owned(),
                        (hit.thickness.value() * 10.0).round() as i32,
                    ));
                }
            }
        }

        self.undo_stack.push(self.current_visibility_snapshot());
        self.part_visibility.clear();
        self.plate_visibility.clear();
        if let Some(armor) = self.current_armor.clone() {
            let zones: Vec<ZoneShape<'_>> = armor
                .zone_part_plates
                .iter()
                .map(|zone| ZoneShape {
                    name: zone.name.as_str(),
                    parts: zone.parts.iter().map(|part| (part.name.as_str(), part.plates.as_slice())).collect(),
                })
                .collect();
            let isolation = isolate(&zones, &hit_zones, &hit_plates);
            self.part_visibility = isolation.parts;
            // The shared map says whether a plate is shown; this one says
            // whether it is hidden.
            self.plate_visibility = isolation
                .plates
                .into_iter()
                .map(|((zone, material_name, thickness_tenths), shown)| {
                    (PlateKey { zone, material_name, thickness_tenths }, !shown)
                })
                .collect();
        }
        self.reupload_current_armor();
        self.changed_settings();
    }

    /// The direction a shell travels given the bearing it comes in on: the
    /// solver's fall angle with a shell to fire, flat without one.
    fn shell_direction(&self, approach: Vec3) -> Vec3 {
        match self.cast_shell_at(self.cast_range) {
            Some((_, impact)) => {
                let fall = impact.impact_angle_horizontal.value().to_radians();
                Vec3::new(approach[0] * fall.cos(), -fall.sin(), approach[2] * fall.cos()).normalize()
            }
            None => approach,
        }
    }

    /// The shell a cast is made with: the chosen one, else the first AP, else
    /// the first the ship carries.
    fn cast_shell_at(&self, range: Km) -> Option<(ShellParams, ImpactResult)> {
        let ship = self.cast_ship.as_ref()?;
        let shell = self
            .cast_shell_index
            .and_then(|index| ship.shells.get(index))
            .or_else(|| ship.shells.iter().find(|shell| shell.ammo_type == AmmoType::AP))
            .or_else(|| ship.shells.first())?;
        let params = ShellParams::from_shell_info(shell)?;
        let impact = solve_for_range(&params, range.to_meters())?;
        Some((params, impact))
    }

    fn build_cast_at(&self, hits: Vec<trajectory::TrajectoryHit>, shell_dir: Vec3, range: Km) -> trajectory::Trajectory {
        let shell = self.cast_shell_at(range);
        let parts = shell.as_ref().map(|(params, impact)| (params, impact));
        trajectory::cast(hits, shell_dir, parts, self.model_extent(), self.continue_on_ricochet, range)
    }

    fn recast_trajectories(&mut self) {
        if self.trajectories.is_empty() {
            self.changed_view();
            return;
        }
        let recast: Vec<trajectory::Trajectory> = std::mem::take(&mut self.trajectories)
            .into_iter()
            .map(|old| {
                let mut cast = self.build_cast_at(old.hits, old.shell_dir, old.range);
                cast.isolating_plates = old.isolating_plates;
                cast.isolating_zones = old.isolating_zones;
                cast
            })
            .collect();
        self.trajectories = recast;
        self.reupload_current_armor();
        self.changed_view();
    }

    fn model_extent(&self) -> f32 {
        match self.model_bounds {
            Some((min, max)) => (max[0] - min[0]).max(max[2] - min[2]),
            None => DEFAULT_MODEL_EXTENT,
        }
    }

    /// Where the gizmo sits, or `None` while there is no ship for it to
    /// orient or the lock owns the camera.
    fn gizmo_rect(&self) -> Option<ViewRect> {
        self.current_armor.as_ref()?;
        self.camera_is_free().then_some(())?;
        self.rect.map(gizmo::gizmo_rect)
    }

    /// The gizmo as it should be drawn, in the same coordinates as the rect.
    pub fn gizmo(&self) -> Option<GizmoView> {
        let rect = self.gizmo_rect()?;
        let center = rect.center();
        let view = self.viewport.camera.view_matrix();
        let balls = gizmo::ball_draw_order(&view)
            .into_iter()
            .map(|(axis, positive, dir, _depth)| GizmoBall {
                axis,
                positive,
                tip: center + dir * gizmo::ARM_LEN,
                hovered: self.gizmo_hover == Some((axis, positive)),
            })
            .collect();
        Some(GizmoView { center, balls })
    }

    /// A pointer button went down at `at`. `clicks` counts a double press as 2.
    pub fn pointer_down(&mut self, at: Vec2, button: Button, clicks: u32) {
        if self.gpu.is_none() {
            return;
        }
        match button {
            Button::Primary => {
                let in_gizmo = self.gizmo_rect().is_some_and(|rect| rect.contains(at));
                self.gizmo_press_in_box = in_gizmo;
                if !in_gizmo && clicks >= 2 && !self.perspective.enabled {
                    if let Some((min, max)) = self.model_bounds {
                        self.viewport.camera.reset(min, max);
                        self.viewport.mark_dirty();
                        self.changed_camera();
                    }
                    self.drag = None;
                    return;
                }
                self.drag = Some(DragState { kind: DragKind::Orbit, last: at, start: at, moved: false });
            }
            Button::Middle => {
                self.gizmo_press_in_box = false;
                self.drag = Some(DragState { kind: DragKind::Pan, last: at, start: at, moved: false });
            }
            Button::Secondary => {}
        }
    }

    /// The pointer moved to `at`: drags the camera, or picks what is under it.
    pub fn pointer_moved(&mut self, at: Vec2) {
        if self.gpu.is_none() {
            return;
        }
        let hover = self.gizmo_rect().and_then(|rect| gizmo::hit_test(rect, &self.viewport.camera, at));
        if hover != self.gizmo_hover {
            self.gizmo_hover = hover;
            self.viewport.mark_dirty();
            self.changed_view();
        }

        if let Some(drag) = self.drag {
            let (dx, dy) = (at.x - drag.last.x, at.y - drag.last.y);
            if dx != 0.0 || dy != 0.0 {
                let size = self.viewport_size();
                // A locked camera is steered on its orbit instead.
                if self.perspective.enabled {
                    if self.steer_perspective(dx, dy) {
                        self.refresh_perspective();
                    }
                } else {
                    match drag.kind {
                        DragKind::Orbit => self.viewport.camera.orbit((dx, dy), size),
                        DragKind::Pan => self.viewport.camera.pan((dx, dy), size),
                    }
                }
                let moved = drag.moved || (at - drag.start).norm() > DRAG_THRESHOLD_PX;
                self.drag = Some(DragState { last: at, moved, ..drag });
                self.viewport.mark_dirty();
                self.changed_camera();
            }
        }

        // Picking runs only while not dragging and while the camera is free.
        let hover_changed = if self.drag.is_none() && self.camera_is_free() {
            let ring_changed = self.update_ring_hover(at);
            self.update_plate_hover(at) || ring_changed
        } else {
            let ring_changed = self.clear_ring_hover();
            self.clear_plate_hover() || ring_changed
        };
        if hover_changed {
            self.changed_view();
        }
    }

    /// The pointer left the viewport.
    pub fn pointer_left(&mut self) {
        let ring = self.clear_ring_hover();
        let plate = self.clear_plate_hover();
        let gizmo = self.gizmo_hover.take().is_some();
        if gizmo {
            self.viewport.mark_dirty();
        }
        if ring || plate || gizmo {
            self.changed_view();
        }
    }

    /// A pointer button came up at `at`. A plain left click snaps the gizmo,
    /// casts or bursts in their modes, or toggles the hovered plate.
    pub fn pointer_up(&mut self, at: Vec2, button: Button, shift: bool) {
        let drag = self.drag.take();
        if self.gpu.is_none() {
            self.gizmo_press_in_box = false;
            return;
        }
        let plain_click = drag.is_some_and(|drag| !drag.moved);
        if button == Button::Primary && self.gizmo_press_in_box && plain_click {
            if let Some(rect) = self.gizmo_rect()
                && let Some((axis, positive)) = gizmo::hit_test(rect, &self.viewport.camera, at)
            {
                self.snap_camera(axis, positive);
            }
        } else if button == Button::Primary && !self.gizmo_press_in_box && plain_click {
            if self.trajectory_mode {
                self.cast_at(at, shift);
            } else if self.splash_mode {
                self.burst_at(at);
            } else if self.camera_is_free()
                && let Some(key) = self.hovered.as_ref().map(|hover| hover.key.clone())
            {
                self.toggle_plate(key);
            }
        }
        self.gizmo_press_in_box = false;
    }

    /// The wheel turned by `dy` logical pixels (positive away from the reader,
    /// which zooms in).
    pub fn wheel(&mut self, dy: f32) {
        if self.gpu.is_none() || dy == 0.0 {
            return;
        }
        if self.perspective.enabled {
            if self.zoom_perspective(dy) {
                self.refresh_perspective();
            }
        } else {
            self.viewport.camera.zoom(dy);
        }
        self.viewport.mark_dirty();
        self.changed_camera();
    }

    /// A pinch scaled the view by `factor` since the last report; above 1
    /// brings the model closer.
    pub fn pinch(&mut self, factor: f32) {
        if self.gpu.is_none() || !factor.is_finite() || factor <= 0.0 || (factor - 1.0).abs() < f32::EPSILON {
            return;
        }
        if self.perspective.enabled {
            // The lock has no distance: a pinch moves between its orbits as
            // the wheel does, one notch per percent.
            if self.zoom_perspective((factor - 1.0) * 100.0) {
                self.refresh_perspective();
            }
        } else {
            let camera = &mut self.viewport.camera;
            camera.distance = (camera.distance / factor).clamp(camera.near * 0.1, camera.far * 0.5);
        }
        self.viewport.mark_dirty();
        self.changed_camera();
    }

    /// A key went down. `key` is a lowercase key name; `command` is Ctrl (Cmd
    /// on macOS). Returns what the pane around the viewport should do.
    pub fn key_down(&mut self, key: &str, command: bool, shift: bool) -> Option<PaneCommand> {
        if self.gpu.is_none() {
            return None;
        }
        if command {
            match key {
                "s" => return Some(PaneCommand::ToggleOptions),
                "t" => {
                    self.set_trajectory_mode(!self.trajectory_mode);
                    return None;
                }
                "z" if !shift => {
                    self.undo_visibility();
                    return None;
                }
                "r" | "z" => {
                    self.redo_visibility();
                    return None;
                }
                _ => {}
            }
        }
        if let Some(motion) = MoveKey::from_key(key, shift) {
            self.held_keys.insert(motion);
        }
        None
    }

    pub fn key_up(&mut self, key: &str) {
        for motion in MoveKey::released_by(key) {
            self.held_keys.remove(&motion);
        }
    }

    /// The viewport lost the keyboard: nothing is held any more.
    pub fn release_keys(&mut self) {
        self.held_keys.clear();
    }

    /// Whether [`Self::tick`] has motion to apply.
    pub fn wants_tick(&self) -> bool {
        (!self.held_keys.is_empty() && self.camera_is_free()) || self.viewport.camera.animation.is_some()
    }

    /// Applies `dt` of held-key motion and of the gizmo snap animation.
    /// Returns whether the camera moved.
    pub fn tick(&mut self, dt: Duration) -> bool {
        let mut moved = false;
        if self.viewport.camera.animation.is_some() {
            self.viewport.camera.update_animation(dt.as_secs_f32());
            moved = true;
        }
        if !self.held_keys.is_empty() && self.camera_is_free() {
            let motion = HeldMotion::from_keys(&self.held_keys);
            if motion.forward != 0.0 || motion.right != 0.0 {
                self.viewport.camera.wasd(motion.forward, motion.right);
            }
            if motion.vertical != 0.0 {
                self.viewport.camera.move_vertical(motion.vertical);
            }
            if motion.rotate != 0.0 {
                self.viewport.camera.rotate_horizontal(motion.rotate);
            }
            if motion.pan_x != 0.0 || motion.pan_y != 0.0 {
                let size = self.viewport_size();
                self.viewport.camera.pan_keys((motion.pan_x, motion.pan_y), dt.as_secs_f32(), size);
            }
            moved = true;
        }
        if moved {
            self.viewport.mark_dirty();
            self.changed_camera();
        }
        moved
    }

    fn snap_camera(&mut self, axis: Axis, positive: bool) {
        let (azimuth, elevation) = camera::ortho_view(axis, positive, self.viewport.camera.azimuth);
        self.viewport.camera.animate_to(azimuth, elevation, GIZMO_SNAP_DURATION_SECS);
        self.viewport.mark_dirty();
        self.changed_camera();
    }

    fn update_plate_hover(&mut self, at: Vec2) -> bool {
        let Some(rect) = self.rect else { return self.clear_plate_hover() };
        if self.current_armor.is_none() {
            return self.clear_plate_hover();
        }
        let hit = self.viewport.pick(at, rect);
        let found = hit
            .as_ref()
            .and_then(|hit| highlight::tooltip_for_hit(hit, &self.mesh_triangle_info))
            .map(|tooltip| (highlight::plate_key_of(tooltip), tooltip.clone()));
        let Some((key, tooltip)) = found else { return self.clear_plate_hover() };

        let key_changed = self.hovered.as_ref().is_none_or(|hover| hover.key != key);
        self.last_plate = Some((tooltip.zone.clone(), tooltip.thickness_mm));
        self.hovered = Some(HoverInfo { key: key.clone(), tooltip, cursor: at });
        if key_changed {
            self.rebuild_hover_highlight(&key);
        }
        true
    }

    fn update_ring_hover(&mut self, at: Vec2) -> bool {
        if self.ring_hovers.is_empty() {
            return self.clear_ring_hover();
        }
        let Some(rect) = self.rect else { return self.clear_ring_hover() };
        let camera = &self.viewport.camera;
        let viewport = &self.viewport;
        let found = camera_rings::nearest_ring_label(
            &self.ring_hovers,
            |point| camera.project_to_screen(viewport.pos_to_world_space(point), rect).map(|at| [at.x, at.y]),
            [at.x, at.y],
        );
        let changed = found != self.hovered_ring.as_ref().map(|ring| ring.label.as_str());
        let moved = self.hovered_ring.as_ref().is_some_and(|ring| ring.cursor != at);
        self.hovered_ring = found.map(|label| RingHover { label: label.to_owned(), cursor: at });
        changed || moved
    }

    fn clear_ring_hover(&mut self) -> bool {
        self.hovered_ring.take().is_some()
    }

    fn clear_plate_hover(&mut self) -> bool {
        let had_hover = self.hovered.take().is_some();
        if let Some((_, old_id)) = self.hover_highlight.take() {
            self.viewport.remove_mesh(old_id);
            self.viewport.mark_dirty();
        }
        had_hover
    }

    fn rebuild_hover_highlight(&mut self, key: &PlateKey) {
        if let Some((_, old_id)) = self.hover_highlight.take() {
            self.viewport.remove_mesh(old_id);
        }
        let Some(gpu) = self.gpu.clone() else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let visibility = VisibilityFilter {
            part: &self.part_visibility,
            plate: &self.plate_visibility,
            show_hidden_only: self.show_hidden_only,
        };
        let mesh_id = highlight::upload_plate_highlight(
            &mut self.viewport,
            &gpu.ctx.device,
            &armor,
            key,
            visibility,
            self.display_settings.show_zero_mm,
        );
        self.hover_highlight = Some((key.clone(), mesh_id));
        self.viewport.mark_dirty();
    }

    fn rebuild_sidebar_highlight(&mut self, key: SidebarHighlightKey) {
        if let Some((_, old_id)) = self.sidebar_highlight.take() {
            self.viewport.remove_mesh(old_id);
        }
        let Some(gpu) = self.gpu.clone() else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let visibility = VisibilityFilter {
            part: &self.part_visibility,
            plate: &self.plate_visibility,
            show_hidden_only: self.show_hidden_only,
        };
        let show_zero_mm = self.display_settings.show_zero_mm;
        let device = &gpu.ctx.device;
        let mesh_id = match &key {
            SidebarHighlightKey::Zone(zone) => {
                highlight::upload_zone_highlight(&mut self.viewport, device, &armor, zone, visibility, show_zero_mm)
            }
            SidebarHighlightKey::Part(zone, part) => highlight::upload_part_highlight(
                &mut self.viewport,
                device,
                &armor,
                zone,
                part,
                visibility,
                show_zero_mm,
            ),
            SidebarHighlightKey::Plate(plate) => {
                highlight::upload_plate_highlight(&mut self.viewport, device, &armor, plate, visibility, show_zero_mm)
            }
        };
        self.sidebar_highlight = Some((key, mesh_id));
        self.viewport.mark_dirty();
    }

    /// Highlights the hull triangles a hovered visibility-tree row stands for.
    pub fn set_sidebar_hover(&mut self, key: SidebarHighlightKey) {
        if self.sidebar_highlight.as_ref().map(|(held, _)| held) == Some(&key) {
            return;
        }
        self.rebuild_sidebar_highlight(key);
    }

    pub fn clear_sidebar_hover(&mut self) {
        if let Some((_, old_id)) = self.sidebar_highlight.take() {
            self.viewport.remove_mesh(old_id);
            self.viewport.mark_dirty();
        }
    }

    /// Clears the row highlight only if it is still `key`'s: a row's leave can
    /// arrive after the next row's enter.
    pub fn clear_sidebar_hover_if(&mut self, key: &SidebarHighlightKey) {
        if self.sidebar_highlight.as_ref().map(|(held, _)| held) == Some(key) {
            self.clear_sidebar_hover();
        }
    }

    pub fn part_visibility(&self) -> &HashMap<(String, String), bool> {
        &self.part_visibility
    }

    pub fn plate_visibility(&self) -> &HashMap<PlateKey, bool> {
        &self.plate_visibility
    }

    pub fn hull_visibility(&self) -> &HashMap<String, bool> {
        &self.hull_visibility
    }

    /// The seam every armor visibility change goes through: snapshots for
    /// undo, applies, re-uploads, and reports a settings change.
    fn snapshot_and_mutate(&mut self, mutate: impl FnOnce(&mut Self)) {
        self.undo_stack.push(self.current_visibility_snapshot());
        mutate(self);
        self.reupload_current_armor();
        self.changed_settings();
    }

    pub fn toggle_plate(&mut self, key: PlateKey) {
        self.snapshot_and_mutate(|this| {
            let hidden = this.plate_visibility.get(&key).copied().unwrap_or(false);
            if hidden {
                this.plate_visibility.remove(&key);
            } else {
                this.plate_visibility.insert(key, true);
            }
        });
    }

    /// "Show all hidden plates".
    pub fn show_all_plates(&mut self) {
        if self.plate_visibility.is_empty() {
            return;
        }
        self.snapshot_and_mutate(|this| this.plate_visibility.clear());
    }

    /// "Disable {material}": turns that part off.
    pub fn disable_material(&mut self, zone: &str, material: &str) {
        let key = (zone.to_owned(), material.to_owned());
        self.snapshot_and_mutate(|this| {
            this.part_visibility.insert(key, false);
        });
    }

    /// Every part on, plate overrides cleared.
    pub fn set_all_parts_visible(&mut self) {
        let Some(armor) = self.current_armor.clone() else { return };
        self.snapshot_and_mutate(|this| {
            for (zone, parts) in &armor.zone_parts {
                for part in parts {
                    this.part_visibility.insert((zone.clone(), part.clone()), true);
                }
            }
            this.plate_visibility.clear();
        });
    }

    /// Every part off.
    pub fn set_all_parts_hidden(&mut self) {
        let Some(armor) = self.current_armor.clone() else { return };
        self.snapshot_and_mutate(|this| {
            for (zone, parts) in &armor.zone_parts {
                for part in parts {
                    this.part_visibility.insert((zone.clone(), part.clone()), false);
                }
            }
        });
    }

    /// Clears plate-level overrides only.
    pub fn reset_plate_overrides(&mut self) {
        if self.plate_visibility.is_empty() {
            return;
        }
        self.snapshot_and_mutate(|this| this.plate_visibility.clear());
    }

    /// Sets one part, clearing that part's plate overrides whatever the new
    /// state is, as the egui part rows do.
    pub fn toggle_part(&mut self, zone: String, part: String, plate_thicknesses: Vec<i32>, checked: bool) {
        self.snapshot_and_mutate(move |this| {
            visibility::clear_plate_overrides(&mut this.plate_visibility, &zone, &part, &plate_thicknesses);
            this.part_visibility.insert((zone, part), checked);
        });
    }

    /// Sets every part of a zone, or with `solo` shows only that zone and
    /// clears every plate override.
    pub fn toggle_zone(&mut self, zone: String, checked: bool, solo: bool) {
        let Some(armor) = self.current_armor.clone() else { return };
        self.snapshot_and_mutate(move |this| {
            if solo {
                for z in &armor.zone_part_plates {
                    let on = z.name == zone;
                    for p in &z.parts {
                        this.part_visibility.insert((z.name.clone(), p.name.clone()), on);
                        for &t in &p.plates {
                            this.plate_visibility.remove(&PlateKey {
                                zone: z.name.clone(),
                                material_name: p.name.clone(),
                                thickness_tenths: t,
                            });
                        }
                    }
                }
                return;
            }
            let Some(z) = armor.zone_part_plates.iter().find(|z| z.name == zone) else { return };
            for p in &z.parts {
                this.part_visibility.insert((zone.clone(), p.name.clone()), checked);
                if checked {
                    for &t in &p.plates {
                        this.plate_visibility.remove(&PlateKey {
                            zone: zone.clone(),
                            material_name: p.name.clone(),
                            thickness_tenths: t,
                        });
                    }
                }
            }
        });
    }

    fn current_visibility_snapshot(&self) -> VisibilitySnapshot {
        VisibilitySnapshot {
            part_visibility: self.part_visibility.clone(),
            plate_visibility: self.plate_visibility.clone(),
        }
    }

    pub fn undo_visibility(&mut self) {
        let current = self.current_visibility_snapshot();
        let Some(prev) = self.undo_stack.undo(current) else { return };
        self.part_visibility = prev.part_visibility;
        self.plate_visibility = prev.plate_visibility;
        self.reupload_current_armor();
        self.changed_settings();
    }

    pub fn redo_visibility(&mut self) {
        let current = self.current_visibility_snapshot();
        let Some(next) = self.undo_stack.redo(current) else { return };
        self.part_visibility = next.part_visibility;
        self.plate_visibility = next.plate_visibility;
        self.reupload_current_armor();
        self.changed_settings();
    }

    /// Re-uploads the armor and every overlay against the current visibility
    /// and settings, without moving the camera.
    fn reupload_current_armor(&mut self) {
        let Some(gpu) = self.gpu.clone() else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        let device = gpu.ctx.device.as_ref();
        let visibility = VisibilityFilter {
            part: &self.part_visibility,
            plate: &self.plate_visibility,
            show_hidden_only: self.show_hidden_only,
        };
        self.mesh_triangle_info =
            upload::reupload_armor_plates(&mut self.viewport, device, &armor, visibility, self.display_settings);
        // The armor pass cleared every mesh, the hull included.
        upload_hull::upload_hull_meshes(
            &mut self.viewport,
            device,
            &gpu.ctx.queue,
            &gpu.pipeline,
            &armor,
            &mut self.hull_mesh_ids,
            &self.hull_visibility,
            self.display_settings.hull_opaque,
            &self.active_camo_textures,
            &self.active_camo_uvs,
        );
        self.aim_mesh_ids.clear();
        self.ring_hovers.clear();
        if self.camera_rings.shown
            && let Some((mode, trajectory)) =
                armor.camera_trajectories.iter().find(|(name, _)| Some(name) == self.camera_rings.mode.as_ref())
        {
            let request = camera_rings::RingRequest {
                mode,
                trajectory,
                fov: self.camera_rings.fov,
                height: self.camera_rings.height,
                zoom_path: self.camera_rings.zoom_path,
                zoom_path_at_fov: self.camera_rings.zoom_path_at_fov,
                zoom_path_at_max_fov: self.camera_rings.zoom_path_at_max_fov,
                waterline_dy: 0.0,
            };
            let (vertices, indices, hovers) = camera_rings::build_camera_rings(&request);
            if !indices.is_empty() {
                self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            }
            self.ring_hovers = hovers;
        }

        if self.display_settings.show_ship_center {
            let (vertices, indices) = hits::build_center_marker(armor.bounds);
            self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
        }

        if !self.hits.is_empty() {
            let (vertices, indices) =
                hits::build_markers(&self.hits, armor.center(), Some(armor.bounds), self.display_settings.marker_opacity);
            if !indices.is_empty() {
                self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            }
        }

        // Scaled to the camera so a marker stays readable as the view pulls back.
        let scale = (self.viewport.camera.distance / CAST_SCALE_REFERENCE).clamp(0.15, 3.0);
        for (index, cast) in self.trajectories.iter().enumerate() {
            let color = TRAJECTORY_PALETTE[index % TRAJECTORY_PALETTE.len()];
            let (vertices, indices) = trajectory::build_mesh(cast, color, scale);
            if !indices.is_empty() {
                self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            }
        }

        if self.show_splash_boxes
            && let Some(data) = armor.splash_data.as_ref()
        {
            let shapes: Vec<&wowsunpack::models::geometry::SplashBox> = data.boxes.iter().collect();
            let (vertices, indices, _labels) = splash::build_splash_box_wireframes(&shapes);
            if !indices.is_empty() {
                self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            }
        }

        if let Some(result) = self.splash_result.as_ref() {
            let (vertices, indices) =
                splash::build_splash_cube_mesh(result.impact_point, result.half_extent, splash::CUBE_COLOR);
            if !indices.is_empty() {
                self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            }
        }

        // Walked over the triangles just uploaded, so a hidden plate's rim is
        // not reported as a hole.
        self.gap_count = 0;
        if self.show_gaps {
            let (vertices, indices, count) = gaps::build_gap_mesh(&armor, visibility, self.display_settings.show_zero_mm);
            self.gap_count = count;
            if !indices.is_empty() {
                self.viewport.add_non_pickable_mesh(device, &vertices, &indices, LAYER_OVERLAY);
            }
        }
        self.aim_perspective(Some(device), armor.bounds);

        // The clear dropped the highlight meshes; rebuild them on the fresh
        // geometry, or drop a hover whose plate just went away.
        self.hover_highlight = None;
        if let Some(hover) = self.hovered.clone() {
            let still_visible = self
                .mesh_triangle_info
                .iter()
                .flat_map(|(_, tooltips)| tooltips)
                .any(|tooltip| highlight::plate_key_of(tooltip) == hover.key);
            if still_visible {
                self.rebuild_hover_highlight(&hover.key);
            } else {
                self.hovered = None;
            }
        }
        if let Some((key, _)) = self.sidebar_highlight.take() {
            self.rebuild_sidebar_highlight(key);
        }
        self.viewport.mark_dirty();
        self.changed_view();
    }

    pub fn display_settings(&self) -> DisplaySettings {
        self.display_settings
    }

    /// The seam every display setting goes through; they change geometry, so
    /// the armor is re-uploaded.
    pub fn mutate_display_settings(&mut self, mutate: impl FnOnce(&mut DisplaySettings)) {
        let before = self.display_settings;
        mutate(&mut self.display_settings);
        if before == self.display_settings {
            return;
        }
        self.reupload_current_armor();
        self.changed_settings();
    }

    /// Re-uploads only the hull meshes.
    fn reupload_hull(&mut self) {
        let Some(gpu) = self.gpu.clone() else { return };
        let Some(armor) = self.current_armor.clone() else { return };
        upload_hull::upload_hull_meshes(
            &mut self.viewport,
            &gpu.ctx.device,
            &gpu.ctx.queue,
            &gpu.pipeline,
            &armor,
            &mut self.hull_mesh_ids,
            &self.hull_visibility,
            self.display_settings.hull_opaque,
            &self.active_camo_textures,
            &self.active_camo_uvs,
        );
    }

    /// The seam every hull visibility change goes through; only the hull is
    /// re-uploaded.
    pub fn mutate_hull_visibility(&mut self, mutate: impl FnOnce(&mut HashMap<String, bool>)) {
        mutate(&mut self.hull_visibility);
        self.reupload_hull();
        self.changed_settings();
    }

    pub fn set_hull_opaque(&mut self, opaque: bool) {
        if self.display_settings.hull_opaque == opaque {
            return;
        }
        self.display_settings.hull_opaque = opaque;
        self.reupload_hull();
        self.changed_settings();
    }

    pub fn selected_camo(&self) -> Option<CamoSchemeId> {
        self.selected_camo
    }

    /// Selects a camo scheme, or stock with `None`. Stock applies at once; a
    /// scheme is decoded by the returned job, which the caller runs off its
    /// UI thread and hands to [`Self::apply_camo`].
    pub fn select_camo(&mut self, id: Option<CamoSchemeId>) -> Option<CamoJob> {
        if self.selected_camo == id {
            return None;
        }
        self.selected_camo = id;
        self.camo_generation = self.camo_generation.wrapping_add(1);
        self.changed_view();
        let Some((id, armor)) = id.zip(self.current_armor.clone()) else {
            self.active_camo_textures.clear();
            self.active_camo_uvs.clear();
            self.reupload_hull();
            self.changed_settings();
            return None;
        };
        let cached = self.camo_texture_cache.get(id);
        Some(CamoJob { generation: self.camo_generation, id, cached, armor })
    }

    /// Takes a finished camo decode, unless the selection moved on while it ran.
    pub fn apply_camo(&mut self, result: CamoResult) {
        if result.generation != self.camo_generation {
            return;
        }
        if let Some(textures) = result.decode.decoded {
            self.camo_texture_cache.insert(result.id, textures);
        }
        self.active_camo_textures = result.decode.textures;
        self.active_camo_uvs = result.decode.uvs;
        self.reupload_hull();
        self.changed_settings();
    }

    pub fn hull_selection(&self) -> HullSelection {
        HullSelection { lod: self.hull_lod, hull: self.selected_hull.clone(), modules: self.selected_modules.clone() }
    }

    /// Picks a hull upgrade, clearing the module choices. Returns whether the
    /// ship has to be reloaded.
    #[must_use]
    pub fn select_hull_upgrade(&mut self, key: String) -> bool {
        if self.selected_hull.as_ref() == Some(&key) {
            return false;
        }
        self.selected_hull = Some(key);
        self.selected_modules.clear();
        self.changed_view();
        true
    }

    /// Picks a hull LOD. Returns whether the ship has to be reloaded.
    #[must_use]
    pub fn select_hull_lod(&mut self, lod: usize) -> bool {
        if self.hull_lod == lod {
            return false;
        }
        self.hull_lod = lod;
        self.changed_view();
        true
    }

    /// Picks a module alternative. Returns whether the ship has to be
    /// reloaded.
    #[must_use]
    pub fn select_module_alternative(&mut self, component: ComponentType, name: String) -> bool {
        if self.selected_modules.get(&component) == Some(&name) {
            return false;
        }
        self.selected_modules.insert(component, name);
        self.changed_view();
        true
    }

    /// Starts a reload: the returned generation goes back to
    /// [`Self::apply_reload`], which drops a reload superseded by a later one.
    pub fn begin_reload(&mut self) -> u64 {
        self.reload_generation += 1;
        self.reload_generation
    }

    /// Takes a reloaded ship without moving the camera: keeps the visibility
    /// entries that still apply, remaps the camo by name, clears undo and
    /// hover.
    pub fn apply_reload(&mut self, generation: u64, armor: Arc<LoadedShipArmor>) {
        if generation != self.reload_generation || self.gpu.is_none() {
            return;
        }
        if let Some(old_id) = self.selected_camo {
            let old_infos = self.current_armor.as_ref().map(|a| a.camo_scheme_infos.as_slice()).unwrap_or(&[]);
            self.selected_camo = remap_camo_selection(old_infos, &armor.camo_scheme_infos, old_id);
            if self.selected_camo.is_none() {
                tracing::warn!("armor viewer: camo scheme {old_id:?} did not survive the reload; reverting to stock");
            }
        }
        // Keyed by the pre-reload ids, which no longer index the new list.
        self.camo_texture_cache.clear();
        self.camo_generation = self.camo_generation.wrapping_add(1);
        if !armor.camera_trajectories.iter().any(|(name, _)| Some(name) == self.camera_rings.mode.as_ref()) {
            self.camera_rings.mode = first_camera_mode(&armor);
        }
        visibility::retain_hull_visibility(&mut self.hull_visibility, &armor.hull_part_groups);
        visibility::retain_part_visibility(&mut self.part_visibility, &armor.zone_parts);
        visibility::retain_plate_visibility(&mut self.plate_visibility, &armor.zone_part_plates);
        self.undo_stack.clear();
        self.hovered = None;
        self.hover_highlight = None;
        self.hovered_ring = None;
        self.ring_hovers.clear();
        self.sidebar_highlight = None;

        let (textures, uvs) = recompute_active_camo(self.selected_camo, &mut self.camo_texture_cache, &armor);
        self.active_camo_textures = textures;
        self.active_camo_uvs = uvs;
        self.model_bounds = Some(armor.bounds);
        self.current_armor = Some(armor);
        self.reupload_current_armor();
    }

    pub fn lighting(&self) -> &LightingSettings {
        &self.viewport.lighting
    }

    /// Lighting is a per-frame uniform, so no mesh is re-uploaded.
    pub fn mutate_lighting(&mut self, mutate: impl FnOnce(&mut LightingSettings)) {
        mutate(&mut self.viewport.lighting);
        self.viewport.mark_dirty();
        self.changed_settings();
    }

    /// Applies a lighting preset, keeping whether lighting is on.
    pub fn set_lighting_preset(&mut self, mut preset: LightingSettings) {
        preset.enabled = self.viewport.lighting.enabled;
        self.viewport.lighting = preset;
        self.viewport.mark_dirty();
        self.changed_settings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_arrows_pan_instead_of_orbit_and_rise() {
        let held: Vec<MoveKey> = ["left", "up"].iter().filter_map(|key| MoveKey::from_key(key, true)).collect();
        assert_eq!(HeldMotion::from_keys(&held), HeldMotion { pan_x: -1.0, pan_y: 1.0, ..HeldMotion::default() });

        let held: Vec<MoveKey> = ["left", "up"].iter().filter_map(|key| MoveKey::from_key(key, false)).collect();
        assert_eq!(HeldMotion::from_keys(&held), HeldMotion { rotate: 1.0, vertical: 1.0, ..HeldMotion::default() });
    }

    /// Shift released before the arrow must not leave the pan running.
    #[test]
    fn releasing_an_arrow_stops_its_pan_whatever_shift_is() {
        let pan = MoveKey::from_key("right", true).expect("shift+right is a movement key");
        assert!(MoveKey::released_by("right").any(|key| key == pan));
    }

    fn hull(entries: &[(&str, bool)]) -> HashMap<String, bool> {
        entries.iter().map(|(name, on)| ((*name).to_owned(), *on)).collect()
    }

    #[test]
    fn nothing_hidden_reads_as_everything_shown() {
        let all = AllVisible::of(&HashMap::new(), &HashMap::new(), &hull(&[("Hull", true)]));
        assert_eq!(all, AllVisible { hull: true, armor: true });
    }

    #[test]
    fn a_hull_with_no_entries_is_not_shown() {
        assert!(!AllVisible::of(&HashMap::new(), &HashMap::new(), &HashMap::new()).hull);
    }

    #[test]
    fn one_hidden_part_or_plate_is_enough_to_say_not_everything() {
        let mut part = HashMap::new();
        part.insert(("Citadel".to_owned(), "Belt".to_owned()), false);
        assert!(!AllVisible::of(&part, &HashMap::new(), &HashMap::new()).armor);

        let mut plate = HashMap::new();
        plate.insert(PlateKey { zone: "Bow".to_owned(), material_name: "Bow_Bottom".to_owned(), thickness_tenths: 160 }, true);
        assert!(!AllVisible::of(&HashMap::new(), &plate, &HashMap::new()).armor);
    }

    #[test]
    fn input_without_a_device_changes_nothing() {
        let mut scene = ArmorScene::new();
        scene.set_rect(ViewRect::new(0.0, 0.0, 200.0, 100.0));
        scene.pointer_down(Vec2::new(10.0, 10.0), Button::Primary, 1);
        scene.pointer_moved(Vec2::new(60.0, 10.0));
        scene.wheel(40.0);
        assert_eq!(scene.take_changes(), Changes::default());
    }

    #[test]
    fn trajectory_and_splash_modes_exclude_each_other() {
        let mut scene = ArmorScene::new();
        scene.set_splash_mode(true);
        scene.set_trajectory_mode(true);
        assert!(!scene.splash_state().shown);
        assert!(scene.take_changes().trajectory_mode);
        scene.set_splash_mode(true);
        assert!(!scene.trajectory_state().shown);
        assert!(scene.take_changes().trajectory_mode);
    }

    #[test]
    fn hull_choices_say_when_a_reload_is_due() {
        let mut scene = ArmorScene::new();
        assert!(scene.select_hull_lod(2));
        assert!(!scene.select_hull_lod(2));
        assert!(scene.select_module_alternative(ComponentType::Artillery, "AB1".to_owned()));
        assert!(scene.select_hull_upgrade("B".to_owned()));
        // A hull upgrade clears the module choices made under the old hull.
        assert!(scene.hull_selection().modules.is_empty());
        let first = scene.begin_reload();
        let second = scene.begin_reload();
        assert!(second > first);
    }

    #[test]
    fn roll_is_held_to_its_limit() {
        let mut scene = ArmorScene::new();
        scene.set_model_roll_deg(40.0);
        assert!((scene.model_roll_deg() - ROLL_LIMIT_DEG).abs() < 1e-4);
    }
}

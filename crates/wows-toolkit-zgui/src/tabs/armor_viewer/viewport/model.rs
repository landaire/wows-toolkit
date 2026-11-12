//! The viewport's state: the scene on the device, the camera, what the reader has hidden, hovered
//! and cast, and the loads in flight. Its methods are grouped by concern in the sibling modules.

use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;
use std::rc::Weak;
use std::sync::Arc;
use std::time::Instant;

use wows_toolkit_armor::camera_rings;
use wows_toolkit_armor::hardpoint_overlay;
use wows_toolkit_armor::hull_picking::HullMeshIndex;
use wows_toolkit_armor::hull_picking::MeshTooltip;
use wows_toolkit_armor::length_marks::MarkLabels;
use wows_toolkit_armor::load_model::SceneProblem;
use wows_toolkit_armor::load_ship;
use wows_toolkit_armor::load_ship::ArmorTriangleTooltip;
use wows_toolkit_armor::load_ship::LoadedShipArmor;
use wows_toolkit_armor::load_ship::PlateKey;
use wows_toolkit_armor::model_refs::HardpointTarget;
use wows_toolkit_armor::scene_load::LoadSlot;
use wows_toolkit_armor::scene_load::SharedLoad;
use wows_toolkit_armor::splash;
use wows_toolkit_armor::trajectory;
use wows_toolkit_armor::trajectory::ArcColor;
use wows_toolkit_armor::trajectory::ArcId;
use wows_toolkit_armor::upload;
use wows_toolkit_armor::upload_hull::HullTextureCache;
use wows_toolkit_armor::viewport::camera::ArcballCamera;
use wows_toolkit_armor::viewport::camera::Axis;
use wows_toolkit_armor::viewport::device::ViewportDevice;
use wows_toolkit_armor::viewport::renderer::Viewport3D;
use wows_toolkit_armor::viewport::types::MeshId;
use wows_toolkit_armor::viewport::types::Vec2;
use wows_toolkit_armor::viewport::types::Vec3;
use wows_toolkit_armor::viewport::types::ViewRect;
use wows_toolkit_armor::visibility::SidebarHighlightKey;
use wows_toolkit_armor::visibility::VisibilityUndoStack;
use wowsunpack::export::camo_textures::CamoSchemeId;
use wowsunpack::export::camo_textures::SchemeTextures;
use wowsunpack::export::camouflage::UvTransform;
use wowsunpack::export::ship::ShipAssets;
use wowsunpack::game_params::keys::ComponentType;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_params::types::ShellInfo;
use wows_replays::types::GameParamId;
use wows_replays::types::ShotId;
use wows_replay_insights::timeline::PreExtractedHit;

use super::types::CameraRingSettings;
use super::types::CastShell;
use super::types::LoadGeneration;
use super::types::MeshSeed;
use super::types::OpeningLayer;
use super::types::PerspectiveSettings;
use super::types::SceneSource;
use super::types::SimAgreement;

/// The device generation zgui reports for the device a viewport draws on.
pub(super) type SceneDevice = ViewportDevice<u64>;

/// The range a cast shell is fired from until one is chosen.
pub(super) const DEFAULT_CAST_RANGE: Km = Km::new(10.0);

/// Whether this viewport has a device to draw on.
pub(super) enum GpuState {
    /// The pipeline is compiling, or the device is lost and not yet replaced.
    Waiting,
    /// Weak: the device registry holds the device, and drops it, with the mesh buffers made on
    /// it, as soon as zgui retires it.
    Ready(Weak<SceneDevice>),
    Failed(String),
}

/// What a press over the viewport is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DragKind {
    Orbit,
    Pan,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct DragState {
    pub(super) kind: DragKind,
    pub(super) last_position: Vec2,
    /// Where the press went down, so small jitter does not count as a drag.
    pub(super) start_position: Vec2,
    /// Whether the pointer has moved past the drag threshold since the press, which tells a plain
    /// click on the gizmo apart from an orbit that started inside its box.
    pub(super) moved: bool,
}

/// Movement keys held for continuous camera motion.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) enum MoveKey {
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

/// The armor plate under the pointer: its key, what its tooltip says, and where the pointer is,
/// in the viewport's own CSS pixels.
#[derive(Clone)]
pub(super) struct HoverInfo {
    pub(super) key: PlateKey,
    pub(super) tooltip: ArmorTriangleTooltip,
    pub(super) cursor: Vec2,
}

/// The mesh under the pointer when no plate is.
pub(super) struct MeshHover {
    pub(super) mesh: HullMeshIndex,
    pub(super) tooltip: MeshTooltip,
    pub(super) cursor: Vec2,
}

/// A ring spreading from a hit that landed as a battle played.
#[derive(Clone, Copy, Debug)]
pub(super) struct HitPulse {
    /// Where the shell struck, in model space.
    pub(super) at: Vec3,
    pub(super) started: Instant,
}

/// A cast arc and what it was fired with.
pub(super) struct CastArc {
    pub(super) id: ArcId,
    pub(super) color: ArcColor,
    pub(super) shell: Option<CastShell>,
    /// The bearing the shell came in on, which a new fall angle is built on.
    pub(super) approach: Vec3,
    pub(super) trajectory: trajectory::Trajectory,
    /// The lines and markers `trajectory` is drawn with, worked out when it changes rather than on
    /// every frame.
    pub(super) drawing: trajectory::ArcDrawing,
}

impl CastArc {
    pub(super) fn set_trajectory(&mut self, trajectory: trajectory::Trajectory) {
        self.drawing = trajectory::drawing(&trajectory);
        self.trajectory = trajectory;
    }
}

/// The export-confirm panel's target, fixed when the panel opens so a selection made while it is
/// open does not retarget it.
pub(super) struct ExportConfirm {
    pub(super) assets: Arc<ShipAssets>,
    pub(super) param_index: String,
    pub(super) display_name: String,
    pub(super) hull_lod: usize,
    pub(super) selected_hull: Option<String>,
    pub(super) selected_modules: HashMap<ComponentType, String>,
}

/// How many schemes' decoded textures are kept. One scheme is several full-size textures, so a
/// reader stepping through a ship's camo list would otherwise hold every one of them.
const CAMO_TEXTURE_CACHE_SIZE: usize = 3;

/// Decoded scheme textures, most recently used last. Shared by `Arc`, so a hit hands the decoder
/// the same textures rather than a copy.
#[derive(Default)]
pub(super) struct CamoTextureCache {
    entries: Vec<(CamoSchemeId, Arc<SchemeTextures>)>,
}

impl CamoTextureCache {
    pub(super) fn get(&mut self, id: CamoSchemeId) -> Option<Arc<SchemeTextures>> {
        let index = self.entries.iter().position(|(held, _)| *held == id)?;
        let entry = self.entries.remove(index);
        let textures = Arc::clone(&entry.1);
        self.entries.push(entry);
        Some(textures)
    }

    pub(super) fn insert(&mut self, id: CamoSchemeId, textures: Arc<SchemeTextures>) {
        self.entries.retain(|(held, _)| *held != id);
        self.entries.push((id, textures));
        if self.entries.len() > CAMO_TEXTURE_CACHE_SIZE {
            self.entries.remove(0);
        }
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Per-hull-part composited camo textures, keyed as `upload_hull::upload_hull_meshes` expects.
pub(super) type CamoTextures = wows_toolkit_armor::camo::TexturesByPart;

/// Ships and shells a replay hands over to compare landed hits against, kept so a comparison
/// asked for before the device was there runs once it is.
pub(super) struct PendingComparison {
    pub(super) taken: Vec<PreExtractedHit>,
    pub(super) shells: HashMap<GameParamId, ShellInfo>,
}

pub struct ViewportModel {
    pub(super) viewport: Viewport3D,
    pub(super) gpu: GpuState,
    /// World-space bounding box of the scene.
    pub(super) model_bounds: Option<(Vec3, Vec3)>,
    /// The camera the scene opened with, which a double-click returns to.
    pub(super) home_camera: Option<ArcballCamera>,
    /// Whether a standalone model's meshes are drawn opaque. Kept apart from
    /// `display_settings.hull_opaque`, the saved setting for ships, because a lone model has
    /// nothing inside it to see through to.
    pub(super) model_hull_opaque: bool,
    /// The viewport's own rectangle, in CSS pixels with its origin at the top left. `None` until
    /// it is first laid out; every screen-space projection waits on it.
    pub(super) last_bounds: Option<ViewRect>,
    pub(super) drag: Option<DragState>,
    /// Where the pointer last moved over the view, so a hover can be picked again when the picking
    /// index lands under a still pointer.
    pub(super) last_pointer: Option<Vec2>,
    pub(super) gizmo_hover: Option<(Axis, bool)>,
    /// Whether the held primary button went down inside the gizmo box, so a release decides
    /// between an orbit and a snap.
    pub(super) gizmo_press_in_box: bool,
    /// Movement keys held down, which move the camera every frame while any is.
    pub(super) held_keys: HashSet<MoveKey>,
    /// A scene handed over while there was no device. Uploaded once there is one.
    pub(super) pending_armor: Option<(Arc<LoadedShipArmor>, OpeningLayer)>,
    pub(super) mesh_seed: MeshSeed,
    /// The subject being loaded into this viewport, shown in place of the scene.
    pub(super) ship_loading: Option<String>,
    /// Why the scene asked for cannot be drawn, shown in its place.
    pub(super) scene_problem: Option<SceneProblem>,
    /// The latest load the pane started into this viewport.
    pub(super) load_generation: LoadGeneration,
    /// The first stage of the load the pane started into this viewport.
    pub(super) opening_load: LoadSlot,
    /// The first stage of the reload running for the scene shown.
    pub(super) reload_load: LoadSlot,
    /// The background stage finishing the scene shown, shared with every viewport showing it.
    pub(super) rest_load: Option<Arc<SharedLoad>>,
    /// The armor displayed, kept so a visibility change can upload again without reloading.
    pub current_armor: Option<Arc<LoadedShipArmor>>,
    /// Part visibility overrides: absent is visible, a present value says whether it is.
    pub part_visibility: HashMap<(String, String), bool>,
    /// What a new ship's armor starts as, from the saved defaults.
    pub(super) armor_all_visible: bool,
    /// Explicitly hidden plates: absent is visible, `true` is hidden.
    pub plate_visibility: HashMap<PlateKey, bool>,
    /// Hull mesh visibility by mesh name: absent is hidden.
    pub hull_visibility: HashMap<String, bool>,
    /// What a new ship's hull starts as, from the saved defaults.
    pub(super) hull_all_visible: bool,
    /// The uploaded hull meshes, removed before the visible set is uploaded again.
    pub(super) hull_mesh_ids: Vec<MeshId>,
    /// Which hull-part-group headers are expanded in the hull tree.
    pub expanded_hull_groups: HashSet<String>,
    /// Which camo-origin group headers are expanded in the camo picker.
    pub expanded_camo_groups: HashSet<String>,
    /// What a reload or an export reads again. `None` until the pane's first load completes.
    pub(super) scene_source: Option<SceneSource>,
    /// The hull upgrade selected, `None` for the stock hull.
    pub selected_hull: Option<String>,
    /// The hull level of detail, 0 the highest.
    pub hull_lod: usize,
    /// Module alternatives selected, by component type.
    pub selected_modules: HashMap<ComponentType, String>,
    /// Bumped by every reload, so a reload that lands after a later one started is dropped.
    pub(super) reload_generation: u64,
    /// The camo scheme selected, `None` for the stock albedo.
    pub selected_camo: Option<CamoSchemeId>,
    pub(super) camo_texture_cache: CamoTextureCache,
    /// Bumped by every camo selection and every armor swap, so a decode that lands after the
    /// reader moved on is dropped.
    pub(super) camo_generation: u64,
    /// The active camo's composited hull textures, by mfm stem.
    pub(super) active_camo_textures: CamoTextures,
    /// The active camo's UV transforms, by mfm stem, for stems left tiled.
    pub(super) active_camo_uvs: HashMap<String, UvTransform>,
    /// The textures the hull meshes sample, one per material.
    pub(super) hull_textures: HullTextureCache,
    /// Whose hardpoints are marked, as the sidebar's checkboxes left them.
    pub(super) hardpoint_targets: HashSet<HardpointTarget>,
    /// The marked hardpoints' names and where each is drawn.
    pub(super) hardpoint_labels: Vec<hardpoint_overlay::HardpointLabel>,
    /// The length marks under the shown scene, with their distances written out.
    pub(super) length_marks: Option<MarkLabels>,
    /// The overlay mesh drawing the hardpoint markers.
    pub(super) hardpoint_mesh: Option<MeshId>,
    /// The scene units per pixel `hardpoint_mesh` was sized for.
    pub(super) hardpoint_world_per_pixel: f32,
    /// The marker under the pointer, by its index in `hardpoint_labels`.
    pub(super) hovered_hardpoint: Option<usize>,
    /// Undo and redo over the part and plate visibility.
    pub(super) undo_stack: VisibilityUndoStack,
    /// Which zone headers are expanded in the armor tree.
    pub expanded_zones: HashSet<String>,
    /// Which material headers are expanded in the armor tree, by zone and material.
    pub expanded_parts: HashSet<(String, String)>,
    /// The armor plate under the pointer.
    pub(super) hovered: Option<HoverInfo>,
    /// What this ship had taken when a replay asked for it.
    pub(super) hits: Vec<PreExtractedHit>,
    /// Rings still spreading from hits that landed as a battle played.
    pub(super) hit_pulses: Vec<HitPulse>,
    /// What this app's own simulation made of each landed shell.
    pub(super) sim_verdicts: HashMap<ShotId, SimAgreement>,
    /// How many armour-piercing shells the comparison could not reach an answer on.
    pub(super) sim_unchecked: usize,
    /// A comparison asked for while there was no device, run once there is one.
    pub(super) pending_comparison: Option<PendingComparison>,
    /// The ship's health at the moment shown, while a replay drives this viewer.
    pub(super) hit_health: Option<f32>,
    /// The camera orbits drawn, so a pointer resting on one can be told which it is.
    pub(super) ring_hovers: Vec<camera_rings::RingHover>,
    /// What the orbit under the pointer reads as, and where to anchor it.
    pub(super) hovered_ring: Option<(String, Vec2)>,
    /// The overlay mesh highlighting the hovered plate.
    pub(super) hover_highlight: Option<(PlateKey, MeshId)>,
    /// The overlay mesh highlighting a hovered tree row.
    pub(super) sidebar_highlight: Option<(SidebarHighlightKey, MeshId)>,
    /// The mesh under the pointer when no plate is.
    pub(super) hovered_mesh: Option<MeshHover>,
    /// The overlay mesh highlighting every mesh named as `hovered_mesh` is.
    pub(super) mesh_highlight: Option<(String, MeshId)>,
    /// Which hull meshes the last hull upload drew: the meshes a hover can hit.
    pub(super) hull_shown: Vec<bool>,
    /// Per-mesh triangle tooltips from the last armor upload, for mapping a pick back to a plate.
    pub(super) mesh_triangle_info: Vec<(MeshId, Vec<ArmorTriangleTooltip>)>,
    /// The zone and thickness of the last plate the pointer was over, kept after the hover ends so
    /// the penetration checker opens on it.
    pub(super) last_plate: Option<(String, f32)>,
    /// Display settings: plate edges, zero-mm plates, armor opacity and the like.
    pub display_settings: upload::DisplaySettings,
    /// Which camera orbits are drawn over the ship.
    pub(super) camera_rings: CameraRingSettings,
    /// Whether the camera is locked to one of the ship's own orbits.
    pub(super) perspective: PerspectiveSettings,
    /// The free camera as it was when the lock went on, put back when it comes off.
    pub(super) camera_before_lock: Option<ArcballCamera>,
    /// The meshes marking where the locked camera is aimed.
    pub(super) aim_mesh_ids: Vec<MeshId>,
    /// Whether the openings in the armor are marked.
    pub(super) show_gaps: bool,
    /// The shells cast so far.
    pub(super) arcs: Vec<CastArc>,
    /// The id the next cast arc takes.
    pub(super) next_arc: ArcId,
    /// The arc the trajectory section is editing.
    pub(super) selected_arc: Option<ArcId>,
    /// Whether a shell that ricochets is followed past the bounce.
    pub(super) continue_on_ricochet: bool,
    /// The ship and shell the next cast is fired with.
    pub(super) cast_shell: Option<CastShell>,
    /// The range the next cast is fired from.
    pub(super) cast_range: Km,
    /// Whether a left click bursts a shell instead of hiding the plate under it.
    pub(super) splash_mode: bool,
    /// The burst on screen, and what it reached.
    pub(super) splash_result: Option<splash::SplashResult>,
    /// Whether the named splash boxes are outlined over the hull.
    pub(super) show_splash_boxes: bool,
    /// The boxes' names and where each is written, while they are outlined.
    pub(super) splash_labels: Vec<splash::SplashBoxLabel>,
    pub(super) gap_count: usize,
    /// Whether only the plates the game's own viewer hides are shown.
    pub(super) show_hidden_only: bool,
    /// The export-confirm panel's target, while it is open.
    pub(super) export_confirm: Option<ExportConfirm>,
    /// The camera snap in flight advances on frame timestamps; this is the last one it advanced
    /// at, so the next frame steps it by the time between them.
    pub(super) last_frame: Option<zgui::vocab::Timestamp>,
}

impl ViewportModel {
    pub(super) fn new() -> Self {
        Self {
            viewport: Viewport3D::new(),
            gpu: GpuState::Waiting,
            model_bounds: None,
            home_camera: None,
            model_hull_opaque: true,
            last_bounds: None,
            drag: None,
            last_pointer: None,
            gizmo_hover: None,
            gizmo_press_in_box: false,
            held_keys: HashSet::new(),
            pending_armor: None,
            mesh_seed: MeshSeed::default(),
            ship_loading: None,
            scene_problem: None,
            load_generation: LoadGeneration::default(),
            opening_load: LoadSlot::default(),
            reload_load: LoadSlot::default(),
            rest_load: None,
            current_armor: None,
            part_visibility: HashMap::new(),
            armor_all_visible: true,
            plate_visibility: HashMap::new(),
            hull_visibility: HashMap::new(),
            hull_all_visible: false,
            hull_mesh_ids: Vec::new(),
            expanded_hull_groups: HashSet::new(),
            expanded_camo_groups: HashSet::new(),
            scene_source: None,
            selected_hull: None,
            hull_lod: load_ship::DEFAULT_LOD,
            selected_modules: HashMap::new(),
            reload_generation: 0,
            selected_camo: None,
            camo_texture_cache: CamoTextureCache::default(),
            camo_generation: 0,
            active_camo_textures: HashMap::new(),
            active_camo_uvs: HashMap::new(),
            hull_textures: HullTextureCache::default(),
            hardpoint_targets: HashSet::new(),
            hardpoint_labels: Vec::new(),
            length_marks: None,
            hardpoint_mesh: None,
            hardpoint_world_per_pixel: 0.0,
            hovered_hardpoint: None,
            undo_stack: VisibilityUndoStack::default(),
            expanded_zones: HashSet::new(),
            expanded_parts: HashSet::new(),
            hovered: None,
            hits: Vec::new(),
            hit_pulses: Vec::new(),
            sim_verdicts: HashMap::new(),
            sim_unchecked: 0,
            pending_comparison: None,
            hit_health: None,
            ring_hovers: Vec::new(),
            hovered_ring: None,
            hover_highlight: None,
            sidebar_highlight: None,
            hovered_mesh: None,
            mesh_highlight: None,
            hull_shown: Vec::new(),
            mesh_triangle_info: Vec::new(),
            last_plate: None,
            display_settings: upload::DisplaySettings::default(),
            camera_rings: CameraRingSettings::default(),
            perspective: PerspectiveSettings::default(),
            camera_before_lock: None,
            aim_mesh_ids: Vec::new(),
            show_gaps: false,
            arcs: Vec::new(),
            next_arc: ArcId::FIRST,
            selected_arc: None,
            continue_on_ricochet: false,
            cast_shell: None,
            cast_range: DEFAULT_CAST_RANGE,
            splash_mode: false,
            splash_result: None,
            show_splash_boxes: false,
            splash_labels: Vec::new(),
            gap_count: 0,
            show_hidden_only: false,
            export_confirm: None,
            last_frame: None,
        }
    }

    /// The device meshes are uploaded to, while there is one.
    pub(super) fn scene(&self) -> Option<Rc<SceneDevice>> {
        match &self.gpu {
            GpuState::Ready(scene) => scene.upgrade(),
            GpuState::Waiting | GpuState::Failed(_) => None,
        }
    }

    /// Stops every load into this viewport and lets its meshes go, for a viewport being closed.
    pub(super) fn release(&mut self) {
        self.opening_load.cancel();
        self.reload_load.cancel();
        self.rest_load = None;
        self.viewport.clear();
    }

    pub(super) fn is_gpu_ready(&self) -> bool {
        self.scene().is_some()
    }

    /// The viewport's size in CSS pixels, or a unit square before it is laid out.
    pub(super) fn viewport_size(&self) -> (f32, f32) {
        match self.last_bounds {
            Some(rect) => (rect.w, rect.h),
            None => (1.0, 1.0),
        }
    }
}

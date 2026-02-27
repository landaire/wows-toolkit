use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use egui_dock::DockState;

use wowsunpack::game_params::types::Km;

use crate::viewport_3d::ArcballCamera;
use crate::viewport_3d::GpuPipeline;
use crate::viewport_3d::MeshId;
use crate::viewport_3d::Vec3;
use crate::viewport_3d::Viewport3D;

/// Key identifying a specific plate: (zone, material_name, thickness in tenths of mm).
/// The thickness discriminator ensures highlights stop at plate boundaries.
pub type PlateKey = (String, String, i32);

/// A material/part within an armor zone, with its sorted unique plate thicknesses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZonePart {
    pub name: String,
    /// Sorted unique plate thicknesses in tenths of mm.
    pub plates: Vec<i32>,
}

/// An armor zone containing multiple material parts, each with plate thicknesses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArmorZone {
    pub name: String,
    pub parts: Vec<ZonePart>,
}

/// Identifies what the user is hovering over in a sidebar/popover for highlight purposes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarHighlightKey {
    /// All visible armor triangles in a zone.
    Zone(String),
    /// All visible armor triangles for a (zone, material/part).
    Part(String, String),
    /// A specific plate by (zone, material, thickness_i32).
    Plate(PlateKey),
    /// One or more hull meshes by name.
    HullMeshes(Vec<String>),
    /// One or more splash boxes by name.
    SplashBoxes(Vec<String>),
}

/// Result of drawing the hull visibility popover.
#[derive(Default)]
pub struct HullPopoverResult {
    /// Whether hull/part visibility toggles changed (requires mesh re-upload).
    pub zone_changed: bool,
    /// Sidebar item currently hovered for highlight purposes.
    pub hovered_key: Option<SidebarHighlightKey>,
    /// New LOD level selected by the user, if changed.
    pub new_lod: Option<usize>,
    /// Whether the hull upgrade selection changed.
    pub hull_changed: bool,
    /// Whether a module alternative selection changed.
    pub module_changed: bool,
}

/// Which tab is active in the unified analysis window.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AnalysisTab {
    #[default]
    Ships,
    Trajectory,
    Splash,
}

/// Persisted default display settings for the armor viewer.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ArmorViewerDefaults {
    pub show_plate_edges: bool,
    pub show_waterline: bool,
    pub show_zero_mm: bool,
    pub armor_opacity: f32,
    pub waterline_opacity: f32,
    pub hull_opaque: bool,
    pub hull_all_visible: bool,
    pub armor_all_visible: bool,
    pub show_splash_boxes: bool,
}

impl Default for ArmorViewerDefaults {
    fn default() -> Self {
        Self {
            show_plate_edges: true,
            show_waterline: true,
            show_zero_mm: false,
            armor_opacity: 1.0,
            waterline_opacity: 0.3,
            hull_opaque: false,
            hull_all_visible: false,
            armor_all_visible: true,
            show_splash_boxes: false,
        }
    }
}

/// Snapshot of visibility state for undo/redo.
#[derive(Clone)]
pub struct VisibilitySnapshot {
    pub part_visibility: HashMap<(String, String), bool>,
    pub plate_visibility: HashMap<PlateKey, bool>,
}

/// Simple undo/redo stack for visibility changes.
#[derive(Default)]
pub struct VisibilityUndoStack {
    undo: Vec<VisibilitySnapshot>,
    redo: Vec<VisibilitySnapshot>,
}

impl VisibilityUndoStack {
    const MAX_ENTRIES: usize = 50;

    /// Push current state before a mutation. Clears the redo stack.
    pub fn push(&mut self, snapshot: VisibilitySnapshot) {
        self.undo.push(snapshot);
        if self.undo.len() > Self::MAX_ENTRIES {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Undo: returns the previous snapshot, pushing current state onto redo.
    pub fn undo(&mut self, current: VisibilitySnapshot) -> Option<VisibilitySnapshot> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        Some(prev)
    }

    /// Redo: returns the next snapshot, pushing current state onto undo.
    pub fn redo(&mut self, current: VisibilitySnapshot) -> Option<VisibilitySnapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

/// Loading state for ShipAssets.
#[derive(Default)]
pub enum ShipAssetsState {
    #[default]
    NotLoaded,
    Loading(Receiver<Result<Arc<wowsunpack::export::ship::ShipAssets>, String>>),
    Loaded(Arc<wowsunpack::export::ship::ShipAssets>),
    Failed(String),
}

/// Settings to clone into a new pane for comparison.
pub struct CompareSettings {
    pub ship_param_index: String,
    pub ship_display_name: String,
    pub camera: ArcballCamera,
    pub part_visibility: HashMap<(String, String), bool>,
    pub hull_visibility: HashMap<String, bool>,
}

/// A pending ship model export request.
pub struct ExportRequest {
    /// GameParam index key for the ship.
    pub param_index: String,
    /// Human-readable ship name.
    pub display_name: String,
    /// Selected hull upgrade key, or `None` for stock.
    pub selected_hull: Option<String>,
}

/// Top-level state for the Armor Viewer tab.
#[allow(dead_code)]
pub struct ArmorViewerState {
    /// Dock state managing split panes. Each tab is an ArmorPane.
    pub dock_state: DockState<ArmorPane>,
    /// Cached ship catalog, built once from GameParams.
    pub ship_catalog: Option<Arc<crate::armor_viewer::ship_selector::ShipCatalog>>,
    /// Shared ShipAssets handle (expensive, created once on first use).
    pub ship_assets: ShipAssetsState,
    /// GPU pipeline shared across all panes.
    pub gpu_pipeline: Option<Arc<GpuPipeline>>,
    /// Counter for generating unique pane IDs.
    pub next_pane_id: u64,
    /// Cached nation flag assets (raw PNG bytes), keyed by nation name.
    pub nation_flag_textures: HashMap<String, Arc<crate::wows_data::GameAsset>>,
    /// Pending export confirmation dialog.
    pub export_confirm: Option<ExportRequest>,
    /// When true, all split panes share the same camera.
    pub mirror_cameras: bool,
    /// When true, armor/hull visibility is synced across all panes.
    pub sync_options: bool,
    /// Shared ship selector search text (single sidebar).
    pub selector_search: String,
    /// Previous frame's search text, used to detect changes.
    pub prev_selector_search: String,
    /// ID of the pane that receives ship selections from the sidebar.
    pub active_pane_id: u64,
    /// Ships added to the penetration comparison list.
    pub comparison_ships: Vec<crate::armor_viewer::penetration::ComparisonShip>,
    /// Search text for the penetration comparison panel.
    pub comparison_search: String,
    /// Whether the penetration comparison floating window is open.
    pub show_comparison_panel: bool,
    /// Whether IFHE (Inertia Fuse for HE Shells) modifier is enabled (+25% HE pen).
    pub ifhe_enabled: bool,
    /// Incremented whenever comparison_ships changes; panes check this to recompute arcs.
    pub comparison_ships_version: u64,
    /// Dock state for the analysis sub-panel (Ships / Trajectory / Splash).
    pub analysis_dock_state: DockState<AnalysisTab>,
}

impl Default for ArmorViewerState {
    fn default() -> Self {
        Self {
            dock_state: DockState::new(vec![ArmorPane::empty(0)]),
            ship_catalog: None,
            ship_assets: ShipAssetsState::default(),
            gpu_pipeline: None,
            next_pane_id: 1,
            nation_flag_textures: HashMap::new(),
            export_confirm: None,
            mirror_cameras: false,
            sync_options: false,
            selector_search: String::new(),
            prev_selector_search: String::new(),
            active_pane_id: 0,
            comparison_ships: Vec::new(),
            comparison_search: String::new(),
            show_comparison_panel: false,
            ifhe_enabled: false,
            comparison_ships_version: 0,
            analysis_dock_state: DockState::new(vec![AnalysisTab::Ships, AnalysisTab::Trajectory, AnalysisTab::Splash]),
        }
    }
}

impl ArmorViewerState {
    pub fn allocate_pane_id(&mut self) -> u64 {
        let id = self.next_pane_id;
        self.next_pane_id += 1;
        id
    }

    /// Apply persisted defaults to all existing panes (used after deserialization).
    pub fn apply_defaults(&mut self, defaults: &ArmorViewerDefaults) {
        for (_, pane) in self.dock_state.iter_all_tabs_mut() {
            pane.show_plate_edges = defaults.show_plate_edges;
            pane.show_waterline = defaults.show_waterline;
            pane.show_zero_mm = defaults.show_zero_mm;
            pane.armor_opacity = defaults.armor_opacity;
        }
    }
}

/// Per-triangle metadata from the armor mesh, for tooltip display.
#[derive(Clone, Debug)]
pub struct ArmorTriangleTooltip {
    pub material_name: String,
    pub zone: String,
    pub thickness_mm: f32,
    /// Per-layer thicknesses (ordered by model_index). Single-layer plates have one entry.
    pub layers: Vec<f32>,
    pub color: [f32; 4],
}

/// Data for a loaded ship's armor.
#[allow(dead_code)]
pub struct LoadedShipArmor {
    pub display_name: String,
    pub meshes: Vec<wowsunpack::export::gltf_export::InteractiveArmorMesh>,
    pub bounds: (Vec3, Vec3),
    pub zones: Vec<String>,
    /// Ordered mapping: zone name -> sorted list of unique material names in that zone.
    pub zone_parts: Vec<(String, Vec<String>)>,
    /// Three-level hierarchy: zone -> materials -> sorted unique plate thicknesses (i32, tenths of mm).
    pub zone_part_plates: Vec<ArmorZone>,
    /// Hull visual meshes (render sets) for optional overlay display.
    pub hull_meshes: Vec<wowsunpack::export::gltf_export::InteractiveHullMesh>,
    /// Hull parts grouped by category (e.g. "Hull", "Main Battery"), each with sorted part names.
    pub hull_part_groups: Vec<(String, Vec<String>)>,
    /// Normalized waterline offset from model origin [-1, 1].
    /// -1 = bottom of bounding box, 0 = pivot (model origin), +1 = top.
    pub dock_y_offset: Option<f32>,
    /// Parsed splash box data for HE splash visualization.
    pub splash_data: Option<crate::armor_viewer::splash::ShipSplashData>,
    /// Splash box names grouped by prefix (e.g. "Bow" → ["CM_SB_Bow_01", "CM_SB_Bow_02"]).
    pub splash_box_groups: Vec<(String, Vec<String>)>,
    /// Hit location data from GameParams (zone name → HitLocation).
    pub hit_locations: Option<std::collections::HashMap<String, wowsunpack::game_params::types::HitLocation>>,
    /// Vertical offset applied by `apply_waterline_offset()`. World-space Y positions
    /// must be shifted by this amount to align with the shifted model coordinates.
    pub waterline_dy: f32,
    /// Decoded hull textures: mfm_path → (width, height, RGBA8 pixels).
    /// Loaded on background thread, uploaded to GPU during `upload_armor_to_viewport`.
    pub hull_textures: HashMap<String, (u32, u32, Vec<u8>)>,
    /// Number of LOD levels available for hull meshes.
    pub hull_lod_count: usize,
    /// The LOD level used to load the current hull meshes.
    pub hull_lod: usize,
    /// Available hull upgrade names: Vec<(param_key, display_label)>, sorted alphabetically.
    /// Display labels are "Hull A", "Hull B", etc. based on alphabetical order.
    pub hull_upgrade_names: Vec<(String, String)>,
    /// The hull upgrade key that was used to load this armor data.
    pub loaded_hull: Option<String>,
    /// Module alternatives: component type -> list of component names.
    /// Only populated for types that have more than one option in the current hull upgrade.
    pub module_alternatives: Vec<(wowsunpack::game_params::keys::ComponentType, Vec<String>)>,
}

impl LoadedShipArmor {
    /// Shift all mesh vertex positions and bounds so the waterline sits at Y=0.
    ///
    /// `dockYOffset` is the waterline Y position in model space (typically a small
    /// negative value). Call this once after construction. All downstream consumers
    /// (upload, picking, edges, trajectories, splash) then work in waterline-relative coordinates.
    pub fn apply_waterline_offset(&mut self) {
        let dy = self.dock_y_offset.map_or(0.0, |offset| -offset);
        if dy.abs() < 1e-7 {
            return;
        }

        for mesh in &mut self.meshes {
            for pos in &mut mesh.positions {
                pos[1] += dy;
            }
        }
        for mesh in &mut self.hull_meshes {
            for pos in &mut mesh.positions {
                pos[1] += dy;
            }
        }
        self.bounds.0.y += dy;
        self.bounds.1.y += dy;
        self.waterline_dy = dy;
    }

    /// Bounding-box center in model space.
    pub fn center(&self) -> Vec3 {
        (self.bounds.0 + self.bounds.1) * 0.5
    }

    /// Maximum extent in the XZ plane (max of width, depth).
    pub fn max_extent_xz(&self) -> f32 {
        let dx = self.bounds.1.x - self.bounds.0.x;
        let dz = self.bounds.1.z - self.bounds.0.z;
        dx.max(dz)
    }
}

/// Cached per-shell simulation results for the analysis panel display.
/// Avoids recomputing `solve_for_range` + `simulate_shell_through_hits` every frame.
pub struct CachedShellSim {
    pub ship_name: String,
    pub ship_index: usize,
    pub shell: wowsunpack::game_params::types::ShellInfo,
    pub sim: Option<crate::armor_viewer::penetration::ShellSimResult>,
}

/// Cached shell simulation data for a trajectory, invalidated when range or comparison ships change.
#[allow(dead_code)]
pub struct ShellSimCache {
    pub sims: Vec<CachedShellSim>,
    /// Last visible hit index derived from the cached sims.
    pub last_visible_hit: Option<usize>,
    /// Range at which these sims were computed.
    pub range_km: Km,
    /// Comparison ships version when these sims were computed.
    pub comparison_ships_version: u64,
}

/// A trajectory with its metadata and visualization mesh.
pub struct StoredTrajectory {
    pub meta: crate::armor_viewer::penetration::TrajectoryMeta,
    pub result: crate::armor_viewer::penetration::TrajectoryResult,
    pub mesh_id: Option<MeshId>,
    /// Last hit index visible before shell detonation (earliest across all shells).
    /// `None` means no shell detonates — all hits are visible.
    pub last_visible_hit: Option<usize>,
    /// Camera distance at the time markers were last uploaded (for scaling).
    pub marker_cam_dist: f32,
    /// Whether this arc's hit plates are isolated in the visibility filter.
    pub show_plates_active: bool,
    /// Whether this arc's hit zones are isolated in the visibility filter.
    pub show_zones_active: bool,
    /// Cached shell simulation results for the analysis panel.
    pub shell_sim_cache: Option<ShellSimCache>,
    /// The `model_roll` (radians) at which this trajectory was ray-cast.
    /// Used to rotate the ray into model space when roll changes.
    pub created_at_roll: f32,
    /// The `model_yaw` (radians) at which this trajectory was ray-cast.
    #[allow(dead_code)]
    pub created_at_yaw: f32,
}

/// State for a single armor viewer pane within the split tree.
#[allow(dead_code)]
pub struct ArmorPane {
    pub id: u64,
    /// Currently selected ship (param_index).
    pub selected_ship: Option<String>,
    /// The 3D viewport for this pane.
    pub viewport: Viewport3D,
    /// Loaded armor data.
    pub loaded_armor: Option<LoadedShipArmor>,
    /// Whether a ship is currently loading.
    pub loading: bool,
    /// Receiver for background ship loading.
    pub load_receiver: Option<Receiver<Result<LoadedShipArmor, String>>>,
    /// Currently hovered triangle info.
    pub hovered_info: Option<ArmorTriangleTooltip>,
    /// Per-part visibility toggles, keyed by (zone, material_name).
    pub part_visibility: HashMap<(String, String), bool>,
    /// Per hull render set visibility (name → visible). Defaults to all false.
    pub hull_visibility: HashMap<String, bool>,
    /// When true, hull renders fully opaque with depth writes (like armor plates).
    pub hull_opaque: bool,
    /// Selected camouflage (future).
    pub selected_camo: Option<String>,
    /// Maps MeshId -> per-triangle tooltip data for picking.
    pub mesh_triangle_info: Vec<(MeshId, Vec<ArmorTriangleTooltip>)>,
    /// Hover highlight: plate key (zone, material_name, thickness_mm rounded) and its overlay mesh.
    pub hover_highlight: Option<(PlateKey, MeshId)>,
    /// Sidebar hover highlight — overlay mesh for the item currently hovered in a visibility popover.
    pub sidebar_highlight: Option<(SidebarHighlightKey, MeshId)>,
    /// Per-plate visibility toggles. Absent = visible. Only plates explicitly hidden are stored.
    pub plate_visibility: HashMap<PlateKey, bool>,
    /// Persisted key for the right-click context menu (plate-level).
    pub context_menu_key: Option<PlateKey>,
    /// Whether to show the waterline plane.
    pub show_waterline: bool,
    /// Whether to show black outlines at plate thickness boundaries.
    pub show_plate_edges: bool,
    /// Whether to show 0mm thickness plates.
    pub show_zero_mm: bool,
    /// Armor plate opacity (0.0–1.0).
    pub armor_opacity: f32,
    /// When true, only plates the in-game viewer hides ("Hull" zone) are rendered.
    pub show_hidden_only: bool,
    /// Undo/redo stack for visibility changes.
    pub undo_stack: VisibilityUndoStack,
    /// Whether to show gap (boundary edge) overlay.
    pub show_gaps: bool,
    /// Number of gap edges found in the last analysis.
    pub gap_count: usize,
    /// Whether trajectory analysis mode is active (click to cast ray).
    pub trajectory_mode: bool,
    /// Stored trajectories with per-trajectory metadata and mesh IDs.
    pub trajectories: Vec<StoredTrajectory>,
    /// Counter for assigning unique trajectory IDs.
    pub next_trajectory_id: u64,
    /// Default ballistic range for new trajectories.
    pub ballistic_range: Km,
    /// Waterline plane opacity (0.0–1.0).
    pub waterline_opacity: f32,
    /// Trajectory impact marker opacity (0.0–1.0).
    pub marker_opacity: f32,
    /// Last comparison_ships_version this pane recomputed arcs for.
    pub comparison_ships_version: u64,
    /// Whether HE splash analysis mode is active (click to place splash volume).
    pub splash_mode: bool,
    /// Current splash analysis result.
    pub splash_result: Option<crate::armor_viewer::splash::SplashResult>,
    /// Overlay mesh IDs for the current splash visualization (cube + highlight).
    pub splash_mesh_ids: Vec<MeshId>,
    /// Whether to show splash box AABBs as wireframe overlays.
    pub show_splash_boxes: bool,
    /// Overlay mesh IDs for splash box wireframes.
    pub splash_box_mesh_ids: Vec<MeshId>,
    /// Label positions for splash box wireframes (for text overlay).
    pub splash_box_labels: Vec<crate::armor_viewer::splash::SplashBoxLabel>,
    /// Per-splash-box visibility toggles. Absent = visible.
    pub splash_box_visibility: HashMap<String, bool>,
    /// Whether hull parts should default to all-visible when a new ship loads.
    /// Carried from `ArmorViewerDefaults::hull_all_visible` at pane creation time.
    pub default_hull_all_visible: bool,
    /// Whether armor parts should default to all-visible when a new ship loads.
    /// Carried from `ArmorViewerDefaults::armor_all_visible` at pane creation time.
    pub default_armor_all_visible: bool,
    /// Desired hull LOD level (0 = highest detail). Changed via the Hull popover dropdown.
    pub hull_lod: usize,
    /// GPU mesh IDs for uploaded hull meshes (so they can be selectively removed on LOD change).
    pub hull_mesh_ids: Vec<MeshId>,
    /// Receiver for background hull-only reload (LOD change).
    pub hull_load_receiver: Option<Receiver<Result<HullReloadData, String>>>,
    /// Receiver for background upgrade-only reload (hull upgrade change without full ship reload).
    pub upgrade_load_receiver: Option<Receiver<Result<UpgradeReloadData, String>>>,
    /// Selected hull upgrade name (GameParam key). `None` = stock (first alphabetically).
    pub selected_hull: Option<String>,
    /// Selected module overrides: component type -> component name.
    /// When a module type has alternatives, this stores the user's selection.
    pub selected_modules: HashMap<wowsunpack::game_params::keys::ComponentType, String>,
    /// When true, trajectory meshes use world-space uniforms (no model rotation).
    /// The standalone viewer sets this to true because it manually recomputes
    /// trajectories when roll changes. The realtime viewer sets this to false so
    /// trajectories rotate with the model matrix (yaw/roll baked via inverse_ship_rotation).
    pub trajectory_world_space: bool,
    /// When true, the shell simulation continues past ricochet plates instead of stopping.
    pub continue_on_ricochet: bool,
}

/// Data returned by a hull-only background reload (LOD change without full ship reload).
pub struct HullReloadData {
    pub hull_meshes: Vec<wowsunpack::export::gltf_export::InteractiveHullMesh>,
    pub hull_part_groups: Vec<(String, Vec<String>)>,
    pub hull_textures: HashMap<String, (u32, u32, Vec<u8>)>,
    pub hull_lod: usize,
    pub hull_lod_count: usize,
}

/// Data returned by an upgrade-only background reload (hull upgrade change without full ship reload).
pub struct UpgradeReloadData {
    /// Replacement armor meshes (hull armor unchanged, turret armor re-mounted).
    pub armor_meshes: Vec<wowsunpack::export::gltf_export::InteractiveArmorMesh>,
    /// Updated zone/part/plate metadata derived from the new armor meshes.
    pub zones: Vec<String>,
    pub zone_parts: Vec<(String, Vec<String>)>,
    pub zone_part_plates: Vec<ArmorZone>,
    /// New hull visual meshes (hull parts + mounted turrets with new mount transforms).
    pub hull_meshes: Vec<wowsunpack::export::gltf_export::InteractiveHullMesh>,
    pub hull_part_groups: Vec<(String, Vec<String>)>,
    pub hull_textures: HashMap<String, (u32, u32, Vec<u8>)>,
    /// Which hull upgrade key was loaded.
    pub loaded_hull: Option<String>,
    /// Updated module alternatives for the new hull upgrade.
    pub module_alternatives: Vec<(wowsunpack::game_params::keys::ComponentType, Vec<String>)>,
}

impl ArmorPane {
    pub fn empty(id: u64) -> Self {
        Self::with_defaults(id, &ArmorViewerDefaults::default())
    }

    pub fn with_defaults(id: u64, defaults: &ArmorViewerDefaults) -> Self {
        Self {
            id,
            selected_ship: None,
            viewport: Viewport3D::new(),
            loaded_armor: None,
            loading: false,
            load_receiver: None,
            hovered_info: None,
            part_visibility: HashMap::new(),
            hull_visibility: HashMap::new(),
            hull_opaque: defaults.hull_opaque,
            selected_camo: None,
            mesh_triangle_info: Vec::new(),
            hover_highlight: None,
            sidebar_highlight: None,
            plate_visibility: HashMap::new(),
            context_menu_key: None,
            show_waterline: defaults.show_waterline,
            show_plate_edges: defaults.show_plate_edges,
            show_zero_mm: defaults.show_zero_mm,
            armor_opacity: defaults.armor_opacity,
            show_hidden_only: false,
            undo_stack: VisibilityUndoStack::default(),
            show_gaps: false,
            gap_count: 0,
            trajectory_mode: false,
            trajectories: Vec::new(),
            next_trajectory_id: 0,
            ballistic_range: Km::new(10.0),
            waterline_opacity: defaults.waterline_opacity,
            marker_opacity: 1.0,
            comparison_ships_version: 0,
            splash_mode: false,
            splash_result: None,
            splash_mesh_ids: Vec::new(),
            show_splash_boxes: defaults.show_splash_boxes,
            splash_box_mesh_ids: Vec::new(),
            splash_box_labels: Vec::new(),
            splash_box_visibility: HashMap::new(),
            default_hull_all_visible: defaults.hull_all_visible,
            default_armor_all_visible: defaults.armor_all_visible,
            hull_lod: 0,
            hull_mesh_ids: Vec::new(),
            hull_load_receiver: None,
            upgrade_load_receiver: None,
            selected_hull: None,
            selected_modules: HashMap::new(),
            trajectory_world_space: true,
            continue_on_ricochet: false,
        }
    }
}

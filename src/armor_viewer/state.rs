use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use egui_dock::DockState;

use crate::viewport_3d::{ArcballCamera, GpuPipeline, MeshId, Viewport3D};

/// Key identifying a specific plate: (zone, material_name, thickness in tenths of mm).
/// The thickness discriminator ensures highlights stop at plate boundaries.
pub type PlateKey = (String, String, i32);

/// Persisted default display settings for the armor viewer.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ArmorViewerDefaults {
    pub show_plate_edges: bool,
    pub show_waterline: bool,
    pub show_zero_mm: bool,
    pub armor_opacity: f32,
}

impl Default for ArmorViewerDefaults {
    fn default() -> Self {
        Self { show_plate_edges: true, show_waterline: true, show_zero_mm: false, armor_opacity: 1.0 }
    }
}

/// Snapshot of visibility state for undo/redo.
#[derive(Clone)]
pub struct VisibilitySnapshot {
    pub part_visibility: HashMap<(String, String), bool>,
    pub plate_visibility: HashMap<PlateKey, bool>,
}

/// Simple undo/redo stack for visibility changes.
pub struct VisibilityUndoStack {
    undo: Vec<VisibilitySnapshot>,
    redo: Vec<VisibilitySnapshot>,
}

impl Default for VisibilityUndoStack {
    fn default() -> Self {
        Self { undo: Vec::new(), redo: Vec::new() }
    }
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
pub enum ShipAssetsState {
    NotLoaded,
    Loading(Receiver<Result<Arc<wowsunpack::export::ship::ShipAssets>, String>>),
    Loaded(Arc<wowsunpack::export::ship::ShipAssets>),
    Failed(String),
}

impl Default for ShipAssetsState {
    fn default() -> Self {
        Self::NotLoaded
    }
}

/// Settings to clone into a new pane for comparison.
pub struct CompareSettings {
    pub ship_param_index: String,
    pub ship_display_name: String,
    pub camera: ArcballCamera,
    pub part_visibility: HashMap<(String, String), bool>,
    pub hull_visibility: HashMap<String, bool>,
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
            mirror_cameras: false,
            sync_options: false,
            selector_search: String::new(),
            prev_selector_search: String::new(),
            active_pane_id: 0,
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
    pub bounds: ([f32; 3], [f32; 3]),
    pub zones: Vec<String>,
    /// Ordered mapping: zone name -> sorted list of unique material names in that zone.
    pub zone_parts: Vec<(String, Vec<String>)>,
    /// Three-level hierarchy: zone -> materials -> sorted unique plate thicknesses (i32, tenths of mm).
    pub zone_part_plates: Vec<(String, Vec<(String, Vec<i32>)>)>,
    /// Hull visual meshes (render sets) for optional overlay display.
    pub hull_meshes: Vec<wowsunpack::export::gltf_export::InteractiveHullMesh>,
    /// Hull parts grouped by category (e.g. "Hull", "Main Battery"), each with sorted part names.
    pub hull_part_groups: Vec<(String, Vec<String>)>,
    /// Ship draft (depth below waterline) in meters, from the hull component.
    pub draft_meters: Option<f32>,
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
    /// Whether to show non-armor hull parts (future).
    pub show_hull: bool,
    /// Selected camouflage (future).
    pub selected_camo: Option<String>,
    /// Maps MeshId -> per-triangle tooltip data for picking.
    pub mesh_triangle_info: Vec<(MeshId, Vec<ArmorTriangleTooltip>)>,
    /// Hover highlight: plate key (zone, material_name, thickness_mm rounded) and its overlay mesh.
    pub hover_highlight: Option<(PlateKey, MeshId)>,
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
            show_hull: false,
            selected_camo: None,
            mesh_triangle_info: Vec::new(),
            hover_highlight: None,
            plate_visibility: HashMap::new(),
            context_menu_key: None,
            show_waterline: defaults.show_waterline,
            show_plate_edges: defaults.show_plate_edges,
            show_zero_mm: defaults.show_zero_mm,
            armor_opacity: defaults.armor_opacity,
            show_hidden_only: false,
            undo_stack: VisibilityUndoStack::default(),
        }
    }
}

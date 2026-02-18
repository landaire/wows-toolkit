use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use crate::armor_viewer::split_pane::SplitNode;
use crate::viewport_3d::{GpuPipeline, MeshId, Viewport3D};

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

/// Top-level state for the Armor Viewer tab.
#[allow(dead_code)]
pub struct ArmorViewerState {
    /// The recursive split-pane tree. Each leaf is a single armor pane.
    pub split_tree: SplitNode,
    /// Cached ship catalog, built once from GameParams.
    pub ship_catalog: Option<Arc<crate::armor_viewer::ship_selector::ShipCatalog>>,
    /// Shared ShipAssets handle (expensive, created once on first use).
    pub ship_assets: ShipAssetsState,
    /// GPU pipeline shared across all panes.
    pub gpu_pipeline: Option<Arc<GpuPipeline>>,
    /// Counter for generating unique pane IDs.
    pub next_pane_id: u64,
    /// Cached nation flag textures.
    pub nation_flag_textures: HashMap<String, egui::TextureHandle>,
    /// When true, all split panes share the same camera.
    pub mirror_cameras: bool,
    /// When true, armor/hull visibility is synced across all panes.
    pub sync_options: bool,
    /// Shared ship selector search text (single sidebar).
    pub selector_search: String,
    /// ID of the pane that receives ship selections from the sidebar.
    pub active_pane_id: u64,
}

impl Default for ArmorViewerState {
    fn default() -> Self {
        Self {
            split_tree: SplitNode::Leaf(ArmorPane::empty(0)),
            ship_catalog: None,
            ship_assets: ShipAssetsState::default(),
            gpu_pipeline: None,
            next_pane_id: 1,
            nation_flag_textures: HashMap::new(),
            mirror_cameras: false,
            sync_options: false,
            selector_search: String::new(),
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
}

/// Per-triangle metadata from the armor mesh, for tooltip display.
#[derive(Clone, Debug)]
pub struct ArmorTriangleTooltip {
    pub model_index: u32,
    pub triangle_index: u32,
    pub material_id: u8,
    pub material_name: String,
    pub zone: String,
    pub thickness_mm: f32,
    pub color: [f32; 4],
}

/// Data for a loaded ship's armor.
#[allow(dead_code)]
pub struct LoadedShipArmor {
    pub ship_name: String,
    pub display_name: String,
    pub meshes: Vec<wowsunpack::export::gltf_export::InteractiveArmorMesh>,
    pub bounds: ([f32; 3], [f32; 3]),
    pub zones: Vec<String>,
    /// Ordered mapping: zone name -> sorted list of unique material names in that zone.
    pub zone_parts: Vec<(String, Vec<String>)>,
    /// Hull visual meshes (render sets) for optional overlay display.
    pub hull_meshes: Vec<wowsunpack::export::gltf_export::InteractiveHullMesh>,
    /// Sorted unique hull render set names.
    pub hull_part_names: Vec<String>,
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
    /// Hover highlight: subcomponent key and its overlay mesh.
    pub hover_highlight: Option<((String, String), MeshId)>,
    /// Pinned (clicked) highlights: subcomponent key -> overlay mesh ID.
    pub pinned_highlights: HashMap<(String, String), MeshId>,
}

impl ArmorPane {
    pub fn empty(id: u64) -> Self {
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
            pinned_highlights: HashMap::new(),
        }
    }
}

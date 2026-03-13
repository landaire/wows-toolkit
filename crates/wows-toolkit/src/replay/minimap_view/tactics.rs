use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;

use egui::Color32;
use egui::CornerRadius;
use egui::FontId;
use egui::Pos2;
use egui::Rect;
use egui::Stroke;
use egui::TextureHandle;
use egui::Vec2;
use parking_lot::Mutex;

use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::map_data::MapInfo;
use wowsunpack::game_params::types::BigWorldDistance;
use wowsunpack::game_params::types::GameParamProvider;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_types::WorldPos;

use crate::collab;
use crate::collab::peer::LocalCapPointEvent;
use crate::collab::peer::LocalEvent;
use crate::collab::protocol::WireCapPoint;
use crate::data::cap_layout::CapLayout;
use crate::data::cap_layout::CapLayoutDb;
use crate::data::cap_layout::CapLayoutKey;
use crate::data::cap_layout::CapPointLayout;
use crate::data::wows_data::SharedWoWsData;
use crate::replay::minimap_view::collab_annotation_to_local;
use crate::replay::minimap_view::get_my_user_id;
use crate::replay::minimap_view::handle_map_click_ping;
use crate::replay::minimap_view::send_annotation_clear;
use crate::replay::minimap_view::send_annotation_full_sync;
use crate::replay::minimap_view::send_annotation_remove;
use crate::replay::minimap_view::send_annotation_update;
use crate::replay::renderer::RendererAssetCache;

use super::Annotation;
use super::AnnotationState;
use super::MapTransform;
use super::PaintTool;
use super::ViewportZoomPan;
use super::shapes::CapPointView;
use super::shapes::GridStyle;
use super::shapes::MapPing;
use super::shapes::PING_DURATION;
use super::shapes::ZoomPanConfig;
use super::shapes::annotation_cursor_icon;
use super::shapes::annotation_screen_bounds;
use super::shapes::compute_canvas_layout;
use super::shapes::compute_map_clip_rect;
use super::shapes::draw_annotation_edit_popup;
use super::shapes::draw_grid;
use super::shapes::draw_map_background;
use super::shapes::draw_pings;
use super::shapes::draw_remote_cursors;
use super::shapes::draw_shortcut_overlay;
use super::shapes::handle_annotation_select_move;
use super::shapes::handle_tool_interaction;
use super::shapes::handle_tool_shortcuts;
use super::shapes::handle_viewport_zoom_pan;
use super::shapes::render_annotation;
use super::shapes::render_cap_point;
use super::shapes::render_selection_highlight;
use super::shapes::render_tool_preview;
use super::shapes::tool_label;
/// Selection highlight color for cap points (desktop-only).
const CAP_SELECTED_COLOR: Color32 = Color32::from_rgb(255, 220, 50);

/// How close to the edge (in screen pixels) a click must be to start a resize drag.
const RESIZE_HANDLE_TOLERANCE: f32 = 8.0;
/// Default radius for newly added cap points (in BigWorld units).
/// Typical cap circles are ~5km = ~167 BW units; 150 is a sensible default.
const DEFAULT_CAP_RADIUS: f32 = 150.0;
/// Serializable ship configuration for presets (mirrors [`super::AnnotationShipConfig`]).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct PresetShipConfig {
    pub param_id: u64,
    pub ship_name: String,
    #[serde(default)]
    pub hull_name: String,
    #[serde(default = "default_one")]
    pub vis_coeff: f32,
    #[serde(default = "default_one")]
    pub gm_coeff: f32,
    #[serde(default = "default_one")]
    pub gs_coeff: f32,
    #[serde(default)]
    pub range_filter: PresetRangeFilter,
}

/// Serializable range filter for presets.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct PresetRangeFilter {
    #[serde(default)]
    pub detection: bool,
    #[serde(default)]
    pub main_battery: bool,
    #[serde(default)]
    pub secondary_battery: bool,
    #[serde(default)]
    pub torpedo: bool,
    #[serde(default)]
    pub radar: bool,
    #[serde(default)]
    pub hydro: bool,
}

fn default_one() -> f32 {
    1.0
}

/// A serializable annotation (mirrors [`Annotation`] but with plain types).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
enum PresetAnnotation {
    Ship {
        pos: [f32; 2],
        yaw: f32,
        species: String,
        friendly: bool,
        #[serde(default)]
        config: Option<PresetShipConfig>,
    },
    FreehandStroke {
        points: Vec<[f32; 2]>,
        color: [u8; 4],
        width: f32,
    },
    Line {
        start: [f32; 2],
        end: [f32; 2],
        color: [u8; 4],
        width: f32,
    },
    Circle {
        center: [f32; 2],
        radius: f32,
        color: [u8; 4],
        width: f32,
        filled: bool,
    },
    Rectangle {
        center: [f32; 2],
        half_size: [f32; 2],
        rotation: f32,
        color: [u8; 4],
        width: f32,
        filled: bool,
    },
    Triangle {
        center: [f32; 2],
        radius: f32,
        rotation: f32,
        color: [u8; 4],
        width: f32,
        filled: bool,
    },
    Arrow {
        points: Vec<[f32; 2]>,
        color: [u8; 4],
        width: f32,
    },
    Measurement {
        start: [f32; 2],
        end: [f32; 2],
        color: [u8; 4],
        width: f32,
    },
}

impl PresetAnnotation {
    fn from_annotation(ann: &Annotation) -> Self {
        match ann {
            Annotation::Ship { pos, yaw, species, friendly, config } => PresetAnnotation::Ship {
                pos: [pos.x, pos.y],
                yaw: *yaw,
                species: species.clone(),
                friendly: *friendly,
                config: config.as_ref().map(|c| PresetShipConfig {
                    param_id: c.param_id,
                    ship_name: c.ship_name.clone(),
                    hull_name: c.hull_name.clone(),
                    vis_coeff: c.vis_coeff,
                    gm_coeff: c.gm_coeff,
                    gs_coeff: c.gs_coeff,
                    range_filter: PresetRangeFilter {
                        detection: c.range_filter.detection,
                        main_battery: c.range_filter.main_battery,
                        secondary_battery: c.range_filter.secondary_battery,
                        torpedo: c.range_filter.torpedo,
                        radar: c.range_filter.radar,
                        hydro: c.range_filter.hydro,
                    },
                }),
            },
            Annotation::FreehandStroke { points, color, width } => PresetAnnotation::FreehandStroke {
                points: points.iter().map(|p| [p.x, p.y]).collect(),
                color: color.to_array(),
                width: *width,
            },
            Annotation::Line { start, end, color, width } => PresetAnnotation::Line {
                start: [start.x, start.y],
                end: [end.x, end.y],
                color: color.to_array(),
                width: *width,
            },
            Annotation::Circle { center, radius, color, width, filled } => PresetAnnotation::Circle {
                center: [center.x, center.y],
                radius: *radius,
                color: color.to_array(),
                width: *width,
                filled: *filled,
            },
            Annotation::Rectangle { center, half_size, rotation, color, width, filled } => {
                PresetAnnotation::Rectangle {
                    center: [center.x, center.y],
                    half_size: [half_size.x, half_size.y],
                    rotation: *rotation,
                    color: color.to_array(),
                    width: *width,
                    filled: *filled,
                }
            }
            Annotation::Triangle { center, radius, rotation, color, width, filled } => PresetAnnotation::Triangle {
                center: [center.x, center.y],
                radius: *radius,
                rotation: *rotation,
                color: color.to_array(),
                width: *width,
                filled: *filled,
            },
            Annotation::Arrow { points, color, width } => PresetAnnotation::Arrow {
                points: points.iter().map(|p| [p.x, p.y]).collect(),
                color: color.to_array(),
                width: *width,
            },
            Annotation::Measurement { start, end, color, width } => PresetAnnotation::Measurement {
                start: [start.x, start.y],
                end: [end.x, end.y],
                color: color.to_array(),
                width: *width,
            },
        }
    }

    fn to_annotation(&self) -> Annotation {
        match self {
            PresetAnnotation::Ship { pos, yaw, species, friendly, config } => Annotation::Ship {
                pos: Vec2::new(pos[0], pos[1]),
                yaw: *yaw,
                species: species.clone(),
                friendly: *friendly,
                config: config.as_ref().map(|c| super::AnnotationShipConfig {
                    param_id: c.param_id,
                    ship_name: c.ship_name.clone(),
                    hull_name: c.hull_name.clone(),
                    vis_coeff: c.vis_coeff,
                    gm_coeff: c.gm_coeff,
                    gs_coeff: c.gs_coeff,
                    range_filter: super::AnnotationRangeFilter {
                        detection: c.range_filter.detection,
                        main_battery: c.range_filter.main_battery,
                        secondary_battery: c.range_filter.secondary_battery,
                        torpedo: c.range_filter.torpedo,
                        radar: c.range_filter.radar,
                        hydro: c.range_filter.hydro,
                    },
                }),
            },
            PresetAnnotation::FreehandStroke { points, color, width } => Annotation::FreehandStroke {
                points: points.iter().map(|p| Vec2::new(p[0], p[1])).collect(),
                color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
                width: *width,
            },
            PresetAnnotation::Line { start, end, color, width } => Annotation::Line {
                start: Vec2::new(start[0], start[1]),
                end: Vec2::new(end[0], end[1]),
                color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
                width: *width,
            },
            PresetAnnotation::Circle { center, radius, color, width, filled } => Annotation::Circle {
                center: Vec2::new(center[0], center[1]),
                radius: *radius,
                color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
                width: *width,
                filled: *filled,
            },
            PresetAnnotation::Rectangle { center, half_size, rotation, color, width, filled } => {
                Annotation::Rectangle {
                    center: Vec2::new(center[0], center[1]),
                    half_size: Vec2::new(half_size[0], half_size[1]),
                    rotation: *rotation,
                    color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
                    width: *width,
                    filled: *filled,
                }
            }
            PresetAnnotation::Triangle { center, radius, rotation, color, width, filled } => Annotation::Triangle {
                center: Vec2::new(center[0], center[1]),
                radius: *radius,
                rotation: *rotation,
                color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
                width: *width,
                filled: *filled,
            },
            PresetAnnotation::Arrow { points, color, width } => Annotation::Arrow {
                points: points.iter().map(|p| Vec2::new(p[0], p[1])).collect(),
                color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
                width: *width,
            },
            PresetAnnotation::Measurement { start, end, color, width } => Annotation::Measurement {
                start: Vec2::new(start[0], start[1]),
                end: Vec2::new(end[0], end[1]),
                color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
                width: *width,
            },
        }
    }
}

/// A serializable cap point for presets.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct PresetCapPoint {
    index: usize,
    world_x: f32,
    world_z: f32,
    radius: f32,
    team_id: i64,
    #[serde(default)]
    frozen: bool,
}

/// A saved tactics board preset.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct TacticsPreset {
    /// User-chosen preset name.
    pub name: String,
    /// Map name (e.g. "spaces/16_OC_bees_to_honey").
    pub map_name: String,
    /// Map ID.
    pub map_id: u32,
    /// Cap points.
    cap_points: Vec<PresetCapPoint>,
    /// Annotations.
    annotations: Vec<PresetAnnotation>,
}

/// Get the presets directory, creating it if needed.
fn presets_dir() -> Option<PathBuf> {
    let storage = crate::storage_dir()?;
    let dir = storage.join("tactics_presets");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// List all saved preset names (without extension).
fn list_preset_names() -> Vec<String> {
    let Some(dir) = presets_dir() else { return vec![] };
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    let mut names: Vec<String> = entries
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".json").map(|s| s.to_string())
        })
        .collect();
    names.sort();
    names
}

/// Save a preset to disk.
fn save_preset(preset: &TacticsPreset) -> Result<(), String> {
    let dir = presets_dir().ok_or("no storage dir")?;
    let path = dir.join(format!("{}.json", &preset.name));
    let json = serde_json::to_string_pretty(preset).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

/// Load a preset from disk.
fn load_preset(name: &str) -> Option<TacticsPreset> {
    let dir = presets_dir()?;
    let path = dir.join(format!("{name}.json"));
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Delete a preset from disk.
fn delete_preset(name: &str) {
    if let Some(dir) = presets_dir() {
        let _ = std::fs::remove_file(dir.join(format!("{name}.json")));
    }
}
#[derive(Clone, Copy, Debug)]
enum CapDragMode {
    Move,
    Resize,
}
/// A capture point on the tactics board (editable copy of [`CapPointLayout`]).
#[derive(Clone, Debug)]
pub struct TacticsCapPoint {
    /// Unique ID for selection and (future) collab sync.
    pub id: u64,
    /// Cap point label index: A=0, B=1, C=2, …
    pub index: usize,
    /// World-space position (BigWorld X/Z).
    pub world_x: f32,
    pub world_z: f32,
    /// Zone radius in BigWorld units.
    pub radius: f32,
    /// Team that owns this cap at start. -1 = neutral.
    pub team_id: i64,
    /// Whether this cap is frozen (loaded from replay data, not user-editable).
    pub frozen: bool,
}

impl TacticsCapPoint {
    fn from_layout(layout: &CapPointLayout, id: u64) -> Self {
        Self {
            id,
            index: layout.index,
            world_x: layout.position.x,
            world_z: layout.position.z,
            radius: layout.radius.value(),
            team_id: layout.team_id,
            frozen: true,
        }
    }

    /// Convert to the rendering view (only the fields needed to draw).
    pub fn view(&self) -> CapPointView {
        CapPointView {
            world_x: self.world_x,
            world_z: self.world_z,
            radius: self.radius,
            team_id: self.team_id,
            index: self.index as u32,
        }
    }

    /// Convert to the wire protocol representation.
    pub fn to_wire(&self) -> WireCapPoint {
        WireCapPoint {
            id: self.id,
            index: self.index as u32,
            world_x: self.world_x,
            world_z: self.world_z,
            radius: self.radius,
            team_id: self.team_id,
            frozen: self.frozen,
        }
    }
}
/// Per-viewport state for the tactics board.
pub struct TacticsBoardState {
    /// Currently selected map (map_id, map_name).
    selected_map: Option<(u32, String)>,
    /// Currently selected mode key (or None for blank).
    selected_mode: Option<CapLayoutKey>,
    /// Human-readable label for the selected mode (for dynamic window title).
    selected_mode_label: Option<String>,
    /// Editable cap points on the board.
    cap_points: Vec<TacticsCapPoint>,
    /// Next unique ID for new cap points.
    next_cap_id: u64,
    /// Loaded map image RGBA data.
    map_image: Option<Arc<crate::replay::renderer::RgbaAsset>>,
    /// MapInfo for coordinate transforms.
    map_info: Option<MapInfo>,
    /// Uploaded map texture (egui handle).
    map_texture: Option<TextureHandle>,
    /// Whether we need to re-upload the texture.
    texture_dirty: bool,
    /// Uploaded ship icon textures (keyed by species name).
    ship_icons: Option<HashMap<String, TextureHandle>>,

    // ── Interaction state ──
    /// Currently selected cap point ID.
    selected_cap: Option<u64>,
    /// Active drag: (cap_id, drag_mode).
    dragging_cap: Option<(u64, CapDragMode)>,
    /// Whether the "Add Cap" tool is active.
    adding_cap: bool,

    // ── Preset state ──
    /// Name for saving a new preset.
    preset_name: String,
    /// Cached list of saved preset names.
    preset_names: Vec<String>,

    // ── Replay scan state ──
    /// Background replay scan progress: (processed, total). None if not scanning.
    scan_progress: Option<Arc<Mutex<(usize, usize)>>>,

    // ── Click ripples ──
    /// Local click ripple animations.
    pings: Vec<MapPing>,
    // ── Collab sync state ──
    /// Version of annotation sync last applied from the collab session.
    applied_annotation_sync_version: u64,
    /// Version of cap point sync last applied from the collab session.
    applied_cap_sync_version: u64,
    /// Version of tactics map last applied from the collab session.
    applied_tactics_map_version: u64,

    // ── Ship annotation config state ──
    /// Lazily built ship catalog for annotation ship search.
    ship_catalog: Option<crate::armor_viewer::ship_selector::ShipCatalog>,
    /// Current search text for ship assignment.
    ship_search_text: String,
}

impl TacticsBoardState {
    /// Reset collab sync version counters so a new session starting at
    /// version 0 will be picked up correctly.
    pub fn reset_applied_sync_versions(&mut self) {
        self.applied_annotation_sync_version = 0;
        self.applied_cap_sync_version = 0;
        self.applied_tactics_map_version = 0;
    }
}

impl Default for TacticsBoardState {
    fn default() -> Self {
        Self {
            selected_map: None,
            selected_mode: None,
            selected_mode_label: None,
            cap_points: Vec::new(),
            next_cap_id: 1,
            map_image: None,
            map_info: None,
            map_texture: None,
            texture_dirty: false,
            ship_icons: None,
            selected_cap: None,
            dragging_cap: None,
            adding_cap: false,
            preset_name: String::new(),
            preset_names: list_preset_names(),
            scan_progress: None,
            pings: Vec::new(),
            applied_annotation_sync_version: 0,
            applied_cap_sync_version: 0,
            applied_tactics_map_version: 0,
            ship_catalog: None,
            ship_search_text: String::new(),
        }
    }
}

impl TacticsBoardState {
    /// Access the selected map (id, name).
    pub fn selected_map(&self) -> Option<(u32, &str)> {
        self.selected_map.as_ref().map(|(id, name)| (*id, name.as_str()))
    }

    /// Access raw map image data for PNG encoding.
    pub fn map_image_raw(&self) -> Option<&crate::replay::renderer::RgbaAsset> {
        self.map_image.as_deref()
    }

    /// Access the cap points.
    pub fn cap_points(&self) -> &[TacticsCapPoint] {
        &self.cap_points
    }

    /// Access the map info (coordinate metadata).
    pub fn map_info(&self) -> Option<&MapInfo> {
        self.map_info.as_ref()
    }
}
/// A tactics board viewport. Created from the session popover or standalone.
pub struct TacticsBoardViewer {
    /// Unique board identifier (random u64).
    pub board_id: u64,
    /// User ID of the board creator (informational, for popover grouping).
    pub owner_user_id: u64,
    /// Whether this board is synced via the collab session.
    pub is_session_board: bool,
    pub open: Arc<AtomicBool>,

    // Shared types
    zoom_pan: Arc<Mutex<ViewportZoomPan>>,
    annotation_state: Arc<Mutex<AnnotationState>>,

    // Tactics-specific
    state: Arc<Mutex<TacticsBoardState>>,

    // External references
    cap_layout_db: Arc<Mutex<CapLayoutDb>>,
    asset_cache: Arc<Mutex<RendererAssetCache>>,
    wows_data: SharedWoWsData,

    // Database
    db_pool: Option<sqlx::SqlitePool>,
    tokio_runtime: Option<Arc<tokio::runtime::Runtime>>,
    window_settings: crate::tab_state::SharedWindowSettings,

    // Collab
    /// Channel to send local UI events (cursors, annotations, pings) to the collab peer task.
    pub collab_local_tx: Option<mpsc::Sender<LocalEvent>>,
    /// Shared collab session state (read each frame for incoming sync).
    pub collab_session_state: Option<Arc<Mutex<collab::SessionState>>>,
    /// Channel to send session commands (full annotation sync, etc.).
    pub collab_command_tx: Option<mpsc::Sender<collab::SessionCommand>>,
}

impl TacticsBoardViewer {
    /// Create a new tactics board viewer.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        board_id: u64,
        owner_user_id: u64,
        cap_layout_db: Arc<Mutex<CapLayoutDb>>,
        asset_cache: Arc<Mutex<RendererAssetCache>>,
        wows_data: SharedWoWsData,
        db_pool: Option<sqlx::SqlitePool>,
        tokio_runtime: Option<Arc<tokio::runtime::Runtime>>,
        window_settings: crate::tab_state::SharedWindowSettings,
    ) -> Self {
        Self {
            board_id,
            owner_user_id,
            is_session_board: false,
            open: Arc::new(AtomicBool::new(true)),
            zoom_pan: Arc::new(Mutex::new(ViewportZoomPan::default())),
            annotation_state: Arc::new(Mutex::new(AnnotationState::default())),
            state: Arc::new(Mutex::new(TacticsBoardState::default())),
            cap_layout_db,
            asset_cache,
            wows_data,
            db_pool,
            tokio_runtime,
            window_settings,
            collab_local_tx: None,
            collab_session_state: None,
            collab_command_tx: None,
        }
    }

    /// Access the shared tactics board state.
    pub fn state_arc(&self) -> &Arc<Mutex<TacticsBoardState>> {
        &self.state
    }

    /// Access the shared annotation state.
    pub fn annotation_state_arc(&self) -> &Arc<Mutex<AnnotationState>> {
        &self.annotation_state
    }

    /// Draw the tactics board viewport as a deferred viewport (separate OS window).
    /// The [`egui::ViewportId`] used by this viewer's deferred viewport.
    pub fn viewport_id(&self) -> egui::ViewportId {
        egui::ViewportId::from_hash_of(("tactics_board", self.board_id))
    }

    pub fn draw(&self, ctx: &egui::Context) {
        let open = self.open.clone();
        let board_id = self.board_id;
        let zoom_pan_arc = self.zoom_pan.clone();
        let annotation_state_arc = self.annotation_state.clone();
        let state_arc = self.state.clone();
        let cap_layout_db = self.cap_layout_db.clone();
        let asset_cache = self.asset_cache.clone();
        let wows_data = self.wows_data.clone();
        let db_pool = self.db_pool.clone();
        let tokio_runtime = self.tokio_runtime.clone();
        let window_settings = self.window_settings.clone();
        let collab_local_tx = self.collab_local_tx.clone();
        let collab_session_state = self.collab_session_state.clone();
        let collab_command_tx = self.collab_command_tx.clone();

        let viewport_id = egui::ViewportId::from_hash_of(("tactics_board", self.board_id));

        // Register this viewport for targeted repaints from the peer task.
        if let Some(ref session_state) = self.collab_session_state {
            let mut s = session_state.lock();
            s.viewport_sinks
                .entry(self.board_id)
                .or_insert_with(|| crate::collab::ViewportSink { frame_tx: None, viewport_id });
        }

        // Apply persisted window size if available.
        let builder = egui::ViewportBuilder::default().with_title("Tactics Board").with_min_inner_size([400.0, 450.0]);
        let builder = window_settings
            .lock()
            .settings
            .get(&crate::tab_state::WindowKind::TacticsBoard)
            .map(|s| s.apply_to_builder(builder.clone(), [800.0, 850.0]))
            .unwrap_or_else(|| builder.with_inner_size([800.0, 850.0]));

        ctx.show_viewport_deferred(viewport_id, builder, move |viewport_ui, _class| {
            let ctx = viewport_ui.ctx().clone();
            if !open.load(Ordering::Relaxed) || crate::app::mitigate_wgpu_mem_leak(&ctx) {
                return;
            }

            let mut state = state_arc.lock();

            // ── Collab: apply incoming annotation + cap point sync ──
            if let Some(session_state) = &collab_session_state {
                let s = session_state.lock();
                if let Some(board_state) = s.tactics_boards.get(&board_id) {
                    if board_state.annotation_sync_version > state.applied_annotation_sync_version {
                        let sync = &board_state.annotation_sync;
                        let mut ann = annotation_state_arc.lock();
                        ann.annotations = sync.annotations.iter().cloned().map(collab_annotation_to_local).collect();
                        ann.annotation_owners = sync.owners.clone();
                        ann.annotation_ids = sync.ids.clone();
                        state.applied_annotation_sync_version = board_state.annotation_sync_version;
                    }
                    if board_state.cap_point_sync_version > state.applied_cap_sync_version {
                        tracing::debug!(
                            "Applying cap sync: version {} -> {}, sync_data={}",
                            state.applied_cap_sync_version,
                            board_state.cap_point_sync_version,
                            board_state.cap_point_sync.cap_points.len(),
                        );
                        state.cap_points = board_state
                            .cap_point_sync
                            .cap_points
                            .iter()
                            .map(|wcp| TacticsCapPoint {
                                id: wcp.id,
                                index: wcp.index as usize,
                                world_x: wcp.world_x,
                                world_z: wcp.world_z,
                                radius: wcp.radius,
                                team_id: wcp.team_id,
                                frozen: wcp.frozen,
                            })
                            .collect();
                        state.next_cap_id = state.cap_points.iter().map(|c| c.id).max().unwrap_or(0) + 1;
                        state.selected_cap = None;
                        state.dragging_cap = None;
                        state.applied_cap_sync_version = board_state.cap_point_sync_version;
                    }
                    if s.tactics_boards_version > state.applied_tactics_map_version {
                        let tmap = &board_state.tactics_map;
                        let map_changed = state.selected_map.as_ref().is_none_or(|(id, _)| *id != tmap.map_id);
                        tracing::debug!(
                            "Applying tactics map sync: version {} -> {}, map={}, changed={}",
                            state.applied_tactics_map_version,
                            s.tactics_boards_version,
                            tmap.map_name,
                            map_changed,
                        );
                        if map_changed {
                            state.selected_map = Some((tmap.map_id, tmap.map_name.clone()));
                            state.selected_mode = None;
                            state.selected_mode_label = None;
                            // Try loading map from local VFS first; fall back to decoding peer's PNG.
                            load_map_image(&mut state, &tmap.map_name, &asset_cache, &wows_data);
                            tracing::debug!(
                                "After load_map_image: map_image={}, map_info={}",
                                state.map_image.is_some(),
                                state.map_info.is_some(),
                            );
                            // Fall back to peer-provided map_info if VFS didn't provide it.
                            if state.map_info.is_none() {
                                state.map_info = tmap.map_info.clone();
                            }
                            if state.map_image.is_none()
                                && !tmap.map_image_png.is_empty()
                                && let Ok(img) = image::load_from_memory(&tmap.map_image_png)
                            {
                                let rgba = img.into_rgba8();
                                let (w, h) = (rgba.width(), rgba.height());
                                state.map_image = Some(Arc::new(crate::replay::renderer::RgbaAsset {
                                    data: rgba.into_raw(),
                                    width: w,
                                    height: h,
                                }));
                                state.texture_dirty = true;
                            }
                        }
                        state.applied_tactics_map_version = s.tactics_boards_version;
                    }
                }
                drop(s);
            }

            // ── Dynamic window title ──
            {
                let mut title = String::from("Tactics Board");
                if let Some((_, ref map_name)) = state.selected_map {
                    title.push_str(" \u{2014} ");
                    title.push_str(&translate_or_pretty(map_name, &wows_data));
                    if let Some(ref label) = state.selected_mode_label {
                        title.push_str(" \u{2014} ");
                        title.push_str(label);
                    }
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
                // Store in session state so the popover shows the exact same title.
                if let Some(ref session_state) = collab_session_state {
                    let mut s = session_state.lock();
                    if let Some(board) = s.tactics_boards.get_mut(&board_id)
                        && board.window_title != title
                    {
                        board.window_title = title;
                    }
                }
            }

            // ── Bottom panel: map/mode selector + cap tools + presets ──
            // Hide for regular peers (non-host, non-co-host) in a collab session.
            let is_authority = collab_session_state
                .as_ref()
                .map(|ss| {
                    let s = ss.lock();
                    s.role.is_host() || s.role.is_co_host()
                })
                .unwrap_or(true); // No session -> standalone, show everything.

            if is_authority {
                egui::Panel::bottom("tactics_bottom_panel").show_inside(viewport_ui, |ui| {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        Self::draw_map_mode_selector(
                            ui,
                            &mut state,
                            &cap_layout_db,
                            &asset_cache,
                            &wows_data,
                            &collab_local_tx,
                            &collab_command_tx,
                            board_id,
                        );
                    });
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        Self::draw_preset_controls(
                            ui,
                            &mut state,
                            &annotation_state_arc,
                            &asset_cache,
                            &wows_data,
                            &cap_layout_db,
                            &collab_command_tx,
                            board_id,
                        );
                        ui.separator();
                        Self::draw_scan_replays_button(
                            ui,
                            &mut state,
                            &cap_layout_db,
                            &wows_data,
                            &db_pool,
                            &tokio_runtime,
                        );
                    });
                    ui.add_space(4.0);
                });
            } // is_authority

            // ── Annotation toolbar ──
            egui::Panel::top("tactics_annotation_toolbar").show_inside(viewport_ui, |ui| {
                let locked =
                    collab_session_state.as_ref().map(|ss| ss.lock().permissions.annotations_locked).unwrap_or(false);
                let mut ann = annotation_state_arc.lock();
                let result =
                    wt_collab_egui::toolbar::draw_annotation_toolbar(ui, &mut ann, state.ship_icons.as_ref(), locked);
                if result.did_clear {
                    send_annotation_clear(&collab_local_tx, Some(board_id));
                }
                if result.did_undo {
                    drop(ann);
                    send_annotation_full_sync(&collab_command_tx, &annotation_state_arc.lock(), Some(board_id));
                }
            });

            // ── Central panel: map viewport ──
            egui::CentralPanel::default().show_inside(viewport_ui, |ui| {
                Self::draw_map_viewport(
                    ui,
                    &mut state,
                    &zoom_pan_arc,
                    &annotation_state_arc,
                    &asset_cache,
                    &wows_data,
                    &collab_local_tx,
                    &collab_session_state,
                    &collab_command_tx,
                    board_id,
                );
            });

            if ctx.input(|i| i.viewport().close_requested()) {
                open.store(false, Ordering::Relaxed);
                // Unregister viewport sink.
                if let Some(ref session_state) = collab_session_state {
                    session_state.lock().viewport_sinks.remove(&board_id);
                }
                ctx.request_repaint();
            }
        });
    }

    /// Draw the map/mode selector and cap tools in the bottom panel.
    #[allow(clippy::too_many_arguments)]
    fn draw_map_mode_selector(
        ui: &mut egui::Ui,
        state: &mut TacticsBoardState,
        cap_layout_db: &Arc<Mutex<CapLayoutDb>>,
        asset_cache: &Arc<Mutex<RendererAssetCache>>,
        wows_data: &SharedWoWsData,
        collab_local_tx: &Option<mpsc::Sender<LocalEvent>>,
        collab_command_tx: &Option<mpsc::Sender<collab::SessionCommand>>,
        board_id: u64,
    ) {
        let db = cap_layout_db.lock();

        // Build merged map list: cap layout DB maps + VFS-discovered maps.
        let mut maps = db.maps(); // Vec<(map_id, map_name)>
        {
            let wdata = wows_data.read();
            if let Ok(spaces) = wdata.vfs.join("spaces")
                && let Ok(entries) = spaces.read_dir()
            {
                for entry in entries {
                    if entry.is_dir().unwrap_or(false) {
                        let dir_name = entry.filename();
                        // Skip dock/port scenes — not playable maps.
                        if dir_name.starts_with("Dock") {
                            continue;
                        }
                        let map_name = format!("spaces/{dir_name}");
                        // Check this map has a minimap image.
                        let has_minimap = entry
                            .join("minimap.png")
                            .map(|p| p.exists().unwrap_or(false))
                            .unwrap_or(false)
                            || entry.join("minimap_water.png").map(|p| p.exists().unwrap_or(false)).unwrap_or(false);
                        if has_minimap && !maps.iter().any(|(_, name)| *name == map_name) {
                            maps.push((0, map_name));
                        }
                    }
                }
            }
        }
        maps.sort_by(|a, b| {
            let la = translate_or_pretty(&a.1, wows_data);
            let lb = translate_or_pretty(&b.1, wows_data);
            la.cmp(&lb)
        });

        // Map dropdown
        ui.label("Map:");
        let current_map_label = state
            .selected_map
            .as_ref()
            .map(|(_, name)| translate_or_pretty(name, wows_data))
            .unwrap_or_else(|| "Select map\u{2026}".to_string());

        egui::ComboBox::from_id_salt("tactics_map_select").selected_text(&current_map_label).width(180.0).show_ui(
            ui,
            |ui| {
                for (map_id, map_name) in &maps {
                    let label = translate_or_pretty(map_name, wows_data);
                    let selected = state.selected_map.as_ref().map(|(_, name)| name) == Some(map_name);
                    if ui.selectable_label(selected, &label).clicked() {
                        state.selected_map = Some((*map_id, map_name.clone()));
                        state.selected_mode = None;
                        state.selected_mode_label = None;
                        state.cap_points.clear();
                        state.selected_cap = None;
                        load_map_image(state, map_name, asset_cache, wows_data);
                        send_tactics_map_opened(
                            collab_local_tx,
                            board_id,
                            *map_id,
                            map_name,
                            &label,
                            &state.map_image,
                            &state.map_info,
                        );
                        send_cap_full_sync(collab_command_tx, &state.cap_points, board_id);
                    }
                }
            },
        );

        ui.separator();

        // Mode dropdown (only if a map is selected)
        if let Some((map_id, _)) = state.selected_map.as_ref() {
            let modes = db.modes_for_map(*map_id);

            // Build display labels, disambiguating when multiple layouts
            // share the same base label (e.g. two "Domination - 3 caps"
            // variants with different cap positions).
            let base_labels: Vec<String> = modes.iter().map(|l| pretty_mode_name(l, wows_data)).collect();

            let mut label_count: HashMap<&str, usize> = HashMap::new();
            for lbl in &base_labels {
                *label_count.entry(lbl.as_str()).or_default() += 1;
            }

            let mut variant_counter: HashMap<&str, usize> = HashMap::new();
            let display_labels: Vec<String> = base_labels
                .iter()
                .map(|lbl| {
                    if label_count[lbl.as_str()] > 1 {
                        let n = variant_counter.entry(lbl.as_str()).or_default();
                        *n += 1;
                        format!("{lbl} (variant {n})")
                    } else {
                        lbl.clone()
                    }
                })
                .collect();

            ui.label("Mode:");
            let current_mode_label = state
                .selected_mode
                .as_ref()
                .and_then(|key| modes.iter().position(|l| &l.key == key).map(|idx| display_labels[idx].clone()))
                .unwrap_or_else(|| "Blank".to_string());

            egui::ComboBox::from_id_salt("tactics_mode_select")
                .selected_text(&current_mode_label)
                .width(200.0)
                .show_ui(ui, |ui| {
                    let is_blank = state.selected_mode.is_none();
                    if ui.selectable_label(is_blank, "Blank").clicked() {
                        state.selected_mode = None;
                        state.selected_mode_label = None;
                        state.cap_points.clear();
                        state.selected_cap = None;
                        send_cap_full_sync(collab_command_tx, &state.cap_points, board_id);
                    }

                    for (layout, label) in modes.iter().zip(display_labels.iter()) {
                        let selected = state.selected_mode.as_ref() == Some(&layout.key);
                        if ui.selectable_label(selected, label).clicked() {
                            state.selected_mode = Some(layout.key.clone());
                            state.selected_mode_label = Some(label.clone());
                            state.selected_cap = None;
                            populate_cap_points(state, layout);
                            send_cap_full_sync(collab_command_tx, &state.cap_points, board_id);
                        }
                    }
                });
        }
    }

    /// Draw preset save/load/delete controls.
    #[allow(clippy::too_many_arguments)]
    fn draw_preset_controls(
        ui: &mut egui::Ui,
        state: &mut TacticsBoardState,
        annotation_state_arc: &Arc<Mutex<AnnotationState>>,
        asset_cache: &Arc<Mutex<RendererAssetCache>>,
        wows_data: &SharedWoWsData,
        cap_layout_db: &Arc<Mutex<CapLayoutDb>>,
        collab_command_tx: &Option<mpsc::Sender<collab::SessionCommand>>,
        board_id: u64,
    ) {
        ui.label("Preset:");

        // Preset name input for saving
        let text_response =
            ui.add(egui::TextEdit::singleline(&mut state.preset_name).desired_width(120.0).hint_text("name…"));

        // Save button
        let can_save = !state.preset_name.is_empty() && state.selected_map.is_some();
        let save_btn = ui.add_enabled(can_save, egui::Button::new("Save"));
        if save_btn.clicked()
            && let Some((map_id, ref map_name)) = state.selected_map
        {
            let ann = annotation_state_arc.lock();
            let preset = TacticsPreset {
                name: state.preset_name.clone(),
                map_name: map_name.clone(),
                map_id,
                cap_points: state
                    .cap_points
                    .iter()
                    .map(|c| PresetCapPoint {
                        index: c.index,
                        world_x: c.world_x,
                        world_z: c.world_z,
                        radius: c.radius,
                        team_id: c.team_id,
                        frozen: c.frozen,
                    })
                    .collect(),
                annotations: ann.annotations.iter().map(PresetAnnotation::from_annotation).collect(),
            };
            if let Err(e) = save_preset(&preset) {
                tracing::warn!("failed to save preset: {e}");
            }
            state.preset_names = list_preset_names();
        }

        ui.separator();

        // Load dropdown
        let selected_label = if !state.preset_name.is_empty() && state.preset_names.contains(&state.preset_name) {
            state.preset_name.clone()
        } else {
            "Load Preset\u{2026}".to_string()
        };
        egui::ComboBox::from_id_salt("tactics_preset_load").selected_text(&selected_label).width(120.0).show_ui(
            ui,
            |ui| {
                // "Default" clears annotations and resets caps to the selected mode
                if ui.selectable_label(false, "Default").clicked() {
                    let mut ann = annotation_state_arc.lock();
                    ann.save_undo();
                    ann.annotations.clear();
                    ann.annotation_ids.clear();
                    ann.annotation_owners.clear();
                    ann.clear_selection();
                    drop(ann);
                    // Re-apply caps from the selected mode
                    let layout = state.selected_mode.as_ref().and_then(|key| cap_layout_db.lock().get(key).cloned());
                    if let Some(ref layout) = layout {
                        populate_cap_points(state, layout);
                    } else {
                        state.cap_points.clear();
                    }
                    state.selected_cap = None;
                    state.preset_name.clear();
                    send_cap_full_sync(collab_command_tx, &state.cap_points, board_id);
                    send_annotation_full_sync(collab_command_tx, &annotation_state_arc.lock(), Some(board_id));
                }
                ui.separator();
                let names = state.preset_names.clone();
                if names.is_empty() {
                    ui.add_enabled(
                        false,
                        egui::Label::new(egui::RichText::new("No saved presets").italics().color(Color32::GRAY)),
                    );
                }
                for name in &names {
                    if ui.selectable_label(false, name).clicked()
                        && let Some(preset) = load_preset(name)
                    {
                        // Load map
                        state.selected_map = Some((preset.map_id, preset.map_name.clone()));
                        state.selected_mode = None;
                        state.selected_mode_label = None;
                        load_map_image(state, &preset.map_name, asset_cache, wows_data);

                        // Load cap points
                        state.cap_points.clear();
                        for pc in &preset.cap_points {
                            let id = state.next_cap_id;
                            state.next_cap_id += 1;
                            state.cap_points.push(TacticsCapPoint {
                                id,
                                index: pc.index,
                                world_x: pc.world_x,
                                world_z: pc.world_z,
                                radius: pc.radius,
                                team_id: pc.team_id,
                                frozen: pc.frozen,
                            });
                        }
                        state.selected_cap = None;

                        // Load annotations
                        let mut ann = annotation_state_arc.lock();
                        ann.save_undo();
                        ann.annotations = preset.annotations.iter().map(|a| a.to_annotation()).collect();
                        ann.annotation_ids = (0..ann.annotations.len()).map(|_| rand::random()).collect();
                        ann.annotation_owners = vec![0; ann.annotations.len()];
                        ann.clear_selection();

                        state.preset_name = name.clone();

                        // Sync to peers
                        send_cap_full_sync(collab_command_tx, &state.cap_points, board_id);
                        send_annotation_full_sync(collab_command_tx, &ann, Some(board_id));
                    }
                }
            },
        );

        // Delete button
        let can_delete = !state.preset_name.is_empty() && state.preset_names.contains(&state.preset_name);
        if ui.add_enabled(can_delete, egui::Button::new("Delete")).clicked() {
            delete_preset(&state.preset_name);
            state.preset_names = list_preset_names();
            state.preset_name.clear();
        }

        // Refresh presets list when text field gets focus
        if text_response.gained_focus() {
            state.preset_names = list_preset_names();
        }
    }

    /// Draw the "Scan Replays" button and progress indicator.
    fn draw_scan_replays_button(
        ui: &mut egui::Ui,
        state: &mut TacticsBoardState,
        cap_layout_db: &Arc<Mutex<CapLayoutDb>>,
        wows_data: &SharedWoWsData,
        db_pool: &Option<sqlx::SqlitePool>,
        tokio_runtime: &Option<Arc<tokio::runtime::Runtime>>,
    ) {
        // Check if a scan is in progress and show progress.
        if let Some(ref progress) = state.scan_progress {
            let (processed, total) = *progress.lock();
            if processed >= total && total > 0 {
                // Scan finished — clear progress.
                state.scan_progress = None;

                // If the user has a VFS-only map selected (map_id=0), resolve
                // the real map_id from the newly populated DB so that the mode
                // dropdown can find the corresponding layouts.
                if let Some((ref mut map_id, ref map_name)) = state.selected_map
                    && *map_id == 0
                {
                    let db = cap_layout_db.lock();
                    if let Some(real_id) = db.maps().iter().find(|(_, n)| n == map_name).map(|(id, _)| *id) {
                        *map_id = real_id;
                    }
                }
            } else {
                ui.label(format!("Scanning… {processed}/{total}"));
                ui.spinner();
                ui.ctx().request_repaint();
                return;
            }
        }

        if ui
            .button("Populate Caps from Replays")
            .on_hover_text("Scan replay files to discover capture point layouts for more game modes")
            .clicked()
        {
            let wdata = wows_data.read();
            let replays_dir = wdata.replays_dir.clone();
            let game_metadata = wdata.game_metadata.clone();
            let game_constants = Arc::clone(&wdata.game_constants);
            drop(wdata);

            let Some(gm) = game_metadata else {
                tracing::warn!("cannot scan replays: game metadata not loaded");
                return;
            };

            let db = Arc::clone(cap_layout_db);
            let scan_pool = db_pool.clone();
            let scan_rt = tokio_runtime.clone();
            let progress = Arc::new(Mutex::new((0usize, 0usize)));
            state.scan_progress = Some(Arc::clone(&progress));

            std::thread::Builder::new()
                .name("tactics-replay-scan".into())
                .spawn(move || {
                    let files = crate::task::replays::replay_filepaths(&replays_dir).unwrap_or_default();
                    let total = files.len();
                    progress.lock().1 = total;

                    let mut inserted = 0usize;
                    for (i, path) in files.iter().enumerate() {
                        // Quick check: parse the meta JSON to get the key without a full parse.
                        if let Ok(replay_file) = wows_replays::ReplayFile::from_file(path) {
                            let key = crate::data::cap_layout::CapLayoutKey {
                                map_id: replay_file.meta.mapId,
                                scenario_config_id: replay_file.meta.scenarioConfigId,
                            };
                            if !db.lock().contains(&key)
                                && let Some(layout) = crate::data::cap_layout::extract_cap_layout_from_replay(
                                    path,
                                    gm.as_ref(),
                                    Some(&game_constants),
                                )
                                && db.lock().insert(layout)
                            {
                                inserted += 1;
                            }
                        }
                        progress.lock().0 = i + 1;
                    }

                    if inserted > 0 {
                        // Save to SQLite if available, otherwise fall back to file.
                        if let (Some(pool), Some(rt)) = (&scan_pool, &scan_rt) {
                            if let Err(e) = rt.block_on(db.lock().save_to_db(pool)) {
                                tracing::warn!("failed to save cap layouts to db: {e}");
                            } else {
                                tracing::info!(
                                    "scan complete: {inserted} new cap layouts saved to db ({total} replays scanned)"
                                );
                            }
                        } else if let Some(cache_path) = crate::data::cap_layout::cache_path() {
                            if let Err(e) = db.lock().save(&cache_path) {
                                tracing::warn!("failed to save cap layout db: {e}");
                            } else {
                                tracing::info!(
                                    "scan complete: {inserted} new cap layouts saved ({total} replays scanned)"
                                );
                            }
                        }
                    } else {
                        tracing::info!("scan complete: no new cap layouts found ({total} replays scanned)");
                    }
                })
                .ok();
        }
    }

    /// Draw the map viewport: map image + cap points + annotations + zoom/pan + interaction.
    #[allow(clippy::too_many_arguments)]
    fn draw_map_viewport(
        ui: &mut egui::Ui,
        state: &mut TacticsBoardState,
        zoom_pan_arc: &Arc<Mutex<ViewportZoomPan>>,
        annotation_state_arc: &Arc<Mutex<AnnotationState>>,
        asset_cache: &Arc<Mutex<RendererAssetCache>>,
        wows_data: &SharedWoWsData,
        collab_local_tx: &Option<mpsc::Sender<LocalEvent>>,
        collab_session_state: &Option<Arc<Mutex<collab::SessionState>>>,
        collab_command_tx: &Option<mpsc::Sender<collab::SessionCommand>>,
        board_id: u64,
    ) {
        // Upload texture if needed
        if state.texture_dirty
            && let Some(ref img) = state.map_image
        {
            let color_image =
                egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.data);
            state.map_texture = Some(ui.ctx().load_texture("tactics_map", color_image, egui::TextureOptions::LINEAR));
            state.texture_dirty = false;
        }

        // Compute layout
        let canvas_size = MINIMAP_SIZE as f32;
        let logical_canvas = Vec2::new(canvas_size, canvas_size);
        let available = ui.available_size();
        let current_zoom = zoom_pan_arc.lock().zoom;
        let (response, painter) = ui.allocate_painter(available, egui::Sense::click_and_drag());
        let layout = compute_canvas_layout(available, logical_canvas, current_zoom, response.rect.min, None);
        let window_scale = layout.window_scale;

        // Zoom/pan input (scroll + middle-click)
        {
            let mut zp = zoom_pan_arc.lock();
            let left_pan_blocked = state.dragging_cap.is_some() || state.adding_cap;
            handle_viewport_zoom_pan(
                ui.ctx(),
                &response,
                &mut zp,
                &layout,
                logical_canvas,
                &ZoomPanConfig { allow_left_drag_pan: true, hud_height: 0.0, handle_tool_yaw: true, map_width: None },
                Some(&mut annotation_state_arc.lock()),
                left_pan_blocked,
            );
        }

        let zp = zoom_pan_arc.lock();
        let transform = MapTransform {
            origin: layout.origin,
            window_scale,
            zoom: zp.zoom,
            pan: zp.pan,
            hud_height: 0.0,
            canvas_width: canvas_size,
            hud_width: canvas_size,
        };
        drop(zp);

        // Clip to map area
        let map_clip = compute_map_clip_rect(&layout, 0.0, None);
        let map_painter = painter.with_clip_rect(map_clip);

        // Draw map background
        if state.map_texture.is_none() && state.map_image.is_none() {
            map_painter.rect_filled(map_clip, CornerRadius::ZERO, Color32::from_gray(30));
            map_painter.text(
                map_clip.center(),
                egui::Align2::CENTER_CENTER,
                "Select a map below",
                FontId::proportional(16.0),
                Color32::from_gray(120),
            );
        }

        // Draw map image
        if let Some(ref map_tex) = state.map_texture {
            draw_map_background(&map_painter, &transform, Some(map_tex.id()));

            // Draw 10x10 grid overlay (matching replay renderer)
            draw_grid(&map_painter, &transform, &GridStyle::default());
        }

        // Upload ship icon textures (lazily, once)
        if state.ship_icons.is_none() {
            let wdata = wows_data.read();
            let raw_icons = asset_cache.lock().get_or_load_ship_icons(&wdata.vfs);
            let mut icons = HashMap::new();
            for (key, asset) in raw_icons.iter() {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [asset.width as usize, asset.height as usize],
                    &asset.data,
                );
                let handle = ui.ctx().load_texture(format!("tactics_ship_{key}"), image, egui::TextureOptions::LINEAR);
                icons.insert(key.clone(), handle);
            }
            state.ship_icons = Some(icons);
        }

        // Draw cap points
        if let Some(ref map_info) = state.map_info {
            if !state.cap_points.is_empty() {
                tracing::trace!("Drawing {} cap points (map_info present)", state.cap_points.len());
            }
            for cap in &state.cap_points {
                let selected = state.selected_cap == Some(cap.id);
                draw_cap_point(&map_painter, &transform, map_info, cap, selected);
            }
        } else if !state.cap_points.is_empty() {
            tracing::debug!("Skipping {} cap points: map_info is None", state.cap_points.len());
        }

        // Draw annotations
        {
            let ann_state = annotation_state_arc.lock();
            let icons_ref = state.ship_icons.as_ref();
            let map_space = state.map_info.as_ref().map(|m| m.space_size as f32);
            let mut placed_labels: Vec<Rect> = Vec::new();
            for ann in &ann_state.annotations {
                render_annotation(ann, &transform, icons_ref, &map_painter, map_space);
                // Range circles for ship annotations with config
                if let Annotation::Ship { pos, config: Some(cfg), .. } = ann
                    && let Some(map_info) = &state.map_info
                {
                    render_annotation_range_circles(
                        ui.ctx(),
                        cfg,
                        *pos,
                        &transform,
                        map_info,
                        &map_painter,
                        wows_data,
                        &mut placed_labels,
                    );
                }
            }
            for &sel in &ann_state.selected_indices {
                if sel < ann_state.annotations.len() {
                    render_selection_highlight(&ann_state.annotations[sel], &transform, &map_painter);
                }
            }
            // Render tool preview (ghost shape at cursor)
            if let Some(cursor_pos) = response.hover_pos() {
                let minimap_pos = transform.screen_to_minimap(cursor_pos);
                render_tool_preview(
                    &ann_state.active_tool,
                    minimap_pos,
                    ann_state.paint_color,
                    ann_state.stroke_width,
                    &transform,
                    None,
                    &map_painter,
                    map_space,
                );
            }
        }

        // ── Collab: send cursor position ──
        if let Some(tx) = collab_local_tx {
            let cursor_pos = response.hover_pos().map(|p| {
                let mp = transform.screen_to_minimap(p);
                [mp.x, mp.y]
            });
            let _ = tx.send(LocalEvent::CursorPosition(cursor_pos));
        }

        // ── Collab: render remote cursors + pings ──
        if let Some(session_state) = collab_session_state {
            let s = session_state.lock();
            draw_remote_cursors(&s.cursors, s.my_user_id, &map_painter, &transform);
            // Collab pings
            let collab_pings: Vec<MapPing> =
                s.pings.iter().map(|p| MapPing { pos: p.pos, color: p.color, time: p.time }).collect();
            drop(s);
            if draw_pings(&collab_pings, &map_painter, &transform) {
                ui.ctx().request_repaint();
                let mut s = session_state.lock();
                s.pings.retain(|p| p.time.elapsed().as_secs_f32() < PING_DURATION);
            }
        }

        // ── Local pings (non-session) ──
        state.pings.retain(|p| p.time.elapsed().as_secs_f32() < PING_DURATION);
        if draw_pings(&state.pings, &map_painter, &transform) {
            ui.ctx().request_repaint();
        }

        // ── Interaction ──
        // Annotation tools take priority when active; otherwise fall through to cap interaction.
        let ann_tool_active = !matches!(annotation_state_arc.lock().active_tool, PaintTool::None);

        if ann_tool_active {
            Self::handle_annotation_interaction(
                ui,
                &response,
                annotation_state_arc,
                &transform,
                collab_local_tx,
                collab_session_state,
                Some(board_id),
            );
        } else if !annotation_state_arc.lock().show_context_menu {
            // Cap interaction only when no annotation tool active and context menu not showing
            if let Some(ref map_info) = state.map_info.clone() {
                Self::handle_cap_interaction(ui, &response, state, &transform, map_info, collab_local_tx, board_id);
            }

            // When annotation tool is None, clicking on map can select/deselect/move annotations
            Self::handle_annotation_select_move_impl(
                &response,
                annotation_state_arc,
                &transform,
                state,
                collab_local_tx,
                collab_session_state,
                Some(board_id),
            );
        }

        // Right-click: open context menu (works in both cap and annotation mode)
        if response.secondary_clicked()
            && let Some(click_pos) = response.interact_pointer_pos()
        {
            let mut ann = annotation_state_arc.lock();
            ann.active_tool = PaintTool::None;
            state.adding_cap = false;
            // Detect cap under cursor for context menu cap options
            ann.context_menu_cap = state
                .map_info
                .as_ref()
                .and_then(|map_info| hit_test_cap(click_pos, &state.cap_points, &transform, map_info));
            ann.show_context_menu = true;
            ann.context_menu_pos = click_pos;
        }

        // Escape: cancel tool / deselect
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            let mut ann = annotation_state_arc.lock();
            if !matches!(ann.active_tool, PaintTool::None) {
                ann.active_tool = PaintTool::None;
            } else {
                ann.clear_selection();
                state.selected_cap = None;
                state.adding_cap = false;
            }
            state.dragging_cap = None;
        }

        // Tool shortcuts (Ctrl+1..7, Ctrl+M)
        handle_tool_shortcuts(ui.ctx(), &mut annotation_state_arc.lock());

        // Show shortcut overlay while Ctrl is held
        draw_shortcut_overlay(ui.ctx(), ui.id().with("tactics_shortcut_overlay"));

        // Stroke width shortcuts
        {
            let mut ann = annotation_state_arc.lock();
            if ui.input(|i| i.key_pressed(egui::Key::OpenBracket)) {
                ann.stroke_width = (ann.stroke_width - 1.0).clamp(1.0, 8.0);
            }
            if ui.input(|i| i.key_pressed(egui::Key::CloseBracket)) {
                ann.stroke_width = (ann.stroke_width + 1.0).clamp(1.0, 8.0);
            }
        }

        // Ctrl+Z to undo
        if ui.ctx().input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            annotation_state_arc.lock().undo();
            send_annotation_full_sync(collab_command_tx, &annotation_state_arc.lock(), Some(board_id));
        }

        // ── Context menu ──
        Self::draw_annotation_context_menu(
            ui,
            annotation_state_arc,
            state,
            collab_local_tx,
            collab_command_tx,
            Some(board_id),
            wows_data,
        );

        // ── Annotation selection edit popup (skip for ships — handled by context menu) ──
        {
            let ann = annotation_state_arc.lock();
            let sel_info = ann.single_selected().and_then(|idx| {
                if idx < ann.annotations.len() {
                    // Skip popup for Ship annotations — config lives in right-click menu
                    if matches!(ann.annotations[idx], Annotation::Ship { .. }) {
                        return None;
                    }
                    let bounds = annotation_screen_bounds(&ann.annotations[idx], &transform);
                    Some((idx, bounds))
                } else {
                    None
                }
            });
            drop(ann);

            if let Some((sel_idx, bounds)) = sel_info {
                let map_space = state.map_info.as_ref().map(|m| m.space_size as f32);
                draw_annotation_edit_popup(
                    ui.ctx(),
                    ui.id().with("tactics_annotation_edit_popup"),
                    annotation_state_arc,
                    sel_idx,
                    bounds,
                    map_space,
                    collab_local_tx,
                    Some(board_id),
                );
            }
        }

        // ── Cap selection edit popup ──
        if let Some(sel_id) = state.selected_cap
            && let Some(cap_idx) = state.cap_points.iter().position(|c| c.id == sel_id)
            && let Some(ref map_info) = state.map_info.clone()
        {
            let cap = &state.cap_points[cap_idx];
            let screen_center = cap_screen_center(cap, &transform, map_info);
            let screen_radius = cap_screen_radius(cap, &transform, map_info);
            let cap_bounds =
                Rect::from_center_size(screen_center, egui::vec2(screen_radius * 2.0, screen_radius * 2.0));
            let popup_pos = Pos2::new(cap_bounds.right() + 8.0, cap_bounds.center().y);

            egui::Area::new(ui.id().with("tactics_cap_edit_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(popup_pos)
                .interactable(true)
                .show(ui.ctx(), |ui| {
                    let frame = egui::Frame::NONE
                        .fill(Color32::from_gray(30))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(egui::Margin::same(6))
                        .stroke(Stroke::new(1.0, Color32::from_gray(80)));
                    frame.show(ui, |ui| {
                        let cap = &mut state.cap_points[cap_idx];

                        // Team selector
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Team").small());
                            let team_label = match cap.team_id {
                                0 => "Team 1 (Green)",
                                1 => "Team 2 (Red)",
                                _ => "Neutral",
                            };
                            let old_team = cap.team_id;
                            egui::ComboBox::from_id_salt("cap_team").selected_text(team_label).width(110.0).show_ui(
                                ui,
                                |ui| {
                                    ui.selectable_value(&mut cap.team_id, -1, "Neutral");
                                    ui.selectable_value(&mut cap.team_id, 0, "Team 1 (Green)");
                                    ui.selectable_value(&mut cap.team_id, 1, "Team 2 (Red)");
                                },
                            );
                            if cap.team_id != old_team {
                                send_cap_update(collab_local_tx, cap, board_id);
                            }
                        });

                        // Radius in km
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Radius").small());
                            let mut km = BigWorldDistance::from(cap.radius).to_km().value();
                            let old_km = km;
                            ui.add(
                                egui::DragValue::new(&mut km)
                                    .speed(0.05)
                                    .range(0.1..=10.0)
                                    .fixed_decimals(2)
                                    .suffix(" km"),
                            );
                            if km != old_km {
                                cap.radius = Km::from(km).to_bigworld().value();
                                send_cap_update(collab_local_tx, cap, board_id);
                            }
                        });

                        // Label (A-Z)
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Label").small());
                            let mut idx = cap.index as i32;
                            let old_idx = idx;
                            ui.add(egui::DragValue::new(&mut idx).speed(0.1).range(0..=25).custom_formatter(|v, _| {
                                let c = (b'A' + v as u8) as char;
                                format!("{c}")
                            }));
                            if idx != old_idx {
                                cap.index = idx as usize;
                                send_cap_update(collab_local_tx, cap, board_id);
                            }
                        });

                        // Delete button (non-frozen caps only)
                        if !cap.frozen
                            && ui
                                .button(
                                    egui::RichText::new(crate::icons::TRASH).color(Color32::from_rgb(255, 100, 100)),
                                )
                                .on_hover_text("Delete cap")
                                .clicked()
                        {
                            let id = cap.id;
                            send_cap_remove(collab_local_tx, id, board_id);
                            state.cap_points.retain(|c| c.id != id);
                            state.selected_cap = None;
                        }
                    });
                });
        }

        // ── Cursor hint ──
        if response.hovered() {
            let ann = annotation_state_arc.lock();
            if let Some(icon) = annotation_cursor_icon(&ann, &response, &transform) {
                ui.ctx().set_cursor_icon(icon);
            } else if state.adding_cap {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            } else if let Some((_, CapDragMode::Move)) = state.dragging_cap {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if let Some((_, CapDragMode::Resize)) = state.dragging_cap {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }

        // ── Tool status label ──
        {
            let ann = annotation_state_arc.lock();
            if let Some(text) = tool_label(&ann.active_tool) {
                let pill_rect =
                    Rect::from_min_size(Pos2::new(map_clip.left() + 8.0, map_clip.top() + 8.0), Vec2::new(120.0, 22.0));
                painter.rect_filled(pill_rect, CornerRadius::same(4), Color32::from_black_alpha(160));
                painter.text(
                    pill_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &text,
                    FontId::proportional(12.0),
                    Color32::WHITE,
                );
            }
        }
    }

    /// Handle cap point click/drag/keyboard interaction.
    fn handle_cap_interaction(
        ui: &egui::Ui,
        response: &egui::Response,
        state: &mut TacticsBoardState,
        transform: &MapTransform,
        map_info: &MapInfo,
        collab_local_tx: &Option<mpsc::Sender<LocalEvent>>,
        board_id: u64,
    ) {
        // Delete key: remove selected cap (only if not frozen)
        if state.selected_cap.is_some()
            && ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
        {
            let sel_id = state.selected_cap.unwrap();
            let is_frozen = state.cap_points.iter().find(|c| c.id == sel_id).is_some_and(|c| c.frozen);
            if !is_frozen {
                state.cap_points.retain(|c| c.id != sel_id);
                state.selected_cap = None;
                state.dragging_cap = None;
                send_cap_remove(collab_local_tx, sel_id, board_id);
            }
        }

        // ── Drag in progress ──
        if let Some((drag_id, drag_mode)) = state.dragging_cap {
            if response.dragged_by(egui::PointerButton::Primary) {
                let delta = response.drag_delta();
                if delta != Vec2::ZERO {
                    let world_delta = screen_delta_to_world(delta, transform, map_info);
                    if let Some(cap) = state.cap_points.iter_mut().find(|c| c.id == drag_id) {
                        match drag_mode {
                            CapDragMode::Move => {
                                cap.world_x += world_delta.x;
                                // Z axis is inverted on minimap (north = -Z)
                                cap.world_z -= world_delta.y;
                            }
                            CapDragMode::Resize => {
                                // Resize by the larger component of the delta
                                let delta_bw = (world_delta.x.abs().max(world_delta.y.abs())) * world_delta.x.signum();
                                cap.radius = (cap.radius + delta_bw).max(0.5);
                            }
                        }
                        send_cap_update(collab_local_tx, cap, board_id);
                    }
                }
            }

            // Release: stop dragging
            if response.drag_stopped_by(egui::PointerButton::Primary) || !ui.input(|i| i.pointer.primary_down()) {
                state.dragging_cap = None;
            }
            return; // Don't process clicks while dragging
        }

        // ── Click handling ──
        if response.clicked()
            && let Some(pointer_pos) = response.interact_pointer_pos()
        {
            if state.adding_cap {
                // Add a new cap at the clicked position
                let minimap_pos = transform.screen_to_minimap(pointer_pos);
                let world = map_info.minimap_to_world_f32(minimap_pos.x, minimap_pos.y, MINIMAP_SIZE);
                let next_index = state.cap_points.iter().map(|c| c.index).max().map(|i| i + 1).unwrap_or(0);
                let id = state.next_cap_id;
                state.next_cap_id += 1;
                let new_cap = TacticsCapPoint {
                    id,
                    index: next_index,
                    world_x: world.x,
                    world_z: world.z,
                    radius: DEFAULT_CAP_RADIUS,
                    team_id: -1,
                    frozen: false,
                };
                send_cap_update(collab_local_tx, &new_cap, board_id);
                state.cap_points.push(new_cap);
                state.selected_cap = Some(id);
                state.adding_cap = false;
            } else {
                // Try to select a cap under the pointer
                state.selected_cap = hit_test_cap(pointer_pos, &state.cap_points, transform, map_info);
            }
        }

        // ── Drag start ──
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(pointer_pos) = response.interact_pointer_pos()
            && !state.adding_cap
        {
            // Check if we're starting a drag on a cap (only non-frozen caps)
            if let Some((cap_id, drag_mode)) = hit_test_cap_drag(pointer_pos, &state.cap_points, transform, map_info) {
                let is_frozen = state.cap_points.iter().find(|c| c.id == cap_id).is_some_and(|c| c.frozen);
                state.selected_cap = Some(cap_id);
                if !is_frozen {
                    state.dragging_cap = Some((cap_id, drag_mode));
                }
            }
        }
    }

    /// Handle annotation tool interaction (freehand, line, circle, rect, triangle, ship, eraser).
    fn handle_annotation_interaction(
        _ui: &egui::Ui,
        response: &egui::Response,
        annotation_state_arc: &Arc<Mutex<AnnotationState>>,
        transform: &MapTransform,
        collab_local_tx: &Option<mpsc::Sender<LocalEvent>>,
        collab_session_state: &Option<Arc<Mutex<collab::SessionState>>>,
        board_id: Option<u64>,
    ) {
        let mut ann = annotation_state_arc.lock();
        let result = handle_tool_interaction(&mut ann, response, transform);

        // Apply deferred mutations
        if result.new_annotation.is_some() || result.erase_index.is_some() {
            ann.save_undo();
        }
        if let Some(a) = result.new_annotation {
            let id: u64 = rand::random();
            let my_user_id = get_my_user_id(collab_session_state);
            ann.annotations.push(a);
            ann.annotation_ids.push(id);
            ann.annotation_owners.push(my_user_id);
            let idx = ann.annotations.len() - 1;
            send_annotation_update(collab_local_tx, &ann, idx, board_id);
        }
        if let Some(idx) = result.erase_index {
            let id = ann.annotation_ids[idx];
            ann.annotations.remove(idx);
            ann.annotation_ids.remove(idx);
            ann.annotation_owners.remove(idx);
            send_annotation_remove(collab_local_tx, id, board_id);
        }
    }

    /// Handle annotation select/move/rotate when no drawing tool is active.
    fn handle_annotation_select_move_impl(
        response: &egui::Response,
        annotation_state_arc: &Arc<Mutex<AnnotationState>>,
        transform: &MapTransform,
        state: &mut TacticsBoardState,
        collab_local_tx: &Option<mpsc::Sender<LocalEvent>>,
        collab_session_state: &Option<Arc<Mutex<collab::SessionState>>>,
        board_id: Option<u64>,
    ) {
        let mut ann = annotation_state_arc.lock();
        let result = handle_annotation_select_move(&mut ann, response, transform);

        // Sync to collab after rotation stopped or annotation moved
        if let Some(idx) = result.rotation_stopped_index {
            send_annotation_update(collab_local_tx, &ann, idx, board_id);
        }
        for &idx in &result.moved_indices {
            send_annotation_update(collab_local_tx, &ann, idx, board_id);
        }

        // Click on empty space -> ping
        if result.selected_by_click
            && !ann.has_selection()
            && let Some(click_pos) = response.hover_pos().map(|p| transform.screen_to_minimap(p))
        {
            drop(ann);
            handle_map_click_ping(click_pos, &mut state.pings, collab_session_state, collab_local_tx);
        }
    }

    /// Draw the annotation context menu (right-click).
    fn draw_annotation_context_menu(
        ui: &egui::Ui,
        annotation_state_arc: &Arc<Mutex<AnnotationState>>,
        state: &mut TacticsBoardState,
        collab_local_tx: &Option<mpsc::Sender<LocalEvent>>,
        collab_command_tx: &Option<mpsc::Sender<collab::SessionCommand>>,
        board_id: Option<u64>,
        wows_data: &SharedWoWsData,
    ) {
        let show_menu = annotation_state_arc.lock().show_context_menu;
        if !show_menu {
            return;
        }

        let menu_pos = annotation_state_arc.lock().context_menu_pos;
        let menu_resp = egui::Area::new(ui.id().with("tactics_paint_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(menu_pos)
            .interactable(true)
            .show(ui.ctx(), |ui| {
                let frame = egui::Frame::NONE
                    .fill(Color32::from_gray(30))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(8))
                    .stroke(Stroke::new(1.0, Color32::from_gray(80)));
                frame.show(ui, |ui| {
                    ui.set_min_width(160.0);

                    // ── Annotation tools ──
                    {
                        let mut ann = annotation_state_arc.lock();
                        let result = wt_collab_egui::toolbar::draw_annotation_menu_common(
                            ui,
                            &mut ann,
                            state.ship_icons.as_ref(),
                        );
                        if result.did_clear {
                            send_annotation_clear(collab_local_tx, board_id);
                        }
                        if result.did_undo {
                            send_annotation_full_sync(collab_command_tx, &ann, board_id);
                        }
                    }

                    // ── Ship annotation config (when a single Ship is selected) ──
                    {
                        let ann = annotation_state_arc.lock();
                        let single_ship_idx = if ann.selected_indices.len() == 1 {
                            let idx = *ann.selected_indices.iter().next().unwrap();
                            if matches!(ann.annotations.get(idx), Some(Annotation::Ship { .. })) {
                                Some(idx)
                            } else {
                                None
                            }
                        } else {
                            None
                        };
                        drop(ann);

                        if let Some(sel_idx) = single_ship_idx {
                            ui.separator();
                            ui.label(egui::RichText::new("Ship Config").small().strong());

                            // Team toggle + delete
                            {
                                let mut ann = annotation_state_arc.lock();
                                let is_friendly = matches!(
                                    ann.annotations.get(sel_idx),
                                    Some(Annotation::Ship { friendly: true, .. })
                                );
                                let (label, color) = if is_friendly {
                                    ("Friendly", super::FRIENDLY_COLOR)
                                } else {
                                    ("Enemy", super::ENEMY_COLOR)
                                };

                                let mut toggle_team = false;
                                let mut do_delete = false;
                                ui.horizontal(|ui| {
                                    let btn = egui::Button::new(egui::RichText::new(label).color(color).small())
                                        .min_size(egui::vec2(60.0, 0.0));
                                    if ui.add(btn).clicked() {
                                        toggle_team = true;
                                    }
                                    if ui
                                        .button(
                                            egui::RichText::new(crate::icons::TRASH)
                                                .color(Color32::from_rgb(255, 100, 100)),
                                        )
                                        .on_hover_text("Delete")
                                        .clicked()
                                    {
                                        do_delete = true;
                                    }
                                });

                                if toggle_team {
                                    if let Some(Annotation::Ship { friendly, .. }) = ann.annotations.get_mut(sel_idx) {
                                        *friendly = !*friendly;
                                    }
                                    send_annotation_update(collab_local_tx, &ann, sel_idx, board_id);
                                }
                                if do_delete {
                                    ann.save_undo();
                                    let id = ann.annotation_ids[sel_idx];
                                    ann.annotations.remove(sel_idx);
                                    ann.annotation_ids.remove(sel_idx);
                                    ann.annotation_owners.remove(sel_idx);
                                    ann.clear_selection();
                                    send_annotation_remove(collab_local_tx, id, board_id);
                                    ann.show_context_menu = false;
                                }
                            }

                            // Lazy-build ship catalog
                            if state.ship_catalog.is_none() {
                                let wdata = wows_data.read();
                                if let Some(ref metadata) = wdata.game_metadata {
                                    state.ship_catalog =
                                        Some(crate::armor_viewer::ship_selector::ShipCatalog::build(metadata));
                                }
                            }

                            // Ship search
                            if let Some(ref catalog) = state.ship_catalog {
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut state.ship_search_text)
                                        .hint_text("Search ship...")
                                        .desired_width(160.0),
                                );
                                if response.changed() || response.gained_focus() {
                                    // Search is live as user types
                                }

                                if !state.ship_search_text.is_empty() {
                                    let query = unidecode::unidecode(&state.ship_search_text).to_lowercase();
                                    let mut results: Vec<&crate::armor_viewer::ship_selector::ShipEntry> = Vec::new();
                                    for nation in &catalog.nations {
                                        for class in &nation.classes {
                                            for ship in &class.ships {
                                                if ship.search_name.contains(&query) {
                                                    results.push(ship);
                                                    if results.len() >= 10 {
                                                        break;
                                                    }
                                                }
                                            }
                                            if results.len() >= 10 {
                                                break;
                                            }
                                        }
                                        if results.len() >= 10 {
                                            break;
                                        }
                                    }

                                    for entry in &results {
                                        let tier = crate::armor_viewer::ship_selector::tier_roman(entry.tier);
                                        let label = format!("{} {}", tier, entry.display_name);
                                        if ui.button(egui::RichText::new(&label).small()).clicked() {
                                            // Assign ship to annotation
                                            let wdata = wows_data.read();
                                            if let Some(ref metadata) = wdata.game_metadata
                                                && let Some(param) = metadata.game_param_by_index(&entry.param_index)
                                            {
                                                let param_id = param.id().raw();
                                                let ship_name = entry.display_name.clone();
                                                let species_str = param
                                                    .species()
                                                    .and_then(|s| s.known())
                                                    .map(|s| format!("{s:?}"))
                                                    .unwrap_or_default();

                                                let mut ann = annotation_state_arc.lock();
                                                ann.save_undo();
                                                if let Some(Annotation::Ship { species, config, .. }) =
                                                    ann.annotations.get_mut(sel_idx)
                                                {
                                                    *species = species_str;
                                                    *config = Some(super::AnnotationShipConfig {
                                                        param_id,
                                                        ship_name,
                                                        hull_name: String::new(),
                                                        vis_coeff: 1.0,
                                                        gm_coeff: 1.0,
                                                        gs_coeff: 1.0,
                                                        range_filter: super::AnnotationRangeFilter::default(),
                                                    });
                                                }
                                                send_annotation_update(collab_local_tx, &ann, sel_idx, board_id);
                                                drop(ann);
                                                state.ship_search_text.clear();
                                            }
                                        }
                                    }
                                }
                            }

                            // Show current ship config if assigned
                            let ann = annotation_state_arc.lock();
                            let has_config =
                                matches!(ann.annotations.get(sel_idx), Some(Annotation::Ship { config: Some(_), .. }));
                            drop(ann);

                            if has_config {
                                // Hull selector
                                let hull_names: Vec<String> = {
                                    let ann = annotation_state_arc.lock();
                                    if let Some(Annotation::Ship { config: Some(cfg), .. }) =
                                        ann.annotations.get(sel_idx)
                                    {
                                        let wdata = wows_data.read();
                                        if let Some(ref metadata) = wdata.game_metadata {
                                            if let Some(param) = metadata.game_param_by_id(cfg.param_id.into()) {
                                                if let Some(vehicle) = param.vehicle() {
                                                    if let Some(hulls) = vehicle.hull_upgrades() {
                                                        hulls.keys().cloned().collect()
                                                    } else {
                                                        Vec::new()
                                                    }
                                                } else {
                                                    Vec::new()
                                                }
                                            } else {
                                                Vec::new()
                                            }
                                        } else {
                                            Vec::new()
                                        }
                                    } else {
                                        Vec::new()
                                    }
                                };

                                if hull_names.len() > 1 {
                                    let mut ann = annotation_state_arc.lock();
                                    if let Some(Annotation::Ship { config: Some(cfg), .. }) =
                                        ann.annotations.get_mut(sel_idx)
                                    {
                                        let current_hull =
                                            if cfg.hull_name.is_empty() { "Default" } else { &cfg.hull_name };
                                        let old_hull = cfg.hull_name.clone();
                                        egui::ComboBox::from_id_salt("ann_hull_select")
                                            .selected_text(current_hull)
                                            .width(140.0)
                                            .show_ui(ui, |ui| {
                                                for hull in &hull_names {
                                                    ui.selectable_value(&mut cfg.hull_name, hull.clone(), hull);
                                                }
                                            });
                                        if cfg.hull_name != old_hull {
                                            send_annotation_update(collab_local_tx, &ann, sel_idx, board_id);
                                        }
                                    }
                                }

                                // Modifier checkboxes
                                {
                                    let mut ann = annotation_state_arc.lock();
                                    let mut changed = false;
                                    if let Some(Annotation::Ship { config: Some(cfg), .. }) =
                                        ann.annotations.get_mut(sel_idx)
                                    {
                                        ui.separator();
                                        ui.label(egui::RichText::new("Modifiers").small().strong());

                                        let mut ce = cfg.vis_coeff < 1.0;
                                        if ui
                                            .checkbox(&mut ce, egui::RichText::new("Concealment Expert").small())
                                            .changed()
                                        {
                                            cfg.vis_coeff = if ce { 0.9 } else { 1.0 };
                                            changed = true;
                                        }

                                        let mut gr = cfg.gm_coeff > 1.0;
                                        if ui.checkbox(&mut gr, egui::RichText::new("Gun Range Mod").small()).changed()
                                        {
                                            cfg.gm_coeff = if gr { 1.16 } else { 1.0 };
                                            changed = true;
                                        }

                                        let mut sr = cfg.gs_coeff > 1.0;
                                        if ui
                                            .checkbox(&mut sr, egui::RichText::new("Secondary Range Mod").small())
                                            .changed()
                                        {
                                            cfg.gs_coeff = if sr { 1.05 } else { 1.0 };
                                            changed = true;
                                        }
                                    }
                                    if changed {
                                        send_annotation_update(collab_local_tx, &ann, sel_idx, board_id);
                                    }
                                }

                                // Range circle toggles
                                {
                                    let mut ann = annotation_state_arc.lock();
                                    let mut changed = false;
                                    if let Some(Annotation::Ship { config: Some(cfg), .. }) =
                                        ann.annotations.get_mut(sel_idx)
                                    {
                                        ui.separator();
                                        ui.label(egui::RichText::new("Range Circles").small().strong());

                                        ui.horizontal(|ui| {
                                            if ui.button(egui::RichText::new("All").small()).clicked() {
                                                cfg.range_filter.detection = true;
                                                cfg.range_filter.main_battery = true;
                                                cfg.range_filter.secondary_battery = true;
                                                cfg.range_filter.torpedo = true;
                                                cfg.range_filter.radar = true;
                                                cfg.range_filter.hydro = true;
                                                changed = true;
                                            }
                                            if ui.button(egui::RichText::new("None").small()).clicked() {
                                                cfg.range_filter.detection = false;
                                                cfg.range_filter.main_battery = false;
                                                cfg.range_filter.secondary_battery = false;
                                                cfg.range_filter.torpedo = false;
                                                cfg.range_filter.radar = false;
                                                cfg.range_filter.hydro = false;
                                                changed = true;
                                            }
                                        });

                                        changed |= ui
                                            .checkbox(
                                                &mut cfg.range_filter.detection,
                                                egui::RichText::new("Detection").small(),
                                            )
                                            .changed();
                                        changed |= ui
                                            .checkbox(
                                                &mut cfg.range_filter.main_battery,
                                                egui::RichText::new("Main Battery").small(),
                                            )
                                            .changed();
                                        changed |= ui
                                            .checkbox(
                                                &mut cfg.range_filter.secondary_battery,
                                                egui::RichText::new("Secondary Battery").small(),
                                            )
                                            .changed();
                                        changed |= ui
                                            .checkbox(
                                                &mut cfg.range_filter.torpedo,
                                                egui::RichText::new("Torpedo").small(),
                                            )
                                            .changed();
                                        changed |= ui
                                            .checkbox(&mut cfg.range_filter.radar, egui::RichText::new("Radar").small())
                                            .changed();
                                        changed |= ui
                                            .checkbox(&mut cfg.range_filter.hydro, egui::RichText::new("Hydro").small())
                                            .changed();
                                    }
                                    if changed {
                                        send_annotation_update(collab_local_tx, &ann, sel_idx, board_id);
                                    }
                                }
                            }
                        }
                    }

                    let context_menu_cap = annotation_state_arc.lock().context_menu_cap;

                    // ── Cap options for right-clicked cap ──
                    if let Some(cap_id) = context_menu_cap
                        && let Some(cap) = state.cap_points.iter_mut().find(|c| c.id == cap_id)
                    {
                        ui.separator();
                        ui.label(egui::RichText::new("Capture Point").small().strong());

                        // Team selector
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Team").small());
                            let team_label = match cap.team_id {
                                0 => "Team 1 (Green)",
                                1 => "Team 2 (Red)",
                                _ => "Neutral",
                            };
                            let old_team = cap.team_id;
                            egui::ComboBox::from_id_salt("ctx_cap_team")
                                .selected_text(team_label)
                                .width(110.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut cap.team_id, -1, "Neutral");
                                    ui.selectable_value(&mut cap.team_id, 0, "Team 1 (Green)");
                                    ui.selectable_value(&mut cap.team_id, 1, "Team 2 (Red)");
                                });
                            if cap.team_id != old_team
                                && let Some(bid) = board_id
                            {
                                send_cap_update(collab_local_tx, cap, bid);
                            }
                        });

                        // Radius in km
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Radius").small());
                            let mut km = BigWorldDistance::from(cap.radius).to_km().value();
                            let old_km = km;
                            ui.add(
                                egui::DragValue::new(&mut km)
                                    .speed(0.05)
                                    .range(0.1..=10.0)
                                    .fixed_decimals(2)
                                    .suffix(" km"),
                            );
                            if km != old_km {
                                cap.radius = Km::from(km).to_bigworld().value();
                                if let Some(bid) = board_id {
                                    send_cap_update(collab_local_tx, cap, bid);
                                }
                            }
                        });

                        // Delete (non-frozen only)
                        if !cap.frozen
                            && ui
                                .button(
                                    egui::RichText::new(crate::icons::TRASH).color(Color32::from_rgb(255, 100, 100)),
                                )
                                .on_hover_text("Delete cap")
                                .clicked()
                        {
                            let id = cap.id;
                            if let Some(bid) = board_id {
                                send_cap_remove(collab_local_tx, id, bid);
                            }
                            state.cap_points.retain(|c| c.id != id);
                            state.selected_cap = None;
                            annotation_state_arc.lock().show_context_menu = false;
                        }
                    }

                    // ── Add Cap button (when map is loaded) ──
                    if state.map_info.is_some() {
                        ui.separator();
                        if ui.button("Add Cap").clicked() {
                            state.adding_cap = true;
                            annotation_state_arc.lock().show_context_menu = false;
                        }
                    }
                });
            });

        // Close menu when clicking outside
        if menu_resp.response.clicked_elsewhere() {
            annotation_state_arc.lock().show_context_menu = false;
        }
    }
}

/// Send a `SetCapPoint` event for a cap point via the collab channel.
fn send_cap_update(collab_local_tx: &Option<mpsc::Sender<LocalEvent>>, cap: &TacticsCapPoint, board_id: u64) {
    if let Some(tx) = collab_local_tx {
        tracing::debug!(
            "send_cap_update: id={} radius={} world=({}, {})",
            cap.id,
            cap.radius,
            cap.world_x,
            cap.world_z
        );
        let _ = tx.send(LocalEvent::CapPoint { board_id, event: LocalCapPointEvent::Set(cap.to_wire()) });
    } else {
        tracing::debug!("send_cap_update: collab_local_tx is None, not sending");
    }
}

/// Send a `RemoveCapPoint` event via the collab channel.
fn send_cap_remove(collab_local_tx: &Option<mpsc::Sender<LocalEvent>>, id: u64, board_id: u64) {
    if let Some(tx) = collab_local_tx {
        let _ = tx.send(LocalEvent::CapPoint { board_id, event: LocalCapPointEvent::Remove { id } });
    }
}

/// Send a full cap point sync via the session command channel (used after bulk operations).
fn send_cap_full_sync(
    collab_command_tx: &Option<mpsc::Sender<collab::SessionCommand>>,
    caps: &[TacticsCapPoint],
    board_id: u64,
) {
    if let Some(tx) = collab_command_tx {
        let wire: Vec<WireCapPoint> = caps.iter().map(|c| c.to_wire()).collect();
        let _ = tx.send(collab::SessionCommand::SyncCapPoints { board_id, cap_points: wire });
    }
}

/// Send a `TacticsMapOpened` event via the collab channel, encoding the map image to PNG.
fn send_tactics_map_opened(
    collab_local_tx: &Option<mpsc::Sender<LocalEvent>>,
    board_id: u64,
    map_id: u32,
    map_name: &str,
    display_name: &str,
    map_image: &Option<Arc<crate::replay::renderer::RgbaAsset>>,
    map_info: &Option<MapInfo>,
) {
    if let Some(tx) = collab_local_tx {
        let map_image_png = map_image
            .as_ref()
            .map(|img| {
                let mut buf = Vec::new();
                if let Some(image) = image::RgbaImage::from_raw(img.width, img.height, img.data.clone()) {
                    let mut cursor = std::io::Cursor::new(&mut buf);
                    let _ = image.write_to(&mut cursor, image::ImageFormat::Png);
                }
                buf
            })
            .unwrap_or_default();
        let _ = tx.send(LocalEvent::TacticsMapOpened {
            board_id,
            owner_user_id: 0, // filled by peer task from session state
            map_name: map_name.to_string(),
            display_name: display_name.to_string(),
            map_id,
            map_image_png,
            map_info: map_info.clone(),
        });
    }
}
/// Load the map image for the given map name.
fn load_map_image(
    state: &mut TacticsBoardState,
    map_name: &str,
    asset_cache: &Arc<Mutex<RendererAssetCache>>,
    wows_data: &SharedWoWsData,
) {
    let wdata = wows_data.read();
    let (image, info) = asset_cache.lock().get_or_load_map(map_name, &wdata.vfs);
    state.map_image = image;
    state.map_info = info;
    state.map_texture = None;
    state.texture_dirty = true;
}

/// Populate editable cap points from a layout.
fn populate_cap_points(state: &mut TacticsBoardState, layout: &CapLayout) {
    state.cap_points.clear();
    for point in &layout.points {
        let id = state.next_cap_id;
        state.next_cap_id += 1;
        state.cap_points.push(TacticsCapPoint::from_layout(point, id));
    }
}

/// Draw a single cap point circle with label on the map.
fn draw_cap_point(
    painter: &egui::Painter,
    transform: &MapTransform,
    map_info: &MapInfo,
    cap: &TacticsCapPoint,
    selected: bool,
) {
    render_cap_point(painter, transform, map_info, &cap.view());

    if selected {
        let world_pos = WorldPos { x: cap.world_x, y: 0.0, z: cap.world_z };
        let minimap_pos = map_info.world_to_minimap(world_pos, MINIMAP_SIZE);
        let center = transform.minimap_to_screen(&minimap_pos);
        let radius_minimap = map_info.world_distance_to_minimap(cap.radius, MINIMAP_SIZE);
        let radius_screen = transform.scale_distance(radius_minimap);

        let sel_width = transform.scale_stroke(2.5);
        painter.circle_stroke(center, radius_screen, Stroke::new(sel_width, CAP_SELECTED_COLOR));
        // Draw resize handles (4 small squares on cardinal edges)
        let handle_size = transform.scale_stroke(4.0);
        for &offset in &[
            Vec2::new(radius_screen, 0.0),
            Vec2::new(-radius_screen, 0.0),
            Vec2::new(0.0, radius_screen),
            Vec2::new(0.0, -radius_screen),
        ] {
            let hpos = center + offset;
            let half = handle_size;
            painter.rect_filled(
                Rect::from_center_size(hpos, Vec2::splat(half * 2.0)),
                CornerRadius::same(1),
                CAP_SELECTED_COLOR,
            );
        }
    }
}

/// Hit-test: find the cap point under a screen position. Returns the cap's ID.
fn hit_test_cap(
    screen_pos: Pos2,
    caps: &[TacticsCapPoint],
    transform: &MapTransform,
    map_info: &MapInfo,
) -> Option<u64> {
    // Iterate in reverse so topmost cap (drawn last) wins
    for cap in caps.iter().rev() {
        let center = cap_screen_center(cap, transform, map_info);
        let radius = cap_screen_radius(cap, transform, map_info);
        let dist = screen_pos.distance(center);
        if dist <= radius {
            return Some(cap.id);
        }
    }
    None
}

/// Hit-test for drag: returns (cap_id, drag_mode). Resize if near edge, Move if inside.
fn hit_test_cap_drag(
    screen_pos: Pos2,
    caps: &[TacticsCapPoint],
    transform: &MapTransform,
    map_info: &MapInfo,
) -> Option<(u64, CapDragMode)> {
    for cap in caps.iter().rev() {
        let center = cap_screen_center(cap, transform, map_info);
        let radius = cap_screen_radius(cap, transform, map_info);
        let dist = screen_pos.distance(center);

        if dist <= radius + RESIZE_HANDLE_TOLERANCE {
            let mode =
                if (dist - radius).abs() < RESIZE_HANDLE_TOLERANCE { CapDragMode::Resize } else { CapDragMode::Move };
            return Some((cap.id, mode));
        }
    }
    None
}

/// Get the screen-space center of a cap point.
fn cap_screen_center(cap: &TacticsCapPoint, transform: &MapTransform, map_info: &MapInfo) -> Pos2 {
    let world_pos = WorldPos { x: cap.world_x, y: 0.0, z: cap.world_z };
    let minimap_pos = map_info.world_to_minimap(world_pos, MINIMAP_SIZE);
    transform.minimap_to_screen(&minimap_pos)
}

/// Get the screen-space radius of a cap point.
fn cap_screen_radius(cap: &TacticsCapPoint, transform: &MapTransform, map_info: &MapInfo) -> f32 {
    let radius_minimap = map_info.world_distance_to_minimap(cap.radius, MINIMAP_SIZE);
    transform.scale_distance(radius_minimap)
}

/// Convert a screen-space drag delta to world-space delta (BigWorld units).
/// Returns (world_dx, world_dz) where positive Y means southward on the map.
fn screen_delta_to_world(screen_delta: Vec2, transform: &MapTransform, map_info: &MapInfo) -> Vec2 {
    // Screen -> minimap pixels
    let minimap_dx = screen_delta.x / (transform.zoom * transform.window_scale);
    let minimap_dy = screen_delta.y / (transform.zoom * transform.window_scale);
    // Minimap pixels -> world (BigWorld) units
    let world_dx = map_info.minimap_distance_to_world(minimap_dx, MINIMAP_SIZE);
    let world_dy = map_info.minimap_distance_to_world(minimap_dy, MINIMAP_SIZE);
    Vec2::new(world_dx, world_dy)
}

/// Translate a map name to a human-readable string, falling back to a pretty format.
fn translate_or_pretty(map_name: &str, wows_data: &SharedWoWsData) -> String {
    let wdata = wows_data.read();
    if let Some(ref gm) = wdata.game_metadata {
        wowsunpack::game_params::translations::translate_map_name(map_name, gm.as_ref())
    } else {
        pretty_map_name(map_name)
    }
}

/// Extract a human-readable map name from a path like "spaces/16_OC_bees_to_honey".
fn pretty_map_name(map_name: &str) -> String {
    let bare = map_name.strip_prefix("spaces/").unwrap_or(map_name);
    let stripped = bare.find('_').map(|i| &bare[i + 1..]).unwrap_or(bare);
    stripped.replace('_', " ")
}

/// Render range circles for a ship annotation with config.
#[allow(clippy::too_many_arguments)]
fn render_annotation_range_circles(
    ctx: &egui::Context,
    cfg: &super::AnnotationShipConfig,
    pos: Vec2,
    transform: &MapTransform,
    map_info: &MapInfo,
    painter: &egui::Painter,
    wows_data: &SharedWoWsData,
    placed_labels: &mut Vec<Rect>,
) {
    use wt_collab_egui::rendering::RangeCircleKind;
    use wt_collab_egui::rendering::draw_range_circle;
    use wt_collab_egui::rendering::minimap_vec2_to_screen;
    use wt_collab_egui::rendering::range_circle_style;

    if cfg.param_id == 0 {
        return;
    }

    // Check if any range is enabled
    let rf = &cfg.range_filter;
    if !(rf.detection || rf.main_battery || rf.secondary_battery || rf.torpedo || rf.radar || rf.hydro) {
        return;
    }

    // Resolve ranges from game data
    let wdata = wows_data.read();
    let metadata = match wdata.game_metadata.as_ref() {
        Some(m) => m,
        None => return,
    };
    let param = match metadata.game_param_by_id(cfg.param_id.into()) {
        Some(p) => p,
        None => return,
    };
    let vehicle = match param.vehicle() {
        Some(v) => v,
        None => return,
    };
    let version = wdata.full_version.unwrap_or(wowsunpack::data::Version { major: 99, minor: 0, patch: 0, build: 0 });
    let hull_name = if cfg.hull_name.is_empty() { None } else { Some(cfg.hull_name.as_str()) };
    let ranges = vehicle.resolve_ranges(Some(metadata.as_ref()), hull_name, version);
    drop(wdata);

    let screen_center = minimap_vec2_to_screen(pos, transform);

    // Convert a distance in meters to screen pixels via minimap space.
    // minimap_px = meters / (space_size_m) * MINIMAP_SIZE
    // space_size_m = space_size (bigworld) * 30
    let space_size_m = map_info.space_size as f32 * 30.0;
    let meters_to_screen = |meters: f32| -> f32 {
        let minimap_px = meters / space_size_m * wows_minimap_renderer::MINIMAP_SIZE as f32;
        transform.scale_distance(minimap_px)
    };

    let mut shapes = Vec::new();

    let draw = |shapes: &mut Vec<egui::Shape>,
                kind: RangeCircleKind,
                meters: f32,
                coeff: f32,
                label_km: f32,
                placed: &mut Vec<Rect>| {
        let (color, alpha, dashed) = range_circle_style(kind);
        let adjusted_m = meters * coeff;
        let screen_r = meters_to_screen(adjusted_m);
        if screen_r < 1.0 {
            return;
        }
        let label = format!("{:.1} km", label_km * coeff);
        draw_range_circle(ctx, shapes, screen_center, screen_r, color, alpha, dashed, Some(&label), Some(placed));
    };

    if rf.detection
        && let Some(km) = ranges.detection_km
    {
        draw(&mut shapes, RangeCircleKind::Detection, km.value() * 1000.0, cfg.vis_coeff, km.value(), placed_labels);
    }
    if rf.main_battery
        && let Some(m) = ranges.main_battery_m
    {
        draw(&mut shapes, RangeCircleKind::MainBattery, m.value(), cfg.gm_coeff, m.to_km().value(), placed_labels);
    }
    if rf.secondary_battery
        && let Some(m) = ranges.secondary_battery_m
    {
        draw(&mut shapes, RangeCircleKind::SecondaryBattery, m.value(), cfg.gs_coeff, m.to_km().value(), placed_labels);
    }
    if rf.torpedo
        && let Some(m) = ranges.torpedo_range_m
    {
        draw(&mut shapes, RangeCircleKind::TorpedoRange, m.value(), 1.0, m.to_km().value(), placed_labels);
    }
    if rf.radar
        && let Some(m) = ranges.radar_m
    {
        draw(&mut shapes, RangeCircleKind::Radar, m.value(), 1.0, m.to_km().value(), placed_labels);
    }
    if rf.hydro
        && let Some(m) = ranges.hydro_m
    {
        draw(&mut shapes, RangeCircleKind::Hydro, m.value(), 1.0, m.to_km().value(), placed_labels);
    }

    painter.extend(shapes);
}

/// Build a human-readable mode label from a cap layout.
fn pretty_mode_name(layout: &CapLayout, wows_data: &SharedWoWsData) -> String {
    let wdata = wows_data.read();
    let scenario_label = if let Some(ref gm) = wdata.game_metadata {
        wowsunpack::game_params::translations::translate_scenario(&layout.scenario, gm.as_ref())
    } else {
        layout.scenario.clone()
    };
    let caps = layout.points.len();
    format!("{scenario_label} - {caps} caps")
}

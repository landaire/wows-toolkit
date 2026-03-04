//! Tactics board: an interactive map planner with editable capture points.
//!
//! Uses the shared [`MinimapView`](super) types for zoom/pan, coordinate
//! transforms, and (eventually) annotations.

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
use egui::Shape;
use egui::Stroke;
use egui::TextureHandle;
use egui::Vec2;
use parking_lot::Mutex;

use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::map_data::MapInfo;
use wowsunpack::game_params::types::BigWorldDistance;
use wowsunpack::game_params::types::Km;
use wowsunpack::game_types::WorldPos;

use crate::cap_layout::CapLayout;
use crate::cap_layout::CapLayoutDb;
use crate::cap_layout::CapLayoutKey;
use crate::cap_layout::CapPointLayout;
use crate::collab;
use crate::collab::peer::LocalCapPointEvent;
use crate::collab::peer::LocalEvent;
use crate::collab::protocol::WireCapPoint;
use crate::minimap_view::collab_annotation_to_local;
use crate::minimap_view::get_my_user_id;
use crate::minimap_view::handle_map_click_ping;
use crate::minimap_view::send_annotation_clear;
use crate::minimap_view::send_annotation_full_sync;
use crate::minimap_view::send_annotation_remove;
use crate::minimap_view::send_annotation_update;
use crate::replay_renderer::RendererAssetCache;
use crate::wows_data::SharedWoWsData;

use super::Annotation;
use super::AnnotationState;
use super::MapTransform;
use super::PaintTool;
use super::ViewportZoomPan;
use super::shapes::GridStyle;
use super::shapes::MapPing;
use super::shapes::PING_DURATION;
use super::shapes::annotation_cursor_icon;
use super::shapes::annotation_screen_bounds;
use super::shapes::draw_annotation_edit_popup;
use super::shapes::draw_annotation_menu_common;
use super::shapes::draw_grid;
use super::shapes::draw_pings;
use super::shapes::draw_remote_cursors;
use super::shapes::draw_shortcut_overlay;
use super::shapes::game_font;
use super::shapes::handle_annotation_select_move;
use super::shapes::handle_scroll_yaw;
use super::shapes::handle_tool_interaction;
use super::shapes::handle_tool_shortcuts;
use super::shapes::render_annotation;
use super::shapes::render_measurement_details;
use super::shapes::render_selection_highlight;
use super::shapes::render_tool_preview;
use super::shapes::tool_label;

// ─── Constants ───────────────────────────────────────────────────────────────

/// Cap point fill alpha, matching the replay renderer's 0.15.
const CAP_FILL_ALPHA: u8 = 38;

/// Cap point fill colors by team_id. -1 = neutral (white), 0 = green, 1 = red.
/// Alpha matches the replay renderer (15%).
const CAP_NEUTRAL_FILL: Color32 = Color32::from_rgba_premultiplied(
    (255 * CAP_FILL_ALPHA as u16 / 255) as u8,
    (255 * CAP_FILL_ALPHA as u16 / 255) as u8,
    (255 * CAP_FILL_ALPHA as u16 / 255) as u8,
    CAP_FILL_ALPHA,
);
const CAP_TEAM0_FILL: Color32 = Color32::from_rgba_premultiplied(
    (76 * CAP_FILL_ALPHA as u16 / 255) as u8,
    (232 * CAP_FILL_ALPHA as u16 / 255) as u8,
    (170 * CAP_FILL_ALPHA as u16 / 255) as u8,
    CAP_FILL_ALPHA,
);
const CAP_TEAM1_FILL: Color32 = Color32::from_rgba_premultiplied(
    (254 * CAP_FILL_ALPHA as u16 / 255) as u8,
    (77 * CAP_FILL_ALPHA as u16 / 255) as u8,
    (42 * CAP_FILL_ALPHA as u16 / 255) as u8,
    CAP_FILL_ALPHA,
);

/// Cap point outline colors (full opacity, matching replay renderer).
const CAP_NEUTRAL_OUTLINE: Color32 = Color32::from_rgb(255, 255, 255);
const CAP_TEAM0_OUTLINE: Color32 = Color32::from_rgb(76, 232, 170);
const CAP_TEAM1_OUTLINE: Color32 = Color32::from_rgb(254, 77, 42);
const CAP_SELECTED_COLOR: Color32 = Color32::from_rgb(255, 220, 50);

/// How close to the edge (in screen pixels) a click must be to start a resize drag.
const RESIZE_HANDLE_TOLERANCE: f32 = 8.0;
/// Default radius for newly added cap points (in BigWorld units).
/// Typical cap circles are ~5km = ~167 BW units; 150 is a sensible default.
const DEFAULT_CAP_RADIUS: f32 = 150.0;

// ─── Serializable preset types ──────────────────────────────────────────────

/// A serializable annotation (mirrors [`Annotation`] but with plain types).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
enum PresetAnnotation {
    Ship { pos: [f32; 2], yaw: f32, species: String, friendly: bool },
    FreehandStroke { points: Vec<[f32; 2]>, color: [u8; 4], width: f32 },
    Line { start: [f32; 2], end: [f32; 2], color: [u8; 4], width: f32 },
    Circle { center: [f32; 2], radius: f32, color: [u8; 4], width: f32, filled: bool },
    Rectangle { center: [f32; 2], half_size: [f32; 2], rotation: f32, color: [u8; 4], width: f32, filled: bool },
    Triangle { center: [f32; 2], radius: f32, rotation: f32, color: [u8; 4], width: f32, filled: bool },
    Arrow { points: Vec<[f32; 2]>, color: [u8; 4], width: f32 },
    Measurement { start: [f32; 2], end: [f32; 2], color: [u8; 4], width: f32 },
}

impl PresetAnnotation {
    fn from_annotation(ann: &Annotation) -> Self {
        match ann {
            Annotation::Ship { pos, yaw, species, friendly } => {
                PresetAnnotation::Ship { pos: [pos.x, pos.y], yaw: *yaw, species: species.clone(), friendly: *friendly }
            }
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
            PresetAnnotation::Ship { pos, yaw, species, friendly } => Annotation::Ship {
                pos: Vec2::new(pos[0], pos[1]),
                yaw: *yaw,
                species: species.clone(),
                friendly: *friendly,
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
    let storage = eframe::storage_dir(crate::APP_NAME)?;
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

// ─── Drag mode ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
enum CapDragMode {
    Move,
    Resize,
}

// ─── Editable cap point ─────────────────────────────────────────────────────

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

    /// Fill color (low alpha, matching replay renderer).
    fn fill_color(&self) -> Color32 {
        match self.team_id {
            0 => CAP_TEAM0_FILL,
            1 => CAP_TEAM1_FILL,
            _ => CAP_NEUTRAL_FILL,
        }
    }

    /// Outline color (full opacity, matching replay renderer).
    fn outline_color(&self) -> Color32 {
        match self.team_id {
            0 => CAP_TEAM0_OUTLINE,
            1 => CAP_TEAM1_OUTLINE,
            _ => CAP_NEUTRAL_OUTLINE,
        }
    }

    fn label(&self) -> String {
        let c = (b'A' + self.index as u8) as char;
        c.to_string()
    }
}

// ─── Tactics board state ────────────────────────────────────────────────────

/// Per-viewport state for the tactics board.
pub struct TacticsBoardState {
    /// Currently selected map (map_id, map_name).
    selected_map: Option<(u32, String)>,
    /// Currently selected mode key (or None for blank).
    selected_mode: Option<CapLayoutKey>,
    /// Editable cap points on the board.
    cap_points: Vec<TacticsCapPoint>,
    /// Next unique ID for new cap points.
    next_cap_id: u64,
    /// Loaded map image RGBA data (pixels, width, height).
    map_image: Option<Arc<(Vec<u8>, u32, u32)>>,
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
        }
    }
}

impl TacticsBoardState {
    /// Access the selected map (id, name).
    pub fn selected_map(&self) -> Option<(u32, &str)> {
        self.selected_map.as_ref().map(|(id, name)| (*id, name.as_str()))
    }

    /// Access raw map image data (pixels, width, height) for PNG encoding.
    pub fn map_image_raw(&self) -> Option<(&Vec<u8>, u32, u32)> {
        self.map_image.as_ref().map(|img| {
            let (ref data, w, h) = **img;
            (data, w, h)
        })
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

// ─── Tactics board viewer ───────────────────────────────────────────────────

/// A tactics board viewport. Created from the session popover or standalone.
pub struct TacticsBoardViewer {
    pub title: Arc<String>,
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
    pub fn new(
        cap_layout_db: Arc<Mutex<CapLayoutDb>>,
        asset_cache: Arc<Mutex<RendererAssetCache>>,
        wows_data: SharedWoWsData,
    ) -> Self {
        Self {
            title: Arc::new("Tactics Board".to_string()),
            open: Arc::new(AtomicBool::new(true)),
            zoom_pan: Arc::new(Mutex::new(ViewportZoomPan::default())),
            annotation_state: Arc::new(Mutex::new(AnnotationState::default())),
            state: Arc::new(Mutex::new(TacticsBoardState::default())),
            cap_layout_db,
            asset_cache,
            wows_data,
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
    pub fn draw(&self, ctx: &egui::Context) {
        let open = self.open.clone();
        let title = self.title.clone();
        let zoom_pan_arc = self.zoom_pan.clone();
        let annotation_state_arc = self.annotation_state.clone();
        let state_arc = self.state.clone();
        let cap_layout_db = self.cap_layout_db.clone();
        let asset_cache = self.asset_cache.clone();
        let wows_data = self.wows_data.clone();
        let collab_local_tx = self.collab_local_tx.clone();
        let collab_session_state = self.collab_session_state.clone();
        let collab_command_tx = self.collab_command_tx.clone();

        let viewport_id = egui::ViewportId::from_hash_of(&*title);

        // Register this viewport for repaint notifications from the peer task.
        if let Some(ref session_state) = self.collab_session_state {
            let mut s = session_state.lock();
            if !s.repaint_viewport_ids.contains(&viewport_id) {
                s.repaint_viewport_ids.push(viewport_id);
            }
        }

        ctx.show_viewport_deferred(
            viewport_id,
            egui::ViewportBuilder::default()
                .with_title(&*title)
                .with_inner_size([800.0, 850.0])
                .with_min_inner_size([400.0, 450.0]),
            move |ctx, _class| {
                // Handle window close
                if ctx.input(|i| i.viewport().close_requested()) {
                    open.store(false, Ordering::Relaxed);
                    // Unregister viewport from repaint notifications.
                    if let Some(ref session_state) = collab_session_state {
                        let mut s = session_state.lock();
                        s.repaint_viewport_ids.retain(|id| *id != viewport_id);
                    }
                    ctx.request_repaint();
                }

                let mut state = state_arc.lock();

                // ── Collab: apply incoming annotation + cap point sync ──
                if let Some(session_state) = &collab_session_state {
                    let s = session_state.lock();
                    if s.annotation_sync_version > state.applied_annotation_sync_version {
                        if let Some(ref sync) = s.current_annotation_sync {
                            let mut ann = annotation_state_arc.lock();
                            ann.annotations =
                                sync.annotations.iter().cloned().map(collab_annotation_to_local).collect();
                            ann.annotation_owners = sync.owners.clone();
                            ann.annotation_ids = sync.ids.clone();
                        }
                        state.applied_annotation_sync_version = s.annotation_sync_version;
                    }
                    if s.cap_point_sync_version > state.applied_cap_sync_version {
                        tracing::debug!(
                            "Applying cap sync: version {} -> {}, sync_data={}",
                            state.applied_cap_sync_version,
                            s.cap_point_sync_version,
                            s.current_cap_point_sync.as_ref().map(|s| s.cap_points.len()).unwrap_or(0),
                        );
                        if let Some(ref sync) = s.current_cap_point_sync {
                            state.cap_points = sync
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
                        }
                        state.applied_cap_sync_version = s.cap_point_sync_version;
                    }
                    if s.tactics_map_version > state.applied_tactics_map_version {
                        if let Some(ref tmap) = s.tactics_map {
                            tracing::debug!(
                                "Applying tactics map sync: version {} -> {}, map={}",
                                state.applied_tactics_map_version,
                                s.tactics_map_version,
                                tmap.map_name,
                            );
                            state.selected_map = Some((tmap.map_id, tmap.map_name.clone()));
                            state.selected_mode = None;
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
                                state.map_image = Some(Arc::new((rgba.into_raw(), w, h)));
                                state.texture_dirty = true;
                            }
                        } else {
                            // Map closed by peer — clear our selection.
                            state.selected_map = None;
                            state.selected_mode = None;
                            state.map_image = None;
                            state.map_info = None;
                            state.map_texture = None;
                            state.cap_points.clear();
                            state.selected_cap = None;
                        }
                        state.applied_tactics_map_version = s.tactics_map_version;
                    }
                    drop(s);
                }

                // ── Bottom panel: map/mode selector + cap tools + presets ──
                // Hide for regular peers (non-host, non-co-host) in a collab session.
                let is_authority = collab_session_state
                    .as_ref()
                    .map(|ss| {
                        let s = ss.lock();
                        s.role.is_host() || s.role.is_co_host()
                    })
                    .unwrap_or(true); // No session → standalone, show everything.

                if is_authority {
                    egui::TopBottomPanel::bottom("tactics_bottom_panel").show(ctx, |ui| {
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
                            );
                            ui.separator();
                            Self::draw_scan_replays_button(ui, &mut state, &cap_layout_db, &wows_data);
                        });
                        ui.add_space(4.0);
                    });
                } // is_authority

                // ── Central panel: map viewport ──
                egui::CentralPanel::default().show(ctx, |ui| {
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
                    );
                });
            },
        );
    }

    /// Draw the map/mode selector and cap tools in the bottom panel.
    fn draw_map_mode_selector(
        ui: &mut egui::Ui,
        state: &mut TacticsBoardState,
        cap_layout_db: &Arc<Mutex<CapLayoutDb>>,
        asset_cache: &Arc<Mutex<RendererAssetCache>>,
        wows_data: &SharedWoWsData,
        collab_local_tx: &Option<mpsc::Sender<LocalEvent>>,
        collab_command_tx: &Option<mpsc::Sender<collab::SessionCommand>>,
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
                        state.cap_points.clear();
                        state.selected_cap = None;
                        load_map_image(state, map_name, asset_cache, wows_data);
                        send_tactics_map_opened(collab_local_tx, *map_id, map_name, &state.map_image, &state.map_info);
                        send_cap_full_sync(collab_command_tx, &state.cap_points);
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
                        state.cap_points.clear();
                        state.selected_cap = None;
                        send_cap_full_sync(collab_command_tx, &state.cap_points);
                    }

                    for (layout, label) in modes.iter().zip(display_labels.iter()) {
                        let selected = state.selected_mode.as_ref() == Some(&layout.key);
                        if ui.selectable_label(selected, label).clicked() {
                            state.selected_mode = Some(layout.key.clone());
                            state.selected_cap = None;
                            populate_cap_points(state, layout);
                            send_cap_full_sync(collab_command_tx, &state.cap_points);
                        }
                    }
                });
        }
    }

    /// Draw preset save/load/delete controls.
    fn draw_preset_controls(
        ui: &mut egui::Ui,
        state: &mut TacticsBoardState,
        annotation_state_arc: &Arc<Mutex<AnnotationState>>,
        asset_cache: &Arc<Mutex<RendererAssetCache>>,
        wows_data: &SharedWoWsData,
        cap_layout_db: &Arc<Mutex<CapLayoutDb>>,
        collab_command_tx: &Option<mpsc::Sender<collab::SessionCommand>>,
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
                    send_cap_full_sync(collab_command_tx, &state.cap_points);
                    send_annotation_full_sync(collab_command_tx, &annotation_state_arc.lock());
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
                        send_cap_full_sync(collab_command_tx, &state.cap_points);
                        send_annotation_full_sync(collab_command_tx, &ann);
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
                            let key = crate::cap_layout::CapLayoutKey {
                                map_id: replay_file.meta.mapId,
                                scenario_config_id: replay_file.meta.scenarioConfigId,
                            };
                            if !db.lock().contains(&key)
                                && let Some(layout) = crate::cap_layout::extract_cap_layout_from_replay(
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
                        if let Some(cache_path) = crate::cap_layout::cache_path() {
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
    ) {
        // Upload texture if needed
        if state.texture_dirty
            && let Some(ref img) = state.map_image
        {
            let (ref data, w, h) = **img;
            let color_image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], data);
            state.map_texture = Some(ui.ctx().load_texture("tactics_map", color_image, egui::TextureOptions::LINEAR));
            state.texture_dirty = false;
        }

        // Compute layout
        let canvas_size = MINIMAP_SIZE as f32;
        let logical_canvas = Vec2::new(canvas_size, canvas_size);
        let available = ui.available_size();
        let scale_x = available.x / logical_canvas.x;
        let scale_y = available.y / logical_canvas.y;
        let fit_scale = scale_x.min(scale_y);
        let fill_scale = scale_x.max(scale_y);

        let current_zoom = zoom_pan_arc.lock().zoom;
        let t = ((current_zoom - 1.0) / 1.0).clamp(0.0, 1.0);
        let window_scale = (fit_scale + t * (fill_scale - fit_scale)).max(0.1);

        let (response, painter) = ui.allocate_painter(available, egui::Sense::click_and_drag());

        // Center scaled canvas
        let scaled_canvas = logical_canvas * window_scale;
        let offset_x = ((available.x - scaled_canvas.x) / 2.0).max(0.0);
        let offset_y = ((available.y - scaled_canvas.y) / 2.0).max(0.0);
        let origin = response.rect.min + Vec2::new(offset_x, offset_y);

        // Zoom/pan input (scroll + middle-click)
        {
            let mut zp = zoom_pan_arc.lock();
            let ann_tool_active = !matches!(annotation_state_arc.lock().active_tool, PaintTool::None);

            // Scroll-wheel: zoom (normal) or rotate (when placing ship)
            if response.hovered() {
                let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll_delta != 0.0 {
                    let scroll_used_by_tool =
                        ann_tool_active && handle_scroll_yaw(&mut annotation_state_arc.lock(), scroll_delta);

                    if !scroll_used_by_tool {
                        let zoom_speed = 0.01;
                        let old_zoom = zp.zoom;
                        let new_zoom = (old_zoom * (1.0 + scroll_delta * zoom_speed)).clamp(1.0, 10.0);
                        if new_zoom != old_zoom {
                            if let Some(cursor) = response.hover_pos() {
                                let local_x = (cursor.x - origin.x) / window_scale;
                                let local_y = (cursor.y - origin.y) / window_scale;
                                let minimap_x = (local_x + zp.pan.x) / old_zoom;
                                let minimap_y = (local_y + zp.pan.y) / old_zoom;
                                zp.pan.x = minimap_x * new_zoom - local_x;
                                zp.pan.y = minimap_y * new_zoom - local_y;
                            }
                            zp.zoom = new_zoom;
                        }
                    }
                }
            }

            // Middle-drag always pans; left-drag pans when nothing else would consume it
            let has_selection = annotation_state_arc.lock().has_selection();
            let cap_dragging = state.dragging_cap.is_some();
            let left_pan = !ann_tool_active
                && !has_selection
                && !cap_dragging
                && !state.adding_cap
                && response.dragged_by(egui::PointerButton::Primary);
            if response.dragged_by(egui::PointerButton::Middle) || left_pan {
                let delta = response.drag_delta();
                zp.pan.x -= delta.x / window_scale;
                zp.pan.y -= delta.y / window_scale;
            }

            if response.double_clicked() {
                zp.zoom = 1.0;
                zp.pan = Vec2::ZERO;
            }

            // Clamp pan so the map can't scroll past its edges.
            let visible_w = available.x.min(scaled_canvas.x) / window_scale;
            let visible_h = available.y.min(scaled_canvas.y) / window_scale;
            let map_zoomed = canvas_size * zp.zoom;
            let max_pan_x = (map_zoomed - visible_w).max(0.0);
            let max_pan_y = (map_zoomed - visible_h).max(0.0);
            zp.pan.x = zp.pan.x.clamp(0.0, max_pan_x);
            zp.pan.y = zp.pan.y.clamp(0.0, max_pan_y);
        }

        let zp = zoom_pan_arc.lock();
        let transform = MapTransform {
            origin,
            window_scale,
            zoom: zp.zoom,
            pan: zp.pan,
            hud_height: 0.0,
            canvas_width: canvas_size,
        };
        drop(zp);

        // Clip to map area
        let map_clip = Rect::from_min_max(origin, Pos2::new(origin.x + scaled_canvas.x, origin.y + scaled_canvas.y));
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
            let map_tl = transform.minimap_to_screen(&wows_minimap_renderer::MinimapPos { x: 0, y: 0 });
            let map_br = transform.minimap_to_screen(&wows_minimap_renderer::MinimapPos {
                x: MINIMAP_SIZE as i32,
                y: MINIMAP_SIZE as i32,
            });
            let map_rect = Rect::from_min_max(map_tl, map_br);
            let mut mesh = egui::Mesh::with_texture(map_tex.id());
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            mesh.add_rect_with_uv(map_rect, uv, Color32::WHITE);
            map_painter.add(Shape::Mesh(mesh.into()));

            // Draw 10x10 grid overlay (matching replay renderer)
            draw_grid(&map_painter, &transform, &GridStyle::default());
        }

        // Upload ship icon textures (lazily, once)
        if state.ship_icons.is_none() {
            let wdata = wows_data.read();
            let raw_icons = asset_cache.lock().get_or_load_ship_icons(&wdata.vfs);
            let mut icons = HashMap::new();
            for (key, (data, w, h)) in raw_icons.iter() {
                let image = egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], data);
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
            for ann in &ann_state.annotations {
                render_annotation(ann, &transform, icons_ref, &map_painter);
                if let Annotation::Measurement { start, end, color, width } = ann {
                    render_measurement_details(*start, *end, *color, *width, &transform, map_space, &map_painter);
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
            );
        } else if !annotation_state_arc.lock().show_context_menu {
            // Cap interaction only when no annotation tool active and context menu not showing
            if let Some(ref map_info) = state.map_info.clone() {
                Self::handle_cap_interaction(ui, &response, state, &transform, map_info, collab_local_tx);
            }

            // When annotation tool is None, clicking on map can select/deselect/move annotations
            Self::handle_annotation_select_move_impl(
                ui,
                &response,
                annotation_state_arc,
                &transform,
                state,
                collab_local_tx,
                collab_session_state,
            );
        }

        // Right-click: open context menu (works in both cap and annotation mode)
        if response.secondary_clicked()
            && let Some(click_pos) = response.interact_pointer_pos()
        {
            let mut ann = annotation_state_arc.lock();
            ann.active_tool = PaintTool::None;
            state.adding_cap = false;
            ann.show_context_menu = true;
            ann.context_menu_pos = click_pos;
            // Detect cap under cursor for context menu cap options
            ann.context_menu_cap = state
                .map_info
                .as_ref()
                .and_then(|map_info| hit_test_cap(click_pos, &state.cap_points, &transform, map_info));
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
            send_annotation_full_sync(collab_command_tx, &annotation_state_arc.lock());
        }

        // ── Context menu ──
        Self::draw_annotation_context_menu(ui, annotation_state_arc, state, collab_local_tx, collab_command_tx);

        // ── Annotation selection edit popup ──
        {
            let ann = annotation_state_arc.lock();
            let sel_info = ann.single_selected().and_then(|idx| {
                if idx < ann.annotations.len() {
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
                                send_cap_update(collab_local_tx, cap);
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
                                send_cap_update(collab_local_tx, cap);
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
                                send_cap_update(collab_local_tx, cap);
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
                            send_cap_remove(collab_local_tx, id);
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
                send_cap_remove(collab_local_tx, sel_id);
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
                        send_cap_update(collab_local_tx, cap);
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
                send_cap_update(collab_local_tx, &new_cap);
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
            send_annotation_update(collab_local_tx, &ann, idx);
        }
        if let Some(idx) = result.erase_index {
            let id = ann.annotation_ids[idx];
            ann.annotations.remove(idx);
            ann.annotation_ids.remove(idx);
            ann.annotation_owners.remove(idx);
            send_annotation_remove(collab_local_tx, id);
        }
    }

    /// Handle annotation select/move/rotate when no drawing tool is active.
    fn handle_annotation_select_move_impl(
        _ui: &egui::Ui,
        response: &egui::Response,
        annotation_state_arc: &Arc<Mutex<AnnotationState>>,
        transform: &MapTransform,
        state: &mut TacticsBoardState,
        collab_local_tx: &Option<mpsc::Sender<LocalEvent>>,
        collab_session_state: &Option<Arc<Mutex<collab::SessionState>>>,
    ) {
        let mut ann = annotation_state_arc.lock();
        let result = handle_annotation_select_move(&mut ann, response, transform);

        // Sync to collab after rotation stopped or annotation moved
        if let Some(idx) = result.rotation_stopped_index {
            send_annotation_update(collab_local_tx, &ann, idx);
        }
        for &idx in &result.moved_indices {
            send_annotation_update(collab_local_tx, &ann, idx);
        }

        // Click on empty space → ping
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
                    ui.set_min_width(200.0);
                    let context_menu_cap;
                    {
                        let mut ann = annotation_state_arc.lock();
                        context_menu_cap = ann.context_menu_cap;

                        let menu_result = draw_annotation_menu_common(ui, &mut ann, state.ship_icons.as_ref());

                        if menu_result.did_clear {
                            send_annotation_clear(collab_local_tx);
                        }
                        if menu_result.did_undo {
                            drop(ann);
                            send_annotation_full_sync(collab_command_tx, &annotation_state_arc.lock());
                        }
                    }

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
                            if cap.team_id != old_team {
                                send_cap_update(collab_local_tx, cap);
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
                                send_cap_update(collab_local_tx, cap);
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
                            send_cap_remove(collab_local_tx, id);
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
fn send_cap_update(collab_local_tx: &Option<mpsc::Sender<LocalEvent>>, cap: &TacticsCapPoint) {
    if let Some(tx) = collab_local_tx {
        tracing::debug!(
            "send_cap_update: id={} radius={} world=({}, {})",
            cap.id,
            cap.radius,
            cap.world_x,
            cap.world_z
        );
        let _ = tx.send(LocalEvent::CapPoint(LocalCapPointEvent::Set(WireCapPoint {
            id: cap.id,
            index: cap.index as u32,
            world_x: cap.world_x,
            world_z: cap.world_z,
            radius: cap.radius,
            team_id: cap.team_id,
            frozen: cap.frozen,
        })));
    } else {
        tracing::debug!("send_cap_update: collab_local_tx is None, not sending");
    }
}

/// Send a `RemoveCapPoint` event via the collab channel.
fn send_cap_remove(collab_local_tx: &Option<mpsc::Sender<LocalEvent>>, id: u64) {
    if let Some(tx) = collab_local_tx {
        let _ = tx.send(LocalEvent::CapPoint(LocalCapPointEvent::Remove { id }));
    }
}

/// Send a full cap point sync via the session command channel (used after bulk operations).
fn send_cap_full_sync(collab_command_tx: &Option<mpsc::Sender<collab::SessionCommand>>, caps: &[TacticsCapPoint]) {
    if let Some(tx) = collab_command_tx {
        let wire: Vec<WireCapPoint> = caps
            .iter()
            .map(|c| WireCapPoint {
                id: c.id,
                index: c.index as u32,
                world_x: c.world_x,
                world_z: c.world_z,
                radius: c.radius,
                team_id: c.team_id,
                frozen: c.frozen,
            })
            .collect();
        let _ = tx.send(collab::SessionCommand::SyncCapPoints { cap_points: wire });
    }
}

/// Send a `TacticsMapOpened` event via the collab channel, encoding the map image to PNG.
fn send_tactics_map_opened(
    collab_local_tx: &Option<mpsc::Sender<LocalEvent>>,
    map_id: u32,
    map_name: &str,
    map_image: &Option<Arc<(Vec<u8>, u32, u32)>>,
    map_info: &Option<MapInfo>,
) {
    if let Some(tx) = collab_local_tx {
        let map_image_png = map_image
            .as_ref()
            .map(|img| {
                let (ref data, w, h) = **img;
                let mut buf = Vec::new();
                if let Some(image) = image::RgbaImage::from_raw(w, h, data.clone()) {
                    let mut cursor = std::io::Cursor::new(&mut buf);
                    let _ = image.write_to(&mut cursor, image::ImageFormat::Png);
                }
                buf
            })
            .unwrap_or_default();
        let _ = tx.send(LocalEvent::TacticsMapOpened {
            map_name: map_name.to_string(),
            map_id,
            map_image_png,
            map_info: map_info.clone(),
        });
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

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
    let world_pos = WorldPos { x: cap.world_x, y: 0.0, z: cap.world_z };
    let minimap_pos = map_info.world_to_minimap(world_pos, MINIMAP_SIZE);
    let center = transform.minimap_to_screen(&minimap_pos);

    let radius_minimap = map_info.world_distance_to_minimap(cap.radius, MINIMAP_SIZE);
    let radius_screen = transform.scale_distance(radius_minimap);

    // Filled circle (low alpha, matching replay renderer)
    painter.circle_filled(center, radius_screen, cap.fill_color());

    // Outline: team-colored (matching replay renderer), thicker + yellow when selected
    let outline_width = transform.scale_stroke(1.5);
    painter.circle_stroke(center, radius_screen, Stroke::new(outline_width, cap.outline_color()));

    if selected {
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

    // Label (game font, matching replay renderer)
    let label = cap.label();
    painter.text(center, egui::Align2::CENTER_CENTER, &label, game_font(11.0 * transform.window_scale), Color32::WHITE);
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
    // Screen → minimap pixels
    let minimap_dx = screen_delta.x / (transform.zoom * transform.window_scale);
    let minimap_dy = screen_delta.y / (transform.zoom * transform.window_scale);
    // Minimap pixels → world (BigWorld) units
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

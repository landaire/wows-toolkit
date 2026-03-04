//! Shared types for minimap-based viewers (replay renderer, tactics board).
//!
//! Types defined here are used by both `replay_renderer` and `tactics` to
//! avoid duplicating map rendering, zoom/pan, and annotation logic.

pub mod shapes;
pub mod tactics;

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc;

use egui::Color32;
use egui::Pos2;
use egui::Vec2;
use parking_lot::Mutex;

use wows_minimap_renderer::MinimapPos;
use wows_minimap_renderer::draw_command::ShipConfigFilter;
use wows_replays::types::EntityId;

use crate::collab;
use crate::collab::peer::LocalAnnotationEvent;
use crate::collab::peer::LocalEvent;

// ─── Zoom/Pan State ─────────────────────────────────────────────────────────

/// Overlay controls visibility state. Persists across frames.
pub struct OverlayState {
    /// Last time the mouse moved or a control was interacted with (ctx.input time).
    pub last_activity: f64,
}

impl Default for OverlayState {
    fn default() -> Self {
        Self { last_activity: 0.0 }
    }
}

/// Zoom and pan state for the minimap viewport. Persists across frames.
pub struct ViewportZoomPan {
    /// Zoom level. 1.0 = no zoom (fit to window). Range: [1.0, 10.0].
    pub zoom: f32,
    /// Pan offset in zoomed-minimap-pixel space.
    /// (0,0) = top-left corner of the map is at the top-left of the viewport.
    pub pan: Vec2,
}

impl Default for ViewportZoomPan {
    fn default() -> Self {
        Self { zoom: 1.0, pan: Vec2::ZERO }
    }
}

/// Encapsulates coordinate transforms for a single frame of viewport rendering.
/// Handles both window-fit scaling and zoom/pan for the map region.
pub struct MapTransform {
    /// Top-left of the allocated painter rect in screen space.
    pub origin: Pos2,
    /// Uniform scale from logical canvas pixels to screen pixels.
    pub window_scale: f32,
    /// Zoom level (1.0 = no zoom).
    pub zoom: f32,
    /// Pan offset in zoomed-minimap-pixel space.
    pub pan: Vec2,
    /// HUD height in logical pixels.
    pub hud_height: f32,
    /// Logical canvas width (768).
    pub canvas_width: f32,
}

impl MapTransform {
    /// Convert a MinimapPos (in [0..768] space) to screen Pos2.
    /// Applies zoom and pan, then window scale. Used for all map elements.
    pub fn minimap_to_screen(&self, pos: &MinimapPos) -> Pos2 {
        let zoomed_x = pos.x as f32 * self.zoom - self.pan.x;
        let zoomed_y = pos.y as f32 * self.zoom - self.pan.y;
        Pos2::new(
            self.origin.x + zoomed_x * self.window_scale,
            self.origin.y + (self.hud_height + zoomed_y) * self.window_scale,
        )
    }

    /// Scale a distance (e.g., radius, icon size) from minimap space to screen space.
    /// Scales with both zoom and window_scale.
    pub fn scale_distance(&self, d: f32) -> f32 {
        d * self.zoom * self.window_scale
    }

    /// Scale a stroke width. Scales with window_scale only (not zoom),
    /// keeping lines readable at all zoom levels.
    pub fn scale_stroke(&self, width: f32) -> f32 {
        width * self.window_scale
    }

    /// Position for HUD elements (ScoreBar, Timer, KillFeed).
    /// These scale with the window but NOT with zoom/pan.
    pub fn hud_pos(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.origin.x + x * self.window_scale, self.origin.y + y * self.window_scale)
    }

    /// The HUD-scaled canvas width in screen pixels.
    pub fn screen_canvas_width(&self) -> f32 {
        self.canvas_width * self.window_scale
    }

    /// Convert a screen Pos2 to minimap logical coords (inverse of minimap_to_screen).
    pub fn screen_to_minimap(&self, screen_pos: Pos2) -> Vec2 {
        let sx = (screen_pos.x - self.origin.x) / self.window_scale;
        let sy = (screen_pos.y - self.origin.y) / self.window_scale - self.hud_height;
        Vec2::new((sx + self.pan.x) / self.zoom, (sy + self.pan.y) / self.zoom)
    }
}

// ─── Annotation / Painting State ─────────────────────────────────────────────

pub const SHIP_SPECIES: [&str; 5] = ["Destroyer", "Cruiser", "Battleship", "AirCarrier", "Submarine"];
pub const FRIENDLY_COLOR: Color32 = Color32::from_rgb(76, 232, 170);
pub const ENEMY_COLOR: Color32 = Color32::from_rgb(254, 77, 42);

/// A single annotation placed on the map.
#[derive(Clone)]
pub enum Annotation {
    Ship { pos: Vec2, yaw: f32, species: String, friendly: bool },
    FreehandStroke { points: Vec<Vec2>, color: Color32, width: f32 },
    Line { start: Vec2, end: Vec2, color: Color32, width: f32 },
    Circle { center: Vec2, radius: f32, color: Color32, width: f32, filled: bool },
    Rectangle { center: Vec2, half_size: Vec2, rotation: f32, color: Color32, width: f32, filled: bool },
    Triangle { center: Vec2, radius: f32, rotation: f32, color: Color32, width: f32, filled: bool },
}

/// Active drawing/placement tool.
#[derive(Clone)]
pub enum PaintTool {
    None,
    PlacingShip { species: String, friendly: bool, yaw: f32 },
    Freehand { current_stroke: Option<Vec<Vec2>> },
    Eraser,
    DrawingLine { start: Option<Vec2> },
    DrawingCircle { filled: bool, center: Option<Vec2> },
    DrawingRect { filled: bool, center: Option<Vec2> },
    DrawingTriangle { filled: bool, center: Option<Vec2> },
}

/// Snapshot of annotation state for undo/redo.
#[derive(Clone)]
pub struct AnnotationSnapshot {
    pub annotations: Vec<Annotation>,
    pub ids: Vec<u64>,
    pub owners: Vec<u64>,
}

/// Persistent annotation layer state.
pub struct AnnotationState {
    pub annotations: Vec<Annotation>,
    /// Unique ID for each annotation (parallel to `annotations`).
    pub annotation_ids: Vec<u64>,
    pub undo_stack: Vec<AnnotationSnapshot>,
    pub active_tool: PaintTool,
    pub paint_color: Color32,
    pub stroke_width: f32,
    pub selected_index: Option<usize>,
    pub show_context_menu: bool,
    pub context_menu_pos: Pos2,
    pub dragging_rotation: bool,
    /// Ships whose trails are explicitly hidden (by player name).
    pub trail_hidden_ships: HashSet<String>,
    /// Ship nearest to right-click position (entity_id, player_name) for context menu options.
    pub context_menu_ship: Option<(EntityId, String)>,
    /// Cap point nearest to right-click position (cap id) for context menu options.
    pub context_menu_cap: Option<u64>,
    /// Per-ship range overrides keyed by entity ID.
    pub ship_range_overrides: HashMap<EntityId, ShipConfigFilter>,
    /// Initial self range filter from saved settings, applied once self entity ID is known.
    pub pending_self_range_filter: Option<ShipConfigFilter>,
    /// Owner user_ids for each annotation (parallel to `annotations`), received from collab sync.
    pub annotation_owners: Vec<u64>,
}

impl Default for AnnotationState {
    fn default() -> Self {
        Self {
            annotations: Vec::new(),
            annotation_ids: Vec::new(),
            undo_stack: Vec::new(),
            active_tool: PaintTool::None,
            paint_color: Color32::YELLOW,
            stroke_width: 2.0,
            selected_index: None,
            show_context_menu: false,
            context_menu_pos: Pos2::ZERO,
            dragging_rotation: false,
            trail_hidden_ships: HashSet::new(),
            context_menu_ship: None,
            context_menu_cap: None,
            ship_range_overrides: HashMap::new(),
            pending_self_range_filter: None,
            annotation_owners: Vec::new(),
        }
    }
}

impl AnnotationState {
    /// Save current annotations as an undo snapshot.
    pub fn save_undo(&mut self) {
        self.undo_stack.push(AnnotationSnapshot {
            annotations: self.annotations.clone(),
            ids: self.annotation_ids.clone(),
            owners: self.annotation_owners.clone(),
        });
        // Cap stack size
        if self.undo_stack.len() > 50 {
            self.undo_stack.remove(0);
        }
    }

    /// Pop the last undo snapshot, restoring annotations.
    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.annotations = prev.annotations;
            self.annotation_ids = prev.ids;
            self.annotation_owners = prev.owners;
            self.selected_index = None;
        }
    }
}

// ─── Collab annotation conversion ────────────────────────────────────────────

/// Convert a collab wire annotation (primitive arrays) to the local annotation (egui types).
pub fn collab_annotation_to_local(ca: crate::collab::types::Annotation) -> Annotation {
    use crate::collab::types as ct;
    match ca {
        ct::Annotation::Ship { pos, yaw, species, friendly } => {
            Annotation::Ship { pos: Vec2::new(pos[0], pos[1]), yaw, species, friendly }
        }
        ct::Annotation::FreehandStroke { points, color, width } => Annotation::FreehandStroke {
            points: points.into_iter().map(|p| Vec2::new(p[0], p[1])).collect(),
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
        },
        ct::Annotation::Line { start, end, color, width } => Annotation::Line {
            start: Vec2::new(start[0], start[1]),
            end: Vec2::new(end[0], end[1]),
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
        },
        ct::Annotation::Circle { center, radius, color, width, filled } => Annotation::Circle {
            center: Vec2::new(center[0], center[1]),
            radius,
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
            filled,
        },
        ct::Annotation::Rectangle { center, half_size, rotation, color, width, filled } => Annotation::Rectangle {
            center: Vec2::new(center[0], center[1]),
            half_size: Vec2::new(half_size[0], half_size[1]),
            rotation,
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
            filled,
        },
        ct::Annotation::Triangle { center, radius, rotation, color, width, filled } => Annotation::Triangle {
            center: Vec2::new(center[0], center[1]),
            radius,
            rotation,
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
            filled,
        },
    }
}

/// Convert a local annotation (egui types) to collab wire annotation (primitive arrays).
pub fn local_annotation_to_collab(a: &Annotation) -> crate::collab::types::Annotation {
    use crate::collab::types as ct;
    match a {
        Annotation::Ship { pos, yaw, species, friendly } => {
            ct::Annotation::Ship { pos: [pos.x, pos.y], yaw: *yaw, species: species.clone(), friendly: *friendly }
        }
        Annotation::FreehandStroke { points, color, width } => ct::Annotation::FreehandStroke {
            points: points.iter().map(|p| [p.x, p.y]).collect(),
            color: color.to_array(),
            width: *width,
        },
        Annotation::Line { start, end, color, width } => ct::Annotation::Line {
            start: [start.x, start.y],
            end: [end.x, end.y],
            color: color.to_array(),
            width: *width,
        },
        Annotation::Circle { center, radius, color, width, filled } => ct::Annotation::Circle {
            center: [center.x, center.y],
            radius: *radius,
            color: color.to_array(),
            width: *width,
            filled: *filled,
        },
        Annotation::Rectangle { center, half_size, rotation, color, width, filled } => ct::Annotation::Rectangle {
            center: [center.x, center.y],
            half_size: [half_size.x, half_size.y],
            rotation: *rotation,
            color: color.to_array(),
            width: *width,
            filled: *filled,
        },
        Annotation::Triangle { center, radius, rotation, color, width, filled } => ct::Annotation::Triangle {
            center: [center.x, center.y],
            radius: *radius,
            rotation: *rotation,
            color: color.to_array(),
            width: *width,
            filled: *filled,
        },
    }
}

// ─── Shared collab helpers ──────────────────────────────────────────────────

/// Send a `SetAnnotation` event for the annotation at `idx` via the collab channel.
pub fn send_annotation_update(tx: &Option<mpsc::Sender<LocalEvent>>, ann: &AnnotationState, idx: usize) {
    if let Some(tx) = tx {
        let _ = tx.send(LocalEvent::Annotation(LocalAnnotationEvent::Set {
            id: ann.annotation_ids[idx],
            annotation: local_annotation_to_collab(&ann.annotations[idx]),
            owner: ann.annotation_owners.get(idx).copied().unwrap_or(0),
        }));
    }
}

/// Send a `RemoveAnnotation` event for the given annotation ID via the collab channel.
pub fn send_annotation_remove(tx: &Option<mpsc::Sender<LocalEvent>>, id: u64) {
    if let Some(tx) = tx {
        let _ = tx.send(LocalEvent::Annotation(LocalAnnotationEvent::Remove { id }));
    }
}

/// Send a `ClearAnnotations` event via the collab channel.
pub fn send_annotation_clear(tx: &Option<mpsc::Sender<LocalEvent>>) {
    if let Some(tx) = tx {
        let _ = tx.send(LocalEvent::Annotation(LocalAnnotationEvent::Clear));
    }
}

/// Send a full annotation sync (used after undo to broadcast the complete state).
pub fn send_annotation_full_sync(tx: &Option<mpsc::Sender<collab::SessionCommand>>, ann: &AnnotationState) {
    if let Some(tx) = tx {
        let collab_anns: Vec<_> = ann.annotations.iter().map(local_annotation_to_collab).collect();
        let _ = tx.send(collab::SessionCommand::SyncAnnotations {
            annotations: collab_anns,
            owners: ann.annotation_owners.clone(),
            ids: ann.annotation_ids.clone(),
        });
    }
}

/// Get the local user's ID from the collab session state, or 0 if not in a session.
pub fn get_my_user_id(session: &Option<Arc<Mutex<collab::SessionState>>>) -> u64 {
    session.as_ref().map(|ss| ss.lock().my_user_id).unwrap_or(0)
}

/// Handle a click on empty map space: create a visible ping and optionally
/// notify collab peers.
///
/// - In a session: uses the user's cursor color, pushes to session pings,
///   and sends `LocalEvent::Ping` so peers see it too.
/// - Not in a session: creates a white local-only ping in `local_pings`.
///
/// The relay system does not echo pings back to the sender, so the sender
/// must always add their own ping locally.
pub fn handle_map_click_ping(
    click_pos: Vec2,
    local_pings: &mut Vec<shapes::MapPing>,
    session: &Option<Arc<Mutex<collab::SessionState>>>,
    tx: &Option<mpsc::Sender<LocalEvent>>,
) {
    let pos = [click_pos.x, click_pos.y];

    if let Some(ss_arc) = session {
        let mut ss = ss_arc.lock();
        let my_id = ss.my_user_id;
        let color = ss.cursors.iter().find(|c| c.user_id == my_id).map(|c| c.color).unwrap_or([255, 255, 255]);
        ss.pings.push(collab::PeerPing { user_id: my_id, color, pos, time: std::time::Instant::now() });
    } else {
        local_pings.push(shapes::MapPing { pos, color: [255, 255, 255], time: std::time::Instant::now() });
    }

    if let Some(tx) = tx {
        let _ = tx.send(LocalEvent::Ping(pos));
    }
}

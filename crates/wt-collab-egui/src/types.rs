//! Egui annotation types and wire-format conversions.
//!
//! These mirror the wire-format [`wt_collab_protocol::types::Annotation`] but
//! use egui's [`Vec2`] and [`Color32`] for efficient rendering.

use std::collections::HashMap;
use std::collections::HashSet;

use egui::Color32;
use egui::FontId;
use egui::Pos2;
use egui::Vec2;
use wt_collab_protocol::protocol::EntityId;
use wt_collab_protocol::protocol::ShipConfigFilter;
use wt_collab_protocol::types as wire;

// ─── Constants ───────────────────────────────────────────────────────────────

pub const SHIP_SPECIES: [&str; 5] = ["Destroyer", "Cruiser", "Battleship", "AirCarrier", "Submarine"];
pub const FRIENDLY_COLOR: Color32 = Color32::from_rgb(76, 232, 170);
pub const ENEMY_COLOR: Color32 = Color32::from_rgb(254, 77, 42);

// ─── Capture Point View ─────────────────────────────────────────────────────

/// Rendering-focused description of a capture point zone.
///
/// Contains only the fields needed to draw a cap point on the minimap.
/// Both `WireCapPoint` (protocol) and desktop `TacticsCapPoint` can convert to this.
#[derive(Clone, Debug)]
pub struct CapPointView {
    /// World-space X position (BigWorld).
    pub world_x: f32,
    /// World-space Z position (BigWorld).
    pub world_z: f32,
    /// Zone radius in BigWorld units.
    pub radius: f32,
    /// Team that owns this cap. 0 = green, 1 = red, anything else = neutral.
    pub team_id: i64,
    /// Label index: 0 → "A", 1 → "B", 2 → "C", …
    pub index: u32,
}

impl From<&wt_collab_protocol::protocol::WireCapPoint> for CapPointView {
    fn from(wire: &wt_collab_protocol::protocol::WireCapPoint) -> Self {
        Self {
            world_x: wire.world_x,
            world_z: wire.world_z,
            radius: wire.radius,
            team_id: wire.team_id,
            index: wire.index,
        }
    }
}

// ─── Local Annotation ────────────────────────────────────────────────────────

/// A single annotation on the map, using egui types for rendering.
#[derive(Clone)]
pub enum Annotation {
    Ship { pos: Vec2, yaw: f32, species: String, friendly: bool, config: Option<AnnotationShipConfig> },
    FreehandStroke { points: Vec<Vec2>, color: Color32, width: f32 },
    Line { start: Vec2, end: Vec2, color: Color32, width: f32 },
    Circle { center: Vec2, radius: f32, color: Color32, width: f32, filled: bool },
    Rectangle { center: Vec2, half_size: Vec2, rotation: f32, color: Color32, width: f32, filled: bool },
    Triangle { center: Vec2, radius: f32, rotation: f32, color: Color32, width: f32, filled: bool },
    Arrow { points: Vec<Vec2>, color: Color32, width: f32 },
    Measurement { start: Vec2, end: Vec2, color: Color32, width: f32 },
}

// ─── Annotation Ship Config ─────────────────────────────────────────────────

/// Ship assignment and configuration for Ship annotations (local egui mirror).
#[derive(Clone, Debug)]
pub struct AnnotationShipConfig {
    /// `GameParamId` as raw `u64`.  0 = unassigned.
    pub param_id: u64,
    /// Localized display name (e.g. "Moskva").
    pub ship_name: String,
    /// Selected hull upgrade name. Empty = default hull.
    pub hull_name: String,
    /// Visibility distance coefficient (1.0 = stock).
    pub vis_coeff: f32,
    /// Main battery range coefficient (1.0 = stock).
    pub gm_coeff: f32,
    /// Secondary battery range coefficient (1.0 = stock).
    pub gs_coeff: f32,
    /// Which range circles to display.
    pub range_filter: AnnotationRangeFilter,
}

impl Default for AnnotationShipConfig {
    fn default() -> Self {
        Self {
            param_id: 0,
            ship_name: String::new(),
            hull_name: String::new(),
            vis_coeff: 1.0,
            gm_coeff: 1.0,
            gs_coeff: 1.0,
            range_filter: AnnotationRangeFilter::default(),
        }
    }
}

/// Range circle visibility flags.
#[derive(Clone, Debug, Default)]
pub struct AnnotationRangeFilter {
    pub detection: bool,
    pub main_battery: bool,
    pub secondary_battery: bool,
    pub torpedo: bool,
    pub radar: bool,
    pub hydro: bool,
}

// ─── Paint Tool ──────────────────────────────────────────────────────────────

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
    DrawingArrow { current_stroke: Option<Vec<Vec2>> },
    DrawingMeasurement { start: Option<Vec2> },
}

// ─── Annotation Snapshot ─────────────────────────────────────────────────────

/// Snapshot of annotation state for undo/redo.
#[derive(Clone)]
pub struct AnnotationSnapshot {
    pub annotations: Vec<Annotation>,
    pub ids: Vec<u64>,
    pub owners: Vec<u64>,
}

// ─── Annotation State ────────────────────────────────────────────────────────

/// Persistent annotation layer state used by both desktop and web clients.
pub struct AnnotationState {
    pub annotations: Vec<Annotation>,
    /// Unique ID for each annotation (parallel to `annotations`).
    pub annotation_ids: Vec<u64>,
    pub undo_stack: Vec<AnnotationSnapshot>,
    pub active_tool: PaintTool,
    pub paint_color: Color32,
    pub stroke_width: f32,
    pub selected_indices: HashSet<usize>,
    pub show_context_menu: bool,
    pub context_menu_pos: Pos2,
    pub dragging_rotation: bool,
    /// Which measurement endpoint is being dragged (0=start, 1=end), if any.
    pub dragging_measurement_endpoint: Option<u8>,
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
            selected_indices: HashSet::new(),
            show_context_menu: false,
            context_menu_pos: Pos2::ZERO,
            dragging_rotation: false,
            dragging_measurement_endpoint: None,
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
            self.selected_indices.clear();
        }
    }

    /// Returns the single selected index, if exactly one is selected.
    pub fn single_selected(&self) -> Option<usize> {
        if self.selected_indices.len() == 1 { self.selected_indices.iter().next().copied() } else { None }
    }

    /// Returns `true` if at least one annotation is selected.
    pub fn has_selection(&self) -> bool {
        !self.selected_indices.is_empty()
    }

    /// Clear all selection.
    pub fn clear_selection(&mut self) {
        self.selected_indices.clear();
    }

    /// Select exactly one annotation index.
    pub fn select_single(&mut self, idx: usize) {
        self.selected_indices.clear();
        self.selected_indices.insert(idx);
    }
}

// ─── Wire ↔ Local Conversion ─────────────────────────────────────────────────

fn wire_ship_config_to_local(c: wire::AnnotationShipConfig) -> AnnotationShipConfig {
    AnnotationShipConfig {
        param_id: c.param_id,
        ship_name: c.ship_name,
        hull_name: c.hull_name,
        vis_coeff: c.vis_coeff,
        gm_coeff: c.gm_coeff,
        gs_coeff: c.gs_coeff,
        range_filter: AnnotationRangeFilter {
            detection: c.range_filter.detection,
            main_battery: c.range_filter.main_battery,
            secondary_battery: c.range_filter.secondary_battery,
            torpedo: c.range_filter.torpedo,
            radar: c.range_filter.radar,
            hydro: c.range_filter.hydro,
        },
    }
}

fn local_ship_config_to_wire(c: &AnnotationShipConfig) -> wire::AnnotationShipConfig {
    wire::AnnotationShipConfig {
        param_id: c.param_id,
        ship_name: c.ship_name.clone(),
        hull_name: c.hull_name.clone(),
        vis_coeff: c.vis_coeff,
        gm_coeff: c.gm_coeff,
        gs_coeff: c.gs_coeff,
        range_filter: wire::AnnotationRangeFilter {
            detection: c.range_filter.detection,
            main_battery: c.range_filter.main_battery,
            secondary_battery: c.range_filter.secondary_battery,
            torpedo: c.range_filter.torpedo,
            radar: c.range_filter.radar,
            hydro: c.range_filter.hydro,
        },
    }
}

/// Convert a wire annotation (primitive arrays) to a local annotation (egui types).
pub fn wire_to_local(ca: wire::Annotation) -> Annotation {
    match ca {
        wire::Annotation::Ship { pos, yaw, species, friendly, config } => Annotation::Ship {
            pos: Vec2::new(pos[0], pos[1]),
            yaw,
            species,
            friendly,
            config: config.map(wire_ship_config_to_local),
        },
        wire::Annotation::FreehandStroke { points, color, width } => Annotation::FreehandStroke {
            points: points.into_iter().map(|p| Vec2::new(p[0], p[1])).collect(),
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
        },
        wire::Annotation::Line { start, end, color, width } => Annotation::Line {
            start: Vec2::new(start[0], start[1]),
            end: Vec2::new(end[0], end[1]),
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
        },
        wire::Annotation::Circle { center, radius, color, width, filled } => Annotation::Circle {
            center: Vec2::new(center[0], center[1]),
            radius,
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
            filled,
        },
        wire::Annotation::Rectangle { center, half_size, rotation, color, width, filled } => Annotation::Rectangle {
            center: Vec2::new(center[0], center[1]),
            half_size: Vec2::new(half_size[0], half_size[1]),
            rotation,
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
            filled,
        },
        wire::Annotation::Triangle { center, radius, rotation, color, width, filled } => Annotation::Triangle {
            center: Vec2::new(center[0], center[1]),
            radius,
            rotation,
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
            filled,
        },
        wire::Annotation::Arrow { points, color, width } => Annotation::Arrow {
            points: points.into_iter().map(|p| Vec2::new(p[0], p[1])).collect(),
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
        },
        wire::Annotation::Measurement { start, end, color, width } => Annotation::Measurement {
            start: Vec2::new(start[0], start[1]),
            end: Vec2::new(end[0], end[1]),
            color: Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
            width,
        },
    }
}

/// Convert a local annotation (egui types) to wire annotation (primitive arrays).
pub fn local_to_wire(a: &Annotation) -> wire::Annotation {
    match a {
        Annotation::Ship { pos, yaw, species, friendly, config } => wire::Annotation::Ship {
            pos: [pos.x, pos.y],
            yaw: *yaw,
            species: species.clone(),
            friendly: *friendly,
            config: config.as_ref().map(local_ship_config_to_wire),
        },
        Annotation::FreehandStroke { points, color, width } => wire::Annotation::FreehandStroke {
            points: points.iter().map(|p| [p.x, p.y]).collect(),
            color: color.to_array(),
            width: *width,
        },
        Annotation::Line { start, end, color, width } => wire::Annotation::Line {
            start: [start.x, start.y],
            end: [end.x, end.y],
            color: color.to_array(),
            width: *width,
        },
        Annotation::Circle { center, radius, color, width, filled } => wire::Annotation::Circle {
            center: [center.x, center.y],
            radius: *radius,
            color: color.to_array(),
            width: *width,
            filled: *filled,
        },
        Annotation::Rectangle { center, half_size, rotation, color, width, filled } => wire::Annotation::Rectangle {
            center: [center.x, center.y],
            half_size: [half_size.x, half_size.y],
            rotation: *rotation,
            color: color.to_array(),
            width: *width,
            filled: *filled,
        },
        Annotation::Triangle { center, radius, rotation, color, width, filled } => wire::Annotation::Triangle {
            center: [center.x, center.y],
            radius: *radius,
            rotation: *rotation,
            color: color.to_array(),
            width: *width,
            filled: *filled,
        },
        Annotation::Arrow { points, color, width } => wire::Annotation::Arrow {
            points: points.iter().map(|p| [p.x, p.y]).collect(),
            color: color.to_array(),
            width: *width,
        },
        Annotation::Measurement { start, end, color, width } => wire::Annotation::Measurement {
            start: [start.x, start.y],
            end: [end.x, end.y],
            color: color.to_array(),
            width: *width,
        },
    }
}

// ─── Shared Session Types ────────────────────────────────────────────────────

/// A click ripple on the map, in minimap coordinates.
pub struct MapPing {
    pub pos: [f32; 2],
    pub color: [u8; 3],
    pub time: web_time::Instant,
}

/// Remote cursor position.
#[derive(Debug, Clone)]
pub struct UserCursor {
    pub user_id: u64,
    pub name: String,
    pub color: [u8; 3],
    pub pos: Option<[f32; 2]>,
    pub last_update: web_time::Instant,
}

/// Permission flags controlled by the host/co-host.
#[derive(Debug, Clone, Default)]
pub struct Permissions {
    pub annotations_locked: bool,
    pub settings_locked: bool,
}

/// Style parameters for the 10×10 minimap grid overlay.
pub struct GridStyle {
    pub grid_color: Color32,
    pub label_color: Color32,
    pub line_width: f32,
    pub label_font: FontId,
}

impl Default for GridStyle {
    fn default() -> Self {
        Self {
            grid_color: Color32::from_rgba_unmultiplied(180, 180, 180, 32),
            label_color: Color32::from_rgba_unmultiplied(200, 200, 200, 180),
            line_width: 0.5,
            label_font: FontId::proportional(9.0),
        }
    }
}

/// Short display name for ship species.
pub fn ship_short_name(species: &str) -> &str {
    match species {
        "Destroyer" => "DD",
        "Cruiser" => "CA",
        "Battleship" => "BB",
        "AirCarrier" => "CV",
        "Submarine" => "SS",
        _ => species,
    }
}

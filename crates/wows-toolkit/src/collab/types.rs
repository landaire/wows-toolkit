//! Shared types for collaborative replay sessions.
//!
//! These types are serializable with rkyv for wire transport and also used
//! directly in the annotation/paint UI layer.

use egui::Color32;
use egui::Pos2;
use egui::Vec2;

/// A single annotation placed on the minimap.
///
/// Coordinates are in minimap pixel space (0..760 native, but annotations
/// may extend slightly beyond for off-edge drawings).
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Annotation {
    Ship { pos: [f32; 2], yaw: f32, species: String, friendly: bool },
    FreehandStroke { points: Vec<[f32; 2]>, color: [u8; 4], width: f32 },
    Line { start: [f32; 2], end: [f32; 2], color: [u8; 4], width: f32 },
    Circle { center: [f32; 2], radius: f32, color: [u8; 4], width: f32, filled: bool },
    Rectangle { center: [f32; 2], half_size: [f32; 2], rotation: f32, color: [u8; 4], width: f32, filled: bool },
    Triangle { center: [f32; 2], radius: f32, rotation: f32, color: [u8; 4], width: f32, filled: bool },
}

/// Active drawing/placement tool state.
#[derive(Clone, Debug)]
pub enum PaintTool {
    None,
    PlacingShip { species: String, friendly: bool, yaw: f32 },
    Freehand { current_stroke: Option<Vec<[f32; 2]>> },
    Eraser,
    DrawingLine { start: Option<[f32; 2]> },
    DrawingCircle { filled: bool, center: Option<[f32; 2]> },
    DrawingRect { filled: bool, center: Option<[f32; 2]> },
    DrawingTriangle { filled: bool, center: Option<[f32; 2]> },
}

// ─── Conversion helpers: egui types ↔ primitive arrays ──────────────────────

#[inline]
pub fn vec2_to_arr(v: Vec2) -> [f32; 2] {
    [v.x, v.y]
}

#[inline]
pub fn arr_to_vec2(a: [f32; 2]) -> Vec2 {
    Vec2::new(a[0], a[1])
}

#[inline]
pub fn pos2_to_arr(p: Pos2) -> [f32; 2] {
    [p.x, p.y]
}

#[inline]
pub fn arr_to_pos2(a: [f32; 2]) -> Pos2 {
    Pos2::new(a[0], a[1])
}

#[inline]
pub fn color32_to_arr(c: Color32) -> [u8; 4] {
    c.to_array()
}

#[inline]
pub fn arr_to_color32(a: [u8; 4]) -> Color32 {
    Color32::from_rgba_premultiplied(a[0], a[1], a[2], a[3])
}

/// Fixed palette of 12 distinct cursor colors for collaborative sessions.
/// Index 0 is reserved for the host. Clients assigned round-robin from index 1.
pub const CURSOR_COLORS: [[u8; 3]; 12] = [
    [255, 87, 87],   // red (host)
    [78, 205, 196],  // teal
    [255, 195, 0],   // amber
    [136, 84, 208],  // purple
    [0, 200, 83],    // green
    [255, 138, 101], // coral
    [41, 182, 246],  // sky blue
    [255, 167, 38],  // orange
    [171, 71, 188],  // magenta
    [102, 187, 106], // light green
    [255, 112, 67],  // deep orange
    [38, 166, 154],  // teal dark
];

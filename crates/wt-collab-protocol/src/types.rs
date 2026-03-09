//! Shared types for collaborative replay sessions.
//!
//! These types are serializable with rkyv for wire transport and also used
//! directly in the annotation/paint UI layer.

/// A single annotation placed on the minimap.
///
/// Coordinates are in minimap pixel space (0..760 native, but annotations
/// may extend slightly beyond for off-edge drawings).
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Annotation {
    Ship { pos: [f32; 2], yaw: f32, species: String, friendly: bool, config: Option<AnnotationShipConfig> },
    FreehandStroke { points: Vec<[f32; 2]>, color: [u8; 4], width: f32 },
    Line { start: [f32; 2], end: [f32; 2], color: [u8; 4], width: f32 },
    Circle { center: [f32; 2], radius: f32, color: [u8; 4], width: f32, filled: bool },
    Rectangle { center: [f32; 2], half_size: [f32; 2], rotation: f32, color: [u8; 4], width: f32, filled: bool },
    Triangle { center: [f32; 2], radius: f32, rotation: f32, color: [u8; 4], width: f32, filled: bool },
    Arrow { points: Vec<[f32; 2]>, color: [u8; 4], width: f32 },
    Measurement { start: [f32; 2], end: [f32; 2], color: [u8; 4], width: f32 },
}

/// Optional ship assignment and configuration for Ship annotations.
///
/// Stores the ship identity (param_id + display name), selected hull, modifier
/// coefficients, and which range circles are visible.  Coefficients are pre-computed
/// products of all relevant captain skills / modernizations so the protocol layer
/// stays independent of game-data structures.
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct AnnotationShipConfig {
    /// `GameParamId` as raw `u64`.  0 = unassigned.
    pub param_id: u64,
    /// Localized display name (e.g. "Moskva").
    pub ship_name: String,
    /// Selected hull upgrade name (e.g. "PRUH510_Moskva_1"). Empty = default hull.
    pub hull_name: String,
    /// Visibility distance coefficient (product of skills/mods). 1.0 = stock.
    pub vis_coeff: f32,
    /// Main battery max distance coefficient. 1.0 = stock.
    pub gm_coeff: f32,
    /// Secondary battery max distance coefficient. 1.0 = stock.
    pub gs_coeff: f32,
    /// Which range circles to display.
    pub range_filter: AnnotationRangeFilter,
}

/// Range circle visibility flags for an annotation ship.
#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct AnnotationRangeFilter {
    pub detection: bool,
    pub main_battery: bool,
    pub secondary_battery: bool,
    pub torpedo: bool,
    pub radar: bool,
    pub hydro: bool,
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

#[cfg(feature = "egui")]
mod egui_helpers {
    use egui::Color32;
    use egui::Pos2;
    use egui::Vec2;

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
}

#[cfg(feature = "egui")]
pub use egui_helpers::*;

// ─── Color utilities ────────────────────────────────────────────────────────

/// Derive a distinct cursor color from a display name using HSV with full
/// saturation and value. The hue is determined by a simple hash of the
/// name bytes, giving each user a stable, recognisable color.
pub fn color_from_name(name: &str) -> [u8; 3] {
    // FNV-1a 32-bit hash for a cheap, well-distributed spread.
    let mut h: u32 = 2_166_136_261;
    for &b in name.as_bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16_777_619);
    }
    let hue = (h % 360) as f32; // 0..360
    let saturation = 0.75_f32; // vivid but not eye-burning
    let value = 0.90_f32; // bright but not white
    hsv_to_rgb(hue, saturation, value)
}

/// Convert HSV (h in 0..360, s/v in 0..1) to an RGB [u8; 3].
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = match h as u32 {
        0..60 => (c, x, 0.0),
        60..120 => (x, c, 0.0),
        120..180 => (0.0, c, x),
        180..240 => (0.0, x, c),
        240..300 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [((r1 + m) * 255.0) as u8, ((g1 + m) * 255.0) as u8, ((b1 + m) * 255.0) as u8]
}

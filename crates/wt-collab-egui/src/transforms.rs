//! Coordinate transforms for the minimap viewport.

use egui::Pos2;
use egui::Rect;
use egui::Vec2;
use wows_minimap_renderer::MinimapPos;

/// Zoom and pan state for the minimap viewport. Persists across frames.
pub struct ViewportZoomPan {
    /// Zoom level. 1.0 = no zoom (fit to window). Range: [1.0, 10.0].
    pub zoom: f32,
    /// Pan offset in zoomed-minimap-pixel space.
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

/// Result of `compute_canvas_layout()`: the window scale and positioning
/// needed to render a logical canvas into the available UI area.
pub struct CanvasLayout {
    /// Uniform scale from logical canvas pixels to screen pixels.
    /// Blends from fit (full canvas visible) at zoom 1.0 to fill (no borders) at zoom 2.0+.
    pub window_scale: f32,
    /// Top-left origin of the scaled canvas in screen space.
    pub origin: Pos2,
    /// Size of the scaled canvas in screen pixels.
    pub scaled_canvas: Vec2,
}

/// Compute the canvas layout: window scale, origin, and scaled size.
///
/// Smoothly blends between fit-scale (entire canvas visible, letterboxed) at zoom 1.0
/// and fill-scale (no empty borders) at zoom 2.0+, centering within the available rect.
pub fn compute_canvas_layout(available: Vec2, logical_canvas: Vec2, zoom: f32, rect_min: Pos2) -> CanvasLayout {
    let scale_x = available.x / logical_canvas.x;
    let scale_y = available.y / logical_canvas.y;
    let fit_scale = scale_x.min(scale_y);
    let fill_scale = scale_x.max(scale_y);
    let t = ((zoom - 1.0) / 1.0).clamp(0.0, 1.0);
    let window_scale = (fit_scale + t * (fill_scale - fit_scale)).max(0.1);

    let scaled_canvas = logical_canvas * window_scale;
    let offset_x = ((available.x - scaled_canvas.x) / 2.0).max(0.0);
    let offset_y = ((available.y - scaled_canvas.y) / 2.0).max(0.0);
    let origin = rect_min + Vec2::new(offset_x, offset_y);

    CanvasLayout { window_scale, origin, scaled_canvas }
}

/// Compute the clip rect for map elements, excluding HUD area at top.
///
/// For viewports with a HUD (replay, web replay view), pass the HUD height in logical pixels.
/// For viewports without a HUD (tactics board), pass 0.0.
pub fn compute_map_clip_rect(layout: &CanvasLayout, hud_height: f32) -> Rect {
    let hud_screen_height = hud_height * layout.window_scale;
    Rect::from_min_max(
        Pos2::new(layout.origin.x, layout.origin.y + hud_screen_height),
        Pos2::new(layout.origin.x + layout.scaled_canvas.x, layout.origin.y + layout.scaled_canvas.y),
    )
}

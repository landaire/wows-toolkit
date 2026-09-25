//! Which part of the map a frame shows.
//!
//! A renderer draws the map at a fixed [`MINIMAP_SIZE`]. Zooming means
//! drawing it larger and showing a window onto it, which is what this
//! describes: the enlargement, and where the window's top-left corner sits in
//! the enlarged map.

use crate::MINIMAP_SIZE;

/// The closest the map can be drawn to its own size, which is the whole map.
pub const MIN_ZOOM: f32 = 1.0;

/// How far in the map can be drawn. The egui viewer's own limit: past this a
/// ship icon is larger than the island it is beside.
pub const MAX_ZOOM: f32 = 10.0;

/// The window onto the map a frame is drawn through.
///
/// `zoom` is how much larger than [`MINIMAP_SIZE`] the drawn map is; `pan` is
/// the window's top-left corner in those enlarged pixels. The default shows
/// the whole map, which is what an export and the egui app's own target
/// want.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapViewport {
    zoom: f32,
    pan: (f32, f32),
}

impl Default for MapViewport {
    fn default() -> Self {
        Self { zoom: MIN_ZOOM, pan: (0.0, 0.0) }
    }
}

impl MapViewport {
    /// The window `zoom` and `pan` name, held inside the map.
    ///
    /// A pan past an edge would show background where the map should be, so
    /// it is clamped rather than refused: a drag that runs off the edge stops
    /// there.
    pub fn new(zoom: f32, pan: (f32, f32)) -> Self {
        let zoom = if zoom.is_finite() { zoom.clamp(MIN_ZOOM, MAX_ZOOM) } else { MIN_ZOOM };
        let span = MINIMAP_SIZE as f32 * (zoom - 1.0);
        let hold = |value: f32| if value.is_finite() { value.clamp(0.0, span) } else { 0.0 };
        Self { zoom, pan: (hold(pan.0), hold(pan.1)) }
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn pan(&self) -> (f32, f32) {
        self.pan
    }

    /// Whether this window is the whole map, which is the case a renderer can
    /// draw without transforming anything.
    pub fn is_whole_map(&self) -> bool {
        self.zoom == MIN_ZOOM
    }

    /// Where map-space `x` lands in the drawn map.
    pub fn x(&self, x: f32) -> f32 {
        x * self.zoom - self.pan.0
    }

    /// Where map-space `y` lands in the drawn map.
    pub fn y(&self, y: f32) -> f32 {
        y * self.zoom - self.pan.1
    }

    /// A map-space length, as drawn.
    ///
    /// Radii and icons grow with the map; stroke widths do not, which is what
    /// keeps a line readable at every zoom.
    pub fn length(&self, distance: f32) -> f32 {
        distance * self.zoom
    }

    /// The map-space point a drawn point came from.
    pub fn to_map(&self, x: f32, y: f32) -> (f32, f32) {
        ((x + self.pan.0) / self.zoom, (y + self.pan.1) / self.zoom)
    }

    /// The window at `zoom` that keeps whatever is under the drawn point
    /// `at` where it is.
    ///
    /// This is what a scroll wheel wants: the map grows around the pointer
    /// rather than around a corner.
    pub fn zoomed_about(&self, zoom: f32, at: (f32, f32)) -> Self {
        let (map_x, map_y) = self.to_map(at.0, at.1);
        let zoom = if zoom.is_finite() { zoom.clamp(MIN_ZOOM, MAX_ZOOM) } else { MIN_ZOOM };
        Self::new(zoom, (map_x * zoom - at.0, map_y * zoom - at.1))
    }

    /// The window moved by a drag of `delta` drawn pixels.
    ///
    /// The map follows the pointer, so the window moves against it.
    pub fn dragged(&self, delta: (f32, f32)) -> Self {
        Self::new(self.zoom, (self.pan.0 - delta.0, self.pan.1 - delta.1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window cannot leave the map, whichever way it is asked to.
    #[test]
    fn a_pan_stops_at_the_map_edge() {
        let whole = MapViewport::new(MIN_ZOOM, (400.0, 400.0));
        assert_eq!(whole.pan(), (0.0, 0.0), "the whole map has nowhere to pan to");

        let doubled = MapViewport::new(2.0, (0.0, 0.0));
        let span = MINIMAP_SIZE as f32;
        assert_eq!(doubled.dragged((-10_000.0, -10_000.0)).pan(), (span, span), "the far corner, not past it");
        assert_eq!(doubled.dragged((10_000.0, 10_000.0)).pan(), (0.0, 0.0), "and the near one");
    }

    /// Zoom is held to what the renderer can draw.
    #[test]
    fn zoom_stays_within_what_can_be_drawn() {
        assert_eq!(MapViewport::new(0.1, (0.0, 0.0)).zoom(), MIN_ZOOM);
        assert_eq!(MapViewport::new(1_000.0, (0.0, 0.0)).zoom(), MAX_ZOOM);
        assert_eq!(MapViewport::new(f32::NAN, (0.0, 0.0)).zoom(), MIN_ZOOM, "and a non-number is not a zoom");
    }

    /// Scrolling zooms around the pointer: whatever was under it stays there.
    #[test]
    fn zooming_about_a_point_leaves_it_where_it_was() {
        let start = MapViewport::new(2.0, (300.0, 200.0));
        let at = (150.0, 400.0);
        let before = start.to_map(at.0, at.1);

        let zoomed = start.zoomed_about(4.0, at);
        let after = zoomed.to_map(at.0, at.1);

        assert!((before.0 - after.0).abs() < 0.01, "{before:?} vs {after:?}");
        assert!((before.1 - after.1).abs() < 0.01, "{before:?} vs {after:?}");
    }

    /// A point maps to the drawn map and back to itself.
    #[test]
    fn the_map_and_the_drawing_agree_on_where_a_point_is() {
        let view = MapViewport::new(3.0, (500.0, 120.0));
        let (x, y) = (321.0, 654.0);
        let (back_x, back_y) = view.to_map(view.x(x), view.y(y));
        assert!((back_x - x).abs() < 0.01);
        assert!((back_y - y).abs() < 0.01);
    }
}

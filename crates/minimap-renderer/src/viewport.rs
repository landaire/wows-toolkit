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
    widen: f32,
}

impl Default for MapViewport {
    fn default() -> Self {
        Self { zoom: MIN_ZOOM, pan: (0.0, 0.0), widen: 1.0 }
    }
}

impl MapViewport {
    /// The window `zoom` and `pan` name, held inside the map.
    ///
    /// A pan past an edge would show background where the map should be, so
    /// it is clamped rather than refused: a drag that runs off the edge stops
    /// there.
    pub fn new(zoom: f32, pan: (f32, f32)) -> Self {
        Self::shaped(zoom, pan, 1.0)
    }

    /// The same, on a window `widen` times wider than it is tall.
    fn shaped(zoom: f32, pan: (f32, f32), widen: f32) -> Self {
        let zoom = if zoom.is_finite() { zoom.clamp(MIN_ZOOM, MAX_ZOOM) } else { MIN_ZOOM };
        // A window cannot be wider than the map it looks at: at zoom Z the
        // map is Z times the window's height, so that is as wide as it can
        // be asked to be.
        let widen = if widen.is_finite() { widen.clamp(1.0, zoom) } else { 1.0 };
        let across = MINIMAP_SIZE as f32 * (zoom - widen);
        let down = MINIMAP_SIZE as f32 * (zoom - 1.0);
        let hold = |value: f32, span: f32| if value.is_finite() { value.clamp(0.0, span) } else { 0.0 };
        Self { zoom, pan: (hold(pan.0, across), hold(pan.1, down)), widen }
    }

    /// How many times wider than tall the window is.
    ///
    /// One at rest, so a frame is square and an exported video is the shape
    /// every other one is.
    pub fn widen(&self) -> f32 {
        self.widen
    }

    /// The same window, stretched toward `aspect` as far as the zoom allows.
    ///
    /// A viewport wider than it is tall leaves the map with empty space
    /// either side of it. Zoomed in there is map to put there, so the window
    /// widens to take it; at rest there is not, and it stays square. What is
    /// between the two follows the zoom, so the sides fill gradually rather
    /// than snapping open.
    pub fn widened(&self, aspect: f32) -> Self {
        Self::shaped(self.zoom, self.pan, aspect)
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

    /// How wide the drawn map is, in drawn pixels.
    pub fn drawn_width(&self) -> f32 {
        MINIMAP_SIZE as f32 * self.widen
    }

    /// The window at `zoom` that keeps whatever is under the drawn point
    /// `at` where it is.
    ///
    /// This is what a scroll wheel wants: the map grows around the pointer
    /// rather than around a corner.
    pub fn zoomed_about(&self, zoom: f32, at: (f32, f32)) -> Self {
        let (map_x, map_y) = self.to_map(at.0, at.1);
        let zoom = if zoom.is_finite() { zoom.clamp(MIN_ZOOM, MAX_ZOOM) } else { MIN_ZOOM };
        Self::shaped(zoom, (map_x * zoom - at.0, map_y * zoom - at.1), self.widen)
    }

    /// The window moved by a drag of `delta` drawn pixels.
    ///
    /// The map follows the pointer, so the window moves against it.
    pub fn dragged(&self, delta: (f32, f32)) -> Self {
        Self::shaped(self.zoom, (self.pan.0 - delta.0, self.pan.1 - delta.1), self.widen)
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

    /// At rest the window is square however wide the viewport is: there is
    /// no more map to put beside it, so widening would show background.
    #[test]
    fn the_whole_map_stays_square() {
        let whole = MapViewport::default().widened(16.0 / 9.0);
        assert_eq!(whole.widen(), 1.0);
        assert_eq!(whole.drawn_width(), MINIMAP_SIZE as f32);
    }

    /// Zoomed in there is map either side to take, so the window widens to
    /// the shape it is asked for.
    #[test]
    fn a_zoomed_window_takes_the_width_it_is_offered() {
        let wide = MapViewport::new(4.0, (0.0, 0.0)).widened(16.0 / 9.0);
        assert!((wide.widen() - 16.0 / 9.0).abs() < 1e-4, "{}", wide.widen());
        assert!((wide.drawn_width() - MINIMAP_SIZE as f32 * 16.0 / 9.0).abs() < 1e-2);
    }

    /// Between the two it follows the zoom, so the sides fill gradually
    /// rather than snapping open: a window cannot be wider than the map it
    /// is looking at.
    #[test]
    fn widening_is_held_to_what_the_zoom_has_to_show() {
        let part_way = MapViewport::new(1.3, (0.0, 0.0)).widened(16.0 / 9.0);
        assert!((part_way.widen() - 1.3).abs() < 1e-4, "held to the zoom: {}", part_way.widen());

        // And the map never runs out from under the window: the widest it
        // can be is the whole map's width.
        let widest = MapViewport::new(2.0, (0.0, 0.0)).widened(99.0);
        assert_eq!(widest.widen(), 2.0);
        assert_eq!(widest.pan().0, 0.0, "which leaves nowhere to pan across to");
    }

    /// A wider window has less room to pan across, since it already shows
    /// more of the map.
    #[test]
    fn a_wider_window_has_less_to_pan_across() {
        let square = MapViewport::new(4.0, (10_000.0, 10_000.0));
        let wide = square.widened(2.0);
        assert!(wide.pan().0 < square.pan().0, "{:?} against {:?}", wide.pan(), square.pan());
        assert_eq!(wide.pan().1, square.pan().1, "and the same room up and down");
    }

    /// A drag and a wheel keep the shape the window was given.
    #[test]
    fn moving_a_wide_window_keeps_it_wide() {
        let wide = MapViewport::new(4.0, (100.0, 100.0)).widened(1.5);
        assert_eq!(wide.dragged((10.0, 10.0)).widen(), 1.5);
        assert_eq!(wide.zoomed_about(6.0, (10.0, 10.0)).widen(), 1.5);
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

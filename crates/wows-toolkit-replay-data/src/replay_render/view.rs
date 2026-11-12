//! Which part of the map the viewport shows, and turning pointer positions
//! into map points and back.
//!
//! Three spaces meet here. A [`WindowPoint`] is where a pointer is in the
//! window, an [`ElementPoint`] is a position inside the element the frame is
//! painted in, and a map point is a `(f32, f32)` in the map's own pixels,
//! which is what a ship's position and a peer's cursor are both in.

use wows_minimap_renderer::CANVAS_HEIGHT;
use wows_minimap_renderer::HUD_HEIGHT;
use wows_minimap_renderer::MINIMAP_SIZE;
use wows_minimap_renderer::viewport::MapViewport;

/// How much of a zoom one pixel of wheel travel is. The egui renderer's own
/// rate, so the wheel feels the same in both.
pub const ZOOM_PER_PIXEL: f32 = 0.01;

/// The strip the renderer reserves above the map, which is where the score
/// bar and the advantage label are drawn.
pub const HUD_STRIP_HEIGHT: f32 = HUD_HEIGHT as f32;

/// A position in the window, in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindowPoint {
    pub x: f32,
    pub y: f32,
}

/// A position inside the element the frame is painted in, in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ElementPoint {
    pub x: f32,
    pub y: f32,
}

/// Where the frame's element sits in the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElementBounds {
    pub origin: WindowPoint,
    pub width: f32,
    pub height: f32,
}

/// The size of a rasterised frame, in its own pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameSize {
    pub width: u32,
    pub height: u32,
}

/// Where a frame was painted: the element it fills, and the frame's own size.
///
/// The frame is drawn to fit inside its element without being stretched, so
/// the two together say where every frame pixel landed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub bounds: ElementBounds,
    pub frame: FrameSize,
}

/// Where the frame's HUD strip lands inside the viewport element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudStrip {
    pub left: f32,
    pub top: f32,
    pub height: f32,
}

/// What a turn of the wheel did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wheel {
    /// The pointer was not over the map, or the wheel did not move.
    Ignored,
    /// The wheel was over the map but the view is already as far as it goes.
    Unchanged,
    Zoomed,
}

/// Which part of the map the viewport shows, and where its frame was last
/// painted.
#[derive(Clone, Debug, PartialEq)]
pub struct MapView {
    view: MapViewport,
    /// Recorded as the frame is painted. A pointer position means nothing
    /// without it, and only the painter knows.
    placement: Option<Placement>,
    /// Where a drag of the map last was.
    dragging: Option<WindowPoint>,
}

impl MapView {
    pub fn new() -> Self {
        Self { view: MapViewport::default(), placement: None, dragging: None }
    }

    pub fn view(&self) -> MapViewport {
        self.view
    }

    /// Shows a different part of the map. Returns whether it changed, which
    /// is when the frame on screen has to be drawn again.
    pub fn set_view(&mut self, view: MapViewport) -> bool {
        if view == self.view {
            return false;
        }
        self.view = view;

        true
    }

    /// Puts the whole map back.
    pub fn reset(&mut self) -> bool {
        self.set_view(MapViewport::default())
    }

    pub fn placement(&self) -> Option<Placement> {
        self.placement
    }

    /// Records where the frame was painted.
    pub fn set_placement(&mut self, placement: Option<Placement>) {
        self.placement = placement;
    }

    /// How many screen pixels one pixel of the drawn frame covers.
    ///
    /// `None` before the frame has been painted once, which is the only time
    /// nothing knows where it is.
    pub fn drawn_scale(&self) -> Option<f32> {
        let placement = self.placement?;
        let (width, height) = (placement.frame.width as f32, placement.frame.height as f32);
        if width <= 0.0 || height <= 0.0 {
            return None;
        }

        // One scale covers both directions, since the frame is not stretched.
        Some((placement.bounds.width / width).min(placement.bounds.height / height))
    }

    /// Where the frame's HUD strip lands inside the viewport element.
    ///
    /// The frame is drawn to fit without stretching, so it is letterboxed:
    /// the strip is not at the element's own top edge.
    pub fn hud_strip(&self) -> Option<HudStrip> {
        let placement = self.placement?;
        let scale = self.drawn_scale()?;
        let drawn_width = placement.frame.width as f32 * scale;
        let drawn_height = placement.frame.height as f32 * scale;
        let left = (placement.bounds.width - drawn_width) / 2.0;
        let top = (placement.bounds.height - drawn_height) / 2.0;

        Some(HudStrip { left, top, height: HUD_STRIP_HEIGHT * scale })
    }

    /// The window as it is drawn: stretched toward the shape of the element
    /// it is drawn in.
    ///
    /// A wide viewport leaves the map with empty space either side. Zoomed
    /// in there is map to put there, so the window takes it; at rest there is
    /// not, and the frame stays square.
    pub fn shaped_view(&self) -> MapViewport {
        let Some(placement) = self.placement else { return self.view };
        let (width, height) = (placement.bounds.width, placement.bounds.height);
        if height <= 0.0 || width <= 0.0 {
            return self.view;
        }

        // Measured against the whole canvas, HUD strip included, since that
        // is what is fitted into the viewport.
        let wanted = width / height * CANVAS_HEIGHT as f32 / MINIMAP_SIZE as f32;
        self.view.widened(wanted)
    }

    /// Where a window position lands on the whole rasterised canvas, which is
    /// the map and whatever gutters are beside it.
    ///
    /// `None` before the frame has been painted once. Not clamped to the
    /// canvas: a caller asks what is there, and the answer for a point off
    /// the edge is nothing.
    pub fn canvas_point(&self, position: WindowPoint) -> Option<(f32, f32)> {
        let placement = self.placement?;
        let scale = self.drawn_scale()?;
        let drawn_width = placement.frame.width as f32 * scale;
        let drawn_height = placement.frame.height as f32 * scale;
        let left = placement.bounds.origin.x + (placement.bounds.width - drawn_width) / 2.0;
        let top = placement.bounds.origin.y + (placement.bounds.height - drawn_height) / 2.0;

        Some(((position.x - left) / scale, (position.y - top) / scale))
    }

    /// Where in the drawn map a window position falls, after the zoom and pan
    /// have been applied.
    ///
    /// What a zoom about the pointer needs, since it re-anchors on a drawn
    /// point rather than a map one. `map_origin` is where the map's corner
    /// sits on the canvas, which a track carries.
    pub fn drawn_point(&self, position: WindowPoint, map_origin: (f32, f32)) -> Option<(f32, f32)> {
        let (x, y) = self.canvas_point(position)?;
        let (x, y) = (x - map_origin.0, y - map_origin.1);
        let span = MINIMAP_SIZE as f32;

        (x >= 0.0 && x < span && y >= 0.0 && y < span).then_some((x, y))
    }

    /// Where in the map a window position falls, in the map's own pixels.
    ///
    /// `None` when the pointer is not over the map: on the HUD strip, or in
    /// the margin beside a frame that does not fill its element.
    pub fn map_point(&self, position: WindowPoint, map_origin: (f32, f32)) -> Option<(f32, f32)> {
        let (x, y) = self.drawn_point(position, map_origin)?;
        Some(self.shaped_view().to_map(x, y))
    }

    /// Where a map point lands inside the viewport element.
    ///
    /// The inverse of [`Self::map_point`], which is what puts a peer's cursor
    /// where they are pointing. `None` before the frame has been painted once,
    /// and for a point the viewport is not currently showing.
    pub fn element_point(&self, at: (f32, f32), map_origin: (f32, f32)) -> Option<ElementPoint> {
        let scale = self.drawn_scale()?;
        let strip = self.hud_strip()?;
        let view = self.shaped_view();
        let drawn = (view.x(at.0), view.y(at.1));
        let span = MINIMAP_SIZE as f32;
        if drawn.0 < 0.0 || drawn.0 >= span || drawn.1 < 0.0 || drawn.1 >= span {
            return None;
        }

        Some(ElementPoint {
            x: strip.left + (drawn.0 + map_origin.0) * scale,
            y: strip.top + (drawn.1 + map_origin.1) * scale,
        })
    }

    /// Where a window position is inside the viewport element.
    pub fn to_element(&self, position: WindowPoint) -> Option<ElementPoint> {
        let origin = self.placement?.bounds.origin;
        Some(ElementPoint { x: position.x - origin.x, y: position.y - origin.y })
    }

    /// Where a position inside the viewport element is in the window.
    pub fn to_window(&self, position: ElementPoint) -> Option<WindowPoint> {
        let origin = self.placement?.bounds.origin;
        Some(WindowPoint { x: position.x + origin.x, y: position.y + origin.y })
    }

    /// Zooms about whatever is under the pointer by `delta_y` pixels of wheel
    /// travel.
    pub fn zoom_by_wheel(&mut self, position: WindowPoint, delta_y: f32, map_origin: (f32, f32)) -> Wheel {
        let Some(at) = self.drawn_point(position, map_origin) else { return Wheel::Ignored };
        if delta_y == 0.0 {
            return Wheel::Ignored;
        }

        let zoom = self.view.zoom() * (1.0 + delta_y * ZOOM_PER_PIXEL);
        if self.set_view(self.shaped_view().zoomed_about(zoom, at)) { Wheel::Zoomed } else { Wheel::Unchanged }
    }

    /// Zooms to `zoom` about the middle of what is on screen, as the egui
    /// renderer's zoom control does: there is no pointer to zoom about.
    pub fn zoom_to(&mut self, zoom: f32) -> bool {
        let middle = MINIMAP_SIZE as f32 / 2.0;
        self.set_view(self.view.zoomed_about(zoom, (middle, middle)))
    }

    /// Starts a drag of the map at `position`. Returns whether it started.
    ///
    /// Only once there is somewhere to drag to: at the whole map, a drag
    /// would do nothing and holding it would swallow the click.
    pub fn start_pan(&mut self, position: WindowPoint) -> bool {
        if self.view.is_whole_map() {
            return false;
        }
        self.dragging = Some(position);

        true
    }

    /// Moves the map under the pointer while a drag is held. Returns whether
    /// the view changed.
    ///
    /// `held` is whether the button that started the drag is still down; a
    /// move without it ends the drag.
    pub fn pan_to(&mut self, position: WindowPoint, held: bool) -> bool {
        let Some(from) = self.dragging else { return false };
        if !held {
            self.dragging = None;
            return false;
        }
        let Some(scale) = self.drawn_scale() else { return false };

        let delta = ((position.x - from.x) / scale, (position.y - from.y) / scale);
        self.dragging = Some(position);
        self.set_view(self.shaped_view().dragged(delta))
    }

    pub fn end_pan(&mut self) {
        self.dragging = None;
    }

    pub fn is_panning(&self) -> bool {
        self.dragging.is_some()
    }
}

impl Default for MapView {
    fn default() -> Self {
        Self::new()
    }
}

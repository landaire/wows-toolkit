//! The spaces a viewport's points live in: device pixels on the surface,
//! points as the view lays out, the canvas the renderer draws, the drawn map
//! after zoom and pan, and map space.

use wows_minimap_renderer::MinimapPos;

use crate::frames::PixelPoint;
use crate::frames::PixelSize;
use crate::frames::ScaleFactor;

/// The size of the canvas the renderer drew, in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanvasSize {
    pub width: u32,
    pub height: u32,
}

/// A point on the whole canvas, in canvas pixels from its top left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasPoint {
    pub x: f32,
    pub y: f32,
}

/// A point in the drawn map, after zoom and pan, in canvas pixels from the
/// map's corner.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawnPoint {
    pub x: f32,
    pub y: f32,
}

/// A point in points from the frame's top left, as the view lays out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalPoint {
    pub x: f32,
    pub y: f32,
}

impl LogicalPoint {
    pub fn from_device(at: PixelPoint, scale: ScaleFactor) -> Self {
        Self { x: at.x / scale.get(), y: at.y / scale.get() }
    }

    pub fn to_device(self, scale: ScaleFactor) -> PixelPoint {
        PixelPoint { x: self.x * scale.get(), y: self.y * scale.get() }
    }
}

/// A whole device pixel. A hover reading is placed by one, so a move within
/// the same pixel does not move it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevicePixel {
    pub x: i32,
    pub y: i32,
}

impl DevicePixel {
    pub fn containing(at: PixelPoint) -> Self {
        Self { x: at.x as i32, y: at.y as i32 }
    }
}

/// A rotation handle as drawn: the handle, and the point on its shape the
/// stem runs to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotationHandle {
    pub handle: PixelPoint,
    pub anchor: PixelPoint,
}

/// Where the canvas lands in the viewport: drawn to fit without stretching,
/// centred.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// Device pixels per canvas pixel.
    pub scale: f32,
    pub left: f32,
    pub top: f32,
}

impl Fit {
    /// `canvas` fitted into `into`, or `None` while either is empty.
    pub fn of(canvas: CanvasSize, into: PixelSize) -> Option<Self> {
        if canvas.width == 0 || canvas.height == 0 || into.is_empty() {
            return None;
        }
        let scale = (into.width as f32 / canvas.width as f32).min(into.height as f32 / canvas.height as f32);
        let left = (into.width as f32 - canvas.width as f32 * scale) / 2.0;
        let top = (into.height as f32 - canvas.height as f32 * scale) / 2.0;

        Some(Self { scale, left, top })
    }
}

/// A map point as the session's drawing and cursor protocol carries it.
pub fn to_collab(at: MinimapPos) -> [f32; 2] {
    [at.x, at.y]
}

/// A map point the session sent.
pub fn from_collab(at: [f32; 2]) -> MinimapPos {
    MinimapPos { x: at[0], y: at[1] }
}

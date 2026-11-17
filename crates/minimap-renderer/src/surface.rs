//! A pixmap drawn on in canvas pixels and rasterised at a [`RenderScale`].

use tiny_skia::Color;
use tiny_skia::FillRule;
use tiny_skia::IntRect;
use tiny_skia::Paint;
use tiny_skia::Path;
use tiny_skia::Pixmap;
use tiny_skia::PixmapPaint;
use tiny_skia::PixmapRef;
use tiny_skia::Rect;
use tiny_skia::Stroke;
use tiny_skia::Transform;

use crate::render_scale::RenderScale;

/// A pixmap whose drawing calls are in canvas pixels.
///
/// Every shape, image and position goes through the scale on its way to the pixels, so one
/// layout draws the same frame at any size.
#[derive(Clone)]
pub(crate) struct Surface {
    pixels: Pixmap,
    width: u32,
    height: u32,
    scale: RenderScale,
}

impl Surface {
    /// A transparent surface `width` by `height` canvas pixels. `None` for an empty one.
    pub(crate) fn new(width: u32, height: u32, scale: RenderScale) -> Option<Self> {
        let pixels = Pixmap::new(scale.pixels(width), scale.pixels(height))?;

        Some(Self { pixels, width, height, scale })
    }

    /// `pixels`, already rasterised at `scale`, as a surface `width` by `height` canvas pixels.
    pub(crate) fn from_pixels(pixels: Pixmap, width: u32, height: u32, scale: RenderScale) -> Self {
        debug_assert_eq!((pixels.width(), pixels.height()), (scale.pixels(width), scale.pixels(height)));

        Self { pixels, width, height, scale }
    }

    /// Width in canvas pixels.
    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    /// Height in canvas pixels.
    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn scale(&self) -> RenderScale {
        self.scale
    }

    /// The rasterised pixels.
    pub(crate) fn pixels(&self) -> &Pixmap {
        &self.pixels
    }

    /// The rasterised pixels, for drawing that places its own pixels at [`Self::scale`].
    pub(crate) fn pixels_mut(&mut self) -> &mut Pixmap {
        &mut self.pixels
    }

    pub(crate) fn fill(&mut self, color: Color) {
        self.pixels.fill(color);
    }

    pub(crate) fn clear(&mut self) {
        self.pixels.data_mut().fill(0);
    }

    pub(crate) fn fill_path(&mut self, path: &Path, paint: &Paint<'_>, rule: FillRule, transform: Transform) {
        self.pixels.fill_path(path, paint, rule, self.to_pixels(transform), None);
    }

    pub(crate) fn stroke_path(&mut self, path: &Path, paint: &Paint<'_>, stroke: &Stroke, transform: Transform) {
        self.pixels.stroke_path(path, paint, stroke, self.to_pixels(transform), None);
    }

    pub(crate) fn fill_rect(&mut self, rect: Rect, paint: &Paint<'_>, transform: Transform) {
        self.pixels.fill_rect(rect, paint, self.to_pixels(transform), None);
    }

    /// Draws an image whose pixels are canvas pixels, enlarged with the surface.
    pub(crate) fn draw_pixmap(
        &mut self,
        x: i32,
        y: i32,
        image: PixmapRef<'_>,
        paint: &PixmapPaint,
        transform: Transform,
    ) {
        self.pixels.draw_pixmap(x, y, image, paint, self.to_pixels(transform), None);
    }

    /// Composites `layer`, drawn at this surface's scale, with its corner at canvas point `(x, y)`.
    ///
    /// Copied pixel for pixel: the layer is already at this size, and resampling it would blur it.
    pub(crate) fn draw_surface(&mut self, x: i32, y: i32, layer: &Surface, paint: &PixmapPaint) {
        let (x, y) = (self.to_pixel(x), self.to_pixel(y));
        self.pixels.draw_pixmap(x, y, layer.pixels.as_ref(), paint, Transform::identity(), None);
    }

    /// A copy of the canvas rectangle `rect`, at this surface's scale.
    pub(crate) fn clone_rect(&self, rect: IntRect) -> Option<Self> {
        let scale = self.scale;
        let pixel_rect = IntRect::from_xywh(
            self.to_pixel(rect.x()),
            self.to_pixel(rect.y()),
            scale.pixels(rect.width()),
            scale.pixels(rect.height()),
        )?;
        let pixels = self.pixels.clone_rect(pixel_rect)?;

        Some(Self { pixels, width: rect.width(), height: rect.height(), scale })
    }

    fn to_pixels(&self, transform: Transform) -> Transform {
        let factor = self.scale.factor();
        transform.post_scale(factor, factor)
    }

    fn to_pixel(&self, at: i32) -> i32 {
        (at as f32 * self.scale.factor()).round() as i32
    }
}

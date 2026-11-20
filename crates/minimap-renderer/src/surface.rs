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

    /// Composites `layer`, drawn at this surface's scale, over this surface at full opacity with
    /// its corner at canvas point `(x, y)`, pixel for pixel.
    ///
    /// The same pixels tiny-skia's source-over draws, without the per-pixel pipeline it runs a
    /// pixmap through. The map layer is composited at least once a frame, and through that pipeline
    /// it costs more than the rest of a frame's fixed work together.
    pub(crate) fn composite(&mut self, x: i32, y: i32, layer: &Surface) {
        let (x, y) = (self.to_pixel(x), self.to_pixel(y));
        let (to_width, to_height) = (self.pixels.width() as i32, self.pixels.height() as i32);
        let (from_width, from_height) = (layer.pixels.width() as i32, layer.pixels.height() as i32);
        let (left, top) = (x.max(0), y.max(0));
        let (right, bottom) = ((x + from_width).min(to_width), (y + from_height).min(to_height));
        if left >= right || top >= bottom {
            return;
        }

        let row_bytes = (right - left) as usize * 4;
        let from = layer.pixels.data();
        let to = self.pixels.data_mut();
        for row in top..bottom {
            let from_start = (((row - y) * from_width + (left - x)) * 4) as usize;
            let to_start = ((row * to_width + left) * 4) as usize;
            let from_row = from[from_start..from_start + row_bytes].as_chunks::<4>().0;
            let to_row = to[to_start..to_start + row_bytes].as_chunks_mut::<4>().0;
            for (under, over) in to_row.iter_mut().zip(from_row) {
                match over[3] {
                    0 => {}
                    u8::MAX => *under = *over,
                    alpha => {
                        let left_through = u32::from(u8::MAX - alpha);
                        for (under, over) in under.iter_mut().zip(over) {
                            // Rounded to nearest, as tiny-skia rounds.
                            let kept = (u32::from(*under) * left_through + 127) / 255;
                            *under = over.saturating_add(kept as u8);
                        }
                    }
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use tiny_skia::BlendMode;
    use tiny_skia::FilterQuality;

    use super::*;

    /// Fills `surface` with premultiplied pixels whose alpha is drawn from `alphas`, where `None`
    /// stands for any alpha.
    fn fill_with_noise(surface: &mut Surface, seed: &mut u32, alphas: &[Option<u8>]) {
        let mut next = || {
            *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            *seed >> 8
        };
        for pixel in surface.pixels.data_mut().as_chunks_mut::<4>().0 {
            let alpha = alphas[next() as usize % alphas.len()].unwrap_or(next() as u8);
            for channel in &mut pixel[..3] {
                *channel = (next() % (u32::from(alpha) + 1)) as u8;
            }
            pixel[3] = alpha;
        }
    }

    /// Over every kind of pixel, at fractional scales and with the layer partly off either edge,
    /// a composite draws exactly what tiny-skia's source-over draws.
    #[test]
    fn a_composite_draws_what_tiny_skia_draws() {
        let paint = PixmapPaint { opacity: 1.0, blend_mode: BlendMode::SourceOver, quality: FilterQuality::Nearest };
        let mut seed = 7;
        for factor in [1.0, 1.5, 2.0] {
            let scale = RenderScale::new(factor).expect("a positive scale");
            for (x, y) in [(0, 0), (13, 30), (-5, -7), (90, 110)] {
                let mut drawn = Surface::new(100, 120, scale).expect("a surface");
                fill_with_noise(&mut drawn, &mut seed, &[Some(u8::MAX), Some(0), None]);
                let mut composited = drawn.clone();
                let mut layer = Surface::new(60, 70, scale).expect("a layer");
                fill_with_noise(&mut layer, &mut seed, &[Some(u8::MAX), Some(0), None, None]);

                drawn.draw_surface(x, y, &layer, &paint);
                composited.composite(x, y, &layer);

                assert!(
                    drawn.pixels.data() == composited.pixels.data(),
                    "at scale {factor} with the layer at ({x}, {y})"
                );
            }
        }
    }
}

//! Drawing a frame: the canvas through the minimap renderer, fitted into the
//! frame surface, with the rotation handle and the peers' pings over it.

use image::RgbImage;
use tiny_skia::Color;
use tiny_skia::FillRule;
use tiny_skia::FilterQuality;
use tiny_skia::Paint;
use tiny_skia::PathBuilder;
use tiny_skia::Pixmap;
use tiny_skia::PixmapPaint;
use tiny_skia::Stroke;
use tiny_skia::Transform;
use wows_minimap_renderer::drawing::DrawnRegion;

use crate::frames::AlphaMode;
use crate::frames::Frame;
use crate::frames::FrameError;
use crate::frames::PixelSize;
use crate::frames::ScaleFactor;
use crate::renderer::geometry::CanvasSize;
use crate::renderer::geometry::Fit;
use crate::renderer::geometry::RotationHandle;
use crate::renderer::viewer::DrawJob;
use crate::renderer::viewer::PingMark;

/// What the view is filled with beside a canvas that does not fill it.
pub const MARGIN: [u8; 3] = [0x14, 0x19, 0x23];

/// The rotation handle's colour, the egui renderer's own.
const HANDLE_COLOR: [u8; 3] = [0xff, 0xff, 0x64];

/// A drawn frame, with the canvas it was fitted from and what a pointer can
/// rest on in it.
pub struct Drawn {
    pub frame: Frame,
    pub canvas: CanvasSize,
    pub regions: Vec<DrawnRegion>,
}

/// Rasterises `job`. The renderer is locked for the one frame only: it is
/// shared with every preview of the same map.
pub fn draw(job: &DrawJob) -> Result<Drawn, FrameError> {
    let (canvas, regions) = {
        let mut renderer = job.renderer.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        renderer.render_regions(job.view, &job.commands)
    };
    let size = CanvasSize { width: canvas.width(), height: canvas.height() };
    let mut frame = fit(&canvas, job.viewport.size, MARGIN)?;

    if let Some(handle) = job.handle {
        draw_handle(&mut frame, handle, job.viewport.scale);
    }
    for ping in &job.pings {
        draw_ping(&mut frame, ping, job.viewport.scale);
    }

    Ok(Drawn { frame: finish(frame, job.viewport.size)?, canvas: size, regions })
}

/// `image` fitted into `size` without stretching, centred over `margin`.
pub fn fit(image: &RgbImage, size: PixelSize, margin: [u8; 3]) -> Result<Pixmap, FrameError> {
    let Some(mut canvas) = Pixmap::new(size.width, size.height) else {
        return Err(FrameError::PixelCount {
            width: size.width,
            height: size.height,
            expected: size.width as usize * size.height as usize * 4,
            actual: 0,
        });
    };
    canvas.fill(Color::from_rgba8(margin[0], margin[1], margin[2], 0xff));

    let image_size = CanvasSize { width: image.width(), height: image.height() };
    if let (Some(fit), Some(layer)) = (Fit::of(image_size, size), layer(image)) {
        canvas.draw_pixmap(
            0,
            0,
            layer.as_ref(),
            &PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() },
            Transform::from_scale(fit.scale, fit.scale).post_translate(fit.left, fit.top),
            None,
        );
    }

    Ok(canvas)
}

/// A pixmap as a frame for the surface.
pub fn finish(pixmap: Pixmap, size: PixelSize) -> Result<Frame, FrameError> {
    Frame::new(size, AlphaMode::Premultiplied, pixmap.take())
}

/// An opaque RGB image as a pixmap.
fn layer(image: &RgbImage) -> Option<Pixmap> {
    let mut layer = Pixmap::new(image.width(), image.height())?;
    for (pixel, rgb) in layer.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(image.pixels()) {
        *pixel = [rgb[0], rgb[1], rgb[2], 0xff];
    }

    Some(layer)
}

/// The handle a picked shape is turned by, and the stem back to the shape so
/// it reads as belonging to it.
fn draw_handle(canvas: &mut Pixmap, handle: RotationHandle, scale: ScaleFactor) {
    let mut paint = Paint::default();
    paint.set_color_rgba8(HANDLE_COLOR[0], HANDLE_COLOR[1], HANDLE_COLOR[2], 0xff);
    paint.anti_alias = true;

    let mut stem = PathBuilder::new();
    stem.move_to(handle.handle.x, handle.handle.y);
    stem.line_to(handle.anchor.x, handle.anchor.y);
    if let Some(stem) = stem.finish() {
        let stroke = Stroke { width: scale.get().max(1.0), ..Stroke::default() };
        canvas.stroke_path(&stem, &paint, &stroke, Transform::identity(), None);
    }

    let radius = wows_toolkit_replay_data::playback::HANDLE_RADIUS * scale.get();
    if let Some(dot) = PathBuilder::from_circle(handle.handle.x, handle.handle.y, radius) {
        canvas.fill_path(&dot, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

/// A ping's ring, grown and faded by its age.
fn draw_ping(canvas: &mut Pixmap, ping: &PingMark, scale: ScaleFactor) {
    let Some(ring) = PathBuilder::from_circle(ping.at.x, ping.at.y, ping.radius.max(1.0)) else { return };
    let mut paint = Paint::default();
    let alpha = (ping.alpha.clamp(0.0, 1.0) * 255.0) as u8;
    paint.set_color_rgba8(ping.color[0], ping.color[1], ping.color[2], alpha);
    paint.anti_alias = true;
    let stroke = Stroke { width: 2.0 * scale.get(), ..Stroke::default() };
    canvas.stroke_path(&ring, &paint, &stroke, Transform::identity(), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_canvas_is_fitted_without_stretching_and_centred() {
        let image = RgbImage::from_pixel(100, 50, image::Rgb([200, 10, 10]));
        let size = PixelSize { width: 100, height: 100 };

        let frame = finish(fit(&image, size, MARGIN).expect("a pixmap"), size).expect("a frame");

        let pixel = |x: u32, y: u32| {
            let at = ((y * size.width + x) * 4) as usize;
            [frame.pixels[at], frame.pixels[at + 1], frame.pixels[at + 2]]
        };
        assert_eq!(pixel(50, 10), MARGIN, "above the letterboxed canvas is margin");
        assert_eq!(pixel(50, 50), [200, 10, 10], "the canvas sits in the middle");
        assert_eq!(pixel(50, 90), MARGIN, "and below it too");
    }
}

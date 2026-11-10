//! A scene readback as a surface frame, with the navigation gizmo drawn over
//! it.
//!
//! The readback is straight-alpha sRGB. The gizmo is drawn with tiny-skia into
//! a pixmap the size of its box and blended over that box only, so the rest of
//! the image is passed through untouched.

use tiny_skia::FillRule;
use tiny_skia::LineCap;
use tiny_skia::Paint;
use tiny_skia::PathBuilder;
use tiny_skia::Pixmap;
use tiny_skia::Stroke;
use tiny_skia::Transform;
use wows_toolkit_armor::scene::GizmoBall;
use wows_toolkit_armor::scene::GizmoView;
use wows_toolkit_armor::scene::axis_rgb;
use wows_toolkit_armor::viewport::camera::Axis;
use wows_toolkit_armor::viewport::gizmo;
use wows_toolkit_armor::viewport::types::Vec2;

use crate::frames::AlphaMode;
use crate::frames::Frame;
use crate::frames::FrameError;
use crate::frames::PixelSize;

/// One render of a pane: tightly packed straight-alpha RGBA8 rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Readback {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// The readback as a frame, with `gizmo` (logical pixels) drawn over it at
/// `scale` device pixels per logical pixel.
pub(super) fn compose(readback: Readback, gizmo: Option<&GizmoView>, scale: f32) -> Result<Frame, FrameError> {
    let Readback { width, height, mut pixels } = readback;
    let size = PixelSize { width, height };
    if let Some(gizmo) = gizmo
        && pixels.len() == width as usize * height as usize * 4
    {
        draw_gizmo(&mut pixels, size, gizmo, scale);
    }

    Frame::new(size, AlphaMode::Straight, pixels)
}

/// The gizmo's box in device pixels, clipped to the frame: `(x, y, w, h)`.
fn gizmo_box(size: PixelSize, center: Vec2, scale: f32) -> Option<(u32, u32, u32, u32)> {
    let half = gizmo::BOX_SIZE / 2.0;
    let left = ((center.x - half) * scale).floor().max(0.0);
    let top = ((center.y - half) * scale).floor().max(0.0);
    let right = ((center.x + half) * scale).ceil().min(size.width as f32);
    let bottom = ((center.y + half) * scale).ceil().min(size.height as f32);
    if right <= left || bottom <= top {
        return None;
    }

    Some((left as u32, top as u32, (right - left) as u32, (bottom - top) as u32))
}

fn draw_gizmo(pixels: &mut [u8], size: PixelSize, gizmo: &GizmoView, scale: f32) {
    let Some((left, top, width, height)) = gizmo_box(size, gizmo.center, scale) else { return };
    let Some(mut pixmap) = Pixmap::new(width, height) else { return };
    let transform = Transform::from_row(scale, 0.0, 0.0, scale, -(left as f32), -(top as f32));

    for ball in &gizmo.balls {
        draw_ball(&mut pixmap, gizmo.center, ball, transform);
    }

    blend_over(pixels, size, &pixmap, left, top);
}

/// One axis: its arm and filled, labelled ball when positive, an outlined
/// ball when negative, and a white ring while hovered.
fn draw_ball(pixmap: &mut Pixmap, center: Vec2, ball: &GizmoBall, transform: Transform) {
    let color = paint(axis_rgb(ball.axis));
    let tip = ball.tip;

    if ball.positive {
        let mut arm = PathBuilder::new();
        arm.move_to(center.x, center.y);
        arm.line_to(tip.x, tip.y);
        if let Some(arm) = arm.finish() {
            pixmap.stroke_path(&arm, &color, &stroke(2.0), transform, None);
        }
        if let Some(disc) = PathBuilder::from_circle(tip.x, tip.y, gizmo::BALL_R) {
            pixmap.fill_path(&disc, &color, FillRule::Winding, transform, None);
        }
        draw_glyph(pixmap, ball.axis, tip, transform);
    } else if let Some(ring) = PathBuilder::from_circle(tip.x, tip.y, gizmo::BALL_R) {
        pixmap.stroke_path(&ring, &color, &stroke(1.5), transform, None);
    }

    if ball.hovered
        && let Some(ring) = PathBuilder::from_circle(tip.x, tip.y, gizmo::BALL_R + 2.0)
    {
        pixmap.stroke_path(&ring, &paint(0xFFFFFF), &stroke(2.0), transform, None);
    }
}

/// The axis letter, stroked in black on its ball.
fn draw_glyph(pixmap: &mut Pixmap, axis: Axis, at: Vec2, transform: Transform) {
    const HALF_WIDTH: f32 = 2.5;
    const HALF_HEIGHT: f32 = 3.5;

    let (w, h) = (HALF_WIDTH, HALF_HEIGHT);
    let strokes: &[&[(f32, f32)]] = match axis {
        Axis::X => &[&[(-w, -h), (w, h)], &[(w, -h), (-w, h)]],
        Axis::Y => &[&[(-w, -h), (0.0, 0.0), (w, -h)], &[(0.0, 0.0), (0.0, h)]],
        Axis::Z => &[&[(-w, -h), (w, -h), (-w, h), (w, h)]],
    };

    let mut path = PathBuilder::new();
    for line in strokes {
        let Some((first, rest)) = line.split_first() else { continue };
        path.move_to(at.x + first.0, at.y + first.1);
        for point in rest {
            path.line_to(at.x + point.0, at.y + point.1);
        }
    }
    if let Some(path) = path.finish() {
        pixmap.stroke_path(&path, &paint(0x000000), &stroke(1.2), transform, None);
    }
}

fn paint(rgb: u32) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255);
    paint.anti_alias = true;
    paint
}

fn stroke(width: f32) -> Stroke {
    Stroke { width, line_cap: LineCap::Round, ..Stroke::default() }
}

/// Source-over of the premultiplied `pixmap` onto the straight-alpha frame at
/// `(left, top)`.
fn blend_over(pixels: &mut [u8], size: PixelSize, pixmap: &Pixmap, left: u32, top: u32) {
    let source = pixmap.data();
    for row in 0..pixmap.height() {
        let y = top + row;
        if y >= size.height {
            break;
        }
        for column in 0..pixmap.width() {
            let x = left + column;
            if x >= size.width {
                break;
            }
            let from = ((row * pixmap.width() + column) * 4) as usize;
            let source_alpha = source[from + 3] as f32 / 255.0;
            if source_alpha == 0.0 {
                continue;
            }

            let to = ((y * size.width + x) * 4) as usize;
            let dest_alpha = pixels[to + 3] as f32 / 255.0;
            let kept = dest_alpha * (1.0 - source_alpha);
            let alpha = source_alpha + kept;
            for channel in 0..3 {
                let premultiplied = source[from + channel] as f32 / 255.0;
                let dest = pixels[to + channel] as f32 / 255.0;
                let value = (premultiplied + dest * kept) / alpha;
                pixels[to + channel] = (value * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            pixels[to + 3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKGROUND: [u8; 4] = [10, 20, 30, 255];

    fn solid(width: u32, height: u32) -> Readback {
        Readback { width, height, pixels: BACKGROUND.repeat(width as usize * height as usize) }
    }

    /// A gizmo in the top-right corner of a `width` logical pixel wide view.
    fn gizmo_view(width: f32) -> GizmoView {
        let center = Vec2::new(width - 8.0 - gizmo::BOX_SIZE / 2.0, 8.0 + gizmo::BOX_SIZE / 2.0);
        let ball = |axis, positive, dx: f32, dy: f32| GizmoBall {
            axis,
            positive,
            tip: center + Vec2::new(dx, dy) * gizmo::ARM_LEN,
            hovered: false,
        };
        GizmoView {
            center,
            balls: vec![
                ball(Axis::X, false, -1.0, 0.0),
                ball(Axis::Y, false, 0.0, 1.0),
                ball(Axis::Z, false, 0.0, -1.0),
                ball(Axis::Z, true, 0.0, 1.0),
                ball(Axis::Y, true, 0.0, -1.0),
                GizmoBall { hovered: true, ..ball(Axis::X, true, 1.0, 0.0) },
            ],
        }
    }

    /// Every pixel that differs from the background, as `(x, y)`.
    fn changed(frame: &Frame) -> Vec<(u32, u32)> {
        frame
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(_, pixel)| **pixel != BACKGROUND)
            .map(|(index, _)| (index as u32 % frame.size.width, index as u32 / frame.size.width))
            .collect()
    }

    #[test]
    fn a_readback_becomes_a_straight_alpha_frame_of_its_size() {
        let frame = compose(solid(4, 3), None, 1.0).expect("a full readback is a frame");

        assert_eq!(frame.alpha, AlphaMode::Straight);
        assert_eq!(frame.size, PixelSize { width: 4, height: 3 });
        assert_eq!(frame.pixels.len(), 4 * 3 * 4);
        assert!(changed(&frame).is_empty());
    }

    #[test]
    fn a_short_readback_is_refused() {
        let readback = Readback { width: 4, height: 3, pixels: vec![0; 4 * 3 * 4 - 1] };

        assert!(compose(readback, Some(&gizmo_view(4.0)), 1.0).is_err());
    }

    #[test]
    fn the_gizmo_changes_only_its_own_box() {
        for scale in [1.0_f32, 2.0] {
            let (width, height) = (200.0_f32, 150.0_f32);
            let readback = solid((width * scale) as u32, (height * scale) as u32);
            let gizmo = gizmo_view(width);

            let frame = compose(readback, Some(&gizmo), scale).expect("a full readback is a frame");
            let touched = changed(&frame);

            let half = gizmo::BOX_SIZE / 2.0;
            let (left, right) = ((gizmo.center.x - half) * scale, (gizmo.center.x + half) * scale);
            let (top, bottom) = ((gizmo.center.y - half) * scale, (gizmo.center.y + half) * scale);
            assert!(!touched.is_empty(), "the gizmo drew nothing at scale {scale}");
            assert!(
                touched.iter().all(|&(x, y)| {
                    (x as f32) >= left.floor()
                        && (x as f32) < right.ceil()
                        && (y as f32) >= top.floor()
                        && (y as f32) < bottom.ceil()
                }),
                "the gizmo drew outside its box at scale {scale}"
            );
            assert!(frame.pixels.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255));
        }
    }

    #[test]
    fn a_positive_ball_is_filled_in_its_axis_colour() {
        let gizmo = gizmo_view(200.0);
        let frame = compose(solid(200, 150), Some(&gizmo), 1.0).expect("a full readback is a frame");

        // Beside the glyph, inside the positive Y ball.
        let tip = gizmo.center + Vec2::new(0.0, -gizmo::ARM_LEN);
        let (x, y) = ((tip.x - 5.0) as u32, tip.y as u32);
        let at = ((y * 200 + x) * 4) as usize;
        let rgb = axis_rgb(Axis::Y);

        assert_eq!(&frame.pixels[at..at + 3], &[(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8]);
    }
}

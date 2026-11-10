//! The CPU-readback bridge from the armor viewport into a gpui_kit
//! `RenderImage`.

use std::sync::Arc;

use gpui_kit::RenderImage;
use image::Frame;

/// Convert a tightly-packed sRGB RGBA8 readback (`width * height * 4`) from
/// `Viewport3D::render_offscreen_rgba` into a gpui_kit `RenderImage`.
///
/// gpui_kit stores images as BGRA, so the R and B bytes are swapped in place before
/// the buffer is wrapped in an `image::Frame`.
pub fn readback_to_render_image(width: u32, height: u32, mut rgba: Vec<u8>) -> Arc<RenderImage> {
    for px in rgba.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    let buffer =
        image::RgbaImage::from_raw(width, height, rgba).expect("readback buffer length equals width * height * 4");
    Arc::new(RenderImage::new(vec![Frame::new(buffer)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `readback_to_render_image` swaps R and B in place so the RGBA readback
    /// becomes BGRA (gpui's `RenderImage` pixel layout); alpha is untouched.
    #[test]
    fn readback_to_render_image_swaps_r_and_b_to_bgra() {
        let rgba = vec![10u8, 20, 30, 40];
        let image = readback_to_render_image(1, 1, rgba.clone());
        let swapped = image.as_bytes(0).expect("single-frame image has frame 0");
        assert_eq!(swapped[0], rgba[2]);
        assert_eq!(swapped[1], rgba[1]);
        assert_eq!(swapped[2], rgba[0]);
        assert_eq!(swapped[3], rgba[3]);
    }
}

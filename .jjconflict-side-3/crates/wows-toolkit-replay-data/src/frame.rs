//! Frames rasterised on the CPU, in the layout a GPU texture takes.

use std::sync::Arc;

use image::RgbImage;

/// One rasterised frame as tightly packed BGRA8 rows, the layout a dynamic
/// texture takes.
#[derive(Clone)]
pub struct BgraFrame {
    width: u32,
    height: u32,
    /// Shared, so a track that shows a frame again hands it over without a
    /// copy.
    bytes: Arc<Vec<u8>>,
}

impl BgraFrame {
    /// The whole of `image`, made opaque.
    pub fn from_rgb(image: &RgbImage) -> Self {
        let (width, height) = image.dimensions();
        let mut bgra = Vec::with_capacity(width as usize * height as usize * 4);
        push_bgra(&mut bgra, image.as_raw());

        Self::from_parts(width, height, bgra)
    }

    /// The `side` by `side` square of `image` whose top edge is row `top`, at
    /// its left edge, made opaque.
    ///
    /// # Panics
    ///
    /// If the square runs past the edge of `image`.
    pub fn from_rgb_square(image: &RgbImage, top: u32, side: u32) -> Self {
        assert!(
            side <= image.width() && top + side <= image.height(),
            "a {side} pixel square at row {top} lies inside a {}x{} image",
            image.width(),
            image.height()
        );
        let stride = image.width() as usize * 3;
        let row_bytes = side as usize * 3;
        let mut bgra = Vec::with_capacity(side as usize * side as usize * 4);
        for row in image.as_raw().chunks_exact(stride).skip(top as usize).take(side as usize) {
            push_bgra(&mut bgra, &row[..row_bytes]);
        }

        Self::from_parts(side, side, bgra)
    }

    fn from_parts(width: u32, height: u32, bgra: Vec<u8>) -> Self {
        Self { width, height, bytes: Arc::new(bgra) }
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn bytes_per_row(&self) -> u32 {
        self.width * 4
    }
}

/// Appends `rgb`'s pixels to `bgra`, opaque.
fn push_bgra(bgra: &mut Vec<u8>, rgb: &[u8]) {
    for pixel in rgb.as_chunks::<3>().0 {
        bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_square_is_cut_from_below_the_given_row_and_stored_bgra() {
        let image = RgbImage::from_fn(3, 4, |x, y| image::Rgb([x as u8, y as u8, 9]));

        let frame = BgraFrame::from_rgb_square(&image, 1, 2);

        assert_eq!((frame.width(), frame.height()), (2, 2));
        assert_eq!(frame.bytes(), [9, 1, 0, 255, 9, 1, 1, 255, 9, 2, 0, 255, 9, 2, 1, 255]);
    }
}

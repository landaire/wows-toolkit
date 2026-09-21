//! The minimap hover preview: a replay's battle played back as a small
//! animation while the pointer rests on its row.
//!
//! When the preview appears is [`wows_toolkit_viewmodel::preview_dwell`],
//! shared with the egui app so both wait the same amount. What it draws is
//! [`wows_minimap_renderer::preview`], the same command set the egui popup
//! paints and the video export encodes, rasterised to an image here because
//! this front end has no painter of its own.
//!
//! Nothing draws these yet. Baking a track reads and parses the replay
//! against its own build's game data, and that pipeline
//! (`wows-toolkit`'s `replay::renderer::preview`) is still in the egui crate;
//! it is free of egui but built on that crate's asset and build caches, so
//! moving it is its own piece of work. Rasterising and frame timing are
//! settled here so that move is the only thing left.
#![allow(dead_code)]

use std::sync::Arc;

use image::RgbImage;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::preview::PreviewRenderer;

/// How long each baked frame is shown. The bake keeps an evenly spaced subset
/// of the battle, so this is a display rate rather than the replay's own
/// clock.
pub const FRAME_INTERVAL: std::time::Duration = std::time::Duration::from_millis(80);

/// A rendered preview: every frame as an image gpui can draw.
pub struct PreviewFrames {
    frames: Vec<Arc<gpui_kit::Image>>,
}

impl PreviewFrames {
    /// Rasterises every frame of a baked track.
    ///
    /// Done once per preview rather than per paint: the frames are small and
    /// few, and re-rasterising on every frame of the *UI* would redraw the
    /// whole map for each one.
    pub fn render(renderer: &mut PreviewRenderer, track: &[Vec<DrawCommand>]) -> Self {
        let frames = track.iter().map(|commands| Arc::new(to_image(renderer.render(commands)))).collect();
        Self { frames }
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// The frame to show `elapsed` into the preview, looping.
    ///
    /// `None` only when there are no frames at all, which is a track that
    /// baked nothing.
    pub fn at(&self, elapsed: std::time::Duration) -> Option<Arc<gpui_kit::Image>> {
        if self.frames.is_empty() {
            return None;
        }
        let step = (elapsed.as_millis() / FRAME_INTERVAL.as_millis().max(1)) as usize;
        self.frames.get(step % self.frames.len()).cloned()
    }
}

/// An RGB frame as a gpui image.
///
/// gpui draws RGBA, so the alpha channel is added here; the renderer composes
/// every frame over opaque map art, so it is fully opaque by construction.
fn to_image(frame: RgbImage) -> gpui_kit::Image {
    let (width, height) = frame.dimensions();
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for pixel in frame.pixels() {
        rgba.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255]);
    }
    gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, encode_png(width, height, &rgba))
}

/// gpui takes encoded image bytes, so the raw frame is wrapped in a PNG.
///
/// Lossless and cheap at preview sizes; the alternative is an uncompressed
/// surface type this crate would have to keep in step with gpui's own.
fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(std::io::Cursor::new(&mut out));
    // The buffer was built from the frame's own dimensions, so the only way
    // this fails is an allocation failure, which is not something a preview
    // can do anything about.
    let _ = image::ImageEncoder::write_image(encoder, rgba, width, height, image::ExtendedColorType::Rgba8);
    out
}

#[cfg(test)]
mod tests {
    use super::FRAME_INTERVAL;
    use super::PreviewFrames;
    use std::sync::Arc;
    use std::time::Duration;

    fn frames(count: usize) -> PreviewFrames {
        // The images themselves are irrelevant to the timing rule; a one
        // pixel PNG stands in for a rendered frame.
        let pixel = super::encode_png(1, 1, &[0, 0, 0, 255]);
        PreviewFrames {
            frames: (0..count)
                .map(|_| Arc::new(gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, pixel.clone())))
                .collect(),
        }
    }

    #[test]
    fn a_track_with_no_frames_shows_nothing() {
        let empty = frames(0);
        assert!(empty.is_empty());
        assert!(empty.at(Duration::ZERO).is_none());
    }

    /// Each frame is held for the display interval, and the track loops
    /// rather than stopping on its last frame.
    #[test]
    fn frames_advance_on_the_interval_and_then_loop() {
        let track = frames(3);
        let shown = |elapsed: Duration| {
            let frame = track.at(elapsed).expect("a frame");
            track.frames.iter().position(|candidate| Arc::ptr_eq(candidate, &frame)).expect("one of ours")
        };

        assert_eq!(shown(Duration::ZERO), 0);
        assert_eq!(shown(FRAME_INTERVAL / 2), 0, "a frame is held for the whole interval");
        assert_eq!(shown(FRAME_INTERVAL), 1);
        assert_eq!(shown(FRAME_INTERVAL * 2), 2);
        assert_eq!(shown(FRAME_INTERVAL * 3), 0, "and it loops");
        assert_eq!(shown(FRAME_INTERVAL * 7), 1);
    }
}

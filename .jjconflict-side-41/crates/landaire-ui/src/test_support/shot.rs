use std::path::Path;
use std::time::Duration;

use zgui::prelude::IntoView;
use zgui_render_wgpu::Pixels;

use crate::error::HeadlessError;
use crate::test_support::window::Drawing;
use crate::test_support::window::HeadlessOptions;
use crate::test_support::window::HeadlessWindow;

/// Opens `view` headlessly with `options`, drawn on the graphics device whatever
/// `options.drawing` says, and writes what it shows to `path` as a PNG once a second of its
/// clock has passed, so whatever moves as it is built has come to rest.
pub fn render_png<F, V>(options: HeadlessOptions, view: F, path: &Path) -> Result<(), HeadlessError>
where
    F: FnMut() -> V + 'static,
    V: IntoView,
{
    let options = HeadlessOptions { drawing: Drawing::Device, ..options };
    let mut window = HeadlessWindow::open(options, view)?;
    window.advance(Duration::from_secs(1));

    window.save_png(path)
}

pub(crate) fn write_png(pixels: &Pixels, path: &Path) -> Result<(), HeadlessError> {
    let size = pixels.size();
    let mut rgba = pixels.bytes().to_vec();
    if pixels.is_bgra() {
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
    }

    let width = u32::try_from(size.width).map_err(|_| HeadlessError::NoPixels)?;
    let height = u32::try_from(size.height).map_err(|_| HeadlessError::NoPixels)?;
    let Some(image) = image::RgbaImage::from_raw(width, height, rgba) else {
        return Err(HeadlessError::NoPixels);
    };

    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)
            .map_err(|source| HeadlessError::Write { path: directory.to_path_buf(), source })?;
    }
    image
        .save_with_format(path, image::ImageFormat::Png)
        .map_err(|source| HeadlessError::Encode { path: path.to_path_buf(), source })
}

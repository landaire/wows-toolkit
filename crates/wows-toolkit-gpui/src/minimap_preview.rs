//! The minimap previews as images gpui can paint.
//!
//! What is drawn and how it is cached is [`wows_toolkit_replay_data::minimap`],
//! shared with the other front ends; this turns its RGB images into decoded
//! `RenderImage`s. Decoded, not encoded: a `gpui::Image` carries compressed
//! bytes that gpui decodes when it paints and caches by id, and a track is far
//! more frames than that cache holds, so every frame of the loop would decode
//! again and paint nothing until it finished.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use gpui_kit::RenderImage;
use image::RgbImage;
use rust_i18n::t;
use wows_minimap_renderer::draw_command::DrawCommand;
use wows_minimap_renderer::viewport::MapViewport;
use wows_toolkit_replay_data::load::GameDataCache;
use wows_toolkit_replay_data::minimap;
use wows_toolkit_replay_data::minimap::PreviewEdge;
use wows_toolkit_replay_data::minimap::PreviewError;

/// A rendered preview whose frames gpui paints.
pub type PreviewFrames = minimap::PreviewFrames<Arc<RenderImage>>;

/// What a surface says instead of a map. `None` for a superseded bake.
pub fn said(error: &PreviewError) -> Option<String> {
    let message = error.message()?;
    Some(t!(message.key, value = message.value).into_owned())
}

/// Plays `path`'s preview, as [`minimap::bake_from_file`] does, handing each
/// image over as gpui paints it.
pub fn bake_from_file(
    path: &Path,
    game_data: &GameDataCache,
    edge: PreviewEdge,
    cancel: &AtomicBool,
    on_map: impl FnOnce(PreviewFrames),
    mut on_frame: impl FnMut(Arc<RenderImage>),
) -> Result<(), PreviewError> {
    minimap::bake_from_file(
        path,
        game_data,
        edge,
        cancel,
        |map| on_map(PreviewFrames::still(to_image(&map))),
        |frame| on_frame(to_image(&frame)),
    )
}

/// The map `map_name` names, with nothing drawn over it, as a track of one
/// frame.
pub fn map_frame(map_name: &str, game_data: &GameDataCache, edge: PreviewEdge) -> Option<PreviewFrames> {
    minimap::map_frame(map_name, game_data, edge).map(|map| PreviewFrames::still(to_image(&map)))
}

/// One map with `commands` on it, at the minimap's own size.
pub fn render_map(
    map_name: &str,
    game_data: &GameDataCache,
    view: MapViewport,
    commands: &[DrawCommand],
) -> Option<Arc<RenderImage>> {
    minimap::render_map(map_name, game_data, view, commands).map(|map| to_image(&map))
}

/// The same, over art the caller holds rather than art this build ships.
pub fn render_map_art(
    held_as: &str,
    art: &RgbImage,
    game_data: &GameDataCache,
    view: MapViewport,
    commands: &[DrawCommand],
) -> Option<Arc<RenderImage>> {
    minimap::render_map_art(held_as, art, game_data, view, commands).map(|map| to_image(&map))
}

/// An RGB image as gpui stores one: BGRA, opaque. The renderer composes every
/// frame over opaque map art, so the alpha added here is opaque by
/// construction.
pub fn to_image(rgb: &RgbImage) -> Arc<RenderImage> {
    let (width, height) = rgb.dimensions();
    let mut bgra = Vec::with_capacity((width * height * 4) as usize);
    for pixel in rgb.pixels() {
        bgra.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255]);
    }
    let buffer = image::RgbaImage::from_raw(width, height, bgra)
        .expect("the buffer is four bytes per pixel of the size it was built at");
    Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]))
}

#[cfg(test)]
mod tests {
    /// A frame is handed over decoded, four bytes per pixel and stored BGRA,
    /// ready to upload.
    #[test]
    fn a_frame_is_stored_as_bgra_pixels() {
        let rgb = image::RgbImage::from_pixel(3, 2, image::Rgb([1, 2, 3]));

        let image = super::to_image(&rgb);

        let bytes = image.as_bytes(0).expect("frame 0");
        assert_eq!(bytes.len(), 3 * 2 * 4);
        assert!(bytes.chunks(4).all(|pixel| pixel == [3, 2, 1, 255]));
    }
}

//! Small square map images for the listing's rows: one per map, decoded and
//! scaled once off the UI thread, and shared by every row on that map.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::App;
use gpui_kit::RenderImage;
use image::Frame;
use image::RgbImage;
use image::RgbaImage;
use wows_toolkit_replay_data::load::GameDataCache;

/// The edge a thumbnail is scaled to, in physical pixels: twice the edge it
/// is drawn at, so it stays sharp on a scale-two display.
const THUMBNAIL_PIXELS: u32 = 64;

/// A map as a replay names it, such as `spaces/16_OC_bees_to_honey`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MapSpace(String);

impl MapSpace {
    pub fn new(space: impl Into<String>) -> Self {
        Self(space.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// Qualified: an imported `Borrow` reaches the tests through `super::*` and
// makes the test macro's own `borrow` calls ambiguous.
impl std::borrow::Borrow<str> for MapSpace {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// Where one map's thumbnail is.
#[derive(Clone)]
pub enum Thumbnail {
    Loading,
    Ready(Arc<RenderImage>),
    /// The build the art is read from ships none for this map.
    Missing,
}

/// The game build a set of thumbnails was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceBuild(u32);

/// What each listed map's row shows, by map.
pub type ThumbnailSlots = Rc<RefCell<HashMap<MapSpace, Thumbnail>>>;

/// The thumbnails one listing has asked for, by map.
pub struct MapThumbnails {
    /// Shared with the row renderer, which runs after the listing's own
    /// render has returned.
    slots: ThumbnailSlots,
    /// The build the slots were read from. A newer one replaces them all.
    source: Option<SourceBuild>,
}

/// Maps a listing still needs, and the build to read them from.
pub struct ThumbnailRequest {
    pub source: SourceBuild,
    pub spaces: Vec<MapSpace>,
}

impl MapThumbnails {
    pub fn new() -> Self {
        Self { slots: Rc::new(RefCell::new(HashMap::new())), source: None }
    }

    /// The slots, for the row renderer.
    pub fn slots(&self) -> ThumbnailSlots {
        Rc::clone(&self.slots)
    }

    /// Marks every map in `spaces` without a slot as loading, and returns
    /// those maps. Thumbnails read from another build are released first.
    /// `None` when nothing is left to load.
    pub fn claim<'a>(
        &mut self,
        source: SourceBuild,
        spaces: impl IntoIterator<Item = &'a str>,
        cx: &mut App,
    ) -> Option<ThumbnailRequest> {
        if self.source != Some(source) {
            self.release(cx);
            self.source = Some(source);
        }

        let mut slots = self.slots.borrow_mut();
        let mut claimed: Vec<MapSpace> = Vec::new();
        for space in spaces {
            if slots.contains_key(space) {
                continue;
            }
            let space = MapSpace::new(space);
            slots.insert(space.clone(), Thumbnail::Loading);
            claimed.push(space);
        }
        if claimed.is_empty() {
            return None;
        }

        Some(ThumbnailRequest { source, spaces: claimed })
    }

    /// Stores a finished load. Dropped when the slots have since been
    /// replaced by another build's.
    pub fn settle(&mut self, source: SourceBuild, space: MapSpace, image: Option<RenderImage>) {
        if self.source != Some(source) {
            return;
        }

        let thumbnail = match image {
            Some(image) => Thumbnail::Ready(Arc::new(image)),
            None => Thumbnail::Missing,
        };
        self.slots.borrow_mut().insert(space, thumbnail);
    }

    /// Removes every thumbnail, and its copy in each window's sprite atlas.
    pub fn release(&mut self, cx: &mut App) {
        self.source = None;
        let slots = std::mem::take(&mut *self.slots.borrow_mut());
        let images: Vec<Arc<RenderImage>> = slots
            .into_values()
            .filter_map(|thumbnail| match thumbnail {
                Thumbnail::Ready(image) => Some(image),
                Thumbnail::Loading | Thumbnail::Missing => None,
            })
            .collect();
        if images.is_empty() {
            return;
        }

        // Deferred: a window being updated is out of the app's window list,
        // and `drop_image` would skip its atlas.
        cx.defer(move |cx| {
            for image in images {
                cx.drop_image(image, None);
            }
        });
    }
}

impl Default for MapThumbnails {
    fn default() -> Self {
        Self::new()
    }
}

/// The newest build `cache` has open, which thumbnails are read from.
pub fn source_build(cache: &GameDataCache) -> Option<SourceBuild> {
    cache.newest_loaded().map(|loaded| SourceBuild(loaded.build()))
}

/// Reads and scales `space`'s art from the `source` build. Blocking: run off
/// the UI thread. `None` when that build is no longer open, or ships no art
/// for the map.
pub fn load(cache: &GameDataCache, source: SourceBuild, space: &MapSpace) -> Option<RenderImage> {
    let loaded = cache.loaded_build(source.0)?;
    let art = wows_minimap_renderer::assets::load_map_image(space.as_str(), loaded.vfs())?;

    Some(to_render_image(&art))
}

/// `art` scaled to the thumbnail edge, in the BGRA order GPUI uploads.
fn to_render_image(art: &RgbImage) -> RenderImage {
    let scaled =
        image::imageops::resize(art, THUMBNAIL_PIXELS, THUMBNAIL_PIXELS, image::imageops::FilterType::Triangle);
    let bgra = RgbaImage::from_fn(THUMBNAIL_PIXELS, THUMBNAIL_PIXELS, |x, y| {
        let [red, green, blue] = scaled.get_pixel(x, y).0;
        image::Rgba([blue, green, red, u8::MAX])
    });

    RenderImage::new(vec![Frame::new(bgra)])
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::*;

    #[gpui_kit::test]
    fn a_map_is_claimed_once_and_a_newer_build_releases_the_rest(cx: &mut TestAppContext) {
        cx.update(|cx: &mut App| {
            let mut thumbnails = MapThumbnails::new();
            let first = SourceBuild(1);
            let request = thumbnails.claim(first, ["spaces/a", "spaces/b", "spaces/a"], cx).expect("two new maps");
            assert_eq!(request.spaces, vec![MapSpace::new("spaces/a"), MapSpace::new("spaces/b")]);
            assert!(thumbnails.claim(first, ["spaces/a"], cx).is_none());

            thumbnails.settle(first, MapSpace::new("spaces/a"), None);
            assert!(matches!(thumbnails.slots().borrow().get("spaces/a"), Some(Thumbnail::Missing)));

            let newer = SourceBuild(2);
            let request = thumbnails.claim(newer, ["spaces/a"], cx).expect("the newer build reads it again");
            assert_eq!(request.spaces, vec![MapSpace::new("spaces/a")]);
            assert!(thumbnails.slots().borrow().get("spaces/b").is_none());

            // A load from the replaced build lands nowhere.
            thumbnails.settle(first, MapSpace::new("spaces/a"), None);
            assert!(matches!(thumbnails.slots().borrow().get("spaces/a"), Some(Thumbnail::Loading)));
        });
    }

    #[test]
    fn thumbnails_are_scaled_square_and_stored_as_bgra() {
        let art = RgbImage::from_pixel(300, 200, image::Rgb([10, 20, 30]));
        let image = to_render_image(&art);
        let bytes = image.as_bytes(0).expect("one frame");

        assert_eq!(bytes.len(), (THUMBNAIL_PIXELS * THUMBNAIL_PIXELS * 4) as usize);
        assert_eq!(&bytes[..4], &[30, 20, 10, 255]);
    }
}

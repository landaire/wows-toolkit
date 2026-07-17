//! Ship-class icon cache: decodes raw game-asset image bytes into
//! `gpui::RenderImage`s, keyed by ship `Species`, and caches the decoded
//! result so repeated renders of the same class reuse one image. Mirrors the
//! egui app's `WorldOfWarshipsData::ship_icons: HashMap<Species, Arc<GameAsset>>`
//! (`data/wows_data.rs`), but stores the already-decoded `RenderImage` rather
//! than raw file bytes.
//!
//! No real icon bytes exist yet: `PlayerRow` (Milestone 1) does not carry a
//! `GameAsset`, so a freshly built `IconCache` always returns `None` from
//! `get` until a later milestone's replay-loading pipeline calls `set` with
//! real PNG bytes resolved from the game's VFS. `table.rs` treats that `None`
//! the same way the egui app treats a missing icon: fall back to the plain
//! species-name label.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::RenderImage;
use image::Frame;
use wowsunpack::game_params::types::Species;

/// Decodes raw image bytes (whatever format `image::load_from_memory` can
/// sniff; the game ships ship-class icons as PNG) into a `RenderImage`.
/// gpui's `RenderImage` buffer is BGRA even though the `image` crate decodes
/// to RGBA, so this swaps the red/blue channels per pixel, matching gpui's
/// own `ClipboardItem::to_image_data` conversion (`gpui::platform`).
pub fn decode_icon(bytes: &[u8]) -> anyhow::Result<RenderImage> {
    let mut buffer = image::load_from_memory(bytes)?.into_rgba8();
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(RenderImage::new(vec![Frame::new(buffer)]))
}

/// Id-keyed cache of decoded ship-class icons, plus a second cache for every
/// other icon kind the expanded row content needs (achievement, ribbon,
/// consumable, modernization, signal, captain-skill), keyed by a caller-chosen
/// string rather than a `Species`. Key convention: `"<kind>:<id>"`, e.g.
/// `"achievement:{icon_key}"`, `"ribbon:{icon_key}"`, `"subribbon:{icon_key}"`,
/// `"consumable:{icon_key}"`, `"modernization:{game_params_name}"`,
/// `"signal:{game_params_name}"`, `"skill:{internal_name}"`.
#[derive(Default)]
pub struct IconCache {
    ship_class: HashMap<Species, Arc<RenderImage>>,
    /// Same empty-until-`set_keyed` story as `ship_class`; nothing calls
    /// `set_keyed` until a later milestone's replay-loading pipeline resolves
    /// real `GameAsset` bytes, so `get_keyed` always returns `None` today and
    /// the expanded-content render layer (`expanded.rs`) falls back to text.
    keyed: HashMap<String, Arc<RenderImage>>,
}

impl IconCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decodes `bytes` and caches the result under `species`, replacing any
    /// previously cached icon for that species. Returns `false` (leaving the
    /// cache untouched) if `bytes` cannot be decoded as an image, so a
    /// corrupt or missing asset degrades to `get` returning `None` instead of
    /// panicking.
    pub fn set(&mut self, species: Species, bytes: &[u8]) -> bool {
        match decode_icon(bytes) {
            Ok(image) => {
                self.ship_class.insert(species, Arc::new(image));
                true
            }
            Err(_) => false,
        }
    }

    /// The cached icon for `species`, if `set` has decoded one. `None` means
    /// "no icon available"; callers fall back to a text label.
    pub fn get(&self, species: Species) -> Option<Arc<RenderImage>> {
        self.ship_class.get(&species).cloned()
    }

    /// Decodes `bytes` and caches the result under `key` (see the struct doc
    /// for the key convention), replacing any previously cached icon for that
    /// key. Returns `false` (leaving the cache untouched) on a decode
    /// failure, matching `set`.
    pub fn set_keyed(&mut self, key: impl Into<String>, bytes: &[u8]) -> bool {
        match decode_icon(bytes) {
            Ok(image) => {
                self.keyed.insert(key.into(), Arc::new(image));
                true
            }
            Err(_) => false,
        }
    }

    /// The cached icon for `key`, if `set_keyed` has decoded one. `None`
    /// means "no icon available"; callers fall back to a text label.
    pub fn get_keyed(&self, key: &str) -> Option<Arc<RenderImage>> {
        self.keyed.get(key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// A 2x2 solid-red PNG, encoded in-memory so the fixture bytes live in
    /// source rather than a binary test asset.
    fn red_png_bytes() -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        let mut bytes = Vec::new();
        image.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png).expect("failed to encode test PNG");
        bytes
    }

    #[test]
    fn decode_icon_swaps_red_and_blue_channels() {
        let image = decode_icon(&red_png_bytes()).expect("valid PNG should decode");
        let pixel = image.as_bytes(0).expect("frame 0 should have pixel data");
        // Red (255, 0, 0, 255) in RGBA becomes (0, 0, 255, 255) in BGRA.
        assert_eq!(&pixel[0..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn decode_icon_rejects_non_image_bytes() {
        assert!(decode_icon(b"not an image").is_err());
    }

    #[test]
    fn icon_cache_get_is_none_until_set() {
        let cache = IconCache::new();
        assert!(cache.get(Species::Destroyer).is_none());
    }

    #[test]
    fn icon_cache_set_caches_by_species_and_get_returns_it() {
        let mut cache = IconCache::new();
        assert!(cache.set(Species::Destroyer, &red_png_bytes()));
        assert!(cache.get(Species::Destroyer).is_some());
        assert!(cache.get(Species::Cruiser).is_none(), "a different species should stay uncached");
    }

    #[test]
    fn icon_cache_set_returns_false_and_leaves_cache_untouched_for_invalid_bytes() {
        let mut cache = IconCache::new();
        assert!(!cache.set(Species::Destroyer, b"not an image"));
        assert!(cache.get(Species::Destroyer).is_none());
    }

    #[test]
    fn icon_cache_get_keyed_is_none_until_set_keyed() {
        let cache = IconCache::new();
        assert!(cache.get_keyed("achievement:dfc").is_none());
    }

    #[test]
    fn icon_cache_set_keyed_caches_by_key_and_get_keyed_returns_it() {
        let mut cache = IconCache::new();
        assert!(cache.set_keyed("achievement:dfc", &red_png_bytes()));
        assert!(cache.get_keyed("achievement:dfc").is_some());
        assert!(cache.get_keyed("achievement:kraken").is_none(), "a different key should stay uncached");
    }

    #[test]
    fn icon_cache_set_keyed_returns_false_and_leaves_cache_untouched_for_invalid_bytes() {
        let mut cache = IconCache::new();
        assert!(!cache.set_keyed("achievement:dfc", b"not an image"));
        assert!(cache.get_keyed("achievement:dfc").is_none());
    }
}

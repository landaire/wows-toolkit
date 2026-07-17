//! Icon cache: resolves the game's GUI icon assets (ship-class, captain-skill,
//! achievement, ribbon/subribbon, consumable, modernization, signal, nation
//! flag) from the preloaded VFS, decodes them into `gpui::RenderImage`s, and
//! caches the decoded result so repeated renders of the same icon reuse one
//! image. Mirrors the egui app's per-kind icon maps on `WorldOfWarshipsData`
//! (`data/wows_data.rs`: `ship_icons`, `ribbon_icons`, `subribbon_icons`,
//! `achievement_icons`, `consumable_icons`, `crew_skill_icons`,
//! `modernization_icons`, `signal_flag_icons`) and its `icon_texture` decode
//! step (`ui/replay_parser/mod.rs`), but stores the already-decoded
//! `RenderImage` rather than raw asset bytes.
//!
//! Every icon kind except ship-class resolves to a PNG, decoded via
//! [`decode_icon`] (the `image` crate). Ship-class icons are the one
//! exception: the game ships them as SVG (`gui/fla/minimap/ship_icons`), which
//! the `image` crate cannot decode, so [`decode_ship_class_svg`] rasterizes
//! them through gpui's own `SvgRenderer` instead -- the same renderer
//! gpui-component uses for its bundled icon SVGs -- and rotates the result 90
//! degrees clockwise to match the egui app's `Image::rotate` on the same
//! asset.
//!
//! `table.rs`/`expanded.rs`/`browser_view.rs` treat a cache miss (an asset
//! absent from this build, or one `populate_from_rows`/`populate_nation_flags`
//! has not been asked to resolve) the same way the egui app treats a missing
//! icon texture: fall back to a plain text label.

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Read;
use std::sync::Arc;

use gpui::RenderImage;
use gpui::SvgRenderer;
use image::Frame;
use wowsunpack::game_assets::GuiAsset;
use wowsunpack::game_assets::GuiAssetDir;
use wowsunpack::game_assets::ShipIconState;
use wowsunpack::game_params::types::Species;
use wowsunpack::vfs::VfsPath;

use super::model::PlayerRow;

/// Decodes raw image bytes (whatever format `image::load_from_memory` can
/// sniff; every icon kind except ship-class ships as PNG) into a
/// `RenderImage`. gpui's `RenderImage` buffer is BGRA even though the `image`
/// crate decodes to RGBA, so this swaps the red/blue channels per pixel,
/// matching gpui's own `ClipboardItem::to_image_data` conversion
/// (`gpui::platform`).
pub fn decode_icon(bytes: &[u8]) -> anyhow::Result<RenderImage> {
    let mut buffer = image::load_from_memory(bytes)?.into_rgba8();
    for pixel in buffer.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(RenderImage::new(vec![Frame::new(buffer)]))
}

/// Rasterizes a ship-class SVG (`gui/fla/minimap/ship_icons`) into a
/// `RenderImage` via gpui's `SvgRenderer`, then rotates the raster 90 degrees
/// clockwise to match the egui app's `Image::rotate(90.0_f32.to_radians(),
/// Vec2::splat(0.5))` on the same asset (`ui/replay_parser/mod.rs`'s
/// `ReplayColumn::Name` arm). `SvgRenderer::render_single_frame` already
/// returns BGRA (premultiplied) bytes -- the same format `decode_icon`
/// produces -- so the rotation only permutes pixel positions, never channel
/// order.
pub fn decode_ship_class_svg(svg_renderer: &SvgRenderer, bytes: &[u8]) -> anyhow::Result<RenderImage> {
    let rendered =
        svg_renderer.render_single_frame(bytes, 1.0).map_err(|e| anyhow::anyhow!("failed to render SVG: {e:?}"))?;
    let size = rendered.size(0);
    let raw = rendered.as_bytes(0).ok_or_else(|| anyhow::anyhow!("rendered SVG has no frame data"))?;
    let buffer = image::RgbaImage::from_raw(size.width.0 as u32, size.height.0 as u32, raw.to_vec())
        .ok_or_else(|| anyhow::anyhow!("rendered SVG buffer size did not match its own dimensions"))?;
    let rotated = image::imageops::rotate90(&buffer);
    Ok(RenderImage::new(vec![Frame::new(rotated)]))
}

/// Id-keyed cache of decoded icons: ship-class icons keyed by `Species`, and
/// every other icon kind (achievement, ribbon, subribbon, consumable,
/// modernization, signal, captain-skill, nation flag) keyed by a
/// caller-chosen string. Key convention: `"<kind>:<id>"`, e.g.
/// `"achievement:{icon_key}"`, `"ribbon:{icon_key}"`, `"subribbon:{icon_key}"`,
/// `"consumable:{icon_key}"`, `"modernization:{game_params_name}"`,
/// `"signal:{game_params_name}"`, `"skill:{internal_name}"`,
/// `"nation:{nation}"`. Cheap to clone: every stored image is `Arc`-shared.
#[derive(Default, Clone)]
pub struct IconCache {
    ship_class: HashMap<Species, Arc<RenderImage>>,
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

    /// Caches an already-decoded icon under `species`, bypassing `decode_icon`
    /// (used for ship-class icons, which `decode_ship_class_svg` decodes via
    /// gpui's `SvgRenderer` rather than the `image` crate).
    pub fn set_image(&mut self, species: Species, image: RenderImage) {
        self.ship_class.insert(species, Arc::new(image));
    }

    /// The cached icon for `species`, if one has been decoded. `None` means
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

    /// Number of distinct ship-class icons currently cached.
    pub fn ship_class_count(&self) -> usize {
        self.ship_class.len()
    }

    /// Number of distinct keyed (non-ship-class) icons currently cached.
    pub fn keyed_count(&self) -> usize {
        self.keyed.len()
    }

    /// Resolves and decodes every icon `rows` references -- one ship-class
    /// icon per distinct `Species` among them, plus one keyed icon per
    /// distinct achievement/ribbon/subribbon/consumable/modernization/signal/
    /// captain-skill the rows carry -- from `vfs`, caching each one. A
    /// per-call dedupe set means a 24-player replay reads (and decodes) each
    /// shared asset -- e.g. every row earning `RIBBON_MAIN_CALIBER`, or every
    /// row flying the same nation's ship class -- only once, not once per
    /// row. An asset absent from this build (an older client missing some
    /// icon, or a key with no matching file) is silently skipped; `get`/
    /// `get_keyed` returning `None` for it is the render layer's existing
    /// signal to fall back to text.
    pub fn populate_from_rows(&mut self, rows: &[PlayerRow], vfs: &VfsPath, svg_renderer: &SvgRenderer) {
        let mut species_seen: HashSet<Species> = HashSet::new();
        let mut keys_seen: HashSet<String> = HashSet::new();

        for row in rows {
            if species_seen.insert(row.ship_class) {
                self.load_ship_class(row.ship_class, vfs, svg_renderer);
            }

            for achievement in &row.achievements {
                let key = format!("achievement:{}", achievement.icon_key);
                self.load_keyed(&mut keys_seen, vfs, key, GuiAsset::Achievement(&achievement.icon_key));
            }
            for ribbon in &row.ribbons {
                let (key, asset) = if ribbon.is_subribbon {
                    (format!("subribbon:{}", ribbon.icon_key), GuiAsset::SubRibbon(&ribbon.icon_key))
                } else {
                    (format!("ribbon:{}", ribbon.icon_key), GuiAsset::Ribbon(&ribbon.icon_key))
                };
                self.load_keyed(&mut keys_seen, vfs, key, asset);
            }
            for consumable in &row.consumables {
                let key = format!("consumable:{}", consumable.icon_key);
                self.load_keyed(&mut keys_seen, vfs, key, GuiAsset::Consumable(&consumable.icon_key));
            }

            let Some(build) = row.translated_build.as_ref() else { continue };
            for slot in build.modernization_slots.iter().flatten() {
                let key = format!("modernization:{}", slot.game_params_name);
                self.load_keyed(&mut keys_seen, vfs, key, GuiAsset::Modernization(&slot.game_params_name));
            }
            for signal in &build.signals {
                let key = format!("signal:{}", signal.game_params_name);
                self.load_keyed(&mut keys_seen, vfs, key, GuiAsset::SignalFlag(&signal.game_params_name));
            }
            for skill in build.captain_skills.iter().flatten().flat_map(|row| &row.skills) {
                let key = format!("skill:{}", skill.internal_name.as_str());
                self.load_keyed(&mut keys_seen, vfs, key, GuiAsset::CrewSkill { name: &skill.internal_name });
            }
        }
    }

    /// Bulk-loads every nation flag under `gui/nation_flags/tiny/` (mirrors
    /// the egui app's `load_ribbon_icons` directory-bulk-load pattern), keyed
    /// by the nation name embedded in each file's name
    /// (`flag_{nation}.png` -> `"nation:{nation}"`). A build with no nation
    /// flags directory (or one that fails to read) leaves the cache
    /// untouched rather than erroring.
    pub fn populate_nation_flags(&mut self, vfs: &VfsPath) {
        let Some(dir) = GuiAssetDir::NationFlags.resolve(vfs, None) else { return };
        let Ok(entries) = dir.read_dir() else { return };

        for entry in entries {
            let filename = entry.filename();
            let Some(stem) = std::path::Path::new(&filename).file_stem().and_then(|s| s.to_str()) else { continue };
            let Some(nation) = stem.strip_prefix("flag_") else { continue };
            let mut data = Vec::new();
            if entry.open_file().and_then(|mut f| f.read_to_end(&mut data).map_err(Into::into)).is_err() {
                continue;
            }
            self.set_keyed(format!("nation:{nation}"), &data);
        }
    }

    fn load_ship_class(&mut self, species: Species, vfs: &VfsPath, svg_renderer: &SvgRenderer) {
        let Some(bytes) = GuiAsset::ShipClassIcon { species, state: ShipIconState::Alive }.read(vfs, None) else {
            return;
        };
        if let Ok(image) = decode_ship_class_svg(svg_renderer, &bytes) {
            self.set_image(species, image);
        }
    }

    fn load_keyed(&mut self, keys_seen: &mut HashSet<String>, vfs: &VfsPath, key: String, asset: GuiAsset<'_>) {
        if !keys_seen.insert(key.clone()) {
            return;
        }
        if let Some(bytes) = asset.read(vfs, None) {
            self.set_keyed(key, &bytes);
        }
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
    fn icon_cache_set_image_caches_an_already_decoded_image() {
        let mut cache = IconCache::new();
        let image = decode_icon(&red_png_bytes()).expect("valid PNG should decode");
        cache.set_image(Species::AirCarrier, image);
        assert!(cache.get(Species::AirCarrier).is_some());
        assert_eq!(cache.ship_class_count(), 1);
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
        assert_eq!(cache.keyed_count(), 1);
    }

    #[test]
    fn icon_cache_set_keyed_returns_false_and_leaves_cache_untouched_for_invalid_bytes() {
        let mut cache = IconCache::new();
        assert!(!cache.set_keyed("achievement:dfc", b"not an image"));
        assert!(cache.get_keyed("achievement:dfc").is_none());
    }

    #[test]
    fn icon_cache_populate_from_rows_is_a_no_op_on_a_row_with_no_achievements_ribbons_or_build() {
        // No VFS/SvgRenderer needed: `populate_from_rows` only ever reaches
        // out to them for keys a row actually carries, and `base_row` (see
        // `test_support.rs`) carries none. This exercises the "nothing to
        // resolve" path without needing a real game install.
        use crate::replay_inspector::test_support::base_row;
        use wows_replays::types::Relation;

        let mut cache = IconCache::new();
        let rows = vec![base_row(1, Relation::new(0), true)];
        // `populate_from_rows` still needs a `VfsPath`/`SvgRenderer` value to
        // call, even though this fixture never reaches the code paths that
        // use them (no achievements/ribbons/consumables/build on `base_row`,
        // and the fixture VFS has no ship_icons directory for the row's own
        // species either). A `PhysicalFS` rooted at a fresh empty temp dir
        // gives a valid, asset-less `VfsPath` without touching a real game
        // install.
        let dir = std::env::temp_dir().join("wtk-gpui-icons-test-populate-noop");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let vfs: VfsPath = wowsunpack::vfs::PhysicalFS::new(&dir).into();
        let svg_renderer = SvgRenderer::new(std::sync::Arc::new(()));

        cache.populate_from_rows(&rows, &vfs, &svg_renderer);

        assert_eq!(cache.ship_class_count(), 0);
        assert_eq!(cache.keyed_count(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
